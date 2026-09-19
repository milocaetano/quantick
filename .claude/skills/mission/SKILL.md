---
name: mission
description: Define and enforce a tiered Quantick mission with a source-based ledger, acceptance criteria, applicable gates and delivery evidence. Use for /mission or a session goal. Campaign children return control to their coordinator.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# Mission

Argument: an optional tier, then the objective — `/mission small the axis
labels overlap at low zoom`. No objective: ask for it before anything else. The
first word selects `small`, `medium`, `high` or `max`, bare or flagged
(`--small`); otherwise keep the whole objective and default to `small`. Use a
flagged tier when the objective itself starts with a tier word. Step 1 echoes
the parse.

A mission owns one branch, worktree and PR, and selects the skills required
for done. [The delivery contract](../../../docs/workflow/delivery.md) owns
requirement reconciliation, gate mapping and proportional validation; apply its
source-preserving preflight before implementation. A campaign child returns
completion to its coordinator. This skill defines done; the built-in `/goal`
supplies continuation (step 9). Rationale: `references/why.md`.

## Tiers

| | `small` (default) | `medium` | `high` | `max` |
| --- | --- | --- | --- | --- |
| **2** ledger | required, terse | required | required | required |
| **3** questions | none; doubts become `S`, bar the exception below | ≤ 2, only where a wrong guess throws work away | ≤ 4 | ≤ 4, re-checked against the plan before code |
| **4** gates | English and *Any code change* whole; other rows only where the diff reaches that territory | full table | full table | full table; UI rows apply to a surface touched even indirectly |
| **5** `GOAL.md` | short: objective, `**Tier:**` line, ledger, `S`, criteria, verbatim request | full | full | full |
| **8** bug pass (`arch-review` step 0) | `code-review` at `low` | `low` | `medium` | `high`; tell the trader `/code-review ultra` exists |
| **8** shape pass | dimensions the diff touches; **8 always** | full | full | full |
| **8** `delivery-review` | **not run** | **completeness pass only**, inline | full | full |
| **9** `/goal` line | skipped | printed | printed | printed |

No tier removes `arch-review`, the bug pass, applicable validation, final-head
CI or the worktree rule. *A call that is the trader's* (step 3) is asked at
every tier; at `small` it means the tier was wrong — raise it in the same
breath. **A tier goes up, never down**: raise it the moment the work proves
bigger and rewrite the tier file (step 6). `pr-gate` exempts `small` from
`delivery-review` only while insertions plus deletions against `origin/main`
stay within `SMALL_TIER_MAX_CHANGED_LINES`; past it, raise the tier or split
the work — never shrink a diff to evade review.

## Steps

1. **Capture.** Restate the objective in one English sentence — it becomes
   `GOAL.md`, the branch name and the PR body's first line. Echo
   `tier: <tier> | objective: <sentence>`; at `small` add
   `(no interrogation, no delivery-review)`.

2. **Ledger.** Before any criterion, decompose the request into distinct
   outcomes/constraints `R1`…`Rn` under the delivery contract's reconciliation
   and source-span mapping. Distinct = independently verifiable; equivalent
   clauses may share an ID. The closing statement of purpose is an ask too, and
   judges the others. Keep a **verbatim fragment** where the wording carries the
   ambiguity; the operative statement is English. Every `R` maps to ≥ 1 `A`
   and every `A` cites ≥ 1 `R` — an `R` without one is a hole, an `A` without
   one is invented scope (take it to the trader or drop it). Operational
   instructions become `G`/`C` lines citing source and evidence, not duplicate
   `R/A`; an explicitly requested workflow change still gets `R/A`. IDs never
   renumber; a withdrawn ask stays, struck through, with the reason.

3. **Interrogate once, before work** — one `AskUserQuestion`, recommended
   option first, in the trader's language, within the tier's budget. Ask only
   for: an **ambiguous reference** (one word, two things, different code); a
   **double meaning** producing different software; a **contradiction** (asks
   that cannot both hold, or one against shipped behaviour); **a number nobody
   chose** where the wrong one is expensive to reverse; **a call that is the
   trader's** (autonomy, money, safety — anything that can place, cancel or
   lose an order — taste, irreversibility); **a narrowing** you are about to
   perform. Never ask what has a repo default (naming, placement, test style,
   prefix, crate), what the code answers in a minute, "should I proceed?", or a
   preference reversible in one edit — decide and record an `S`. Over budget, ask
   the costliest; the rest become `S` lines marked *wanted to ask*, with the
   reading taken; if none qualify, say so. Answers are `D1`…; reopening
   one is a scope change. Later doubts become `S` unless unsafe or likely to
   waste completed work.

4. **Criteria and gates.** Derive criteria from the ledger, then inject by
   kind (at `small`: *Any mission* and *Any code change* whole, other rows only
   where genuinely reached — a row that keeps applying means not `small`):

   | The mission… | Injected criteria |
   | --- | --- |
   | Any mission at all | **every artifact in English** (`arch-review` dimension 8, the language guard). At every tier copy the four reserved `G-AI` lines below verbatim into the local goal, declaring their PR/CI evidence destinations; never claim they already ran. |
   | Any code change | four checks green after rebasing on latest `main`; **performance impact declared** in the plan — every touched path classified per-trade / per-depth / per-frame / rare; `arch-review` Blockers/Should-fixes resolved or deferred in the PR body |
   | Touches a hot path | measured, not believed: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs. a `main` control run, or a fixture bench — numbers in the PR body |
   | Touches anything user-visible | `ui-harness`: every new/changed surface reachable by env hook in the same change; its QA pass with every cell PASS or explicitly accepted; `trader-ux-review` with no unresolved Blocker |
   | Adds a capability (feed, bar type, indicator, layer, panel, crate) | `new-extension`: port named, registration-only edits, defaults preserve today, fake second implementation tested, blast radius in the PR body |
   | Adds something a trader *does* (action, tool, trade, lock) | drivable without a mouse — act/read/discover from `arch-review`'s *The second operator*; where the class has no registry, carving one is part of the work or the plan says why it stays local |
   | Engine / determinism territory | test-first: fixture + expected output before the code; golden test |
   | Docs/skills only | proportional local proof and full final-head CI; shape dimensions 1–7 and 9 waived — **dimension 8 and step 0 always apply**; operational instruction changes need behavioral proof; scripts/config/tests take the full shape pass |

   Write down each non-applicable gate and why. List delivery-review PASS, the
   open PR and final-verifier PASS as closing steps `C1`…, not `A`/`G`; at
   `small` omit the delivery-review step. Show the checklist; no approval
   needed.

5. **Persist — in the worktree, so step 6 runs first.** Write
   `<worktree>/.claude/GOAL.md` in English, overwriting any previous one (one
   in the main checkout is off-branch and `delivery-review` returns NOT
   GRADEABLE). Order: objective and why; `**Tier:**` line with justification
   (at `small`, justify the exemption); ledger; `D…`; `S…` (each saying why
   assuming was safe); criteria; N/A with reasons; then **the full verbatim
   request** as an attributed quotation — without it, NOT GRADEABLE. Items:

   ```markdown
   - [ ] **A3** — <one observable outcome two readers would agree on>.
         *Evidence:* <named test, exit code, screenshot, verdict, quoted file>.
         → <PR/issue section or CI artifact URL>. *(R3, R4)*
   ```

   Stable IDs (`A` mission-specific, `G` injected), one outcome each, `(R…)`
   on `A` only; transcript-only claims are UNPROVEN. Every tier includes:

   <!-- required-ai-review-goal-gates:v1 -->
   - [ ] **G-AI1** — AI review is executed for the current PR review.
   - [ ] **G-AI2** — A durable AI-review report is published on the PR.
   - [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
   - [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
   <!-- end required-ai-review-goal-gates:v1 -->

6. **Ground.** Check `git worktree list` for an existing worktree or branch for
   this goal and a live writer; reuse, never duplicate. Otherwise cut a fresh
   worktree from updated `main` per `CLAUDE.md` (from an issue, `/issue start
   <N>` first). Before the first edit, arm it and record the tier — per-branch,
   in the worktree's git dir, never committed, rewritten whenever raised;
   `guardrails.sh` accepts only `<current-branch> <tier>` and `pr-gate` reads
   this `mission-tier` file, not `GOAL.md`:

   ```sh
   WT=/path/to/worktree
   CRATE=quantick-app          # the crate you are about to edit
   TIER=medium                 # small | medium | high | max
   cd "$WT" &&
     cargo build -p quantick-guards &&
     cargo check -p "$CRATE" --all-targets &&
     printf '%s %s\n' "$(git rev-parse --abbrev-ref HEAD)" "$TIER" \
       > "$(git rev-parse --absolute-git-dir)/mission-tier"
   ```

7. **Stay on track.** Refuse scope creep; state a necessary detour and tie it
   to the mission, or take it to the user. Narrowing stated scope is a step 3
   question whenever it surfaces. Keep the checklist in the todo list.

8. **Verify, then be graded.** Each criterion checked off with its own
   evidence; none without. Results and links go in the PR; raw evidence stays
   outside Git per the delivery contract.
   1. **Draft PR** (if `ship` has not), carrying every field filled:

      ```text
      <!-- quantick-mission-summary:v1 -->
      Objective: <one sentence>
      Tier: <tier>
      Source: <linked issue or retained user-request reference>
      Criteria: <IDs and delivered/deferred/open disposition>
      Validation: <commands/scenarios and results; link external raw artifacts>
      <!-- end quantick-mission-summary:v1 -->
      ```

      and below it the whole `GOAL.md` in `<details>`, kept current. Never
      track `GOAL*` or evidence.
   2. **`Skill(arch-review)`** — every tier; its producer records
      `arch-review-ok`. Never write a marker directly.
   3. **`Skill(ai-review)`** — every tier, same PR/key; its producer records
      `ai-review-complete`. Close every thread `list` returns.
   4. **`Skill(delivery-review)`** — last; its producer records
      `delivery-review-ok`. Skipped only at `small`.
   5. **Final completion** — after exact-head CI and any ready transition, from
      the task worktree, even for an already-ready PR:
      `sh .claude/hooks/mission_ship_gate.sh mission <pr>`. No PASS, no
      completion.
   6. After PASS, delete the local `.claude/GOAL.md`; the PR and reports are
      the record.

   After repairs, get current verdicts under the delivery contract's delta
   follow-up rules before replacing stale markers. Reviewers never edit.

9. **`/goal`** (not at `small`; campaign children return their condition to
   the coordinator instead). Right after step 4, print for the user to paste,
   in English, under 4,000 characters, every criterion kept as an observable
   outcome, with a finite turn bound — the evaluator reads session output
   only and changes no permission:

   ```text
   /goal <the criteria from step 4, as one measurable end state, plus "or stop after N turns">
   ```

## What done means

The final verifier reads these stable clauses literally and refuses ID drift.

<!-- what-done-means:v1 -->
- **D1** — The open PR is non-draft and matches branch, head and base.
- **D2** — Every registered CI check is green at that head.
- **D3** — Architecture has a current projection and durable PASS report.
- **D4** — Delivery has both when applicable; only bounded `small` is exempt.
- **D5** — AI has current completion, a durable report and zero listed threads.
- **D6** — The PR body carries the concise mission summary, current-head evidence and report URLs.
- **D7** — The final verifier publishes and verifies this literal reconciliation.
- **D8** — Only the user merges to `main`.
<!-- end what-done-means:v1 -->

Campaign merges still need their explicit grant. Never ask routine permission
to push, publish the draft, run reviews, complete readiness or run the final
verifier.
