import argparse
import json
import os
import pathlib
import platform
import statistics
import subprocess
import tempfile
import time

import psutil


def status(binary, environment):
    result = subprocess.run(
        [str(binary), "--status"], env=environment, text=True,
        capture_output=True, timeout=5, check=True,
    )
    return json.loads(result.stdout)["snapshot"]


def cpu_seconds(process):
    times = process.cpu_times()
    return times.user + times.system


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--max-rss-mib", type=float)
    parser.add_argument("--max-growth-mib", type=float, default=16)
    arguments = parser.parse_args()
    if not 10 <= arguments.seconds <= 3600:
        parser.error("--seconds must be between 10 and 3600")
    binary = arguments.binary.resolve(strict=True)
    measurements = {
        "platform": platform.platform(),
        "architecture": platform.machine(),
        "mode": "native_no_device" if arguments.native else "headless_no_device",
        "hardware_tested": False,
        "binary_bytes": binary.stat().st_size,
    }
    with tempfile.TemporaryDirectory(prefix="superlight-resources-") as temporary:
        root = pathlib.Path(temporary)
        environment = dict(os.environ, SUPERLIGHT_CONFIG_DIR=str(root))
        launch = [str(binary), "--background"]
        if not arguments.native:
            launch.append("--headless")
        with (root / "stdout.txt").open("w") as stdout, (root / "stderr.txt").open("w") as stderr:
            child = subprocess.Popen(launch, env=environment, stdout=stdout, stderr=stderr)
            try:
                process = psutil.Process(child.pid)
                startup = time.monotonic()
                deadline = startup + 20
                while True:
                    if child.poll() is not None:
                        raise AssertionError((root / "stderr.txt").read_text(errors="replace"))
                    try:
                        initial = status(binary, environment)
                        break
                    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                        if time.monotonic() >= deadline:
                            raise AssertionError("Service startup exceeded 20 seconds")
                        time.sleep(0.1)
                measurements["ready_milliseconds"] = round((time.monotonic() - startup) * 1000, 3)
                if initial["device"] is not None:
                    raise AssertionError("This benchmark requires a runner without a Logitech device")
                time.sleep(5)
                samples = []
                cpu_start = cpu_seconds(process)
                wall_start = time.monotonic()
                while time.monotonic() - wall_start < arguments.seconds:
                    if child.poll() is not None:
                        raise AssertionError("Service exited during the idle measurement")
                    samples.append(process.memory_info().rss)
                    time.sleep(0.5)
                elapsed = time.monotonic() - wall_start
                cpu_used = cpu_seconds(process) - cpu_start
                rss_before_requests = process.memory_info().rss
                request_start = time.monotonic()
                for _ in range(250):
                    state = status(binary, environment)
                    if state["device"] is not None:
                        raise AssertionError("A Logitech device connected during the benchmark")
                measurements["status_250_seconds"] = round(time.monotonic() - request_start, 3)
                time.sleep(2)
                rss_after_requests = process.memory_info().rss
                growth = max(0, rss_after_requests - rss_before_requests)
                measurements.update({
                    "duration_seconds": round(elapsed, 3),
                    "idle_cpu_seconds": round(cpu_used, 6),
                    "idle_cpu_percent_one_core": round(cpu_used * 100 / elapsed, 4),
                    "idle_rss_min_bytes": min(samples),
                    "idle_rss_median_bytes": int(statistics.median(samples)),
                    "idle_rss_peak_bytes": max(samples),
                    "rss_before_status_requests_bytes": rss_before_requests,
                    "rss_after_status_requests_bytes": rss_after_requests,
                    "status_request_growth_bytes": growth,
                    "resident_threads": process.num_threads(),
                    "resident_child_processes": len(process.children(recursive=True)),
                    "native_ready": state["native_ready"],
                    "permissions": state["permissions"],
                    "error_count": len(state["errors"]),
                })
                if process.children(recursive=True):
                    raise AssertionError("The background service retained a settings subprocess")
                if arguments.max_rss_mib is not None and max(samples) > arguments.max_rss_mib * 1024 * 1024:
                    raise AssertionError(f"Idle resident memory exceeded {arguments.max_rss_mib} MiB")
                if growth > arguments.max_growth_mib * 1024 * 1024:
                    raise AssertionError(f"Status requests increased resident memory by {growth} bytes")
                subprocess.run([str(binary), "--quit"], env=environment, capture_output=True, timeout=5, check=True)
                if child.wait(timeout=10) != 0:
                    raise AssertionError("Service shutdown failed")
                measurements["clean_shutdown"] = True
            finally:
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)
                arguments.output.parent.mkdir(parents=True, exist_ok=True)
                arguments.output.write_text(json.dumps(measurements, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(measurements, indent=2))


if __name__ == "__main__":
    main()
