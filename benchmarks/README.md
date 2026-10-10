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

# MX Master 3S hardware soak and sleep/wake runbook

`macos/soak.py` checks how the installed SuperLight recovers from mouse power cycles and Mac sleep, and how its memory changes over hours. It attaches to the running app. It never stops, restarts or reconfigures SuperLight. It only calls `superlight --status` every second and `superlight --refresh` after each recovery.

Each phase writes to its own subdirectory of one output directory: `soak.jsonl` (status transitions, new errors, clock gaps and memory samples), `summary.json` and `manifest.json`. After every phase the script rebuilds `<output>/summary.json` and `<output>/mice-entry.json` from all phases in that directory. `mice-entry.json` has the shape of a `physical_verification` item in `compatibility/mice.json`. A phase key is in `checked` only if that phase passed. `input_latency` always stays in `not_checked`, because no instrument exists for it.

The `stats` object in `superlight --status` shows which recovery path ran:

- `connects` and `disconnects`: full HID reconnects. Mac sleep and a Bluetooth power cycle should increase these.
- `relinks`: the mouse re-linked while the HID device stayed open (HID++ `0x41` or `0x1D4B`). A power cycle on a Bolt or Unifying receiver may use this path.
- `reapplies`: complete reapplications of DPI, SmartShift and button diversion. Each successful connect or relink adds one.
- `last_connected_at` and `last_relink_at`: Unix time in seconds, or `null`.

Every command below runs from the SuperLight checkout. Use the same `OUT` for all phases.

```sh
OUT="$PWD/.working/soak-$(date +%Y%m%d)"
```

## 1. Prepare (5 minutes)

1. Install a SuperLight build that includes the `stats` counters. Older builds fail the `stats_available` check, because they cannot tell a relink from a reconnect.
2. Move the stale Mouser LaunchAgent: `launchctl bootout gui/$(id -u)/io.github.tombadash.mouser; mv ~/Library/LaunchAgents/io.github.tombadash.mouser.plist ~/Desktop/`. The `bootout` error "No such process" is fine.
3. Quit Mouser, OpenLogi and Logi Options+ if any of them is running.
4. In SuperLight settings, set DPI to a value that is not the mouse default, for example 1600, and map at least two buttons, for example back and forward. The DPI read-back then proves that SuperLight reapplied its settings.
5. Run the read-only check:

   ```sh
   uv run --no-project --python 3.12 python benchmarks/macos/soak.py check
   ```

   It exits 0 when every required item is `ok: true`: SuperLight running, `--status` reachable, `permissions.listen` and `permissions.inject` true, `native_ready` true, mouse connected, `stats` present, no other remapper running, and no Mouser LaunchAgent. `logi_options_plus_launch_agent`, `caffeinate_not_running` and `power_source` are information only. Note the `transport` under `device_connected`. It becomes the transport in the mice entry.

Every phase runs the same check first and refuses to start if a required item fails.

## 2. Reconnect storm (10 to 15 minutes, attended)

```sh
uv run --no-project --python 3.12 python benchmarks/macos/soak.py reconnect --output "$OUT"
```

The script runs 10 cycles. In each cycle:

1. When asked, slide the power switch under the mouse to OFF and press Enter. In cycle 5, hold the back button down first, switch the mouse off while holding it, release it, then press Enter.
2. Wait. The script waits up to 15 seconds for the mouse to disappear. On a Bolt or Unifying receiver it may stay listed. The script says so and continues.
3. When asked, slide the switch to ON, move the mouse a little, and press Enter.
4. The script waits for the mouse to be ready, for a connect or relink counter to change, and then reads DPI and SmartShift back with `--refresh`.
5. Answer `y` or `n`: press two mapped buttons and say whether both performed their actions. Then say whether any button, modifier key or drag is stuck. Do not press the mode shift button above the wheel during the run. It changes the wheel mode on the mouse, and the SmartShift read-back would then not match the configuration.

A cycle passes when the mouse is ready within 90 seconds, DPI and SmartShift match the configuration, both mapped buttons work, nothing is stuck and `dropped_events` did not change. The phase passes when all 10 cycles pass. `summary.json` lists each cycle's recovery time and its path: `reconnect`, `relink` or `none_observed`.

## 3. Sleep and wake (about 10 minutes, attended)

Do not run `caffeinate` or anything else that blocks sleep during this phase.

```sh
uv run --no-project --python 3.12 python benchmarks/macos/soak.py sleep --output "$OUT"
```

The script runs 3 cycles. In each cycle:

1. When asked, press Enter, then within 10 seconds choose Apple menu > Sleep.
2. Wait at least 1 minute. Wake the Mac with a key press, log in, and return to the Terminal window.
3. When asked, move the mouse a little and press Enter.
4. Answer the same two `y` or `n` questions as in the reconnect storm.

A cycle passes on the same conditions as a reconnect cycle, and the script must also have seen the sleep. It counts the sleep as seen if `suspended` was true or the wall clock jumped by more than 20 seconds. The summary also lists the `pmset -g log` Sleep and Wake lines from the phase.

## 4. Memory soak (2 to 8 hours, unattended)

Connect the Mac to power. Do not log out or switch users. A read-only check with the screen locked once showed `native_ready` and both permissions as false. The cause is not confirmed, so check the `native_ready` transitions in `soak.jsonl` afterwards.

```sh
uv run --no-project --python 3.12 python benchmarks/macos/soak.py memory --output "$OUT" --hours 4
```

The script starts `caffeinate -dims` tied to its own process, so the Mac stays awake only during this phase. It samples the SuperLight processes every 10 seconds with the same counters as `observe.py` and polls `--status` every second. The status polling adds IPC work to the service, so the CPU and wakeup numbers from this phase are not comparable with the background comparison above. You do not need to touch the mouse. If the mouse goes to sleep on its own, the transitions are recorded.

The phase passes when it runs for the full time, lasts at least 30 minutes, the SuperLight process set does not change, the mouse is connected at the end, and the median physical footprint of the last 10-minute bucket is at most 5 MiB above the first bucket. Change the limit with `--max-growth-mib`. The summary also reports footprint, resident memory and thread count (first, last, max), the footprint slope in MiB per hour, CPU percent and wakeups per second. Ctrl-C stops the soak early. The partial result is saved and does not pass.

## 5. Record the result in compatibility/mice.json

```sh
uv run --no-project --python 3.12 python benchmarks/macos/soak.py record --output "$OUT"
git diff compatibility/
```

`record` copies `$OUT/summary.json` to `compatibility/evidence/<date>-mx-master-3s-<transport>-soak.json` and appends `$OUT/mice-entry.json` to the MX Master 3S `physical_verification` list. It refuses if the evidence file already exists or if the phases ran on different SuperLight builds. The entry identifies the build by the SHA-256 of `Contents/MacOS/superlight`. Do not edit the earlier macOS 26.3 entry. Add the new entry next to it.

The `checked` list holds only `discovery` and the soak keys that passed. If you also confirmed specific buttons, add their keys, for example `back_play_pause`, by hand before you commit. `soak.jsonl` files stay in `$OUT`. They contain configuration and error text, so review them before publishing.
