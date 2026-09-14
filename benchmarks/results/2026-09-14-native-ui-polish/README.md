# Native UI polish: partial comparison

One matched baseline/candidate pair completed on the same running hardware service and unchanged configuration. The later trial lost foreground ownership and was rejected. These are GUI-process measurements, not total application measurements.

| UI process | Visible CPU, one-core % | Hidden CPU, one-core % |
| --- | ---: | ---: |
| Baseline | 0.06277 | 0.00404 |
| Refined | 0.18225 | 0.00440 |

The refined UI used more CPU in this single visible trial. No efficiency improvement or statistically reliable regression is established. Three repetitions per build were planned but not completed. The valid raw samples and summary are retained here; incomplete/rejected samples are excluded.

Each recorded sample validates the unlocked session, foreground ownership when visible, window presence, and expected hidden state. Source references and binary hashes are in `manifest.json`. These results do not measure interaction latency, animation frame rate, energy, or long-run memory stability.
