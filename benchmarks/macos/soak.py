import argparse
import datetime
import hashlib
import json
import os
import pathlib
import platform
import shutil
import statistics
import subprocess
import sys
import threading
import time

from compare_openlogi import LAUNCH_AGENTS, launch_agent, remappers
from observe import processes, sample

HOME = pathlib.Path.home()
DEFAULT_APP = HOME / "Applications/SuperLight.app"
REPO = pathlib.Path(__file__).resolve().parents[2]
PHASES = ("reconnect", "sleep", "memory")
KEYS = {"reconnect": "reconnect_storm", "sleep": "sleep_wake", "memory": "long_run_memory"}
SOAK_KEYS = ("sleep_wake", "reconnect_storm", "long_run_memory", "input_latency")
MIB = 1024 * 1024


def now():
    return time.time()


def save(path, value, exclusive=True):
    with path.open("x" if exclusive else "w", encoding="utf-8") as file:
        json.dump(value, file, indent=2)
        file.write("\n")


def ask(prompt):
    return input(f"\n{prompt} ").strip()


def confirm(prompt):
    while True:
        answer = ask(f"{prompt} [y/n]").lower()
        if answer in ("y", "yes", "n", "no"):
            return answer.startswith("y")


class Service:
    def __init__(self, app):
        self.app = app
        self.executable = app / "Contents/MacOS/superlight"

    def call(self, flag, timeout=10):
        try:
            result = subprocess.run([str(self.executable), flag], capture_output=True,
                                    text=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return None, f"{flag} timed out after {timeout} s"
        if result.returncode:
            return None, (result.stderr or result.stdout).strip() or f"exit {result.returncode}"
        return result.stdout, None

    def snapshot(self):
        output, error = self.call("--status")
        if error is not None:
            return None, error
        try:
            response = json.loads(output)
        except json.JSONDecodeError as failure:
            return None, f"--status returned invalid JSON: {failure}"
        if not response.get("ok") or response.get("snapshot") is None:
            return None, response.get("error") or "--status returned no snapshot"
        return response["snapshot"], None

    def refresh(self):
        return self.call("--refresh")[1]

    def pids(self):
        prefix = str(self.app) + "/Contents/MacOS/"
        mine = os.getpid()
        return sorted(pid for pid, parent, command in processes()
                      if command.startswith(prefix) and parent != mine)

    def build(self):
        digest = hashlib.sha256()
        with self.executable.open("rb") as file:
            for block in iter(lambda: file.read(1 << 20), b""):
                digest.update(block)
        return digest.hexdigest()


def expected_settings(snapshot):
    settings = (snapshot.get("config") or {}).get("settings") or {}
    device = snapshot.get("device") or {}
    expected = {}
    if device.get("supports_dpi"):
        dpi = settings.get("dpi", 1000)
        expected["dpi"] = int(min(max(float(dpi), device.get("dpi_min", 200)), device.get("dpi_max", 8000)))
    if device.get("supports_smart_shift"):
        freespin = settings.get("smart_shift_mode") == "freespin"
        enabled = bool(settings.get("smart_shift_enabled", False))
        threshold = int(min(max(float(settings.get("smart_shift_threshold", 25)), 1), 50))
        expected["smart_shift"] = {
            "mode": "freespin" if freespin and not enabled else "ratchet",
            "enabled": enabled,
            "threshold": threshold if enabled else 25,
        }
    return expected


def state_of(snapshot):
    device = snapshot.get("device")
    permissions = snapshot.get("permissions") or {}
    return {
        "device": device is not None,
        "transport": device.get("transport") if device else None,
        "dpi": device.get("dpi") if device else None,
        "smart_shift": device.get("smart_shift") if device else None,
        "native_ready": snapshot.get("native_ready"),
        "suspended": snapshot.get("suspended"),
        "paused": snapshot.get("paused"),
        "hardware_pending": snapshot.get("hardware_pending"),
        "listen": permissions.get("listen"),
        "inject": permissions.get("inject"),
        "dropped_events": snapshot.get("dropped_events"),
        "stats": snapshot.get("stats"),
    }


def new_errors(previous, current):
    for overlap in range(min(len(previous), len(current)), 0, -1):
        if previous[-overlap:] == current[:overlap]:
            return current[overlap:]
    return list(current)


class Recorder:
    def __init__(self, service, path, interval=1.0):
        self.service = service
        self.path = path
        self.interval = interval
        self.file = path.open("x", encoding="utf-8")
        self.lock = threading.Lock()
        self.stop_event = threading.Event()
        self.state = None
        self.snapshot = None
        self.status_error = None
        self.errors = []
        self.seen = {"device": False, "suspended": False, "device_absent": False}
        self.last_wall = now()
        self.last_mono = time.monotonic()
        self.sleep_gaps = []
        self.thread = threading.Thread(target=self.loop, daemon=True)

    def write(self, kind, **fields):
        row = {"t": round(now(), 3), "kind": kind, **fields}
        with self.lock:
            self.file.write(json.dumps(row) + "\n")
            self.file.flush()

    def poll(self):
        wall, mono = now(), time.monotonic()
        gap = (wall - self.last_wall) - (mono - self.last_mono)
        if gap > 20:
            self.sleep_gaps.append({"at": wall, "seconds": round(gap, 1)})
            self.write("clock_gap", seconds=round(gap, 1))
        self.last_wall, self.last_mono = wall, mono
        snapshot, error = self.service.snapshot()
        with self.lock:
            if error is not None:
                if error != self.status_error:
                    self.status_error = error
                    self.file.write(json.dumps({"t": round(wall, 3), "kind": "status_error",
                                                "error": error}) + "\n")
                    self.file.flush()
                return
            self.status_error = None
            state = state_of(snapshot)
            changed = {key: value for key, value in state.items()
                       if self.state is None or self.state.get(key) != value}
            fresh = new_errors(self.errors, snapshot.get("errors") or [])
            self.errors = list(snapshot.get("errors") or [])
            self.state, self.snapshot = state, snapshot
            self.seen["device"] |= state["device"]
            self.seen["device_absent"] |= not state["device"]
            self.seen["suspended"] |= bool(state["suspended"])
        if changed:
            self.write("transition", changed=changed)
        if fresh:
            self.write("errors", new=fresh)

    def loop(self):
        while not self.stop_event.is_set():
            began = time.monotonic()
            self.poll()
            self.stop_event.wait(max(0, self.interval - (time.monotonic() - began)))

    def start(self):
        self.poll()
        self.thread.start()

    def stop(self):
        self.stop_event.set()
        self.thread.join(timeout=15)
        self.file.close()

    def current(self):
        with self.lock:
            return self.state, self.snapshot

    def went_away(self):
        with self.lock:
            return self.seen["device_absent"] or self.seen["suspended"] or bool(self.sleep_gaps)

    def reset_seen(self):
        with self.lock:
            for key in self.seen:
                self.seen[key] = False
            self.sleep_gaps.clear()

    def wait_for(self, predicate, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            state, _ = self.current()
            if state is not None and predicate(state):
                return True
            time.sleep(0.2)
        state, _ = self.current()
        return state is not None and predicate(state)


def ready(state):
    return state["device"] and state["native_ready"] and not state["hardware_pending"] \
        and not state["suspended"]


def stats_delta(before, after):
    if not before or not after:
        return None
    return {key: after.get(key, 0) - before.get(key, 0)
            for key in ("connects", "disconnects", "relinks", "reapplies")}


def recovery_path(delta):
    if delta is None:
        return "unknown_no_stats"
    if delta["connects"] > 0:
        return "reconnect"
    if delta["relinks"] > 0:
        return "relink"
    return "none_observed"


def readback(service, recorder):
    error = service.refresh()
    time.sleep(2)
    recorder.poll()
    _, snapshot = recorder.current()
    if snapshot is None or snapshot.get("device") is None:
        return {"ok": False, "refresh_error": error, "reason": "no device after refresh"}
    expected = expected_settings(snapshot)
    actual = {key: snapshot["device"].get(key) for key in expected}
    return {"ok": error is None and actual == expected, "refresh_error": error,
            "expected": expected, "actual": actual}


def human_checks(hold_cycle):
    button = confirm("Press two buttons you have mapped in SuperLight, for example back and "
                     "forward. Did both perform their mapped actions?")
    stuck = confirm("Is any button, modifier key or drag stuck (cursor dragging, Cmd held, menu "
                    "stuck open)?" if not hold_cycle else
                    "You held a button through the disconnect. Is any button, modifier or drag "
                    "stuck now?")
    return {"mapped_buttons_work": button, "stuck_input": stuck}


def recovery_cycle(service, recorder, label, prompt_off, prompt_on, absent_timeout, ready_timeout,
                   counter_timeout, hold_cycle=False):
    _, before_snapshot = recorder.current()
    before_stats = (before_snapshot or {}).get("stats")
    before_dropped = (before_snapshot or {}).get("dropped_events")
    recorder.reset_seen()
    recorder.write("cycle_start", cycle=label)
    ask(prompt_off)
    off_at = time.monotonic()
    went_absent = recorder.wait_for(lambda state: recorder.went_away(), absent_timeout)
    absent_after = time.monotonic() - off_at if went_absent else None
    if not went_absent:
        print(f"The device was still listed after {absent_timeout:.0f} s. This is expected on a "
              "Bolt or Unifying receiver, which stays connected while the mouse is off.")
    ask(prompt_on)
    on_at = time.monotonic()

    def counted(state):
        return before_stats is None or state["stats"] is None or any(
            state["stats"].get(key, 0) > before_stats.get(key, 0) for key in ("connects", "relinks"))

    recovered = recorder.wait_for(ready, ready_timeout)
    counter_changed = recovered and recorder.wait_for(counted, counter_timeout)
    recovered = recovered and recorder.wait_for(ready, 30)
    recovery_s = round(time.monotonic() - on_at, 2) if recovered else None
    check = readback(service, recorder) if recovered else {"ok": False, "reason": "not ready"}
    human = human_checks(hold_cycle)
    _, after_snapshot = recorder.current()
    after_stats = (after_snapshot or {}).get("stats")
    delta = stats_delta(before_stats, after_stats)
    dropped = None if after_snapshot is None or before_dropped is None else \
        after_snapshot.get("dropped_events", 0) - before_dropped
    with recorder.lock:
        seen = dict(recorder.seen)
        gaps = list(recorder.sleep_gaps)
    result = {
        "cycle": label,
        "held_button": hold_cycle,
        "device_absent_observed": seen["device_absent"],
        "suspended_observed": seen["suspended"],
        "clock_gaps": gaps,
        "absent_after_s": round(absent_after, 2) if absent_after is not None else None,
        "recovered": recovered,
        "counter_change_observed": counter_changed,
        "recovery_s": recovery_s,
        "readback": check,
        "stats_delta": delta,
        "path": recovery_path(delta),
        "dropped_events_delta": dropped,
        **human,
    }
    result["passed"] = bool(recovered and check["ok"] and human["mapped_buttons_work"]
                            and not human["stuck_input"] and dropped == 0)
    recorder.write("cycle_end", result=result)
    print(json.dumps({key: result[key] for key in
                      ("cycle", "passed", "path", "recovery_s", "dropped_events_delta")}))
    return result


def reconnect_phase(service, recorder, args):
    hold = min(args.hold_cycle, args.cycles)
    cycles = []
    for index in range(1, args.cycles + 1):
        holding = index == hold
        off = (f"[{index}/{args.cycles}] Hold the back button down and keep holding it. While "
               "holding, slide the power switch under the mouse to OFF. Release the button, then "
               "press Enter." if holding else
               f"[{index}/{args.cycles}] Slide the power switch under the mouse to OFF, then press "
               "Enter.")
        on = "Slide the power switch to ON, move the mouse a little, then press Enter."
        cycles.append(recovery_cycle(service, recorder, index, off, on, args.absent_timeout,
                                     args.ready_timeout, args.counter_timeout, hold_cycle=holding))
    passed = len(cycles) >= 10 and all(cycle["passed"] for cycle in cycles)
    return {"phase": "reconnect", "cycles": cycles, "required_cycles": 10,
            "passed_cycles": sum(cycle["passed"] for cycle in cycles), "passed": passed,
            "paths": {path: sum(cycle["path"] == path for cycle in cycles)
                      for path in {cycle["path"] for cycle in cycles}}}


def pmset_events(since):
    try:
        output = subprocess.run(["pmset", "-g", "log"], capture_output=True, text=True,
                                timeout=60).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    events = []
    for line in output.splitlines():
        parts = line.split(None, 4)
        if len(parts) < 4 or parts[3] not in ("Sleep", "Wake", "DarkWake"):
            continue
        try:
            stamp = datetime.datetime.strptime(" ".join(parts[:3]), "%Y-%m-%d %H:%M:%S %z")
        except ValueError:
            continue
        if stamp.timestamp() >= since:
            events.append(line.strip())
    return events


def caffeinate_running():
    return [row for row in processes() if pathlib.PurePath(row[2]).name == "caffeinate"]


def sleep_phase(service, recorder, args):
    began = now()
    if caffeinate_running():
        print("caffeinate is running and can block sleep. Stop it before this phase.", file=sys.stderr)
    cycles = []
    for index in range(1, args.cycles + 1):
        off = (f"[{index}/{args.cycles}] Press Enter, then within 10 seconds choose Apple menu > "
               "Sleep. Wait at least 1 minute. Wake the Mac with a key press, log in, and return "
               "to this window.")
        on = "Move the mouse a little, then press Enter."
        result = recovery_cycle(service, recorder, index, off, on, args.absent_timeout + 60,
                                args.ready_timeout, args.counter_timeout)
        slept = result["suspended_observed"] or bool(result["clock_gaps"])
        result["sleep_observed"] = slept
        result["passed"] = result["passed"] and slept
        cycles.append(result)
    events = pmset_events(began)
    passed = len(cycles) >= 3 and all(cycle["passed"] for cycle in cycles)
    return {"phase": "sleep", "cycles": cycles, "required_cycles": 3,
            "passed_cycles": sum(cycle["passed"] for cycle in cycles), "passed": passed,
            "pmset_events": events}


def slope_mib_per_hour(points):
    if len(points) < 2:
        return None
    xs = [x for x, _ in points]
    ys = [y for _, y in points]
    mean_x, mean_y = statistics.fmean(xs), statistics.fmean(ys)
    denominator = sum((x - mean_x) ** 2 for x in xs)
    if denominator == 0:
        return None
    return sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys)) / denominator / MIB


def memory_phase(service, recorder, args):
    caffeinate = None
    if not args.no_caffeinate:
        caffeinate = subprocess.Popen(["caffeinate", "-dims", "-w", str(os.getpid())])
    seconds = args.hours * 3600
    began = time.monotonic()
    rows = []
    pid_sets = []
    try:
        while True:
            elapsed = time.monotonic() - began
            pids = service.pids()
            measured = []
            for pid in pids:
                try:
                    measured.append(sample(pid))
                except OSError:
                    continue
            if pids != (pid_sets[-1] if pid_sets else None):
                pid_sets.append(list(pids))
                recorder.write("pids", pids=pids)
            row = {"elapsed_s": round(elapsed, 2), "processes": measured,
                   "load_average": os.getloadavg()}
            rows.append(row)
            recorder.write("sample", **row)
            if elapsed >= seconds:
                break
            time.sleep(max(0, began + len(rows) * args.sample_interval - time.monotonic()))
    except KeyboardInterrupt:
        recorder.write("interrupted")
    finally:
        if caffeinate is not None:
            caffeinate.terminate()
    return summarize_memory(rows, pid_sets, recorder, args)


def summarize_memory(rows, pid_sets, recorder, args):
    rows = [row for row in rows if row["processes"]]
    if not rows:
        return {"phase": "memory", "passed": False, "reason": "no samples"}
    totals = [(row["elapsed_s"], sum(p["footprint_bytes"] for p in row["processes"]),
               sum(p["rss_bytes"] for p in row["processes"]),
               sum(p["threads"] for p in row["processes"])) for row in rows]
    bucket = args.bucket_minutes * 60
    buckets = {}
    for elapsed, footprint, rss, threads in totals:
        buckets.setdefault(int(elapsed // bucket), []).append((footprint, rss, threads))
    bucket_rows = [{"start_min": key * args.bucket_minutes,
                    "footprint_mib_median": statistics.median(v[0] for v in values) / MIB,
                    "rss_mib_median": statistics.median(v[1] for v in values) / MIB,
                    "threads_max": max(v[2] for v in values)}
                   for key, values in sorted(buckets.items())]
    hours = totals[-1][0] / 3600
    first, last = rows[0], rows[-1]
    stable = len(pid_sets) == 1
    cpu = None
    wakeups = None
    if stable and totals[-1][0] > 0:
        start = {p["pid"]: p for p in first["processes"]}
        end = {p["pid"]: p for p in last["processes"]}
        if start.keys() == end.keys():
            span = totals[-1][0] - totals[0][0]
            ns = sum(end[p]["user_ns"] + end[p]["system_ns"] - start[p]["user_ns"] - start[p]["system_ns"]
                     for p in start)
            cpu = ns / span / 1e7 if span > 0 else None
            wakeups = {key: sum(end[p][key] - start[p][key] for p in start) / span
                       for key in ("idle_wakeups", "interrupt_wakeups")} if span > 0 else None
    growth = bucket_rows[-1]["footprint_mib_median"] - bucket_rows[0]["footprint_mib_median"]
    state, _ = recorder.current()
    completed = hours >= args.hours * 0.99
    passed = bool(completed and args.hours >= 0.5 and stable and growth <= args.max_growth_mib
                  and state is not None and state["device"])
    return {
        "phase": "memory",
        "requested_hours": args.hours,
        "measured_hours": round(hours, 3),
        "completed": completed,
        "samples": len(rows),
        "sample_interval_s": args.sample_interval,
        "stable_process_set": stable,
        "pid_sets": pid_sets,
        "footprint_mib": {"first": totals[0][1] / MIB, "last": totals[-1][1] / MIB,
                          "max": max(t[1] for t in totals) / MIB,
                          "median": statistics.median(t[1] for t in totals) / MIB},
        "rss_mib": {"first": totals[0][2] / MIB, "last": totals[-1][2] / MIB,
                    "max": max(t[2] for t in totals) / MIB},
        "threads": {"first": totals[0][3], "last": totals[-1][3], "max": max(t[3] for t in totals)},
        "footprint_slope_mib_per_hour": slope_mib_per_hour([(t[0] / 3600, t[1]) for t in totals]),
        "footprint_growth_first_to_last_bucket_mib": growth,
        "max_growth_mib": args.max_growth_mib,
        "cpu_percent_one_core": cpu,
        "wakeups_per_second": wakeups,
        "buckets": bucket_rows,
        "device_present_at_end": bool(state and state["device"]),
        "passed": passed,
    }


def check(service):
    items = []

    def add(name, ok, detail, required=True):
        items.append({"name": name, "ok": bool(ok), "required": required, "detail": detail})

    pids = service.pids() if service.executable.exists() else []
    add("superlight_running", pids, {"app": str(service.app), "pids": pids})
    snapshot, error = service.snapshot() if pids else (None, "not running")
    add("status_reachable", snapshot is not None, error or "ok")
    permissions = (snapshot or {}).get("permissions") or {}
    add("permissions", permissions.get("listen") and permissions.get("inject"),
        {"listen": permissions.get("listen"), "inject": permissions.get("inject")})
    add("native_ready", (snapshot or {}).get("native_ready"), (snapshot or {}).get("native_ready"))
    device = (snapshot or {}).get("device")
    add("device_connected", device is not None,
        None if device is None else {key: device.get(key) for key in ("name", "transport", "dpi", "battery")})
    add("stats_available", snapshot is not None and "stats" in snapshot,
        (snapshot or {}).get("stats") if snapshot and "stats" in snapshot else
        "this build predates the stats counters, so relink and reconnect cannot be told apart")
    others = {kind: rows for kind, rows in remappers().items() if kind != "superlight"}
    add("no_other_remapper", not others, others)
    mouser = launch_agent(LAUNCH_AGENTS["mouser"])
    add("no_mouser_launch_agent", not mouser["exists"], mouser)
    options = launch_agent(LAUNCH_AGENTS["logi_options_plus"])
    add("logi_options_plus_launch_agent", True, options, required=False)
    add("caffeinate_not_running", not caffeinate_running(),
        [row[0] for row in caffeinate_running()], required=False)
    try:
        power = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True,
                               timeout=10).stdout.splitlines()[:1]
    except (OSError, subprocess.TimeoutExpired):
        power = None
    add("power_source", True, power, required=False)
    ok = all(item["ok"] for item in items if item["required"])
    return {"ok": ok, "os": os_label(), "architecture": platform.machine(), "items": items}


def os_label():
    try:
        version = subprocess.run(["sw_vers", "-productVersion"], capture_output=True, text=True,
                                 timeout=10).stdout.strip()
    except OSError:
        version = platform.mac_ver()[0]
    return f"macOS {version}"


def combine(output, model):
    phases = {}
    for phase in PHASES:
        path = output / phase / "summary.json"
        if path.exists():
            phases[phase] = json.loads(path.read_text(encoding="utf-8"))
    manifests = [json.loads((output / phase / "manifest.json").read_text(encoding="utf-8"))
                 for phase in phases if (output / phase / "manifest.json").exists()]
    builds = sorted({manifest["build"] for manifest in manifests})
    transports = sorted({manifest["transport"] for manifest in manifests if manifest.get("transport")})
    checked = ["discovery"] if any(manifest.get("device_seen") for manifest in manifests) else []
    checked += [KEYS[phase] for phase, summary in phases.items() if summary.get("passed")]
    checked = [key for key in ["discovery", *SOAK_KEYS] if key in checked]
    not_checked = [key for key in SOAK_KEYS if key not in checked]
    date = datetime.date.today().isoformat()
    transport_slug = "-".join(t.lower() for t in transports) or "unknown"
    evidence_file = f"compatibility/evidence/{date}-{model.replace('_', '-')}-{transport_slug}-soak.json"
    parts = []
    if "reconnect" in phases:
        summary = phases["reconnect"]
        parts.append(f"reconnect storm {summary['passed_cycles']}/{len(summary['cycles'])} cycles "
                     f"passed, paths {summary['paths']}")
    if "sleep" in phases:
        summary = phases["sleep"]
        parts.append(f"sleep/wake {summary['passed_cycles']}/{len(summary['cycles'])} cycles passed")
    if "memory" in phases:
        summary = phases["memory"]
        if "footprint_mib" in summary:
            parts.append(f"memory soak {summary['measured_hours']} h, footprint "
                         f"{summary['footprint_mib']['first']:.2f} to {summary['footprint_mib']['last']:.2f} MiB, "
                         f"slope {summary['footprint_slope_mib_per_hour'] or 0:.3f} MiB/h")
    entry = {
        "os": manifests[0]["os"] if manifests else os_label(),
        "architecture": manifests[0]["architecture"] if manifests else platform.machine(),
        "transport": transports[0] if len(transports) == 1 else ", ".join(transports) or "unknown",
        "date": date,
        "build": builds[0] if len(builds) == 1 else ", ".join(builds),
        "checked": checked,
        "not_checked": not_checked,
        "evidence": "benchmarks/macos/soak.py: " + "; ".join(parts) + f". Raw summary: {evidence_file}. "
                    "input_latency has no instrument.",
    }
    summary = {"generated_at": now(), "model": model, "phases": phases, "manifests": manifests,
               "single_build": len(builds) == 1, "evidence_file": evidence_file, "entry": entry}
    save(output / "summary.json", summary, exclusive=False)
    save(output / "mice-entry.json", entry, exclusive=False)
    return entry


def record(output):
    summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
    entry = summary["entry"]
    evidence = REPO / summary["evidence_file"]
    if evidence.exists():
        raise SystemExit(f"{evidence} already exists")
    if not summary["single_build"]:
        raise SystemExit("Phases ran on different builds. Rerun them on one build before recording.")
    mice_path = REPO / "compatibility/mice.json"
    mice = json.loads(mice_path.read_text(encoding="utf-8"))
    model = next(model for model in mice["models"] if model["model"] == summary["model"])
    model["physical_verification"].append(entry)
    evidence.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(output / "summary.json", evidence)
    mice_path.write_text(json.dumps(mice, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"evidence": str(evidence), "mice_json": str(mice_path), "entry": entry}, indent=2))


def run_phase(args, service):
    report = check(service)
    if not report["ok"]:
        failed = [item["name"] for item in report["items"] if item["required"] and not item["ok"]]
        allowed = {"stats_available"} if args.allow_no_stats else set()
        if set(failed) - allowed:
            print(json.dumps(report, indent=2))
            raise SystemExit(f"Prerequisites failed: {', '.join(failed)}. Run --check for details.")
    directory = args.output / args.phase
    directory.mkdir(parents=True, exist_ok=False)
    recorder = Recorder(service, directory / "soak.jsonl", args.status_interval)
    recorder.start()
    state, snapshot = recorder.current()
    manifest = {"phase": args.phase, "started_at": now(), "app": str(service.app),
                "build": service.build(), "os": report["os"], "architecture": report["architecture"],
                "transport": state["transport"] if state else None,
                "device": ((snapshot or {}).get("device") or {}).get("name"),
                "device_seen": bool(state and state["device"]), "check": report,
                "arguments": {key: str(value) for key, value in vars(args).items()}}
    save(directory / "manifest.json", manifest)
    phase = {"reconnect": reconnect_phase, "sleep": sleep_phase, "memory": memory_phase}[args.phase]
    try:
        summary = phase(service, recorder, args)
    except KeyboardInterrupt:
        summary = {"phase": args.phase, "passed": False, "reason": "interrupted"}
    finally:
        recorder.stop()
    events = [json.loads(line) for line in (directory / "soak.jsonl").read_text(encoding="utf-8").splitlines()]
    summary["new_errors"] = [error for event in events if event["kind"] == "errors" for error in event["new"]]
    summary["status_errors"] = [event["error"] for event in events if event["kind"] == "status_error"]
    summary["final_state"] = recorder.state
    save(directory / "summary.json", summary)
    entry = combine(args.output, args.model)
    print(json.dumps({"phase": args.phase, "passed": summary.get("passed"),
                      "output": str(directory), "mice_entry": entry}, indent=2))
    return 0 if summary.get("passed") else 1


def main():
    parser = argparse.ArgumentParser(description="SuperLight hardware soak, reconnect and sleep/wake runs.")
    parser.add_argument("phase", choices=("check", *PHASES, "record"))
    parser.add_argument("--app", type=pathlib.Path, default=DEFAULT_APP)
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--cycles", type=int)
    parser.add_argument("--hold-cycle", type=int, default=5)
    parser.add_argument("--hours", type=float, default=4)
    parser.add_argument("--sample-interval", type=float, default=10)
    parser.add_argument("--bucket-minutes", type=int, default=10)
    parser.add_argument("--max-growth-mib", type=float, default=5)
    parser.add_argument("--absent-timeout", type=float, default=15)
    parser.add_argument("--counter-timeout", type=float, default=20)
    parser.add_argument("--ready-timeout", type=float, default=90)
    parser.add_argument("--status-interval", type=float, default=1)
    parser.add_argument("--model", default="mx_master_3s")
    parser.add_argument("--no-caffeinate", action="store_true")
    parser.add_argument("--allow-no-stats", action="store_true")
    args = parser.parse_args()
    service = Service(args.app.resolve())
    if args.phase == "check":
        report = check(service)
        print(json.dumps(report, indent=2))
        return 0 if report["ok"] else 1
    if args.output is None:
        parser.error("--output is required")
    args.output = args.output.resolve()
    if args.phase == "record":
        record(args.output)
        return 0
    if args.cycles is None:
        args.cycles = 10 if args.phase == "reconnect" else 3
    return run_phase(args, service)


if __name__ == "__main__":
    sys.exit(main())
