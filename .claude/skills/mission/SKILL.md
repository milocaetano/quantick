---
name: mission
description: Define and enforce a mission for the current session — read the request into a traceable ledger, interrogate what is ambiguous, derive acceptance criteria including the standard quantick gates (arch-review, delivery-review, visual-qa, trader-ux-review, ui-harness hooks) that match the kind of work, keep every action aligned, prove every criterion with recorded evidence, and hand back a ready-to-paste /goal condition. Use when the user types /mission <objective> or asks to set a goal for the session/task.
---

# Mission

Argument: the session objective (e.g. `/mission make the heatmap render at
60fps`). If missing, ask the user what it is before doing anything else.

The mission is the orchestrator: it decides which of the other skills are part
of *done* so the user never has to list them. One session, one mission, one
branch, one worktree, one PR.

**This skill is not `/goal`.** Claude Code ships a built-in `/goal` that sets a
completion condition and keeps re-running turns until a small fast model
judges the condition met. It knows nothing about this repo. The two compose:
this skill decides *what done means*, `/goal` keeps the session from stopping
before it. Step 9 hands over the line to paste.

## The failure this skill is shaped around

A request carrying eight asks becomes six criteria, and the two that fell out
of the paraphrase are invisible from that moment on. Then the same agent that
wrote the criteria ticks its own boxes, and the trader finds the gap by using
the thing. Every step below closes one part of that: the ledger makes a
dropped ask visible, the interrogation makes a wrong reading expensive early
instead of late, the checklist format makes a criterion gradeable by someone
else, and `delivery-review` is that someone else.

## Steps

1. **Capture the mission**: restate the objective in one sentence **written in
   English**. That sentence is not a note to self — it becomes
   `.claude/GOAL.md`, the branch name and the first line of the PR body, every
   one of them a repository artifact. Saying it back to the trader in their own
   language too is welcome; the version written down is the English one.

2. **Build the request ledger.** Before deriving a single criterion, decompose
   the request into atomic asks, numbered `R1`…`Rn`.

   - An ask is **atomic** when it can be delivered, or not delivered, on its
     own. "X, and also Y" is two lines. A sentence naming a defect is an ask:
     the ask is that the defect goes.
   - The closing statement of purpose ("so that we can…") is an ask too, and
     the one that judges the others. Ledger it.
   - Keep the trader's own words as a **verbatim fragment** wherever the
     wording carries the ambiguity, or where restating it would put words in
     their mouth. Keep each *fragment* to the words that carry the ambiguity —
     a ledger line quoting three sentences where three words would do is not
     the exemption `CLAUDE.md` grants. The operative statement on each line is
     English. This is about the fragments in the table; the request quoted in
     full at the foot of the file is step 5's, and is required — see there for
     why it is one marked, attributed quotation rather than a licence to leave
     prose untranslated.
   - Map every `R` to at least one **`A` criterion**, and cite at least one `R`
     from every `A`. **An `R` with no criterion is a hole. An `A` with no `R`
     is scope you invented** — take it to the trader or drop it.
   - The injected gates, `G1`…`Gn`, carry **no** `R` tail and are not scope you
     invented: no trader ever asked for `cargo clippy` to pass. They come from
     step 4's table, which is their provenance. Only `A` lines answer to the
     ledger.
   - Numbers are stable for the life of the mission. Never renumber. An ask the
     trader withdraws stays on the ledger, struck through, with the reason.

   The ledger is what turns "they only did part of it" from a feeling into a
   detectable event.

3. **Interrogate — once, before any work starts.** Raise everything that
   qualifies in a single `AskUserQuestion` call (at most four questions,
   recommended option first, in whatever language the trader speaks).

   **What earns a question:**

   - **Ambiguous reference** — a word naming two different things in this repo,
     where the two lead to different code.
   - **Double meaning** — a phrase that reads two ways, and the two readings
     produce different software. This is the trader's own stated concern; take
     it literally.
   - **Contradiction** — two asks in one request that cannot both be satisfied,
     or an ask that contradicts something already shipped.
   - **A number nobody chose** — "fast", "a few", "most", where the code needs
     an exact value and the wrong one is expensive to reverse.
   - **A call that is the trader's** — autonomy (how much the agent may do
     unattended), money and safety (anything that can place, cancel or lose an
     order), taste (what the chart looks like), and irreversibility.
   - **A narrowing you are about to perform** — if you are about to deliver
     less than what was said, that is never a private decision.

   **What does not earn a question** — decide it, and record it as an
   assumption in step 5:

   - Anything with a conventional default in this repo: naming, file placement,
     test style, branch prefix, which crate it lands in.
   - Anything the code answers in under a minute of reading. Read it.
   - "Should I proceed?" — the mission is the answer.
   - A preference reversible in one edit.

   Write the answers into `GOAL.md` as `D1`…`Dn`, **Decisions taken by the
   trader**. A decision recorded there is settled: re-opening one is a scope
   change, not a judgement call.

   **When more than four things qualify** — and with six categories above, they
   will — rank by the cost of being wrong: how much work a wrong guess throws
   away, and how hard it is to reverse. Ask the top four. Record **every one
   you did not ask** as an `S` line marked *wanted to ask*, with the reading you
   went with. Do not let the cap swallow them: dropping the fifth ambiguity in
   silence is the same failure as dropping the fifth ask, and the `S` list is
   the only place `delivery-review` can find it.

   If nothing qualifies, say so in one line. "Nothing ambiguous enough to ask"
   is a legitimate outcome — but it is stated, never silent.

   After this round the mission runs on its own. A doubt surfacing later
   becomes an assumption, unless it is unsafe or would waste work already done
   if guessed wrong; those go to the trader when they arise.

4. **Classify it and inject the standard gates.** Derive the mission-specific
   criteria from the ledger — every `R` discharged — then add the gates for its
   kind:

   | The mission… | Injected acceptance criteria |
   | --- | --- |
   | Any mission at all | **every artifact in English** — the rule, its scope and its exemptions live in `CLAUDE.md`, which is already loaded and is their single owner; do not restate or count them here. Graded by `arch-review` dimension 8, enforced by `crates/app/tests/language_guard.rs`. It costs one edit now and a review round later |
   | Any code change | four checks green after rebasing on latest `main`; **performance impact declared** — classify every touched path by rate (per-trade / per-depth / per-frame / rare, the `arch-review` table) as part of the plan, not the review; `arch-review` run with every Blocker/Should-fix resolved or deferred in the PR body |
   | Touches a hot path (per-trade, per-depth, per-frame) | evidence that performance is flat or better, not a belief: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs. a `main` control run, or a bench over a fixture — measured before the PR, numbers in its body |
   | Touches anything user-visible | follow `ui-harness`: every new/changed surface reachable by env hook (hook added in the same change); `visual-qa` pass with all surfaces PASS or defects explicitly accepted; `trader-ux-review` with no unresolved Blocker |
   | Adds a capability (feed, bar type, indicator, layer, panel, crate) | follow `new-extension`: port named, registration-only edits, defaults preserve today's behaviour, fake second implementation tested, blast radius (added vs. edited files) stated in the PR body |
   | Adds something a trader *does* (an action, a tool, a trade, a lock) | drivable without a mouse — read `arch-review`'s *The second operator* and take its act/read/discover criteria from there, rather than from a summary that drifts. Where the capability class has no registry yet (there is none today for actions like a trade or a platform lock), carving one is part of the work, per `new-extension`'s carve-the-port rule — name it in the plan or state why the capability stays local |
   | Engine / determinism territory | test-first: fixture + expected output written before the code; golden test guards determinism |
   | Docs/skills only | four checks still run (they are cheap when nothing compiled changed); `arch-review`'s shape dimensions 1–7 waived — **dimension 8 (English) is not**, since docs are exactly where a foreign-language line hides, and neither is its step 0 bug pass; `pr-gate` still wants both markers, so both review skills run and report what they found. The waiver covers prose. A shell script, a config file or a test shipping alongside the prose is not prose — it takes the full shape pass |

   Write down what is **not applicable and why**, too. A gate silently omitted
   and a gate deliberately excluded look identical to the next reader, and only
   one of them is honest.

   ### Closing steps are not criteria

   Two things finish every mission and **neither is an `A` or a `G`**:
   `delivery-review` returns PASS, and the PR is open. List them separately, as
   `C1`…`Cn` under a **Closing steps** heading. Archiving `GOAL.md` is *not*
   among them — step 8 puts it before the reviews, as the branch's last commit,
   and listing it here would send a reader to do it afterwards and stale both
   markers.

   They are not criteria because they cannot be graded when the grading
   happens. `delivery-review` reads the checklist and grades every `A` and `G`
   against the shipped branch — but its own verdict does not exist while it is
   being written, and `pr-gate` will not let the PR open until that verdict is
   recorded. Written as criteria, those two lines come back UNPROVEN on every
   mission, the fix loop burns three rounds on gaps no edit can close, and the
   gate escalates to the trader every single time. A gate that always fails
   teaches everyone to ignore it, which costs more than not having it.

   Present the merged checklist to the user before starting work.

5. **Persist it — in the worktree, which means step 6 happens first.** Cut the
   branch and the worktree before writing anything, then write the mission to
   `<worktree>/.claude/GOAL.md`, in English, so it survives context compaction.
   Overwrite any previous one.

   The order matters and used to be the other way round. A `GOAL.md` written
   into the main checkout is not on the branch, so step 8's `git mv` has no
   source to rename there and stages a commit onto `main` if run from the main
   checkout — and `delivery-review`, which looks for the checklist *on the
   branch*, returns NOT GRADEABLE. The stranded `GOAL-archive-*.md` files
   sitting untracked in the main checkout are what that ordering leaves behind. The file keeps its
   name: dozens of archives already use it, and renaming the record would buy
   nothing.

   `GOAL.md` carries, in this order: the objective and why it matters; the
   request ledger; the trader's decisions `D1`…`Dn`; the assumptions
   `S1`…`Sn`; the acceptance criteria; what is not applicable and why; and
   last, **the request as received, quoted in full and verbatim**.

   That last section is not decoration and it is not optional. `delivery-review`
   reads `GOAL.md` and nothing else — it never sees this conversation. Without
   the original request in the file, the ledger becomes its own source of
   truth, and an ask this step dropped while *writing* the ledger is an ask no
   reviewer can ever find. With it, the reviewer re-derives the asks from the
   trader's own words and reports what the ledger failed to carry. The verbatim
   request is what makes the gate something other than the mission grading its
   own summary, which is why `delivery-review` refuses to grade a goal file
   that lacks it.

   On the language rule: this is **one marked, attributed quotation** — the
   request as received, in a headed section that names its source and date —
   which is the shape `CLAUDE.md`'s exemption describes. It is not a licence to
   leave prose untranslated elsewhere in the file: every other line, including
   the operative statement on each ledger row, is English. Say in the section's
   own preamble why the words are not translated, so the exemption is claimed
   openly rather than assumed.

   ### The checklist format

   This format is a contract, not a style preference: `delivery-review` reads
   it, and a criterion it cannot grade is a criterion nobody grades. Every line
   carries four things.

   ```markdown
   - [ ] **A3** — <one observable outcome, stated so two readers would agree
         whether it happened>.
         *Evidence:* <what proves it — a named test, a command's exit code, a
         screenshot, a review verdict, a quoted section of a file>.
         → <path where that evidence will be written>. *(R3, R4)*
   ```

   - **Stable ID.** `A1`…`An` for mission-specific, `G1`…`Gn` for the injected
     gates. Never renumbered, so a review round can name a line and still be
     understood a week later.
   - **One observable outcome.** Not "the ledger works well" — "every ask
     appears as a numbered line mapped to a criterion". If you cannot say how
     an outsider would check it, the criterion is not finished being written.
   - **Evidence kind, and the path it lands at.** Naming the path in advance is
     what stops evidence from being remembered instead of recorded. A criterion
     whose evidence exists only as a claim in the session transcript comes back
     from `delivery-review` as **UNPROVEN**, which is not a pass.
   - **Ledger back-reference.** The `(R…)` tail, on `A` lines only — `G` lines
     come from the gate table, not from an ask. It is how a dropped ask gets
     found before the PR instead of after.

   Assumptions get their own list, `S1`…`Sn`, each with the reason it was safe
   to assume rather than ask. `delivery-review` audits that list too: an
   assumption that turned out to drive the design is a question that should
   have been asked in step 3.

6. **Set up the ground — before step 5 writes anything.** Fresh worktree from
   updated `main` under `../quantick-worktrees/` per CLAUDE.md; never work in
   the main checkout, and check the worktree for a live writer before the first
   write. The `worktree-guard` hook denies the write if this step is skipped.
   It is numbered after step 5 because the checklist is *derived* first — but
   it is *performed* first, so the goal file lands on the branch that will
   carry it.

7. **Stay on track**: refuse scope creep. A necessary detour is stated
   explicitly and tied back to the mission (or taken to the user). Keep the
   checklist in the todo list so progress is visible. Narrowing the user's
   stated scope is not a judgement call to make alone — it is a step 3
   question, whenever it surfaces.

8. **Verify, then be graded.** Check off each criterion with its own evidence —
   command output, test result, screenshot path, review verdict — and write
   that evidence where the criterion said it would land. A criterion without
   evidence is unmet.

   Then run both reviews. They answer different questions and neither
   substitutes for the other — one asks whether this is well built, the other
   whether it is what was asked for:

   **Archive before you review, not after.** The markers hold shas, so the
   archive has to be part of the branch the reviews actually graded:

   1. **Archive**, as the mission's last commit, before either review runs.
      Assign the slug first: an unquoted `<slug>` is two shell redirections and
      not a placeholder, so pasted verbatim it renames the file to
      `GOAL-archive-.md` and reads a file called `slug`.

      ```sh
      SLUG=my-mission-slug
      git mv .claude/GOAL.md ".claude/GOAL-archive-$SLUG.md"
      git commit -m "docs: archive the $SLUG mission"
      ```

   2. **`Skill(arch-review)`** — shape and bugs, over the final branch. It
      records `arch-review-ok` itself when the review closes.
   3. **`Skill(delivery-review)`** — conformance, over the same final branch.
      It records `delivery-review-ok` itself, on PASS only.
   4. **`gh pr create`.**

   The order is the whole point, and getting it backwards is a trap with a
   pleasant-looking exit. Archive *after* recording the markers and that commit
   moves `HEAD`, both markers go stale, `pr-gate` denies — and the cheapest way
   out is to re-stamp both markers without re-running either review, which
   silently destroys the one property the sha-based marker exists to give.
   Nothing would catch that; the gate would still say two reviews passed.

   `delivery-review` runs after `arch-review` because it grades the branch as
   shipped, including whatever the shape review made you change. It reads the
   checklist from the archived file at that point, which it knows how to find.

   If either review sends you back to the code, you commit again — and then
   both markers are stale by design. Re-run the review that owns each one
   before re-recording it. Re-stamping a marker whose review did not run again
   is the one dishonest move this whole mechanism cannot detect.

9. **Hand over the `/goal` condition.** Right after step 4, print the built-in
   command for the user to paste, so the session keeps working across turns
   without them prompting each step:

   ```text
   /goal <the criteria from step 4, as one measurable end state, plus "or stop after N turns">
   ```

   Rules the built-in imposes on that line:

   - **4,000 characters maximum.** If the criteria from step 4 do not fit,
     compress rather than drop: state each criterion as a terse observable
     outcome ("clippy/fmt/build/test exit 0", "delivery-review returned PASS",
     "PR URL printed", "GOAL.md archived"), strip all rationale and repo
     context, and collapse per-surface detail into one line ("all visual-qa
     surfaces PASS"). Only if it still overflows, keep the gates (checks, both
     reviews, PR, archive) and summarize the mission-specific criteria into the
     fewest observable outcomes that still prove them. Count the characters
     before printing the line.
   - The evaluator **does not run commands or read files**. It only judges what
     has appeared in the conversation, so every criterion must be something
     this session's own output demonstrates — "`cargo test --workspace` exits
     0 and the PR URL is printed", not "the code is correct".
   - Include a bound (`or stop after 20 turns`) so a stuck mission ends.
   - It does not change permissions. Pair with auto mode for unattended runs.
   - Write the line in English, like the criteria it restates. Those same
     sentences go into the PR body, and a condition written in one language
     while the record is in another is two things to keep in sync.

## What done means

Done = the PR is open, CI is green, `delivery-review` returned PASS, and the
evidence is in the PR body. Not merged — merging is the user's call, always.
Do not ask permission to push or open the PR; opening it *is* the mission's
final step.
