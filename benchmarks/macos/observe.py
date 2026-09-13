import argparse
import ctypes
import json
import os
import pathlib
import statistics
import subprocess
import time


class Usage(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64)
        for name in (
            "user_ns system_ns idle_wakeups interrupt_wakeups pageins wired_bytes "
            "rss_bytes footprint_bytes start exit child_user child_system "
            "child_idle child_interrupt child_pageins child_elapsed read_bytes "
            "written_bytes qos_default qos_maintenance qos_background qos_utility "
            "qos_legacy qos_initiated qos_interactive billed_system serviced_system "
            "logical_writes peak_footprint_bytes instructions cycles billed_energy "
            "serviced_energy interval_peak runnable"
        ).split()
    ]


class TaskInfo(ctypes.Structure):
    _fields_ = [
        (name, ctypes.c_uint64)
        for name in "virtual resident user system threads_user threads_system".split()
    ] + [
        (name, ctypes.c_int32)
        for name in (
            "policy faults pageins cow messages_sent messages_received mach_syscalls "
            "unix_syscalls context_switches threads running priority"
        ).split()
    ]


PROC = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
PROC.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
PROC.proc_pid_rusage.restype = ctypes.c_int
PROC.proc_pidinfo.argtypes = [
    ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int
]
PROC.proc_pidinfo.restype = ctypes.c_int


def sample(pid):
    usage = Usage()
    if PROC.proc_pid_rusage(pid, 4, ctypes.byref(usage)) != 0:
        raise OSError(ctypes.get_errno(), f"Cannot sample PID {pid}")
    task = TaskInfo()
    if PROC.proc_pidinfo(pid, 4, 0, ctypes.byref(task), ctypes.sizeof(task)) != ctypes.sizeof(task):
        raise OSError(ctypes.get_errno(), f"Cannot inspect PID {pid}")
    names = (
        "user_ns system_ns idle_wakeups interrupt_wakeups pageins rss_bytes "
        "footprint_bytes peak_footprint_bytes read_bytes written_bytes instructions cycles"
    ).split()
    return {"pid": pid, **{name: getattr(usage, name) for name in names},
            "threads": task.threads, "context_switches": task.context_switches}


def processes():
    rows = subprocess.check_output(["ps", "-axo", "pid=,ppid=,comm="], text=True)
    return [(int(parts[0]), int(parts[1]), parts[2])
            for row in rows.splitlines() if len(parts := row.strip().split(None, 2)) == 3]


def app_pids(bundle):
    prefix = str(pathlib.Path(bundle).resolve()) + "/Contents/MacOS/"
    rows = processes()
    selected = {pid for pid, _, command in rows if command.startswith(prefix)}
    while True:
        children = {pid for pid, parent, _ in rows if parent in selected}
        if children <= selected:
            return sorted(selected)
        selected |= children


def summarize(samples):
    first, last = samples[0], samples[-1]
    seconds = last["elapsed_s"] - first["elapsed_s"]
    keys = ("footprint_bytes", "rss_bytes", "threads")
    totals = {key: [sum(p[key] for p in s["processes"]) for s in samples] for key in keys}
    start = {p["pid"]: p for p in first["processes"]}
    end = {p["pid"]: p for p in last["processes"]}
    stable = start.keys() == end.keys() and all(
        {p["pid"] for p in s["processes"]} == start.keys() for s in samples
    )
    metrics = {"seconds": seconds, "samples": len(samples), "stable_process_set": stable}
    for key, values in totals.items():
        metrics[key] = {"median": statistics.median(values), "min": min(values), "max": max(values),
                        "first": values[0], "last": values[-1]}
    if stable and seconds > 0:
        deltas = {key: sum(end[p][key] - start[p][key] for p in start)
                  for key in ("user_ns", "system_ns", "idle_wakeups", "interrupt_wakeups",
                              "context_switches", "read_bytes", "written_bytes", "pageins")}
        metrics["deltas"] = deltas
        metrics["cpu_percent_one_core"] = (deltas["user_ns"] + deltas["system_ns"]) / seconds / 1e7
        metrics["idle_wakeups_per_second"] = deltas["idle_wakeups"] / seconds
        metrics["interrupt_wakeups_per_second"] = deltas["interrupt_wakeups"] / seconds
        metrics["context_switches_per_second"] = deltas["context_switches"] / seconds
    return metrics


def collect(bundle, output, seconds, interval=1.0):
    began = time.monotonic()
    samples = []
    with pathlib.Path(output).open("x", encoding="utf-8") as file:
        while True:
            pids = app_pids(bundle)
            if not pids:
                raise RuntimeError(f"No running processes for {bundle}")
            row = {"elapsed_s": time.monotonic() - began,
                   "processes": [sample(pid) for pid in pids], "load_average": os.getloadavg()}
            samples.append(row)
            file.write(json.dumps(row) + "\n")
            file.flush()
            if row["elapsed_s"] >= seconds:
                break
            time.sleep(max(0, began + len(samples) * interval - time.monotonic()))
    return summarize(samples)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--seconds", type=float, default=120)
    args = parser.parse_args()
    print(json.dumps(collect(args.bundle, args.output, args.seconds), indent=2))


if __name__ == "__main__":
    main()
