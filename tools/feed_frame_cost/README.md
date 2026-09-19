# Feed frame correctness and cost protocol

This tooling exports two immutable product revisions and adds the same external
frame fixture to each. `verify.py` pins the original archives, entire source
trees, declared overlays, lock/toolchain and every execution tool. Preparation
does not run the app. Its manifest must match the separately published decision.

`build.py` runs the Linux owned-session cleanup proof before two sequential,
locked release app-test compiles, jobs1. `run_geometry.py` invokes only the named
untimed renderer test: both viewport sizes, observed1.5scale, real panel bounds,
three actual galleys and clip containment. Candidate actual/reference painting
must have identical geometry. Compilation and untimed execution require an
explicit finite decision; the workflow records their combined release.

`run_pairs.py` is a separate performance stage. It is not called by the geometry
workflow and requires a later finite execution decision. The four fixed cells
retain median<=1.05, p95<=1.10 and CV<=5%, three warmup and15 measured pairs per
cell,8000seed prints,30warmup/600measured frames and64prints per whole frame.
Whole-frame geometry includes the candidate panel's real chart-space cost.
Focused reference uses the candidate's actual cached painter. No retry, trimming,
adaptive warmup or weakening of a gate is supported.

Each child receives a constructed environment and an owned session. Receipts
contain environment names, never values. First-failure stdout/stderr, started
and terminal records, source identities and owned-group cleanup are retained.
LF serialization and explicitly framed output keep preparation and parsing
consistent across Windows/Linux. Python tests on Windows do not prove Linux
cleanup, Rust correctness, performance or native rendering.

Source subjects: controlf217fcf3db65dac0fc1cbb98956d9155000522ac and
candidate0e0970ab0d9db99470f5427aeac021ba13c5e4ac. The branch carrying these tools
does not change either measured product. Original source archives are retrieved
from the existing feed-handoff artifact; they are not committed again. Decisions,
failed preparations and review history are retained on issue495 under campaign525.
