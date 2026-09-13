# macOS background comparison

`results/2026-09-13-macos-background/` contains six raw process traces, their summaries, executable hashes, CPU-counter calibration, and SHA-256 checksums.

Mouser v3.7.3 was compared with the installed SuperLight 4.0.0-alpha.1 Rust service on an M3 Pro, macOS 26.3, AC power, with an MX Master 3S connected over Bluetooth at 1600 DPI. The installed SuperLight binary does not embed its source revision. Its SHA-256, rather than an assumed Git commit, identifies the measured build.

Both apps ran in the background with settings hidden or closed. Three fresh processes per app each warmed up for 30 seconds after connection, then were sampled for 30 seconds at one-second intervals. The order was Mouser, SuperLight, SuperLight, Mouser, Mouser, SuperLight. Only one remapper ran at a time. Configuration copies used identical mappings. Original configuration bytes were unchanged after restoration.

| Median across three trials | Mouser | SuperLight |
| --- | ---: | ---: |
| CPU, percent of one core | 1.936% | 1.167% |
| CPU trial range | 0.641–2.907% | 0.451–1.572% |
| Physical footprint | 138.86 MiB | 14.09 MiB |
| Resident memory | 253.52 MiB | 50.56 MiB |
| Package idle wakeups/s | 1.43 | 0.27 |
| Interrupt wakeups/s | 31.24 | 12.57 |
| Context switches/s | 306.31 | 333.17 |
| Retired instructions/s | 33.97 million | 21.88 million |
| CPU cycles/s | 43.50 million | 27.72 million |
| Disk reads, writes, page-ins during each interval | 0 | 0 |

CPU medians were 40% lower, resident memory 80% lower, and physical footprint 90% lower for SuperLight in this run. Context switches were 9% higher. CPU ranges overlap. These are short observations from one desktop with other applications running, not proof of a fixed CPU improvement, battery-life benefit, active input latency, feature parity, or long-term stability. Earlier exploratory help-command and headless comparisons are excluded.

CPU is derived from cumulative user and system time, not `ps`'s smoothed CPU percentage. `proc_pid_rusage` reports Mach ticks on this machine. The observer obtains `mach_timebase_info` and converts them to nanoseconds. The runner checks the conversion against `time.process_time_ns` before touching either app. Wakeup counters are not joules or watts. Memory units are MiB, with 1 MiB = 1,048,576 bytes.

## Repeat

This temporarily stops the installed remapper, takes private configuration backups in the output directory, runs each app with separate configuration directories, then restarts SuperLight with the original configuration. Use a Mac with Accessibility and Input Monitoring already granted to both apps. Close unsaved settings before running. Do not use the mouse or change windows during collection.

```sh
uv run --no-project --python 3.12 --with pyobjc-framework-Quartz==12.2.2 python benchmarks/macos/compare.py \
  --mouser /absolute/path/to/Mouser.app \
  --superlight /absolute/path/to/SuperLight.app \
  --output "$PWD/.working/comparison-new" \
  --warmup 30 --seconds 30 --background
```

The output directory must not already exist. `comparison.json` includes every successful trial. `restoration.json` records configuration equality and restored hardware readiness. Output also includes private configuration backups, so do not publish the entire directory. The committed results exclude those backups, configuration files, and foreground application details.

Omit `--background` to require visible settings windows and include the native SuperLight settings process. A process exit, changed process count, changed window state, or connection failure rejects the trial instead of counting the incomplete app as a performance improvement.

Verify committed evidence with:

```sh
cd benchmarks/results/2026-09-13-macos-background
shasum -a 256 --check sha256.txt
```
