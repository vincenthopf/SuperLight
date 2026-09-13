import argparse
import json
import os
import pathlib
import subprocess
import time

from AppKit import NSRunningApplication

from compare import health, save, status, stop, visible_windows
from observe import app_pids, collect, processes, summarize


def window(pid, hidden, resize=False):
    app = NSRunningApplication.runningApplicationWithProcessIdentifier_(pid)
    if app is None:
        raise RuntimeError("GUI exited")
    if hidden:
        app.hide()
    else:
        app.unhide()
        app.activateWithOptions_(3)
    deadline = time.monotonic() + 5
    while bool(visible_windows([pid])) == hidden and time.monotonic() < deadline:
        time.sleep(.1)
    if bool(visible_windows([pid])) == hidden:
        raise RuntimeError("GUI did not reach requested visibility")
    if resize:
        script = f'tell application "System Events" to tell (first process whose unix id is {pid})\nset position of window 1 to {{80, 80}}\nset size of window 1 to {{1120, 790}}\nend tell'
        result = subprocess.run(["osascript", "-e", script], capture_output=True, text=True, timeout=10)
        if result.returncode:
            raise RuntimeError(result.stderr)


def main():
    parser = argparse.ArgumentParser()
    for name in ("baseline", "candidate", "installed", "output"):
        parser.add_argument("--" + name, type=pathlib.Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    bundles = {name: getattr(args, name).resolve() for name in ("baseline", "candidate", "installed")}
    original_path = pathlib.Path.home() / "Library/Application Support/SuperLight/config.json"
    original = original_path.read_bytes()
    config = json.loads(original)
    config["settings"]["start_at_login"] = False
    config["settings"]["start_minimized"] = True
    config["settings"]["appearance_mode"] = "dark"
    installed_exe = bundles["installed"] / "Contents/MacOS/superlight"
    active = None
    trials = []
    try:
        stop(bundles["installed"], installed_exe, os.environ)
        for index, name in enumerate(["baseline", "candidate", "candidate", "baseline", "baseline", "candidate"]):
            prefix = f"{index + 1:02}-{name}"
            directory = output / (prefix + "-config")
            directory.mkdir()
            save(directory / "config.json", config)
            env = {**os.environ, "SUPERLIGHT_CONFIG_DIR": str(directory)}
            bundle = bundles[name]
            exe = bundle / "Contents/MacOS/superlight"
            with (output / (prefix + ".log")).open("x") as log:
                child = subprocess.Popen([str(exe), "--settings"], env=env, stdout=log,
                                         stderr=log, stdin=subprocess.DEVNULL, start_new_session=True)
            active = (bundle, exe, env, child)
            deadline = time.monotonic() + 40
            while time.monotonic() < deadline:
                current = health(status(exe, env))
                windows = visible_windows(app_pids(bundle))
                if child.poll() is not None:
                    raise RuntimeError(f"{name} exited during startup")
                if current and current["native_ready"] and current["device"] and windows:
                    break
                time.sleep(.25)
            else:
                raise RuntimeError(f"{name} not ready")
            ui = windows[0]["pid"]
            window(ui, False)
            print(f"{prefix}: hardware ready, UI {ui}, warming up 30s", flush=True)
            time.sleep(30)
            if len(app_pids(bundle)) != 2 or not visible_windows([ui]):
                raise RuntimeError("Wrong process or window state")
            measured = collect(bundle, output / (prefix + "-visible.jsonl"), 30)
            if not measured["stable_process_set"] or not visible_windows([ui]):
                raise RuntimeError("Visible measurement invalid")
            rows = [json.loads(line) for line in (output / (prefix + "-visible.jsonl")).read_text().splitlines()]
            ui_metrics = summarize([{**row, "processes": [p for p in row["processes"] if p["pid"] == ui]} for row in rows])
            service_metrics = summarize([{**row, "processes": [p for p in row["processes"] if p["pid"] != ui]} for row in rows])
            window(ui, True)
            time.sleep(3)
            if visible_windows([ui]):
                raise RuntimeError("UI not hidden")
            hidden = collect(bundle, output / (prefix + "-hidden.jsonl"), 20)
            hidden_rows = [json.loads(line) for line in (output / (prefix + "-hidden.jsonl")).read_text().splitlines()]
            hidden_ui = summarize([{**row, "processes": [p for p in row["processes"] if p["pid"] == ui]} for row in hidden_rows])
            window(ui, False)
            time.sleep(2)
            after = health(status(exe, env))
            if not after or not after["native_ready"] or not after["device"] or not visible_windows([ui]):
                raise RuntimeError("Restore visibility or hardware readiness failed")
            record = {"trial": index + 1, "name": name, "visible_combined": measured,
                      "visible_ui": ui_metrics, "visible_service": service_metrics,
                      "hidden_combined": hidden, "hidden_ui": hidden_ui,
                      "health_after": after, "restored_visibility": True}
            save(output / (prefix + "-summary.json"), record)
            trials.append(record)
            print(f"{prefix}: UI CPU {ui_metrics['cpu_percent_one_core']:.4f}% visible, "
                  f"{hidden_ui['cpu_percent_one_core']:.4f}% hidden", flush=True)
            stop(bundle, exe, env, child)
            active = None
            time.sleep(3)
        save(output / "comparison.json", {"trials": trials})
    finally:
        if active:
            stop(*active)
        with (output / "restore.log").open("x") as log:
            subprocess.Popen([str(installed_exe), "--settings"], stdin=subprocess.DEVNULL,
                             stdout=log, stderr=log, start_new_session=True)
        time.sleep(8)
        restored = health(status(installed_exe, os.environ))
        unchanged = original_path.read_bytes() == original
        save(output / "restoration.json", {"config_unchanged": unchanged, "health": restored})
        print(f"Installed app restored. Config unchanged: {unchanged}", flush=True)
        if not unchanged or not restored or not restored["native_ready"]:
            raise RuntimeError("Restoration validation failed")


if __name__ == "__main__":
    main()
