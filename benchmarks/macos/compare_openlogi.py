import argparse
import hashlib
import json
import os
import pathlib
import plistlib
import signal
import statistics
import subprocess
import time

from observe import TIMEBASE, bundle_pids, collect, processes, sample

ORDER = ["openlogi", "superlight", "superlight", "openlogi", "openlogi", "superlight"]
HOME = pathlib.Path.home()
OPENLOGI_CONFIG = HOME / ".config/openlogi/config.toml"
SUPERLIGHT_CONFIG = HOME / "Library/Application Support/SuperLight/config.json"
LAUNCH_AGENTS = {
    "mouser": HOME / "Library/LaunchAgents/io.github.tombadash.mouser.plist",
    "superlight": HOME / "Library/LaunchAgents/io.github.vincenthopf.SuperLight.plist",
    "logi_options_plus": pathlib.Path("/Library/LaunchAgents/com.logi.optionsplus.plist"),
}
OPENLOGI_SERVICE = f"gui/{os.getuid()}/org.openlogi.agent.service"


class Stopped(Exception):
    pass


def save(path, value):
    with path.open("x", encoding="utf-8") as file:
        json.dump(value, file, indent=2)
        file.write("\n")


def remapper_kind(command):
    path = pathlib.PurePath(command)
    lower = command.lower()
    if "/superlight.app/" in lower or path.name == "superlight":
        return "superlight"
    if "/openlogi.app/" in lower or path.name.startswith("openlogi-"):
        return None if path.name == "openlogi" else "openlogi"
    if "mouser" in lower:
        return "mouser"
    if "logioptionsplus" in lower:
        return "logi_options_plus"
    return None


def remappers():
    found = {}
    for pid, _, command in processes():
        kind = remapper_kind(command)
        if kind is not None and pid != os.getpid():
            found.setdefault(kind, []).append({"pid": pid, "command": command})
    return found


def conflicts(target):
    return {kind: rows for kind, rows in remappers().items() if kind != target}


def commands(pids):
    rows = {pid: command for pid, _, command in processes()}
    return {str(pid): rows.get(pid) for pid in pids}


def bundle_kib(bundle):
    return int(subprocess.check_output(["du", "-sk", str(bundle)], text=True).split()[0])


def bundle_version(bundle):
    with (bundle / "Contents/Info.plist").open("rb") as file:
        info = plistlib.load(file)
    return info.get("CFBundleShortVersionString"), info.get("CFBundleVersion")


def launch_agent(path):
    if not path.exists():
        return {"path": str(path), "exists": False}
    try:
        with path.open("rb") as file:
            plist = plistlib.load(file)
    except (OSError, plistlib.InvalidFileException) as error:
        return {"path": str(path), "exists": True, "readable": False, "error": str(error)}
    program = plist.get("Program") or (plist.get("ProgramArguments") or [None])[0]
    return {"path": str(path), "exists": True, "readable": True, "label": plist.get("Label"),
            "run_at_load": plist.get("RunAtLoad"), "program": program,
            "program_exists": program is not None and pathlib.Path(program).exists()}


def wait_until(predicate, timeout, interval=0.3):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    return predicate()


def terminate(pids):
    for pid in pids:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue


class SuperLight:
    name = "superlight"

    def __init__(self, bundle, config_dir=None):
        self.bundle = bundle
        self.executable = bundle / "Contents/MacOS/superlight"
        self.env = dict(os.environ)
        if config_dir is not None:
            self.env["SUPERLIGHT_CONFIG_DIR"] = str(config_dir)
        self.child = None

    def pids(self, _bundle=None):
        return bundle_pids(self.bundle)

    def snapshot(self):
        try:
            result = subprocess.run([str(self.executable), "--status"], env=self.env,
                                    capture_output=True, text=True, timeout=5)
        except subprocess.TimeoutExpired:
            return None
        if result.returncode:
            return None
        return json.loads(result.stdout)["snapshot"]

    def health(self):
        snapshot = self.snapshot()
        if snapshot is None:
            return None
        return {key: snapshot.get(key) for key in
                ("native_ready", "paused", "suspended", "permissions", "device", "dropped_events", "errors")}

    def ready(self):
        health = self.health()
        return health if health and health["native_ready"] and health["device"] is not None else None

    def device_absent(self):
        health = self.health()
        return health is not None and health["device"] is None

    def start(self, log_path, flag="--background"):
        with log_path.open("x") as log:
            self.child = subprocess.Popen([str(self.executable), flag], env=self.env,
                                          stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                          start_new_session=True)

    def after_ready(self, background):
        return None

    def stop(self):
        subprocess.run([str(self.executable), "--quit"], env=self.env, capture_output=True, timeout=10)
        if not wait_until(lambda: not self.pids(), 5):
            terminate(self.pids())
        if not wait_until(lambda: not self.pids(), 10):
            raise RuntimeError(f"SuperLight did not stop: {self.pids()}")
        if self.child is not None:
            self.child.wait(timeout=5)
            self.child = None


class OpenLogi:
    name = "openlogi"

    def __init__(self, bundle, device_name, start_mode):
        self.bundle = bundle
        self.cli = bundle / "Contents/MacOS/openlogi"
        self.device_name = device_name.lower()
        self.start_mode = start_mode

    def pids(self, _bundle=None):
        return bundle_pids(self.bundle, exclude={"openlogi"})

    def gui_pids(self):
        prefix = str(self.bundle.resolve()) + "/Contents/MacOS/openlogi-desktop"
        return [pid for pid, _, command in processes() if command == prefix]

    def api(self, *arguments):
        try:
            result = subprocess.run([str(self.cli), "api", *arguments], capture_output=True,
                                    text=True, timeout=5)
        except subprocess.TimeoutExpired:
            return None
        try:
            reply = json.loads(result.stdout)
        except json.JSONDecodeError:
            return None
        return reply.get("data") if reply.get("ok") else None

    def api_supported(self):
        result = subprocess.run([str(self.cli), "api", "status"], capture_output=True, text=True, timeout=10)
        try:
            reply = json.loads(result.stdout)
        except json.JSONDecodeError:
            return False, result.returncode, result.stderr.strip()[-400:]
        return reply.get("schema_version") == 1, result.returncode, reply

    def devices(self):
        data = self.api("devices")
        if data is None:
            return None
        return [device for device in data.get("devices", [])
                if device.get("online") and self.device_name in (device.get("name") or "").lower()]

    def health(self):
        status = self.api("status")
        if status is None:
            return None
        return {"status": status, "devices": self.devices()}

    def ready(self):
        health = self.health()
        if health is None:
            return None
        status = health["status"]
        if status.get("hook_installed") and status.get("inventory") == "ready" and health["devices"]:
            return health
        return None

    def device_absent(self):
        devices = self.devices()
        return devices is not None and not devices

    def start(self, log_path, flag=None):
        if self.start_mode == "kickstart":
            command = ["launchctl", "kickstart", OPENLOGI_SERVICE]
        else:
            command = ["open", "-g", str(self.bundle)]
        with log_path.open("x") as log:
            log.write(" ".join(command) + "\n")
            log.flush()
            subprocess.run(command, stdout=log, stderr=log, check=True, timeout=15)

    def after_ready(self, background):
        if not background or not self.gui_pids():
            return None
        terminate(self.gui_pids())
        if not wait_until(lambda: not self.gui_pids(), 10):
            raise RuntimeError("OpenLogi settings window process did not exit")
        time.sleep(2)
        health = wait_until(self.ready, 15)
        if health is None:
            raise RuntimeError("OpenLogi agent lost readiness after its settings process exited")
        return health

    def service_state(self):
        result = subprocess.run(["launchctl", "print", OPENLOGI_SERVICE], capture_output=True, text=True)
        lines = [line.strip() for line in result.stdout.splitlines()
                 if line.strip().startswith(("state =", "pid =", "job state ="))]
        return {"returncode": result.returncode, "lines": lines}

    def stop(self):
        terminate(self.pids())
        if not wait_until(lambda: not self.pids(), 10):
            raise RuntimeError(f"OpenLogi did not stop: {commands(self.pids())}")
        time.sleep(3)
        if self.pids():
            raise RuntimeError(f"OpenLogi restarted after SIGTERM: {commands(self.pids())}")


def calibrate():
    before = sample(os.getpid())
    start = time.process_time_ns()
    while time.process_time_ns() - start < 100_000_000:
        pass
    elapsed = time.process_time_ns() - start
    after = sample(os.getpid())
    rusage = sum(after[key] - before[key] for key in ("user_ns", "system_ns"))
    calibration = {"process_time_ns": elapsed, "rusage_ns": rusage, "ratio": rusage / elapsed,
                   "numer": TIMEBASE.numer, "denom": TIMEBASE.denom}
    if not 0.98 < calibration["ratio"] < 1.02:
        raise RuntimeError(f"CPU counter calibration failed: {calibration}")
    return calibration


def inventory(bundles):
    return {
        "remappers": remappers(),
        "conflicts_when_measuring": {name: conflicts(name) for name in bundles},
        "bundle_pids": {name: commands(bundle_pids(bundle, exclude={"openlogi"}))
                        for name, bundle in bundles.items()},
        "bundle_kib": {name: bundle_kib(bundle) for name, bundle in bundles.items()},
        "versions": {name: bundle_version(bundle) for name, bundle in bundles.items()},
        "launch_agents": {name: launch_agent(path) for name, path in LAUNCH_AGENTS.items()},
        "openlogi_config_sha256": hashlib.sha256(OPENLOGI_CONFIG.read_bytes()).hexdigest()
        if OPENLOGI_CONFIG.exists() else None,
    }


def prompt(text):
    input(f"\n>>> {text} Press Enter to continue. ")


def run_trial(app, index, output, args):
    prefix = f"{index:02}-{app.name}"
    blocking = conflicts(app.name)
    if blocking:
        raise RuntimeError(f"Another remapper is running before {prefix}: {blocking}")
    if app.pids():
        raise RuntimeError(f"{app.name} is already running before {prefix}: {commands(app.pids())}")
    if args.power_cycle:
        prompt("Switch the mouse off, wait 3 seconds, switch it on, wait until macOS reconnects it.")
    began = time.monotonic()
    app.start(output / f"{prefix}-process.log", "--background" if args.background else "--settings")
    before = wait_until(app.ready, args.ready_timeout)
    launch_to_ready = time.monotonic() - began
    if before is None:
        raise RuntimeError(f"{app.name} did not become ready within {args.ready_timeout}s: {app.health()}")
    after_gui = app.after_ready(args.background)
    print(f"{prefix}: ready in {launch_to_ready:.2f}s. Warming up for {args.warmup}s", flush=True)
    time.sleep(args.warmup)
    blocking = conflicts(app.name)
    if blocking:
        raise RuntimeError(f"Another remapper started during {prefix}: {blocking}")
    measured = app.pids()
    processes_before = commands(measured)
    metrics = collect(app.bundle, output / f"{prefix}.jsonl", args.seconds, finder=app.pids)
    if not metrics["stable_process_set"]:
        raise RuntimeError(f"{app.name} process set changed during measurement")
    after = app.ready()
    if after is None:
        raise RuntimeError(f"{app.name} lost readiness during measurement")
    trial = {"name": app.name, "trial": index, "launch_to_ready_s": launch_to_ready,
             "processes": processes_before, "metrics": metrics, "health_before": before,
             "health_after_gui_exit": after_gui, "health_after": after}
    save(output / f"{prefix}-summary.json", trial)
    print(f"{prefix}: CPU {metrics['cpu_percent_one_core']:.4f}%, "
          f"footprint {metrics['footprint_bytes']['median'] / 1048576:.2f} MiB, "
          f"{len(measured)} processes", flush=True)
    return trial


def reconnect(app, output, args):
    app.start(output / f"reconnect-{app.name}-process.log", "--background" if args.background else "--settings")
    if wait_until(app.ready, args.ready_timeout) is None:
        raise RuntimeError(f"{app.name} did not become ready for the reconnect step: {app.health()}")
    app.after_ready(args.background)
    prompt(f"[{app.name}] Switch the mouse OFF now.")
    if not wait_until(app.device_absent, 30):
        raise RuntimeError(f"{app.name} still reports the mouse 30s after it was switched off")
    input(f"\n>>> [{app.name}] Press Enter at the same moment you switch the mouse ON. ")
    began = time.monotonic()
    health = wait_until(app.ready, args.ready_timeout, interval=0.2)
    seconds = time.monotonic() - began if health is not None else None
    print(f"reconnect-{app.name}: {'ready after ' + format(seconds, '.2f') + 's' if seconds else 'not ready'}",
          flush=True)
    return {"name": app.name, "enter_to_ready_s": seconds, "health": health}


def median_of(trials, extract):
    values = [extract(trial) for trial in trials]
    return {"median": statistics.median(values), "min": min(values), "max": max(values), "values": values}


def summarize(trials, sizes):
    summary = {}
    for name in ("openlogi", "superlight"):
        rows = [trial for trial in trials if trial["name"] == name]
        if not rows:
            continue
        summary[name] = {
            "trials": len(rows),
            "bundle_kib": sizes[name],
            "launch_to_ready_s": median_of(rows, lambda t: t["launch_to_ready_s"]),
            "cpu_percent_one_core": median_of(rows, lambda t: t["metrics"]["cpu_percent_one_core"]),
            "footprint_mib": median_of(rows, lambda t: t["metrics"]["footprint_bytes"]["median"] / 1048576),
            "rss_mib": median_of(rows, lambda t: t["metrics"]["rss_bytes"]["median"] / 1048576),
            "threads": median_of(rows, lambda t: t["metrics"]["threads"]["median"]),
            "idle_wakeups_per_second": median_of(rows, lambda t: t["metrics"]["idle_wakeups_per_second"]),
            "interrupt_wakeups_per_second": median_of(rows, lambda t: t["metrics"]["interrupt_wakeups_per_second"]),
            "context_switches_per_second": median_of(rows, lambda t: t["metrics"]["context_switches_per_second"]),
            "process_count": median_of(rows, lambda t: len(t["processes"])),
        }
    return summary


def summary_text(summary, reconnects, restoration, mode):
    keys = [("bundle_kib", "Bundle size, KiB", None), ("launch_to_ready_s", "Launch to ready, s", "{:.2f}"),
            ("cpu_percent_one_core", "CPU, % of one core", "{:.3f}"), ("footprint_mib", "Footprint, MiB", "{:.2f}"),
            ("rss_mib", "Resident memory, MiB", "{:.2f}"), ("idle_wakeups_per_second", "Idle wakeups/s", "{:.2f}"),
            ("interrupt_wakeups_per_second", "Interrupt wakeups/s", "{:.2f}"),
            ("context_switches_per_second", "Context switches/s", "{:.1f}"), ("threads", "Threads", "{:.0f}"),
            ("process_count", "Processes", "{:.0f}")]
    names = [name for name in ("openlogi", "superlight") if name in summary]
    lines = ["# OpenLogi vs SuperLight idle comparison", "",
             f"Medians across trials, {mode} mode, one remapper at a time.", "",
             "| Metric | " + " | ".join(names) + " |", "| --- |" + " ---: |" * len(names)]
    for key, label, fmt in keys:
        cells = []
        for name in names:
            value = summary[name][key]
            cells.append(str(value) if fmt is None else fmt.format(value["median"]))
        lines.append(f"| {label} | " + " | ".join(cells) + " |")
    for row in reconnects:
        lines.append(f"\nReconnect, {row['name']}: Enter to ready "
                     f"{'not ready' if row['enter_to_ready_s'] is None else format(row['enter_to_ready_s'], '.2f') + ' s'}.")
    lines.append(f"\nOpenLogi config.toml changed: {restoration.get('openlogi_config_changed')}.")
    lines.append(f"SuperLight config.json changed: {restoration.get('superlight_config_changed')}.")
    lines.append(f"SuperLight restored ready: {restoration.get('superlight_ready')}.")
    return "\n".join(lines) + "\n"


def raise_stopped(signum, _frame):
    raise Stopped(f"signal {signum}")


def main():
    parser = argparse.ArgumentParser(
        description="Compare idle cost of OpenLogi and SuperLight. One remapper runs at a time. "
                    "Order O,S,S,O,O,S. Installed SuperLight is restored at the end.")
    parser.add_argument("--openlogi", type=pathlib.Path, default=pathlib.Path("/Applications/OpenLogi.app"))
    parser.add_argument("--superlight", type=pathlib.Path, default=HOME / "Applications/SuperLight.app")
    parser.add_argument("--output", type=pathlib.Path, help="new directory for results")
    parser.add_argument("--warmup", type=float, default=30)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--ready-timeout", type=float, default=60)
    parser.add_argument("--device-name", default="MX Master", help="substring of the device name to wait for")
    parser.add_argument("--openlogi-start", choices=("gui", "kickstart"), default="gui",
                        help="gui opens OpenLogi.app with open -g, kickstart starts the launchd agent only")
    parser.add_argument("--foreground", action="store_true",
                        help="keep the OpenLogi settings process running and open SuperLight settings")
    parser.add_argument("--power-cycle", action="store_true", help="prompt for a mouse power cycle before each trial")
    parser.add_argument("--reconnect", action="store_true", help="after the trials, time recovery after a mouse power cycle")
    parser.add_argument("--check", action="store_true", help="print discovery and conflict state, change nothing")
    args = parser.parse_args()
    args.background = not args.foreground
    bundles = {"openlogi": args.openlogi.resolve(), "superlight": args.superlight.resolve()}
    if args.check:
        print(json.dumps(inventory(bundles), indent=2))
        return
    if args.output is None:
        parser.error("--output is required unless --check is given")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    start_state = inventory(bundles)
    blocking = {kind: rows for kind, rows in start_state["remappers"].items() if kind != "superlight"}
    if blocking:
        raise SystemExit(f"Quit these before running: {json.dumps(blocking)}")
    openlogi = OpenLogi(bundles["openlogi"], args.device_name, args.openlogi_start)
    supported, code, reply = openlogi.api_supported()
    if not supported:
        raise SystemExit(f"OpenLogi CLI has no usable 'api status' (exit {code}): {reply}. "
                         "Upgrade OpenLogi to 0.8.13 or later.")
    backups = output / "private-backups"
    backups.mkdir(mode=0o700)
    originals = {}
    for name, path in (("openlogi", OPENLOGI_CONFIG), ("superlight", SUPERLIGHT_CONFIG)):
        if path.exists():
            originals[name] = path.read_bytes()
            (backups / path.name).write_bytes(originals[name])
    if "superlight" not in originals:
        raise SystemExit(f"No SuperLight configuration at {SUPERLIGHT_CONFIG}")
    config = json.loads(originals["superlight"])
    config["settings"]["start_at_login"] = False
    config["settings"]["start_minimized"] = False
    config_dir = output / "superlight-config"
    config_dir.mkdir()
    save(config_dir / "config.json", config)
    superlight = SuperLight(bundles["superlight"], config_dir)
    installed = SuperLight(bundles["superlight"])
    running = [row["command"] for row in start_state["remappers"].get("superlight", [])]
    restore_flag = "--background"
    for pid in installed.pids():
        out = subprocess.run(["ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True).stdout
        if "--settings" in out:
            restore_flag = "--settings"
    save(output / "manifest.json", {
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "os": subprocess.check_output(["sw_vers"], text=True),
        "cpu": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
        "memory_bytes": int(subprocess.check_output(["sysctl", "-n", "hw.memsize"])),
        "power": subprocess.check_output(["pmset", "-g", "batt"], text=True),
        "warmup_s": args.warmup, "sample_s": args.seconds, "order": ORDER,
        "mode": "background" if args.background else "foreground", "openlogi_start": args.openlogi_start,
        "device_name": args.device_name, "power_cycle_between_trials": args.power_cycle,
        "workload": "idle, physical Logitech connected, no scripted input",
        "cpu_calibration": calibrate(), "start_state": start_state, "superlight_running_at_start": running,
        "restore_flag": restore_flag, "openlogi_api_status_at_start": reply,
        "executables_sha256": {
            "superlight": hashlib.sha256(superlight.executable.read_bytes()).hexdigest(),
            "openlogi_agent": hashlib.sha256(
                (bundles["openlogi"] / "Contents/Library/LoginItems/OpenLogi Agent.app/Contents/MacOS/openlogi-agent")
                .read_bytes()).hexdigest()},
        "config_sha256": {name: hashlib.sha256(content).hexdigest() for name, content in originals.items()},
    })
    apps = {"openlogi": openlogi, "superlight": superlight}
    trials, reconnects, active = [], [], None
    previous = {sig: signal.signal(sig, raise_stopped) for sig in (signal.SIGTERM, signal.SIGHUP)}
    failure = None
    try:
        installed.stop()
        for index, name in enumerate(ORDER, start=1):
            active = apps[name]
            trials.append(run_trial(active, index, output, args))
            active.stop()
            active = None
            time.sleep(3)
        if args.reconnect:
            for name in ("openlogi", "superlight"):
                active = apps[name]
                reconnects.append(reconnect(active, output, args))
                active.stop()
                active = None
                time.sleep(3)
    except (KeyboardInterrupt, Exception) as error:
        failure = f"{type(error).__name__}: {error}"
        print(f"Stopping early: {failure}", flush=True)
    finally:
        cleanup = []
        for app in ([active] if active is not None else []) + [openlogi, superlight]:
            try:
                if app.pids():
                    app.stop()
            except Exception as error:
                cleanup.append(f"{app.name}: {error}")
        restoration = {"failure": failure, "cleanup_errors": cleanup, "restore_flag": restore_flag,
                       "openlogi_service": openlogi.service_state(), "openlogi_pids": commands(openlogi.pids())}
        for name, path in (("openlogi", OPENLOGI_CONFIG), ("superlight", SUPERLIGHT_CONFIG)):
            if name in originals:
                restoration[f"{name}_config_changed"] = (not path.exists()) or path.read_bytes() != originals[name]
        installed.start(output / "restore.log", restore_flag)
        health = wait_until(installed.ready, 30)
        restoration.update({"superlight_ready": health is not None, "superlight_health": health or installed.health(),
                            "superlight_pids": commands(installed.pids())})
        save(output / "restoration.json", restoration)
        sizes = start_state["bundle_kib"]
        summary = summarize(trials, sizes)
        save(output / "comparison.json", {"complete": failure is None and len(trials) == len(ORDER),
                                          "trials": trials, "reconnect": reconnects, "summary": summary})
        text = summary_text(summary, reconnects, restoration, "background" if args.background else "foreground")
        (output / "summary.md").write_text(text)
        for sig, handler in previous.items():
            signal.signal(sig, handler)
        print(text, flush=True)
        print(f"Results: {output}", flush=True)
    problems = [failure] if failure else []
    problems += restoration["cleanup_errors"]
    if not restoration["superlight_ready"]:
        problems.append("SuperLight did not become ready after restore")
    if any(restoration.get(f"{name}_config_changed") for name in originals):
        problems.append("A configuration file changed. Backups are in private-backups")
    if problems:
        raise SystemExit("; ".join(problems))


if __name__ == "__main__":
    main()
