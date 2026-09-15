# Performance evidence status

Not yet measured; no flat-or-better verdict is claimed for A7.
The machine is concurrently compiling unrelated worktrees. Their processes
and targets were left untouched. A contended sample would not discharge the
paired dense-input requirement.

Prepared comparison: the existing ignored test
app::tests::control_plane_tests::control_idle_dense_replay_benchmark.
It uses8000backfilled trades,30warmup frames,600measured frames and64live
trades per frame. Run exact test executables in alternating ABBA order with
the same process-local QUANTICK_BUBBLES override removed; separately compare
idle and feature-enabled active/ready/future states. Do not reuse the ordinary
candidate executable to claim feature-hook performance.

Preserved baseline executable:
C:/src/quantick-worktrees/feat-evidence-resources/target/debug/deps/quantick_app-938f257d5e0579f7.exe
SHA256 C076FF05E14F8629B3850242426F80F6A743E7A8AD972A59E68A51133AD84ECB.
Current ordinary candidate executable:
C:/src/quantick-worktrees/feat-evidence-resources/target/debug/deps/quantick_app-7d3f212efc7d7c85.exe
SHA256 BFC19329AFEAC5132A0DE11AD458A365EFA16B12B415D7656D815F6D8D3666B5.

The benchmark reports frame CPU average/p99/worst and trades per second.
It does not measure presented desktop FPS, pixel correctness or visual UX.
Feature-enabled executable identity must be recorded after its separate test
build. Current model transitions are fixed-size and allocate no storage;
active-pane reconciliation scans at most four panes, not all session tabs.
Annotation conversion is rare and resolves at most three anchors.
