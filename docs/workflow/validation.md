# Initial workflow throughput validation

Issue: [#344](https://github.com/milocaetano/quantick/issues/344).
Base: `a808b2d87b36d73041027e4d20c053544b454a96`.
This report records the initial candidate `db1c178`; subsequent PR-review
repairs add executable guards and durable review tooling. Final-head local/CI
results and independent follow-ups are recorded on PR #345. The initial budget
reduction below was limited to the old scan scope, not a net context saving;
the follow-up extends accounting to the delegated contract trees.

## Observed failure and intervention

The [#339 checkpoint](https://github.com/milocaetano/quantick/issues/333#issuecomment-5568946571)
reports a repaired creation-order omission followed by two new ledger facets.
Raw open-count comparison treated 1 -> 2 as a stall despite closing the original
finding. #341 also had ledger repair work, but its failed runtime CI was a real
blocker, not evidence that all delay was procedural. These observations motivate
stable source maps, finding identities, bounded repairs and input-based local
validation. They do not establish a measured throughput gain.

## Independent instruction exercises

Two fresh, read-only agents received the repository instructions and
[supplied histories](delivery-cases.md), without expected answers. Their full
responses are retained in [cases 1-6](evidence/exercises-a.md) and
[cases 7-11](evidence/exercises-b.md). They did not execute or merge live PRs.

The retained responses reject frozen-map omissions (case 3), stale/executable
input reuse (4-7), counter resets (8), unauthorized merges (9), unreviewed scope
changes (10) and premature adoption (11). Cases 1-2 distinguish ledger repair
from genuine CI failure. They exposed draft/readiness wording contradictions,
which the follow-up repaired; case 1 also clarified prior coordinator reporting.

## Verification and limits

Local validation uses the delivery contract's instruction-change path:

- `cargo test -p quantick-guards`: 149 unit, 18 guard integration and 5 agreement
  tests passed. Context growth was repaired by shortening instructions; the
  aggregate ceiling decreased from 235,924 to 230,594 bytes.
- `sh .claude/hooks/guardrails_test.sh` under Git Bash: 146 passed, 0 failed,
  including 35 campaign context tests. The first run found missing marker names
  in `ship`; the corrected run passed. Hook behavior was not changed.
- Bundled skill-creator `quick_validate.py`: all four changed canonical skills
  passed. Relative file links and `git diff --check` passed.
- The raw local outputs are supplied to independent reviewers; required full
  workspace CI is recorded at the actual PR head on GitHub before readiness.
  No local full-workspace run or runtime evidence reuse is claimed.

Claude task skills and the Codex adapters reach the same canonical delivery
contract. Behavioral exercises evaluate instruction decisions, not unattended
production execution or a quantified speedup. A running campaign remains on
its existing workflow until reviewed adoption after the user's main merge.
