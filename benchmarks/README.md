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

# OpenLogi comparison runbook

`macos/compare_openlogi.py` compares the idle cost of OpenLogi and SuperLight with the same `observe.py` metrics. It runs one remapper at a time in the order OpenLogi, SuperLight, SuperLight, OpenLogi, OpenLogi, SuperLight. Each trial waits for readiness, warms up for 30 seconds, then samples for 30 seconds at one-second intervals. It also records launch-to-ready time and bundle size.

SuperLight runs with a copy of its configuration in the output directory. OpenLogi cannot be isolated this way, because starting its agent from a terminal changes the macOS permission owner. OpenLogi therefore runs with your real `~/.config/openlogi/config.toml`. The script backs up that file and reports whether its bytes changed. The installed SuperLight is stopped at the start and always restarted at the end, including after Ctrl-C or a failed trial.

Readiness rules:

- SuperLight is ready when `superlight --status` reports `native_ready` and a connected device.
- OpenLogi is ready when `openlogi api status` reports `hook_installed` and `inventory` `ready`, and `openlogi api devices` lists an online device whose name contains `MX Master`. `openlogi list` is not used, because it can open the mouse directly with the CLI's own identity.

OpenLogi processes are found anywhere inside `OpenLogi.app`, including the agent in `Contents/Library/LoginItems/OpenLogi Agent.app` and the overlay. The `openlogi` CLI is excluded from the measured set.

## 1. Upgrade OpenLogi to 0.8.13

Upgrade before measuring. The installed 0.8.3 has no `openlogi api` command, so the script cannot check readiness and will refuse to run. A comparison against the current release is also the one that matters to readers.

1. Quit SuperLight: `~/Applications/SuperLight.app/Contents/MacOS/superlight --quit`.
2. Download `https://updates.openlogi.org/releases/v0.8.13/OpenLogi-v0.8.13-macos-arm64.dmg`.
3. Drag `OpenLogi.app` to `/Applications` and choose Replace.
4. Check the version: `defaults read /Applications/OpenLogi.app/Contents/Info CFBundleShortVersionString` prints `0.8.13`.

## 2. Grant OpenLogi permissions

The permissions belong to **OpenLogi Agent**, not to OpenLogi. The `+` picker does not open app bundles, so use Go to Folder.

1. Open System Settings > Privacy & Security > Input Monitoring.
2. Click `+`. Press Cmd+Shift+G. Paste `/Applications/OpenLogi.app/Contents/Library/LoginItems/` and press Return.
3. Select `OpenLogi Agent.app`, click Open, and turn its switch on.
4. Repeat steps 2 and 3 in Privacy & Security > Accessibility.
5. If an older `OpenLogi Agent` row already exists, turn it off and on again after the upgrade.

Then open OpenLogi once with SuperLight still quit. Confirm the MX Master 3S appears and its buttons work. Set the OpenLogi mappings you want to compare, matching your SuperLight mappings where both apps support them. Quit OpenLogi as described in section 6.

## 3. Pre-checks

1. Stale Mouser LaunchAgent. `~/Library/LaunchAgents/io.github.tombadash.mouser.plist` starts an old Mouser copy at login. Move it out of the way: `launchctl bootout gui/$(id -u)/io.github.tombadash.mouser; mv ~/Library/LaunchAgents/io.github.tombadash.mouser.plist ~/Desktop/`. The `bootout` error "No such process" is fine.
2. Logi Options+. `pgrep -fl logioptionsplus` must print nothing. If it prints a process, quit Logi Options+ from its menu bar icon.
3. Only one remapper. `pgrep -fl -i -E "openlogi|mouser|logioptionsplus"` must print nothing. SuperLight may be running. The script stops it.
4. Run the read-only check: `uv run --no-project --python 3.12 python benchmarks/macos/compare_openlogi.py --check`. Under `remappers`, only `superlight` may appear. Under `launch_agents`, `mouser.exists` should be `false`.
5. Use the same terminal app that ran earlier SuperLight comparisons. SuperLight trials start from that terminal. If SuperLight never reaches `native_ready`, give that terminal Input Monitoring and Accessibility.
6. Connect the Mac to power. Close other heavy apps. Do not touch the mouse during sampling.

## 4. Run

From the SuperLight checkout:

```sh
uv run --no-project --python 3.12 python benchmarks/macos/compare_openlogi.py \
  --output "$PWD/.working/openlogi-$(date +%Y%m%d-%H%M)" \
  --power-cycle --reconnect
```

The run takes about 10 to 12 minutes and needs you at the keyboard. `--power-cycle` asks you to switch the mouse off and on before each trial, so each app starts from the mouse's default state instead of the previous app's state. `--reconnect` adds one recovery test per app at the end: switch the mouse off when asked, then press Enter at the same moment you switch it on. The time to readiness includes your reaction time, so compare the two apps, not the absolute number. Without both flags the run takes about 8 minutes and needs no input.

By default OpenLogi is started with `open -g /Applications/OpenLogi.app` and its settings process is closed once the agent is ready, so both apps are measured without a settings window. `--openlogi-start kickstart` starts only the launchd agent. That works only if OpenLogi's Launch at login setting is on. `--foreground` keeps both settings windows open.

The script refuses to start if Mouser, Logi Options+ or OpenLogi is running. It rejects a trial if another remapper appears, the measured process set changes, or the app loses the mouse. It exits non-zero if SuperLight does not come back ready or a configuration file changed.

## 5. Feature-parity checklist for the MX Master 3S

Run these by hand with each app, one app at a time. Record Yes, No or Not supported for each.

1. Back and forward buttons perform their mapped actions.
2. Gesture button: click, and each of up, down, left and right.
3. Thumb wheel: horizontal scroll or its mapped action.
4. Mode shift button above the wheel toggles ratchet and free spin.
5. SmartShift: automatic switch at the configured sensitivity.
6. DPI: set a value and confirm pointer speed changes.
7. Per-app profile: a mapping changes when you switch to the configured app.
8. Battery level shows in the app or menu bar.
9. Pause and resume remapping.
10. Mouse power-cycle: mappings, DPI and SmartShift return without restarting the app.
11. Mac sleep and wake: mappings, DPI and SmartShift return.
12. Quit the app: the mouse returns to default behaviour with no stuck button.
13. Setup friction: number of permission prompts and manual steps on first launch.

## 6. Quit OpenLogi and restore SuperLight

1. Quit OpenLogi from its menu bar icon. Quit is final. launchd does not restart the agent after it.
2. If the icon is missing: `pkill -TERM -x openlogi-agent; pkill -TERM -x openlogi-desktop; pkill -TERM -f "OpenLogi Overlay"`.
3. Check: `pgrep -fl -i openlogi` prints nothing.
4. If you will not keep OpenLogi, turn it off in System Settings > General > Login Items & Extensions. Turn off the agent in Input Monitoring and Accessibility only after it has quit.
5. The script restarts SuperLight itself. If it did not, run `open ~/Applications/SuperLight.app`. Check with `~/Applications/SuperLight.app/Contents/MacOS/superlight --status`: `native_ready` is `true` and `device` is not null.

## 7. Results

The output directory must not exist before the run.

- `summary.md`: medians for bundle size, launch-to-ready time, CPU, footprint, resident memory, wakeups, context switches, threads and process count, plus reconnect times and configuration checks.
- `comparison.json`: every trial, per-process commands, health before and after, reconnect results and the summary.
- `NN-<app>.jsonl` and `NN-<app>-summary.json`: raw one-second samples and per-trial metrics.
- `manifest.json`: macOS version, CPU, power, versions, bundle sizes, executable hashes, start state and CPU counter calibration.
- `restoration.json`: whether `config.toml` and SuperLight `config.json` changed, OpenLogi launchd state, and SuperLight health after restore.
- `private-backups/`: copies of both configuration files. Do not publish them.

Before publishing, copy only the summaries, JSONL traces and manifest into `benchmarks/results/<date>-openlogi/` and add a `sha256.txt`.
