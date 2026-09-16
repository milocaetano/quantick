# Performance evidence — measured, qualified result

Current runtime source: 1b4bad7ebb257d0c0a016db6994338592b367bda.
On 2026-09-15, the read-only validator ran five finite ABBA blocks for each
of idle, active, ready and future: 80 canonical invocations, all successful.
Each invocation uses the same 8000-print backfill, 30 warmup frames,
600 measured frames and 64 live prints/frame. All inherited QUANTICK_*
variables are cleared in the child, then only the requested range hook is set.
The test creates its own isolated stores. No compiler/linker ran during the
admitted measurements; the trader's separate release app remained ambient.

A is the preserved merged pre-extraction executable described below.
B is the current feature-enabled test executable
quantick_app-4895c18869080ee5.exe, SHA-256
DF11DF1B756A4EF6FC7315F2FE79109217256A43218BF00285D055A300C85147.
The ordinary executable in the historical preparation section is not B.

| State | Mean / median average-CPU delta | Mean / median p99 delta | Mean / median worst delta | Mean / median throughput delta |
| --- | --- | --- | --- | --- |
| idle | +1.571% / +2.202% | +1.892% / +2.839% | +19.672% / +6.031% | -1.385% / -2.173% |
| active | -0.637% / -0.086% | -1.787% / -2.357% | +6.586% / +0.149% | +0.680% / +0.095% |
| ready | +0.646% / -0.314% | +2.254% / +3.518% | +5.543% / +3.843% | -0.563% / +0.333% |
| future | +0.985% / +1.315% | +0.615% / +2.122% | +33.489% / +8.681% | -0.831% / -1.493% |

The first active block was +4.494% average CPU and +5.422% p99. Four
predeclared confirmation blocks did not reproduce that slowdown: average
deltas were -4.003%, -4.020%, +0.430%, -0.086%. All four confirmation p99
deltas favored B. This supports active-state noise around parity, not a
persistent active regression. Other central deltas are small and mixed;
worst frames have substantial outliers in both variants. It does not justify
an unconditional flat-or-better verdict for every state. A7/G4 remain subject
to independent review and any required focused follow-up; no margin is waived.

Raw receipts remain outside Git under
C:/src/quantick-worktrees/outside-eight-coordination/mvu-performance/:

- abba-raw-1b4bad7-v2.txt: SHA-256 38ADE6C717A58F55E92B8A02A19E101ACFDF4777FFFF8C17590F4B52E4EC2308.
- abba-confirmation-blocks-2-5-1b4bad7.txt: SHA-256 D14B8B47CB01C01E0F454802660B8253A2BB11BBA674DE39D78BD84FD151920C.
- all-five-block-distributions-1b4bad7.txt: SHA-256 590A9FF04F7E483E061FA3BC3AE643D7A72EBE507C8D090B02A99DDAE6457EF3.
- receipt-1b4bad7.md and confirmation-receipt-1b4bad7.md retain absolute
  values, all block ranges, timestamps, environment and limitations.

The first 16-call log had mixed UTF-16/UTF-8 encoding. It remains retained
as noncanonical, excluded from the numbers above, not rewritten as success.
The benchmark proves CPU/throughput timings, not presented FPS or allocation
counts. It does not assert quick-range state on every measured frame; live
appends can cover the initial future anchors and autoscroll the range out of
view. Persistent future visual behavior has a separate zero-live capture.
Source inspection proves bounded, fixed-size model transitions without
allocation or locks; pane reconciliation early-returns when idle and scans
at most four active panes otherwise. Annotation conversion resolves at most
three anchors on a rare action. These are source facts, not timing claims.

## Historical preparation (before the measurements above)

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
