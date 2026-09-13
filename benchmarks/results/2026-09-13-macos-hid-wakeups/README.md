# macOS HID wakeup optimization result

This result compares the same SuperLight service with and without the macOS HID no-event run-loop slice change. The baseline and candidate were built with the same Rust toolchain and run on macOS 26.3, Apple M3 Pro, with an MX Master 3S over Bluetooth.

Each build ran three fresh background trials. Each trial waited 30 seconds after the mouse connected, then sampled for 60 seconds. Only one service ran at a time. Configurations were copied into private directories. All trials retained hardware readiness, had no dropped events, and reported no errors. The installed SuperLight application and its configuration were restored and verified unchanged after the run.

| Median | Baseline | Candidate | Candidate change |
| --- | ---: | ---: | ---: |
| CPU, one core | 0.555% | 0.457% | 1.22× lower |
| Interrupt wakeups/sec | 17.97 | 4.45 | 4.04× fewer |
| Package idle wakeups/sec | 1.62 | 0.20 | 8.08× fewer |
| Context switches/sec | 163.12 | 116.19 | 1.40× fewer |
| Resident memory | 44.22 MiB | 44.27 MiB | No meaningful change |
| Physical footprint | 13.30 MiB | 13.25 MiB | No meaningful change |

CPU ranges overlapped. The clearest result is fewer interrupt wakeups: the trial ranges did not overlap (14.90–20.20/sec baseline versus 4.42–4.52/sec candidate). Package idle-wakeup and CPU ranges did overlap. The user reported normal operation after being asked to test Back play/pause, Forward next track, thumb wheel volume, and gesture controls. This is qualitative feedback, not a counted or timed input test.

The source change is one line in `crates/superlight-service/src/macos_hid.rs`: the run loop waits up to 250 ms rather than 50 ms when there is no report. IOHID callbacks still wake the run loop as soon as a report arrives. The outer hardware read budget remains 250 ms. This removes intermediate 50 ms timer expirations without changing that budget. Report and removal callbacks can still wake the run loop before its deadline. Input latency was not instrumented.

This does not prove battery or energy savings. `powermetrics` was unavailable without sudo credentials. It also does not establish long-run memory behavior or input latency.
