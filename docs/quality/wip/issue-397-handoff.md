# WIP handoff: MT5 connection state machine

Status: **unfinished preservation snapshot; not approved for merge**.

Source issue: https://github.com/milocaetano/quantick/issues/397
Campaign: https://github.com/milocaetano/quantick/issues/472
Original local branch: `feat/mt5-connection-lifecycle`
Snapshot parent: `c9138e6100921bf70f8126a4cf308c47bbe8b183`

The user requested remote preservation for evaluation in a new campaign. This commit records existing work, not task completion or a performance/score claim. No runtime repair, rebase, main merge, campaign merge or issue closure is part of this publication. Local worktrees remain available.

## Known state and blockers

Unfinished pure protocol machine and socket/clock boundary. Latest retained TCP005 performance result is valid FAIL: median1.042078 passes, p951.164977 fails. Crate182tests/3ignored and262guards passed historically. Four runtime/performance repairs and four stalls retained; no fifth attempt authorized. Not approved for integration.

Existing results are historical evidence, not newly rerun checks or approval at this preservation commit. All original acceptance criteria, failed samples, retry counters, safety constraints and future review/CI gates remain. Evaluate the full diff from the snapshot parent before reuse; do not cherry-pick blindly onto a changed base.

## Evidence

- https://github.com/milocaetano/quantick/issues/397#issuecomment-5705195928

The adjacent source manifest records every pre-handoff changed/untracked source or evidence file, deletions, byte sizes and SHA256. The adjacent goal snapshot, when present, is the unfinished local mission record, not a completed mission archive. External build products and machine-wide logs are not included; source and task-owned existing evidence are preserved.
