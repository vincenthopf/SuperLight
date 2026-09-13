# Rust rewrite evidence

The compatibility target is upstream Mouser v3.6.0 at `34d93f70b2a84e425698e0d3d748cae2c5d18911`. The fork started at `e780641d3e709f914d6273985da9ac2ab85a7322`.

## Test-first baseline

The original repository already contains tests. Before implementing Rust, all 261 upstream v3.6.0 tests passed on macOS arm64 in run [34684699431](https://github.com/vincenthopf/SuperLight/actions/runs/34684699431). The fork's existing Ubuntu compilation, Python tests and QML lint also passed in run [34684699440](https://github.com/vincenthopf/SuperLight/actions/runs/34684699440).

`compatibility/test_v36.py` was committed before the Rust implementation. Its characterization run passed against the immutable upstream implementation in [34685131319](https://github.com/vincenthopf/SuperLight/actions/runs/34685131319). Rust CI evaluates the same requests with both implementations and compares outputs. This is separate from the original Python suite, which cannot directly exercise a Rust replacement. Contract counts describe the covered requests, not proof of complete physical-device parity.

## Verified automated results

On September 13, 2026, [Rust run 34729133268](https://github.com/vincenthopf/SuperLight/actions/runs/34729133268) passed all four jobs at `fe55501522956430ce82cdc8f322dad8db357982`.

| Runner | Rust tests passed | Differential v3.6 contracts | Package build | Native input ready in resource sample |
| --- | ---: | ---: | --- | --- |
| macOS 14, Apple Silicon | 140 | 3,974 | Passed | Yes |
| macOS 15, Intel | 140 | 3,974 | Passed | Yes |
| Windows x64 | 135 | 3,974 | Passed | Yes |
| Ubuntu x64 | 143 | 3,974 | Passed | No, uinput permission unavailable |

All Rust test counts have zero failures and zero ignored tests. Counts differ because platform-specific tests are compiled only on their target. Every job passed formatting, service and workspace Clippy with warnings denied, five release-packager tests, optimized executable builds, package creation, resource limits and artifact uploads.

The lifecycle checks passed startup, singleton enforcement, pause/resume, validated configuration saves, bounded input, unknown-field preservation, restart, migration, invalid-startup protection and shutdown. The allocation test passed one million HID parse/decode/router button edges with zero allocations after initialization. That test does not measure all operating-system callbacks or the settings UI.

The settings process rendered and closed successfully under Xvfb on Linux. macOS and Windows compiled and tested the settings editor, but the GUI smoke-test step is Linux-only. Native GUI interaction on those platforms remains a manual acceptance check.

## Measured resource use

The following are release-service measurements from the same run. Each uses an isolated configuration directory, five seconds of warm-up, approximately 30 seconds of idle sampling and 250 status requests. No Logitech device is attached and the settings UI is not running. RSS is resident memory, not total system memory or active-device usage. CPU is expressed as a percentage of one core.

| Runner | Headless median RSS, MiB | Native-mode median RSS, MiB | Native-mode peak RSS, MiB | Native-mode idle CPU, % | RSS growth after 250 status requests, bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| macOS Apple Silicon | 6.12 | 25.42 | 25.42 | 0.1368 | 0 |
| macOS Intel | 2.59 | 14.42 | 15.20 | 0.2527 | 0 |
| Windows x64 | 8.71 | 11.16 | 11.16 | 0.0000 | 12,288 |
| Ubuntu x64 | 3.22 | 3.29 | 3.29 | 0.0000 | 4,096 |

The Linux native-mode sample is a permission-denied fallback, not an active evdev/uinput remapping measurement. A recorded CPU value of zero means no measurable CPU-time increase in that sample, not a guarantee of zero CPU use. All eight measurements reported clean shutdown, zero retained child processes and zero recorded service errors. Native readiness and permission status must be considered separately from the error count.

| Runner | Service executable, bytes | Settings executable, bytes |
| --- | ---: | ---: |
| macOS Apple Silicon | 1,065,008 | 8,517,056 |
| macOS Intel | 1,073,280 | 9,202,488 |
| Windows x64 | 1,061,888 | 9,382,912 |
| Ubuntu x64 | 1,346,208 | 11,814,808 |

These executable sizes refer to `target/release`, before macOS bundle signing. The Apple Silicon application archive checksum and executable permission bits were also inspected after download. The archive contains the service, settings executable, documentation and license notices, without Python or QML runtime files.

Raw test logs, dependency trees, executable SHA-256 hashes and resource JSON reports are in the run's evidence artifacts:

- [Apple Silicon evidence](https://github.com/vincenthopf/SuperLight/actions/runs/34729133268/artifacts/10309037600)
- [Intel macOS evidence](https://github.com/vincenthopf/SuperLight/actions/runs/34729133268/artifacts/10308238246)
- [Windows evidence](https://github.com/vincenthopf/SuperLight/actions/runs/34729133268/artifacts/10308743367)
- [Linux evidence](https://github.com/vincenthopf/SuperLight/actions/runs/34729133268/artifacts/10308778220)

The downloaded evidence ZIP hashes matched GitHub's artifact digests. These artifacts have a 14-day retention period and are scheduled to expire on September 27, 2026. The summary above remains in Git history.

## Compatibility boundaries

The protocol retains 20-byte BLE output reports, optional input report IDs, software ID 0x0A, the response-function increment quirk, direct Bluetooth and six receiver slots, model-specific DPI limits, gesture CID preference and capability selection, basic/enhanced SmartShift function IDs, and the fixed-ratchet 0xFF threshold. Configuration migrations preserve the original v1 through v9 semantics and unknown fields.

Intentional safety differences include rejecting oversized writes instead of truncating them, correlating replies and errors to the actual receiver slot and request, rejecting malformed configuration without overwriting it, bounded queues, and avoiding changes to non-Logitech devices.

The deployed application consists of a native Rust service and a separate on-demand settings process. Python remains only in development, compatibility and packaging scripts. The legacy Python/Qt application has been removed from this branch. Its source remains available at the original fork commit and in Git history.

## Risks and release gates

The automated results support engineering review. They do not establish production readiness or complete end-to-end hardware parity. Before a production release, record:

- Physical Bluetooth and Bolt checks for discovery, button remapping, gestures, DPI, SmartShift, battery reporting and application profiles.
- Sleep/wake, receiver reconnect, permission revocation/recovery, secure-input transitions and release of held remapped buttons under failure.
- Native GUI interaction and start-at-login checks on macOS and Windows, and Linux evdev/uinput checks with the supplied permissions installed.
- Long-running idle and active-device resource measurements on macOS and Windows, including repeated settings-window open/close cycles.
- Distribution signing and notarization where required. Current macOS packages are ad-hoc signed, not notarized distribution builds.

No claim that the reported 5 GB usage has been reproduced or fixed on the user's machine is made. The 30-second no-device samples are not a long-duration leak test. No production release or hardware acceptance is implied by a green CI run.
