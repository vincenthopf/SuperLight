# macOS native UI efficiency result

This compares the previous native SwiftUI settings UI with the optimized build on macOS 26.3, Apple M3 Pro, 36 GB RAM, with an MX Master 3S connected over Bluetooth.

The candidate changes are:

- Build the SwiftUI/AppKit inspector with `-O -whole-module-optimization`.
- Use Swift Observation rather than publishing every model property through `ObservableObject`.
- Do not assign unchanged service snapshots or editor values.
- Coalesce IPC requests so a background refresh cannot discard a user command.
- Stop UI refresh timers while the application is hidden, minimized, or occluded. Refresh resumes when visible.
- Keep service polling intervals unchanged to preserve foreground-profile and Secure Input behavior.

Each build ran three fresh trials. The settings window stayed visible for a 30-second sample after a 30-second warm-up, then hidden for a 20-second sample. Both processes remained connected to the physical mouse. The run checked for stable process sets, visible/hidden state, native readiness, and zero dropped events. The installed app was restored and its configuration bytes were unchanged.

| Median | Previous UI | Optimized UI | Change |
| --- | ---: | ---: | ---: |
| UI CPU, visible | 5.75% | 0.003% | **1,900× lower observed CPU** |
| UI CPU, hidden | 0.014% | 0.005% | **2.8× lower** |
| Combined CPU, visible | 5.93% | 0.174% | **34.1× lower** |
| UI resident memory | 121.6 MiB | 114.6 MiB | **1.06× lower** |
| UI physical footprint | 53.2 MiB | 47.3 MiB | **1.13× lower** |
| Service CPU, visible | 0.192% | 0.170% | **1.13× lower** |

The visible UI CPU ranges did not overlap: previous 4.73–5.75%, candidate 0.0027–0.0032%. The large reduction is primarily from avoiding full SwiftUI model publication on unchanged 2-second status polls. It is an observed idle workload result, not a universal CPU guarantee.

The candidate preserved device readiness in every trial. The user manually tested Back → play/pause, Forward → next track, thumb wheel → volume, and the gesture button on the optimized service with no behavior change. The standalone model checks passed 25 cases covering unchanged polls, queued actions, saves, conflicts, service restarts, theme changes, and profile edits.

The candidate UI binary was 3,078,208 bytes versus 3,652,832 bytes before optimization. The service binary was unchanged for this GUI comparison.

## Limits

- This measures an idle visible settings window and hidden window, not active dragging or rapid profile edits.
- No input latency, energy, or battery measurement was available. `powermetrics` requires sudo credentials on this machine.
- The original UI’s 2-second polling work is included in the baseline. The candidate still polls while visible, but unchanged responses no longer invalidate the view tree.
