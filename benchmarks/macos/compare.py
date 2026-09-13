import argparse
import hashlib
import json
import os
import pathlib
import signal
import subprocess
import time

import Quartz

from observe import TIMEBASE, app_pids, collect, sample


def save(path, value):
    with path.open("x", encoding="utf-8") as file:
        json.dump(value, file, indent=2)
        file.write("\n")


def visible_windows(pids):
    windows = Quartz.CGWindowListCopyWindowInfo(Quartz.kCGWindowListOptionOnScreenOnly, 0)
    return [
        {"pid": int(w["kCGWindowOwnerPID"]), "bounds": dict(w["kCGWindowBounds"])}
        for w in windows
        if w["kCGWindowOwnerPID"] in pids and w["kCGWindowLayer"] == 0
        and w["kCGWindowBounds"]["Width"] >= 700 and w["kCGWindowBounds"]["Height"] >= 450
    ]


def status(executable, env):
    result = subprocess.run([str(executable), "--status"], env=env, capture_output=True,
                            text=True, timeout=5)
    if result.returncode:
        return None
    return json.loads(result.stdout)["snapshot"]


def health(snapshot):
    if snapshot is None:
        return None
    return {key: snapshot.get(key) for key in
            ("native_ready", "paused", "suspended", "permissions", "device", "dropped_events", "errors")}


def stop(bundle, executable, env, child=None):
    if child is not None and child.poll() is not None:
        return
    if executable.name == "superlight":
        subprocess.run([str(executable), "--quit"], env=env, capture_output=True, timeout=10)
    for pid in app_pids(bundle):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue
    deadline = time.monotonic() + 10
    while app_pids(bundle) and time.monotonic() < deadline:
        time.sleep(0.1)
    if app_pids(bundle):
        raise RuntimeError(f"App did not stop: {bundle}")
    if child is not None:
        child.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mouser", type=pathlib.Path, required=True)
    parser.add_argument("--superlight", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--warmup", type=float, default=30)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--background", action="store_true")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    bundles = {"mouser": args.mouser.resolve(), "superlight": args.superlight.resolve()}
    executables = {name: bundle / "Contents/MacOS" / ("Mouser" if name == "mouser" else "superlight")
                   for name, bundle in bundles.items()}
    files = {name: pathlib.Path.home() / "Library/Application Support" / name / "config.json"
             for name in ("Mouser", "SuperLight")}
    originals = {name: path.read_bytes() for name, path in files.items() if path.exists()}
    backups = output / "private-backups"
    backups.mkdir(mode=0o700)
    for name, content in originals.items():
        (backups / f"{name}.json").write_bytes(content)
    config = json.loads(originals["SuperLight"])
    config["settings"]["start_at_login"] = False
    config["settings"]["start_minimized"] = False
    home = output / "mouser-home"
    config_dirs = {"mouser": home / "Library/Application Support/Mouser",
                   "superlight": output / "superlight-config"}
    for directory in config_dirs.values():
        directory.mkdir(parents=True)
        save(directory / "config.json", config)
    envs = {"mouser": {**os.environ, "HOME": str(home)},
            "superlight": {**os.environ, "SUPERLIGHT_CONFIG_DIR": str(config_dirs["superlight"])}}
    mouser_log = home / "Library/Logs/Mouser/mouser.log"
    before_cpu = sample(os.getpid())
    start_cpu = time.process_time_ns()
    while time.process_time_ns() - start_cpu < 100_000_000:
        pass
    elapsed_cpu = time.process_time_ns() - start_cpu
    after_cpu = sample(os.getpid())
    rusage_cpu = sum(after_cpu[key] - before_cpu[key] for key in ("user_ns", "system_ns"))
    calibration = {"process_time_ns": elapsed_cpu, "rusage_ns": rusage_cpu,
                   "ratio": rusage_cpu / elapsed_cpu, "numer": TIMEBASE.numer, "denom": TIMEBASE.denom}
    if not 0.98 < calibration["ratio"] < 1.02:
        raise RuntimeError(f"CPU counter calibration failed: {calibration}")
    save(output / "manifest.json", {
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "os": subprocess.check_output(["sw_vers"], text=True),
        "cpu": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
        "memory_bytes": int(subprocess.check_output(["sysctl", "-n", "hw.memsize"])),
        "power": subprocess.check_output(["pmset", "-g", "batt"], text=True),
        "warmup_s": args.warmup, "sample_s": args.seconds,
        "workload": "idle, physical Logitech connected, no scripted input",
        "settings_window": "closed/hidden" if args.background else "visible",
        "cpu_calibration": calibration,
        "order": ["mouser", "superlight", "superlight", "mouser", "mouser", "superlight"],
        "executables_sha256": {name: hashlib.sha256(exe.read_bytes()).hexdigest()
                               for name, exe in executables.items()},
        "config_sha256": {name: hashlib.sha256(content).hexdigest() for name, content in originals.items()}
    })
    stop(bundles["superlight"], executables["superlight"], os.environ)
    if app_pids(bundles["mouser"]):
        raise RuntimeError("An existing Mouser process must be stopped before the benchmark")
    trials = []
    active = None
    try:
        for index, name in enumerate(["mouser", "superlight", "superlight", "mouser", "mouser", "superlight"]):
            prefix = f"{index + 1:02}-{name}"
            offset = mouser_log.stat().st_size if mouser_log.exists() else 0
            with (output / f"{prefix}-process.log").open("x") as log:
                flag = ("--start-hidden" if name == "mouser" else "--background") if args.background else ("--show-window" if name == "mouser" else "--settings")
                child = subprocess.Popen([str(executables[name]), flag],
                                         env=envs[name], stdout=log, stderr=log, start_new_session=True)
            active = (name, child)
            deadline = time.monotonic() + 40
            ready = False
            before = None
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    raise RuntimeError(f"{name} exited during startup: {child.returncode}")
                if name == "superlight":
                    before = health(status(executables[name], envs[name]))
                    ready = before is not None and before["native_ready"] and before["device"] is not None
                elif mouser_log.exists():
                    with mouser_log.open() as log:
                        log.seek(offset)
                        text = log.read()
                    ready = "[MouseHook] Device Connected" in text and "remapping is active" in text
                visible = bool(visible_windows(app_pids(bundles[name])))
                if ready and visible == (not args.background):
                    break
                time.sleep(0.3)
            else:
                raise RuntimeError(f"{name} did not become ready with the requested window state")
            print(f"{prefix}: connected, window state verified. Warming up for {args.warmup}s", flush=True)
            time.sleep(args.warmup)
            expected = 1 if name == "mouser" or args.background else 2
            if len(app_pids(bundles[name])) != expected:
                raise RuntimeError(f"Unexpected process count for {name}")
            if any(app_pids(bundle) for other, bundle in bundles.items() if other != name):
                raise RuntimeError("The other remapper is still running")
            windows_before = visible_windows(app_pids(bundles[name]))
            if bool(windows_before) == args.background:
                raise RuntimeError("Incorrect settings window state")
            metrics = collect(bundles[name], output / f"{prefix}.jsonl", args.seconds)
            windows_after = visible_windows(app_pids(bundles[name]))
            if not metrics["stable_process_set"] or bool(windows_after) == args.background:
                raise RuntimeError("App process set or visibility changed during measurement")
            after = health(status(executables[name], envs[name])) if name == "superlight" else None
            if after is not None and (not after["native_ready"] or after["device"] is None):
                raise RuntimeError("SuperLight disconnected during measurement")
            if name == "mouser":
                with mouser_log.open() as log:
                    log.seek(offset)
                    text = log.read()
                (output / f"{prefix}-hardware.log").write_text(text)
                if "Device Disconnected" in text:
                    raise RuntimeError("Mouser disconnected during measurement")
            trial = {"name": name, "trial": index + 1, "metrics": metrics,
                     "health_before": before, "health_after": after,
                     "windows_before": windows_before, "windows_after": windows_after}
            save(output / f"{prefix}-summary.json", trial)
            trials.append(trial)
            print(f"{prefix}: CPU {metrics['cpu_percent_one_core']:.4f}%, "
                  f"footprint {metrics['footprint_bytes']['median'] / 1048576:.2f} MiB", flush=True)
            stop(bundles[name], executables[name], envs[name], child)
            active = None
            time.sleep(3)
        save(output / "comparison.json", {"trials": trials})
    finally:
        if active is not None:
            name, child = active
            stop(bundles[name], executables[name], envs[name], child)
        unchanged = {name: files[name].read_bytes() == content for name, content in originals.items()}
        with (output / "restore.log").open("x") as log:
            subprocess.Popen([str(executables["superlight"]), "--settings"], env=os.environ,
                             stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        time.sleep(8)
        restored = health(status(executables["superlight"], os.environ))
        save(output / "restoration.json", {"config_unchanged": unchanged, "health": restored,
                                          "pids": app_pids(bundles["superlight"])})
        print(f"Restored SuperLight. Config unchanged: {unchanged}", flush=True)
        if not all(unchanged.values()) or restored is None or not restored["native_ready"]:
            raise RuntimeError("Restoration did not pass validation")


if __name__ == "__main__":
    main()
