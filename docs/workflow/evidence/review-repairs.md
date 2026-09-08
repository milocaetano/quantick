# PR 345 review repair evidence

This follow-up preserves Claude's commits `3076823` and `8bc7de2` and addresses
the review threads on PR #345. It is repair batch 3 after those two repair
commits, not a reset of the mission's budget. Final reports and exact-head
verification are published on the PR after the reviewed commit exists.

| Finding | Correction and regression evidence |
| --- | --- |
| Unbudgeted contract trees (3953374894, 3957199204) | `context.rs` includes campaign/workflow Markdown and excludes workflow evidence. Its relocation regression detects weight moved outside skill directories. Baseline reseed explicitly separates old-scope weight from newly tracked bytes; no net context saving is claimed. |
| Missing instruction dependency checks (3957193898) | `instruction_links` checks entrypoints, PR template, skills and docs. `deletion_and_rename_break_incoming_contract_links` removes/renames the actual target; `each_surface_resolves_from_its_own_directory` exercises relative bases. This is file reachability, not Markdown anchor validation. |
| Missing persistent counters (3953374907, 3957196491) | `progress-show/record/check` use paginated append-only PR records. Python tests simulate restart, ordinary discussion, lost POST responses, conflicts, missing revisions, resets, stricter limits and two stalled batches. Thread identities remain distinct from discussion reply counts. |
| Full snapshot for every mutation (3953374913) | Campaign state now defines compact pending/result operation journal comments and full recovery checkpoints. Byte bounds require linked partitioning rather than truncation. Independent case 13 tests timeout, conflicting intent, readback and ownership. |
| Anchoring the completeness reviewer (3953374922) | The reviewer derives distinct asks from the source before consulting the retained map, then preserves equivalent IDs. Missing real asks still fail; a different partition of equivalent prose does not. |
| Duplicate validation/scheduling rules (3953374925) | The campaign table is replaced by delegation to the canonical table; mission owns tier selection from child risk, not parent inheritance. Broader tier redesign stays in #324. |
| Draft command and tier references (3953374917) | Claude's prior repairs retain explicit `--draft`, the segment-visible command spelling and the runnable tier-read reference. |
| Eligibility limitation (3953374899) | Code-touching children still require full code verification. Relief applies to eligible later evidence-only deltas with valid input proof, not to relabeling a code PR as prose. Compilation caching and broader tier redesign are not claimed by this fix. |

The local execution now includes full fmt/clippy/build/test because executable
guard and workflow tooling changed. The shared hook suite includes the Python
progress regressions. Raw logs are supplied to independent reviewers; final-head
CI is required again. Earlier prose-only verification remains historical evidence,
not a substitute for these checks.

[Independent cases 12-13](repair-exercises.md) distinguish missing artifact links
from absent historical actions. This PR does not retroactively satisfy a
pre-edit mission requirement, waive missing evidence, replenish budgets or grant
main merge. Existing campaigns first adopt the reviewed workflow; specific
unmet historical obligations or exhausted limits still require a user decision.

The progress helper validates record consistency, not the authenticity of a
claimed user grant. One coordinator writes; concurrent conflicting revisions
stop work. Agents must inspect authorization evidence. Campaign journaling is
an instruction protocol, not a new background service or automatic scheduler.

## Local verification at the repair tree

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets` and
`cargo build --workspace` passed. The default parallel workspace test runner
failed the unchanged core UI capture budget twice (268 and 251 us against
250 us). Its isolated run passed at 93 us. The full workspace suite then passed
with `cargo test --workspace -- --test-threads=1`; no test, assertion, threshold
or source file in the application was changed. Preserve this execution-mode
qualification; do not report the failed default runs as passes. Final CI still
uses its normal runner and must pass on the new head.

The shared shell suite passed 147 checks, including the 19 new offline Python
progress tests and 35 campaign cases. The modified delivery-review skill passed
its bundled validator. Raw outputs, including the two failed runs, accompany
independent review. Cases 12-13 returned the historical-authority and journal
recovery decisions preserved above.
