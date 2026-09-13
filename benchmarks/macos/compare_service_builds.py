import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import time

from compare import health, save, status, stop, visible_windows
from observe import TIMEBASE, app_pids, collect, processes, sample


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=pathlib.Path)
    parser.add_argument("--candidate", required=True, type=pathlib.Path)
    parser.add_argument("--installed", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--seconds", type=float, default=60)
    parser.add_argument("--warmup", type=float, default=30)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    installed = args.installed.resolve()
    restore_exe = installed / "Contents/MacOS/superlight"
    executables = {name: getattr(args, name).resolve() for name in ("baseline", "candidate")}
    original_path = pathlib.Path.home() / "Library/Application Support/SuperLight/config.json"
    original = original_path.read_bytes()
    (output / "private-config-backup.json").write_bytes(original)
    config = json.loads(original)
    config["settings"]["start_at_login"] = False
    config["settings"]["start_minimized"] = True
    initial = sample(os.getpid())
    started = time.process_time_ns()
    while time.process_time_ns() - started < 100_000_000:
        pass
    elapsed = time.process_time_ns() - started
    finished = sample(os.getpid())
    ratio = sum(finished[k] - initial[k] for k in ("user_ns", "system_ns")) / elapsed
    if not 0.98 < ratio < 1.02:
        raise RuntimeError("CPU counter calibration failed")
    order = ["baseline", "candidate", "candidate", "baseline", "baseline", "candidate"]
    save(output / "manifest.json", {
        "order": order, "warmup_s": args.warmup, "sample_s": args.seconds,
        "workload": "background service, settings closed, physical mouse connected, no scripted input",
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "cpu_calibration": {"ratio": ratio, "numer": TIMEBASE.numer, "denom": TIMEBASE.denom},
        "os": subprocess.check_output(["sw_vers"], text=True),
        "cpu": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
        "power": subprocess.check_output(["pmset", "-g", "batt"], text=True),
        "binaries": {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in executables.items()}
    })
    active = None
    trials = []
    stop(installed, restore_exe, os.environ)
    try:
        for index, name in enumerate(order):
            prefix = f"{index + 1:02}-{name}"
            directory = output / f"{prefix}-config"
            directory.mkdir()
            save(directory / "config.json", config)
            env = {**os.environ, "SUPERLIGHT_CONFIG_DIR": str(directory)}
            exe = executables[name]
            with (output / f"{prefix}.log").open("x") as log:
                child = subprocess.Popen([str(exe), "--background"], env=env,
                                         stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                         start_new_session=True)
            active = (child, exe, env)
            deadline = time.monotonic() + 40
            while time.monotonic() < deadline:
                before = health(status(exe, env))
                if child.poll() is not None:
                    raise RuntimeError(f"{name} exited before becoming ready")
                if before is not None and before["native_ready"] and before["device"] is not None:
                    break
                time.sleep(0.25)
            else:
                raise RuntimeError(f"{name} did not become hardware ready")
            print(f"{prefix}: mouse connected, warming up {args.warmup}s", flush=True)
            time.sleep(args.warmup)
            others = [pid for pid, parent, command in processes()
                      if pid != child.pid and (parent == child.pid or pathlib.Path(command).name in ("superlight", "superlight-ui", "Mouser"))]
            if others or visible_windows([child.pid]):
                raise RuntimeError(f"Unexpected app processes or windows: {others}")
            metrics = collect(None, output / f"{prefix}.jsonl", args.seconds, process_ids=[child.pid])
            after = health(status(exe, env))
            if after is None or not after["native_ready"] or after["device"] is None or child.poll() is not None:
                raise RuntimeError(f"{name} did not remain healthy")
            trial = {"trial": index + 1, "name": name, "metrics": metrics,
                     "health_before": before, "health_after": after}
            save(output / f"{prefix}-summary.json", trial)
            trials.append(trial)
            print(f"{prefix}: CPU {metrics['cpu_percent_one_core']:.4f}%, "
                  f"interrupts {metrics['interrupt_wakeups_per_second']:.2f}/s, "
                  f"context switches {metrics['context_switches_per_second']:.2f}/s", flush=True)
            subprocess.run([str(exe), "--quit"], env=env, capture_output=True, check=True, timeout=10)
            child.wait(timeout=10)
            active = None
            time.sleep(3)
        save(output / "comparison.json", {"trials": trials})
    finally:
        if active is not None:
            child, exe, env = active
            subprocess.run([str(exe), "--quit"], env=env, capture_output=True, timeout=10)
            child.wait(timeout=10)
        with (output / "restore.log").open("x") as log:
            subprocess.Popen([str(restore_exe), "--settings"], stdin=subprocess.DEVNULL,
                             stdout=log, stderr=log, start_new_session=True)
        time.sleep(8)
        restored = health(status(restore_exe, os.environ))
        unchanged = original_path.read_bytes() == original
        save(output / "restoration.json", {"config_unchanged": unchanged, "health": restored,
                                          "pids": app_pids(installed)})
        print(f"Installed app restored. Configuration unchanged: {unchanged}", flush=True)
        if not unchanged or restored is None or not restored["native_ready"]:
            raise RuntimeError("Restoration did not pass validation")


if __name__ == "__main__":
    main()
