# Withdrawn GUI measurement

Do not cite these results as foreground GUI performance. Screen lock was confirmed after this run. Quartz can report windows on-screen while the session is locked, and the original collector did not validate lock state or foreground ownership. Consequently the baseline could keep polling while the candidate correctly stopped work for occlusion. The 1,900x and 34x figures were withdrawn.

These original summaries remain for audit only. A replacement collector validates lock state, foreground PID and hidden state at every sample. Model checks and build results are separate from this invalid measurement.
