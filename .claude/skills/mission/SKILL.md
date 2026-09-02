---
name: mission
description: Define and enforce a mission for the current session — read the request into a traceable ledger, interrogate what is ambiguous, derive acceptance criteria including the standard quantick gates (arch-review, delivery-review, visual-qa, trader-ux-review, ui-harness hooks) that match the kind of work, keep every action aligned, prove every criterion with recorded evidence, and hand back a ready-to-paste /goal condition. Use when the user types /mission <objective> or asks to set a goal for the session/task.
---

# Mission

Argument: an optional tier, then the session objective — `/mission small the
axis labels overlap at low zoom`. If the objective is missing, ask for it
before doing anything else.

**The tier is the first word, and only when that word is one of `small`,
`medium`, `high` or `max`** — bare or flagged (`--small`), the two being the
same instruction. Anything else is objective text and the objective keeps every
word of it. With no tier given, the mission runs at **`small`**.

The bare form misreads an objective that genuinely opens with one of those
words (`/mission small fonts are unreadable`). Two things hold it, and neither
pretends to be a parser: step 1's echo names **what the tier drops**, not
merely the word it read, and the flagged form exists for exactly the sentence a
bare word guesses wrong on. Use it when the tier word reads as an adjective.

The mission is the orchestrator: it decides which other skills are part of
*done* so the user never has to list them. One session, one mission, one
branch, one worktree, one PR.

**This skill is not `/goal`.** Claude Code's built-in `/goal` sets a completion
condition and keeps re-running turns until a small fast model judges it met; it
knows nothing about this repo. The two compose — this skill decides *what done
means*, `/goal` keeps the session from stopping before it. Step 9 hands over
the line to paste.

The reasoning behind the flow, and what each rule was bought with, is
`references/why.md`. Read it when changing a rule, not when following one.

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

**What no tier buys.** `arch-review`, the four checks and the worktree rule
hold at every tier. A tier shortens a review; it never removes the bug pass.

**The one question `small` still asks.** Step 3's *a call that is the
trader's* — money, safety, irreversibility, autonomy. If it is being asked, the
work was never `small`: raise the tier in the same breath.

**A tier goes up, never down.** Raise it the moment the work turns out bigger
than it looked, and rewrite the tier file from step 6 when you do. Lowering one
mid-mission cannot be told apart from dodging a review that was about to fail,
so it is not available.

**What `small` costs at the gate.** It is the only tier `pr-gate` can see: a
`small` branch opens its PR on `arch-review-ok` alone. That hole is bounded
rather than trusted — the exemption lapses once the branch exceeds
`SMALL_TIER_MAX_CHANGED_LINES` in `guardrails.sh` (insertions plus deletions
against `origin/main`), and past it the branch pays in full whatever the tier
file says. So the word has to be true when the branch *ships*: a `small`
mission that grows either raises its tier and runs `delivery-review`, or splits
until a branch really is small. Shrinking a diff to get under a review is the
dishonest third option. `.claude/hooks/README.md` owns the mechanism.

## Steps

1. **Capture the mission**: restate the objective in one sentence **in
   English** — that sentence becomes `.claude/GOAL.md`, the branch name and the
   first line of the PR body. Saying it back in the trader's own language too
   is welcome; the version written down is the English one.

   **Echo the parse in the same breath**, on one line: `tier: <tier> |
   objective: <the sentence>`. **At `small` the echo also names what the tier
   drops** — `tier: small (no interrogation, no delivery-review) | objective:
   …`. A trader skimming one line will not catch a misparsed word, but they
   will catch a mission announcing it is about to skip a review they wanted.

2. **Build the request ledger.** Before deriving a single criterion, decompose
   the request into atomic asks, numbered `R1`…`Rn`.

   - An ask is **atomic** when it can be delivered, or not delivered, on its
     own. "X, and also Y" is two lines. A sentence naming a defect is an ask.
   - The closing statement of purpose ("so that we can…") is an ask too, and
     the one that judges the others.
   - Keep the trader's own words as a **verbatim fragment** where the wording
     carries the ambiguity — the words that carry it, not three sentences where
     three words would do. The operative statement on each line is English.
   - Map every `R` to at least one **`A` criterion**, and cite at least one `R`
     from every `A`. **An `R` with no criterion is a hole. An `A` with no `R`
     is scope you invented** — take it to the trader or drop it.
   - The injected gates `G1`…`Gn` carry **no** `R` tail: no trader ever asked
     for `cargo clippy` to pass. Step 4's table is their provenance.
   - Numbers are stable for the life of the mission. Never renumber. A
     withdrawn ask stays on the ledger, struck through, with the reason.

   The ledger is what turns "they only did part of it" from a feeling into a
   detectable event.

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

   **What earns a question:**

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

   Write the answers into `GOAL.md` as `D1`…`Dn`, **Decisions taken by the
   trader**. A decision recorded there is settled: re-opening one is a scope
   change.

   **When more than four things qualify**, rank by the cost of being wrong —
   how much work a wrong guess throws away, and how hard it is to reverse — ask
   the top four, and record every one you did not ask as an `S` marked *wanted
   to ask*. Dropping the fifth ambiguity in silence is the same failure as
   dropping the fifth ask.

   If nothing qualifies, say so in one line. Legitimate, but stated, never
   silent. After this round the mission runs on its own: a later doubt becomes
   an assumption unless it is unsafe or would waste work already done.

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
   | Docs/skills only | four checks still run; `arch-review`'s shape dimensions 1–7 and 9 waived — **dimension 8 is not**, and neither is step 0; `pr-gate` wants whichever markers the tier owes. The waiver covers prose: a shell script, a config file or a test shipping alongside it takes the full shape pass |

   Write down what is **not applicable and why**. A gate silently omitted and
   one deliberately excluded look identical to the next reader, and only one of
   them is honest.

   ### Closing steps are not criteria

   Two things finish every mission and **neither is an `A` or a `G`**:
   `delivery-review` returns PASS, and the PR is open. List them as `C1`…`Cn`
   under **Closing steps**. **At `small` the first is not listed at all** — a
   closing step the mission is exempt from is not one it owes, and writing it
   down leaves the archive recording an obligation nothing will discharge.
   Archiving `GOAL.md` is *not* among them: step 8 puts it before the reviews.

   They are not criteria because they cannot be graded when the grading
   happens — `delivery-review`'s own verdict does not exist while it is being
   written, so written as criteria they come back UNPROVEN on every mission and
   burn the fix loop on gaps no edit can close.

   Present the merged checklist to the user before starting work.

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

   The tier line is not bookkeeping: `delivery-review` reads this file and
   nothing else, so a branch declaring `small` needs the file to say why the
   exemption was earned. At `small` the file may drop the decisions and the
   not-applicable sections when both are empty, and keeps everything else.

   The verbatim request is not optional. Without it the ledger becomes its own
   source of truth, and an ask dropped while *writing* the ledger is one no
   reviewer can ever find — `delivery-review` refuses to grade a goal file that
   lacks it. On the language rule this is **one marked, attributed
   quotation**, the shape `CLAUDE.md`'s exemption describes; say in the
   section's preamble why the words are not translated, and keep every other
   line English.

   ### The checklist format

   A contract, not a style preference: `delivery-review` reads it, and a
   criterion it cannot grade is a criterion nobody grades.

   ```markdown
   - [ ] **A3** — <one observable outcome, stated so two readers would agree
         whether it happened>.
         *Evidence:* <what proves it — a named test, a command's exit code, a
         screenshot, a review verdict, a quoted section of a file>.
         → <path where that evidence will be written>. *(R3, R4)*
   ```

   - **Stable ID.** `A1`…`An` mission-specific, `G1`…`Gn` injected gates. Never
     renumbered.
   - **One observable outcome.** Not "the ledger works well" — "every ask
     appears as a numbered line mapped to a criterion". If you cannot say how
     an outsider would check it, the criterion is not finished being written.
   - **Evidence kind, and the path it lands at.** Naming the path in advance is
     what stops evidence from being remembered instead of recorded. Evidence
     that exists only as a claim in the transcript comes back **UNPROVEN**.
   - **Ledger back-reference.** The `(R…)` tail, on `A` lines only.

   Assumptions get their own list, `S1`…`Sn`, each with the reason it was safe
   to assume rather than ask. `delivery-review` audits that list: an assumption
   that turned out to drive the design is a question step 3 should have asked.

6. **Set up the ground — before step 5 writes anything.** Fresh worktree from
   updated `main` under `../quantick-worktrees/` per `CLAUDE.md`; never the
   main checkout, and check the worktree for a live writer before the first
   write. The `worktree-guard` hook denies the write if this step is skipped.

   **Arm the worktree before the first edit**, with both commands, in the new
   worktree. Both assignments are inside the fence on purpose: `WT` does not
   survive between tool calls, and an unquoted `<placeholder>` is not a
   placeholder to `sh` — it is two redirections.

   ```sh
   WT=/path/to/worktree
   CRATE=quantick-app          # the crate you are about to edit
   cd "$WT" &&
     cargo build -p quantick-guards &&   # arms guard-watch; no dependencies
     cargo check -p "$CRATE" --all-targets
   ```

   `CLAUDE.md`'s *Verification loop* owns why. What belongs here is only that
   the two commands run **before the first edit**: a guard armed afterwards has
   already missed the edit worth reporting.

   **Record the tier here**, before the first line of work, beside the two
   review markers in that worktree's own git dir — per-branch, never committed:

   ```sh
   WT=/path/to/worktree
   TIER=medium                 # small | medium | high | max
   cd "$WT" &&
     printf '%s %s\n' "$(git rev-parse --abbrev-ref HEAD)" "$TIER" \
       > "$(git rev-parse --absolute-git-dir)/mission-tier"
   ```

   **The branch name is half the record.** A bare tier word would outlive the
   mission that wrote it, and the next branch checked out in that worktree
   would inherit an exemption it never asked for. `guardrails.sh` refuses a
   declaration naming any other branch, and refuses the one-field format
   outright rather than guessing.

   `pr-gate` reads that file and nothing else: a tier declared only in
   `GOAL.md` changes nothing at the gate. Rewrite the file with the same
   command whenever the tier is raised.

7. **Stay on track**: refuse scope creep. A necessary detour is stated
   explicitly and tied back to the mission, or taken to the user. Keep the
   checklist in the todo list so progress is visible. Narrowing the user's
   stated scope is a step 3 question, whenever it surfaces.

8. **Verify, then be graded.** Check off each criterion with its own evidence —
   command output, test result, screenshot path, review verdict — and write
   that evidence where the criterion said it would land. A criterion without
   evidence is unmet.

   **Archive before you review, not after.** The markers hold shas, so the
   archive has to be part of the branch the reviews actually graded.

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
   4. **`gh pr create`** — and **the PR body names the tier**, beside the four
      verification boxes. The third of the three places a tier is recorded, and
      the only public one: a `small` tier stated where reviewers look is one
      they can dispute; one stated only to the hook is one nobody can.

   A `small` mission still archives `GOAL.md`. Nothing grades it at that tier,
   and it is written anyway — the file is the only record of what the branch
   was for.

   The order is the whole point. Archive *after* recording the markers and that
   commit moves `HEAD`, both markers go stale, `pr-gate` denies — and the
   cheapest way out is to re-stamp both without re-running either review, which
   destroys the one property a sha-based marker exists to give. If either
   review sends you back to the code, you commit again and both markers are
   stale by design: re-run the review that owns each one before re-recording
   it.

9. **Hand over the `/goal` condition.** Skipped at `small`. At every other
   tier, right after step 4, print the built-in command for the user to paste:

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

Done = the PR is open, CI is green, `delivery-review` returned PASS, and the
evidence is in the PR body. At `small`, where that review does not run, done is
the same line without it — the PR open, CI green, `arch-review` closed, the
evidence in the body. Not merged — merging is the user's call, always. Do not
ask permission to push or open the PR; opening it *is* the mission's final
step, at every tier.
