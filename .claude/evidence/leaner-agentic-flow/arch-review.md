# arch-review — refactor/leaner-agentic-flow

Step 0 graded `fb57687a5475824a3ba52be819e7fd6c5238515f`. The shape pass and
this verdict cover the branch through `13c000b0a273c6d3f496f7b952bcbbe670ea4673`,
the commit that resolves every finding below -- named explicitly, because the
marker holds only a hash and an undated verdict cannot be told apart from one
written for an earlier head. This file is committed after that sha, so the sha
it names is its parent rather than its own; that is the ordering, not an error. Mission tier `high`, read from
`mission-tier` in the worktree's git dir, which agrees with the goal file's
`**Tier:**` line.

## Step 0 — the bug pass

`code-review` ran at **xhigh**, and that is not the level this tier asks for.
The tier ladder puts `high` at `medium`, one notch below its name; the skill
was invoked as `Skill(code-review), args: "refactor/leaner-agentic-flow medium"`
and reported back *"No effort level was typed, so I reused xhigh — the level
from last time"*. The argument did not reach it. Recorded rather than smoothed
over, because the whole point of naming the level in this header is that a
pass of unknown depth is not a pass: this one was **deeper** than the tier
bought, which is the harmless direction, and the same failure would have been
silent had it landed the other way. A branch touching the invocation path
should fix the argument handoff; it is a PR follow-up, not a Blocker on this
change.

**15 findings, 15 confirmed, 15 resolved.** None deferred. The list, and where
each was answered:

| # | Where | Finding | Resolution |
| --- | --- | --- | --- |
| 1 | `ship/SKILL.md:33` | Still carried its own "up to three rounds" — the second count this branch removed from `delivery-review`, in the skill that actually orchestrates both reviews. A `/ship` session would reach the six-round outcome the change exists to stop. | Points at the chain budget; defers the remainder into the PR body. |
| 2 | `delivery-review` | "A verdict is FAIL if either pass says so" made the escalation unable to overturn a false `PARTIAL` — pure added cost with no possible effect. | The strong pass is the verdict on lines it re-graded; both readings recorded when they differ. |
| 3 | `delivery-review` | The two-pass escalation existed only as an aside; steps 2, 3 and 4 still described one dispatch, so an agent executing the steps in order never performs it. | New *The escalation dispatch* section wires when/inputs/returns/merge, and steps 3 and 4 carry the merge rule. |
| 4 | `delivery-review` | Routing the whole reviewer to `sonnet` weakened the **completeness** pass, whose `UNLEDGERED` is a discovery grade with a false-*negative* failure mode — nothing for the escalation to catch. `medium` ran the same pass on the strong model, inverting the tiers. | The completeness pass keeps the strong model and is never escalated. Only the criteria pass starts on `sonnet`. |
| 5 | `size.rs` | `remedies` classified by substring, so a malformed `!budget` handed the author "carve a module" for a typo in a data file. | Root fix: `Finding` carries its remedy from construction. New `BASELINE_REMEDY`. |
| 6 | `ui-harness:117` | "per the table above" pointed at a table that moved. | Names the three rows and the file. |
| 7 | `ui-harness` | The floating-surface hook rule ended up at the bottom of the 61KB data file the skill says to grep, leaving the body telling authors to edit `app.rs` — which the size guard then fails. | Lifted into *Adding a new hook* as the stated exception. |
| 8 | `ui-harness` | "no row was dropped" was inaccurate: five hooks stayed in `SKILL.md` because the split followed hunk boundaries. | All moved; the registry holds 126 rows, the skill none. |
| 9 | `main.rs` | `--tighten` usage and both result messages described entries only, and the budget was rewritten on *any* gap — revoking deliberately signed headroom. | Messages updated; the rewrite is now gated on `BUDGET_SLACK`, the same test `budget_verdict` applies. |
| 10 | `size.rs` | `BUDGET_SLACK`'s doc claimed a bound the mechanism does not give: `recorded()` sums ceilings, so real hidden headroom is `BUDGET_SLACK + entries × SLACK` ≈ 4,100 lines. | Stated in the doc, with why summing measured counts would be worse. |
| 11 | `size.rs` | The "good news, tighten" finding was routed to `BUDGET_REMEDY`, which never names `--tighten`. | New `BUDGET_SLACK_REMEDY`. |
| 12 | `size.rs` | `remedies` was the only new function with no test. | Three tests: per class, the parse case, and the mixed run. |
| 13 | `size.rs` | Dead `|| contains("nothing caps them")` duplicating a literal from a format string. | Gone with the typed `Finding`. |
| 14 | `CLAUDE.md` | The routing rule named a three-kind taxonomy the repo exercises for one kind, and its `sonnet` example named `visual-qa`, which dispatches no agent. | Narrowed and made explicit: one routed call site today, `haiku` describes no existing dispatch, written as the standard the next one meets. |
| 15 | `arch-review:46` | The step-0 sub-budget could consume the whole chain budget, and the shape pass this branch diagnosed as unbounded was still unbounded. | A round is redefined as one sweep of everything that owes a review; the step-0 rule is a skip-rule within a round, not a counter. |

## Verdict

- **Correctness** — 15 from step 0, all confirmed, all resolved; nothing open.
  The level it ran at is recorded above and is a follow-up, not a finding
  against this diff.
- **Docking** — the only new port is `Finding`, and it made the guard's three
  violation classes extensible: a fourth arrives with its own remedy and no
  edit to the dispatcher. `Guard.remedy` is gone, so nothing central needs
  touching to add one.
- **Performance** — flat. Every added path is `rare` by the dimension-2 table:
  `budget_verdict` is arithmetic over ~18 parsed entries, `remedies` scans a
  list that only exists once a guard has already failed. `cargo test -p
  quantick-guards` runs 31 unit tests in 0.15s.
- **Operability** — no surface. A build-time guard with no UI and nothing for
  an agent to drive.
- **Proof** — 12 new unit tests in `crates/guards/src/size.rs`
  (`#[cfg(test)]`, private access, so unit rather than integration). The pair
  that would fail on a regression: `a_raise_that_pays_for_nothing_is_over_budget`
  and `tighten_never_raises_the_budget`.
- **Accumulation** — **trunk flat.** No tracked file was touched; no ceiling
  moved. The baseline diff is one added line, `!budget 61467`. `size.rs` is at
  ~700 production lines against a 1,500 threshold, so it remains untracked.
- **Language** — the guard passes (`cargo test -p quantick-guards`, language
  scan green), **and** I read the prose, the branch name and the commit
  messages myself. All English. The archived goal file quotes the trader's
  Portuguese in one marked, attributed section, which is the exemption
  `CLAUDE.md` grants and which its own preamble claims openly.
