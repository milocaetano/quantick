---
name: mission
description: Define and enforce a tiered Quantick mission with a source-based ledger, acceptance criteria, applicable gates and delivery evidence. Use for /mission or a session goal. Campaign children return control to their coordinator.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# Mission

Argument: an optional tier, then the session objective — `/mission small the
axis labels overlap at low zoom`. If the objective is missing, ask for it
before doing anything else.

The first word selects `small`, `medium`, `high` or `max`, bare or flagged
(`--small`). Otherwise retain the whole objective and default to `small`.
Use an explicit flagged tier when the objective itself starts with a tier word;
step 1 echoes the parse.

The mission selects the skills required for done. Each mission owns one
branch, worktree and PR.

[The delivery contract](../../../docs/workflow/delivery.md) owns requirement
reconciliation, operational gate mapping and proportional validation. Apply its
source-preserving preflight before implementation; campaign child completion
returns to the coordinator without a new user-pasted goal.

**This skill defines done; `/goal` supplies continuation.** Claude's built-in
`/goal` repeats turns until its evaluator accepts the condition, without
repository knowledge. Step 9 provides that condition.

Read `references/why.md` when changing a rule; it explains the rationale.

## Tiers

A tier buys less ceremony **on the record**, with a gate that knows it did.

| | `small` (default) | `medium` | `high` | `max` |
| --- | --- | --- | --- | --- |
| **2** — request ledger | required, terse | required | required | required |
| **3** — interrogation | skipped; every doubt becomes an `S` assumption, bar the one exception below | at most two questions, and only where a wrong guess throws work away | the full round, at most four | the full round, re-checked against the plan before code is written |
| **4** — injected gates | English, and *Any code change* whole. Every other row applies only where the diff actually reaches that territory | the full table | the full table | the full table, and the UI rows apply to a surface touched even indirectly |
| **5** — `GOAL.md` | short form: objective, **the `**Tier:**` line**, ledger, `S`, criteria, verbatim request | full | full | full |
| **8** — bug pass (`arch-review` step 0) | `code-review` at `low` | at `low` | at `medium` | at `high`, and the trader is told `/code-review ultra` exists |
| **8** — shape pass | only the dimensions the diff touches; **8 always** | full | full | full |
| **8** — `delivery-review` | **not run** | **completeness pass only**, inline | runs in full | runs in full |
| **9** — the `/goal` line | skipped | printed | printed | printed |

**What no tier buys.** `arch-review`, applicable validation and the worktree
rule hold at every tier. A tier never removes the bug pass or final-head CI.

**The one question `small` still asks.** Step 3's *a call that is the
trader's* — money, safety, irreversibility, autonomy. If it is being asked, the
work was never `small`: raise the tier in the same breath.

**A tier goes up, never down.** Raise it the moment the work turns out bigger
than it looked, and rewrite the tier file from step 6 when you do. Lowering one
mid-mission cannot be told apart from dodging a review that was about to fail,
so it is not available.

`pr-gate` exempts `small` from `delivery-review` only while insertions plus
deletions against `origin/main` stay within `SMALL_TIER_MAX_CHANGED_LINES`.
Past it, raise the tier or split the work; never shrink a diff to evade review.
`.claude/hooks/README.md` owns the mechanism.

## Steps

1. **Capture the mission**: restate the objective in one sentence **in
   English** — that sentence becomes `.claude/GOAL.md`, the branch name and the
   first line of the PR body. Saying it back in the trader's own language too
   is welcome; the version written down is the English one.

   Echo `tier: <tier> | objective: <sentence>`. At `small`, explicitly add
   `(no interrogation, no delivery-review)` so the exemption is visible.

2. **Build the request ledger.** Before deriving a single criterion, decompose
   the request into distinct outcomes/constraints, numbered `R1`…`Rn`, using
   the delivery contract's reconciliation and source-span mapping.

   - An ask is distinct when it has an independently verifiable outcome or
     constraint. Equivalent source clauses may share an ID. Operational
     instructions map to applicable `G`/`C` obligations, not duplicate `R/A`.
   - The closing statement of purpose ("so that we can…") is an ask too, and
     the one that judges the others.
   - Keep the trader's own words as a **verbatim fragment** where the wording
     carries the ambiguity — the words that carry it, not three sentences where
     three words would do. The operative statement on each line is English.
   - Map every `R` to at least one **`A` criterion**, and cite at least one `R`
     from every `A`. **An `R` with no criterion is a hole. An `A` with no `R`
     is scope you invented** — take it to the trader or drop it.
   - Gates `G1`…`Gn` and closing steps `C1`…`Cn` cite their authoritative
     source and evidence. An explicitly requested workflow change still gets
     `R/A` coverage; classification cannot hide a requested outcome.
   - Numbers are stable for the life of the mission. Never renumber. A
     withdrawn ask stays on the ledger, struck through, with the reason.

3. **Interrogate — once, before any work starts.** Raise everything that
   qualifies in a single `AskUserQuestion` call (at most four questions,
   recommended option first, in whatever language the trader speaks).

   **The tier sets the budget**: four questions at `high` and `max`, two at
   `medium`, none at `small` — where every doubt becomes an `S` assumption
   instead, except *a call that is the trader's*, which is asked at every tier
   and means the tier was wrong. Under a reduced budget, everything that
   qualified and went unasked is an `S` line marked *wanted to ask*, carrying
   the reading you went with. A tier lowers what you ask; never what you
   record.

   **Ask only for:**

   - **Ambiguous reference** — a word naming two different things in this repo,
     where the two lead to different code.
   - **Double meaning** — a phrase that reads two ways, and the two readings
     produce different software.
   - **Contradiction** — two asks that cannot both be satisfied, or an ask that
     contradicts something already shipped.
   - **A number nobody chose** — "fast", "a few", "most", where the code needs
     an exact value and the wrong one is expensive to reverse.
   - **A call that is the trader's** — autonomy, money and safety (anything
     that can place, cancel or lose an order), taste, and irreversibility.
   - **A narrowing you are about to perform** — delivering less than what was
     said is never a private decision.

   **What does not earn a question** — decide it, and record it as an `S`:
   anything with a conventional default in this repo (naming, file placement,
   test style, branch prefix, which crate); anything the code answers in under
   a minute of reading; "should I proceed?"; a preference reversible in one
   edit.

   Record answers as `D1`…`Dn` in `GOAL.md`. Reopening a settled user decision
   is a scope change.

   Over four qualifying questions: ask the costliest four and record the rest
   as `S` marked *wanted to ask*. If none qualify, say so. Later doubts become
   assumptions unless unsafe or likely to waste completed work.

4. **Classify it and inject the standard gates.** Derive the mission-specific
   criteria from the ledger — every `R` discharged — then add the gates for its
   kind.

   At `small`, two rows are injected outright: *Any mission at all*, and the
   whole of *Any code change* — the four checks, **the declared performance
   impact**, and `arch-review` resolved. Every remaining row applies solely
   where the diff genuinely reaches that territory. A narrower reading of the
   same table, never a different one: if a row keeps applying anyway, the
   mission is not `small`.

   | The mission… | Injected acceptance criteria |
   | --- | --- |
   | Any mission at all | **every artifact in English** — `CLAUDE.md` owns the rule, its scope and its exemptions; do not restate them here. Graded by `arch-review` dimension 8, enforced by `crates/guards/src/language.rs` |
   | Any code change | four checks green after rebasing on latest `main`; **performance impact declared** — classify every touched path by rate (per-trade / per-depth / per-frame / rare) as part of the plan, not the review; `arch-review` run with every Blocker/Should-fix resolved or deferred in the PR body |
   | Touches a hot path | evidence that performance is flat or better, not a belief: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs. a `main` control run, or a bench over a fixture — measured before the PR, numbers in its body |
   | Touches anything user-visible | follow `ui-harness`: every new/changed surface reachable by env hook, added in the same change; `visual-qa` pass with all surfaces PASS or defects explicitly accepted; `trader-ux-review` with no unresolved Blocker |
   | Adds a capability (feed, bar type, indicator, layer, panel, crate) | follow `new-extension`: port named, registration-only edits, defaults preserve today's behaviour, fake second implementation tested, blast radius stated in the PR body |
   | Adds something a trader *does* (an action, a tool, a trade, a lock) | drivable without a mouse — take the act/read/discover criteria from `arch-review`'s *The second operator*, not from a summary that drifts. Where the capability class has no registry yet, carving one is part of the work per `new-extension` — name it in the plan or say why the capability stays local |
   | Engine / determinism territory | test-first: fixture + expected output written before the code; golden test guards determinism |
   | Docs/skills only | proportional local proof under the delivery contract and full final-head CI; `arch-review`'s shape dimensions 1–7 and 9 waived for prose — **dimension 8 and step 0 always apply**. Operational instruction changes require behavioral proof; scripts/config/tests take the full shape pass. Tier-based review gates remain unchanged |

   Write down every non-applicable gate and why it does not apply.

   ### Closing steps are not criteria

   Two things finish every mission and **neither is an `A` or a `G`**:
   `delivery-review` returns PASS, and the PR is open. List them as `C1`…`Cn`
   under **Closing steps**. **At `small` the first is not listed at all** — a
   closing step the mission is exempt from is not one it owes, and writing it
   down leaves the archive recording an obligation nothing will discharge.
   Archiving `GOAL.md` is not among them: step 8 puts it before the reviews.

   Show the checklist before work; no routine approval is required.

5. **Persist it — in the worktree, which means step 6 happens first.** Cut the
   branch and worktree before writing anything, then write the mission to
   `<worktree>/.claude/GOAL.md`, in English, so it survives compaction.
   Overwrite any previous one. A `GOAL.md` written into the main checkout is
   not on the branch, and `delivery-review` — which looks for the checklist
   *on the branch* — returns NOT GRADEABLE.

   `GOAL.md` carries, in this order: the objective and why it matters; **the
   tier, as a `**Tier:**` line naming it and why the work earns it**; the
   request ledger; the decisions `D1`…`Dn`; the assumptions `S1`…`Sn`; the
   acceptance criteria; what is not applicable and why; and last, **the request
   as received, quoted in full and verbatim**.

   At `small`, the tier line says why the exemption was earned; empty decisions
   and not-applicable sections may be omitted. `delivery-review` refuses a file
   without the verbatim request. Mark it as an attributed quotation under
   `CLAUDE.md`'s language exemption; keep every other line English.

   ### The checklist format

   ```markdown
   - [ ] **A3** — <one observable outcome, stated so two readers would agree
         whether it happened>.
         *Evidence:* <what proves it — a named test, a command's exit code, a
         screenshot, a review verdict, a quoted section of a file>.
         → <path where that evidence will be written>. *(R3, R4)*
   ```

   Each item has a stable ID (`A` mission-specific, `G` injected), one
   observable outcome, an evidence kind and destination path, and an `(R…)`
   tail on `A` lines only. Never renumber. Transcript-only claims are UNPROVEN.

   Assumptions get their own list, `S1`…`Sn`, each with the reason it was safe
   to assume rather than ask. `delivery-review` audits that list: an assumption
   that turned out to drive the design is a question step 3 should have asked.

6. **Set up the ground — before step 5 writes anything.** Fresh worktree from
   updated `main` under `../quantick-worktrees/` per `CLAUDE.md`; never the
   main checkout, and check the worktree for a live writer before the first
   write. The `worktree-guard` hook denies the write if this step is skipped.

   **Arm the worktree before the first edit** with both commands. Keep the
   assignments inside the same shell call and replace every placeholder.

   ```sh
   WT=/path/to/worktree
   CRATE=quantick-app          # the crate you are about to edit
   cd "$WT" &&
     cargo build -p quantick-guards &&   # arms guard-watch; no dependencies
     cargo check -p "$CRATE" --all-targets
   ```

   Both commands run **before the first edit**.

   **Record the tier here**, before the first line of work, beside the two
   review markers in that worktree's own git dir — per-branch, never committed:

   ```sh
   WT=/path/to/worktree
   TIER=medium                 # small | medium | high | max
   cd "$WT" &&
     printf '%s %s\n' "$(git rev-parse --abbrev-ref HEAD)" "$TIER" \
       > "$(git rev-parse --absolute-git-dir)/mission-tier"
   ```

   `guardrails.sh` accepts only `<current-branch> <tier>`; `pr-gate` reads that
   file, not `GOAL.md`. Rewrite it whenever the tier is raised.

7. **Stay on track**: refuse scope creep. A necessary detour is stated
   explicitly and tied back to the mission, or taken to the user. Keep the
   checklist in the todo list so progress is visible. Narrowing the user's
   stated scope is a step 3 question, whenever it surfaces.

8. **Verify, then be graded.** Check off each criterion with its own evidence —
   command output, test result, screenshot path, review verdict — and write
   that evidence where the criterion said it would land. A criterion without
   evidence is unmet.

   **Archive before review.** The archive belongs in the reviewed diff.

   1. **Archive**, as the mission's last commit, before either review runs.
      Assign the slug first — an unquoted `<slug>` is two shell redirections.

      ```sh
      WT=/path/to/worktree
      SLUG=my-mission-slug
      # `mv`, not `git mv`: `.gitignore` lists `.claude/GOAL.md`, so the live
      # file is never tracked and `git mv` aborts with "not under version
      # control". Only the archive it becomes is tracked.
      cd "$WT" &&
        mv .claude/GOAL.md ".claude/GOAL-archive-$SLUG.md" &&
        git add ".claude/GOAL-archive-$SLUG.md" &&
        git commit -m "docs: archive the $SLUG mission"
      ```

   2. **`Skill(arch-review)`** — shape and bugs, over the final branch, at the
      effort and breadth this tier sets. It records `arch-review-ok` itself.
      Every tier runs it.
   3. **`Skill(delivery-review)`** — conformance, over the same final branch.
      It records `delivery-review-ok` itself, on PASS only. **Skipped at
      `small`**, and only there.
   4. **PR readiness** — `ship` may publish the draft before reviews; `ai-review`
      needs its resolvable threads. Ready waits for current reviews and CI.
      The PR body names the tier and labels local/reused/CI verification.
      `ai-review` owns the durable PR report and `ai-review-complete` projection;
      completion and zero unresolved threads are required at every tier.

   A `small` mission still archives `GOAL.md` as its durable objective record.

   After repairs, obtain current verdicts under the delivery contract's delta
   follow-up rules before replacing stale markers. Reviewers never edit.

9. **Hand over the `/goal` condition.** Skipped at `small`. At every other
   tier, right after step 4, print the built-in command for the user to paste:

   Campaign children instead return their completion condition to the
   coordinator, which owns available host continuation; no per-child prompt.

   ```text
   /goal <the criteria from step 4, as one measurable end state, plus "or stop after N turns">
   ```

   - **4,000 characters maximum.** Compress rather than drop: state each
     criterion as a terse observable outcome ("clippy/fmt/build/test exit 0",
     "delivery-review returned PASS", "PR URL printed", "GOAL.md archived"),
     strip rationale and repo context, collapse per-surface detail into one
     line. Count the characters before printing.
   - The evaluator **does not run commands or read files** — every criterion
     must be something this session's own output demonstrates.
   - Include a bound (`or stop after 20 turns`) so a stuck mission ends.
   - It does not change permissions. Pair with auto mode for unattended runs.
   - Write the line in English, like the criteria it restates.

## What done means

Done = the PR is ready, CI is green, `delivery-review` returned PASS, and the
evidence is in the PR body. At `small`, where that review does not run, done is
the same line without it — the PR ready, CI green, `arch-review` closed, the
evidence in the body. Main merging is exclusively the user's action;
intermediate campaign merges follow their explicit grant and integration contract. Do not
ask routine permission to push, publish the draft or complete readiness.
