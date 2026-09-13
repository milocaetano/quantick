# Goal: synchronize main 5c7b7d1a into campaign/lean-a-plus

Bring `origin/main` at `5c7b7d1a` (#433, quick range volume profiles) into
`campaign/lean-a-plus` through one reviewed merge commit, preserving every
behaviour both sides added and every campaign invariant, so that the campaign
keeps building on the trunk the trader uses and its consolidated PR does not
inherit a stale base (integration contract step 6).

**Tier:** high — the coordinator assigned it: a merge across the app's pane
owners, the control plane's annotate module and matrices, and the guard
baselines, where a wrong resolution silently drops behaviour.

Campaign synchronization X3 of #367. Base: `campaign/lean-a-plus` at
`5222c911`. Source: `origin/main` at `5c7b7d1a`. Template: X2, PR #437.

## Request ledger

- **R1** — Bring main at `5c7b7d1a` into the campaign with a `--no-ff` merge
  commit, first parent the campaign tip, conflicts resolved in that commit,
  no rebase and no force push; the message names the source SHA and #433.
- **R2** — Preserve both sides' behaviour: where #433 changed code the
  campaign moved into siblings, apply the change to the sibling that now owns
  it ("re-home, don't reintroduce"); #433's right-drag range, its action bar,
  its dismissal and its capability behave as on main.
- **R3** — Keep the campaign invariants: every production file at or under
  1,500 production lines with the size baseline empty and `!budget 0`; the
  chunked tape (no append copies more than one chunk; counts-based
  session-length tests); the live-envelope bounds; the retry and UI behaviour
  matrices with rows for #433's additions (the retry row with a readback);
  released-schema compatibility green without regenerating released
  baselines; generated artifacts regenerated, never hand-merged; extension
  baselines at measured numbers.
- **R4** — Deliver it as a draft PR against `campaign/lean-a-plus`, reviewed
  at tier high, with CI watched and readiness requested; never merge, never
  touch main.
- **R5** — (Coordinator, mid-task) Merge the campaign's new tip (`8aa3dc03`,
  #446) into this branch as a normal merge commit, rerun
  `guardrails_test.sh`, and run `mission_ship_gate.sh ship <n>` as the last
  step after CI and readiness, reporting its output.
- **R6** — (Coordinator, mid-task, D28) Keep the four repairs of #433's code
  with their failing-first tests, name them and D28 in the PR body under
  their own heading, and file one follow-up issue for the four deferred
  round-three findings.

## Decisions (from the coordinator's brief)

- **D1** — `git merge --no-ff origin/main`; resolve in the merge commit.
- **D2** — Re-home into the owning sibling (`pane.rs` conflicts, the roots
  baseline, the capability inventory); preserve #433's behaviour and every
  campaign invariant.
- **D3** — Size baseline stays empty with `!budget 0`; every production file
  at or under 1,500; extension baselines via `--tighten`, any raise signed.
- **D4** — Regenerate the capability inventory, observer catalog, UI
  behaviour matrix and retry matrix through their documented commands; #433's
  annotate and gesture additions get retry (with readback) and UI rows;
  released schemas untouched.
- **D5** — Validation: the four checks each alone (tests with
  `env -u QUANTICK_BUBBLES`), `cargo test -p quantick-guards`,
  `guardrails_test.sh`, bridge checks only if bridge files changed, #433's
  own tests by name, the chunked-tape and session-length tests by name.
- **D6** — Reviews: arch-review (step 0 code-review at `medium`, focused on
  the conflict resolutions, at most three rounds), ai-review, delivery-review.
- **D7** — Draft PR with base exactly `campaign/lean-a-plus`; `gh pr ready`
  once, via Bash with the cd prefix.
- **D8** — (Coordinator decision D26 on #367) `mission_ship_gate.sh` does not
  govern synchronization PRs; the hooks are not touched here.
- **D28** — (Coordinator decision D28, mid-task; source: the coordinator's
  second message, quoted below) Keep the four repairs of #433's code
  (`6b181df5`: the right-drag no longer pans, a slipped right-click raises no
  range; `ba900a24`: a hidden pane's action bar goes away, the stale-divider
  clamp no longer panics), each with its failing-first test: confirmed
  correctness defects, carried to main by the consolidated campaign PR, so no
  separate main port. Name the four fixes and D28 in the PR body under their
  own heading, and file one follow-up issue for the four deferred round-three
  findings (labels `area:app` `type:fix`, milestone "v0.1 - Engine core",
  each with its evidence, "Found by campaign #367 sync X3 (PR #445)").

## Assumptions

- **S1** — `annotate.fixed_range_profile.create` takes the same readback as
  the other annotate creates (`analysis.drawings`,
  `tabs[].panes[].drawings[].author.client_name`, "created by caller"): it
  places through the same `place` path with the same descriptor, so its
  policy is `forbidden` and its effect is one attributed drawing. Safe: the
  every-forbidden-row and annotation transport tests now drive it and pass.
- **S2** — The fixed-range-profile tool's UI row moves from
  `pending_capability` to the new capability, as the arrow, text and
  rectangle rows map to theirs; the new right-drag is its own `Authored`
  row mapped to the same capability, because #433's action bar places the
  profile by dispatching that capability (`app/drawing_input.rs`
  `apply_quick_range`). Safe: additive rows, the drift guard and the
  generator tests pass, and the prose counts are updated with it.
- **S3** — The bridge Python checks are not run: #433 changes no file under
  `bridge/` or `tools/`. Safe: D5 makes them conditional on that.

- **S4** — (Added after step 0, round one.) Two defects step 0 confirmed in
  #433's own code — a right-drag also panned the chart, collapsing the range
  onto one bar; a right-click slipping 4-6 px raised a range beside the menu —
  are repaired in this branch rather than deferred, because arch-review
  grades a confirmed correctness finding as a Blocker and R2 asks for #433's
  behaviour, which the defects defeat. Safe: two small, tested edits in the
  files that own the gestures (`pane.rs` body pan, `pane/quick_range.rs`
  threshold, `surfaces/drawing_chrome/quick_range.rs` comparison); main still
  carries the defects until the consolidated PR or a port, which the handoff
  reports to the coordinator. Step 0 round two found two more in the same
  code — the action bar kept the geometry of a pane a layout stopped
  painting, and the right-drag's pointer clamp panicked on a stale lane
  divider — repaired the same way, for the same reason. *Wanted to ask* the
  coordinator before the first repair (delivery-review: this assumption
  drove the design); asked after that review, and answered by **D28**, which
  replaces this assumption as the authority for the repairs.

## Acceptance criteria

- [ ] **A1** — The merge commit's first parent is `5222c911`, its second
      `5c7b7d1a`, and its message names `5c7b7d1a` and #433.
      *Evidence:* `git log -1 --format='%P %s'` of the merge commit, quoted
      in the PR body. → PR body. *(R1)*
- [ ] **A2** — Every conflicted hunk is listed with both sides and its
      resolution, each re-homed change naming the sibling that owns it; every
      line #433 added to a conflicted code file is present after the merge.
      *Evidence:* the conflict table. → PR body. *(R1, R2)*
- [ ] **A3** — #433's own tests pass by name on the merged head:
      `a_secondary_drag_is_temporary_until_it_is_dismissed_or_converted`,
      `the_regular_ruler_remains_after_an_unrelated_chart_click`, the four
      `quick_range` tests,
      `a_fixed_range_profile_is_reachable_as_an_annotation_capability`,
      `only_the_secondary_button_opens_the_layer_menu`,
      `a_right_click_on_the_tape_configures_the_tape_without_losing_the_chart`,
      `the_committed_registry_is_what_the_generator_emits` (hooks).
      *Evidence:* named test runs. → PR body. *(R2)*
- [ ] **A4** — The size guard passes with `crates/guards/size-baseline.txt`
      unchanged (no entry, `!budget 0`) and every production file at or under
      1,500; the extension roots baseline is what `--tighten` wrote, with the
      reason recorded. *Evidence:* `cargo test -p quantick-guards` and
      `--report`. → PR body. *(R3)*
- [ ] **A5** — The chunked tape and live envelope hold:
      `building_the_tape_live_never_copies_more_than_one_chunk`,
      `the_growth_check_fails_a_contiguous_tape`, the `session_length_tests`,
      `deal_bars_on_a_chunked_tape_show_what_a_contiguous_tape_showed` and the
      three envelope tests pass; `crates/app/src/app/health/` and
      `crates/app/src/worker_progress/` are identical to the base.
      *Evidence:* named test runs and an empty `git diff` against the base.
      → PR body. *(R3)*
- [ ] **A6** — Generated artifacts are regenerated by their commands; the
      retry matrix has a row for `annotate.fixed_range_profile.create` with a
      readback, driven by the annotation and every-forbidden-row transport
      tests; the UI behaviour matrix maps `tool.fixed-range-profile` and a new
      `drawing.quick_range_profile` row to the capability; the released-schema
      compatibility tests pass and `schemas/control/released/` is untouched.
      *Evidence:* dump commands, `retry_matrix` and `operability` tests, the
      rows in `docs/control-plane/{retry-matrix,ui-behaviour-matrix}.md`,
      `published_schema_compatibility` tests, the empty released diff.
      → PR body. *(R3)*
- [ ] **A10** — One follow-up issue lists the four deferred round-three
      findings with their evidence, labelled `area:app` `type:fix`, milestone
      "v0.1 - Engine core", saying "Found by campaign #367 sync X3 (PR #445)".
      *Evidence:* `gh issue view 447` (https://github.com/milocaetano/quantick/issues/447), linked from the PR body. → PR body.
      *(R6)*
- [ ] **A8** — #433's right-drag measures the bars it crossed without panning
      the chart, and a right-click slipping within egui's click distance
      raises no range; hiding the owning pane takes the action bar away; a
      stale lane divider left of the band does not panic. *Evidence:*
      `a_secondary_drag_measures_the_bars_it_crossed_without_panning`,
      `a_right_click_that_slips_within_the_click_distance_raises_no_range`,
      `hiding_the_pane_that_owns_a_range_takes_its_action_bar_away` and
      `a_right_drag_survives_a_stale_divider_left_of_the_band` pass, each
      failing before its repair; and the pan repair's side effect — a middle
      drag, which the body pan also answered, now follows the pointer 1:1
      instead of twice as far — is pinned by
      `a_middle_drag_pans_with_the_pointer_not_twice_as_far` (-200 px for a
      100 px drag before the repair); the PR body names the four fixes and D28
      under their own heading. → PR body. *(R2, R6)*
- [ ] **A9** — The campaign tip `8aa3dc03` (#446) is merged into the branch
      by a merge commit, the main merge `2b2f7e5c` is not rewritten, and
      `guardrails_test.sh` passes after it. *Evidence:* the merge commit's
      parents, `git merge-base --is-ancestor origin/campaign/lean-a-plus
      HEAD`, the hook suite's count. → PR body. *(R5)*
- [ ] **A7** — A draft PR with base `campaign/lean-a-plus` exists and is never
      merged by this mission. *Evidence:* `gh pr view` state. → handoff.
      *(R4)*

## Injected gates

- [ ] **G1** — Every artifact in English. *Evidence:* arch-review dimension 8
      and the language guard. → PR review report.
- [ ] **G2** — Four checks green, each run alone. *Evidence:* local logs and
      final-head CI. → PR body.
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate. *Evidence:* the PR body's performance section. → PR body.
- [ ] **G4** — `arch-review` run, every Blocker/Should-fix resolved or
      deferred in the PR body. *Evidence:* its report URL. → PR body.
- [ ] **G5** — Adds something a trader does (#433's right-drag profile):
      drivable without a mouse. *Evidence:* the capability inventory,
      retry-matrix and UI-matrix rows for `annotate.fixed_range_profile.create`.
      → PR body.
- [ ] **G6** — The brief's extra validation (D5): `cargo test -p
      quantick-guards`, `sh .claude/hooks/guardrails_test.sh`. *Evidence:*
      logs and the PR body's verification section. → PR body.
- [ ] **G7** — The brief's constraint that another child owns
      `.claude/hooks/*`, `docs/campaign/integration.md` and the ship skill:
      this branch's diff against its base edits none of them. *Evidence:*
      `git diff --stat origin/campaign/lean-a-plus...HEAD -- .claude/hooks
      docs/campaign/integration.md .claude/skills/ship` is empty. → PR body.
- [ ] **G8** — Touches something user-visible (S4's repairs change how the
      right-drag behaves: it no longer pans, a slipped right-click raises no
      range, a hidden pane's action bar goes away; and a middle drag pans 1:1
      instead of twice as far): `trader-ux-review` of the
      right-drag flow with no unresolved Blocker; every state the repairs
      touch is reachable without a hand — the range states by #433's
      `QUANTICK_QUICK_RANGE_DEMO` hook and by the gateway's
      `annotate.fixed_range_profile.create`, the hidden pane by
      `layout.preset.apply`; no repair changes how any state is drawn, so
      #433's `visual-qa` captures stand and are not re-taken. *Evidence:* the
      trader-UX report on the PR, and the four A8 tests driving the states
      through the app frame. → PR comment and PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open against `campaign/lean-a-plus`.
- **C3** — `gh pr ready` requested once, via Bash with the cd prefix, after
  the reviews and final-head CI.
- **C4** — `sh .claude/hooks/mission_ship_gate.sh ship <n>` from the worktree
  after C3 returns `MISSION-COMPLETION: PASS` as a main sync (zero or one own
  archive); its output, or any refusal and its reason, reported to the
  coordinator (R5; #446 made it campaign-aware, superseding D8's exemption).
- **C5** — The handoff block the brief lists (branch; PR URL; head; base tip;
  conflict table; invariants; guard output; regenerated artifacts and matrix
  rows added; review verdicts with URLs; markers, or the exact `printf` lines
  if writing them is denied; CI; ready accepted or denied; any
  human_decision; the coordinator's next action) returned to the
  coordinator.

## Not applicable

- *Touches a hot path*: #433's per-frame paths (the quick-range input and
  paint) were measured on main in #433 (59-60 fps, flat against main); this
  merge re-homes one paint call and adds no per-print work. S4's repairs
  edit per-frame paths at constant cost — one more button test in the pan
  condition, one `options` read per pane frame, three `Option` moves in the
  surface, one `max` before the clamp — with no loop or allocation, so a
  measurement would compare noise; the chunked tape's counts tests (A5) are
  the campaign's hot-path proof.
- *Touches anything user-visible*: no longer claimed not applicable — S4's
  repairs change the right-drag's behaviour; graded under G8.
- *Adds a capability* / *Engine determinism*: the capability arrives already
  reviewed from main; no engine code changes. This mission's own new code is
  matrix rows, two transport-test entries, and S4's four repairs with their
  tests in `pane.rs`, `pane/quick_range.rs` and `surfaces/drawing_chrome/`.
- *Bridge Python checks*: no bridge or tools file changes (S3).

## The request as received

> Attributed quotation: the coordinator's brief for task X3, verbatim.

> You are executing campaign synchronization task **X3** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: merge `origin/main` at `5c7b7d1a` (PR #433, "feat(app): add quick range volume profiles", 39 files) into `campaign/lean-a-plus` through a reviewed PR. This is the same procedure as X2 (PR #437, read its body and review comments first: it is your template, including its conflict table, the re-homing approach and the checks it ran).
>
> ## Read first
> 1. `C:\src\quantick\CLAUDE.md`; `C:\src\quantick\docs\campaign\integration.md` step 6; `C:\src\quantick\docs\workflow\delivery.md`; `C:\src\quantick\.claude\skills\mission\SKILL.md` (tier `high`; steps 1, 2, 4, 5, 7, 8, 9; step 6 done).
> 2. PR #437 (`gh pr view 437 --comments`) and PR #433 (`gh pr view 433`; `git show --stat 5c7b7d1a^2` and the diff against the campaign).
> 3. Coordinator decision D26 on #367: `mission_ship_gate.sh` does not govern sync PRs (follow-up #439 is being implemented in parallel by Q15; do not touch the hooks).
>
> ## Worktree
> - `C:\src\quantick-worktrees\sync-main-5c7b7d1a` (Git Bash `/c/src/quantick-worktrees/sync-main-5c7b7d1a`), branch `sync/main-5c7b7d1a`, at the campaign tip `5222c911`. Every command starts with `cd /c/src/quantick-worktrees/sync-main-5c7b7d1a &&`. Never write to `C:\src\quantick` or other worktrees.
>
> ## Decisions (as X2's)
> - D1: `git merge --no-ff origin/main`, conflicts resolved in the merge commit; message names source `5c7b7d1a` and #433.
> - D2: re-home main's changes into the campaign's split siblings (the trial merge conflicts in `crates/app/src/pane.rs`, `crates/guards/extension-roots-baseline.txt`, `docs/control-plane/capability-inventory.md`; the rest may auto-merge); preserve #433's behaviour and every campaign invariant (chunked tape, no append copying more than one chunk, the session-length counts tests, the live-envelope bounds).
> - D3: size baseline stays empty with `!budget 0`; every production file at or under 1,500 production lines (split by owner if a re-homed change crosses it); extension baselines through `cargo run -p quantick-guards -- --tighten`, any raise signed with its reason.
> - D4: regenerate every generated artifact through its documented command (capability inventory, observer catalog, UI behaviour matrix, retry matrix); #433's new annotate/gesture additions must have rows in the retry matrix (with readback) and the UI behaviour matrix (capability or classified exclusion); released schemas untouched; `published_schema_compatibility` green without regenerating released baselines.
> - D5: validation: the four ordered checks, each alone; `cargo test -p quantick-guards`; `sh .claude/hooks/guardrails_test.sh`; bridge Python checks if bridge files changed; #433's own tests by name; the counts-based session-length and tape tests by name.
> - D6: reviews at tier `high`: `arch-review` (step 0 `code-review` at `medium`, focused on the conflict resolutions, every conflicted hunk listed), `ai-review` completion, `delivery-review` (R1 bring main 5c7b7d1a in; R2 preserve both sides; R3 campaign invariants). At most three step-0 rounds.
> - D7: draft PR base exactly `campaign/lean-a-plus`: `cd <wt> && gh pr create --draft --base campaign/lean-a-plus --title "chore(campaign): synchronize main 5c7b7d1a into campaign/lean-a-plus" --body-file -` with a heredoc body (tier; "Campaign synchronization X3 of #367 (integration.md step 6); source main 5c7b7d1a (#433)"; conflict table; invariants; regenerated artifacts; local verification; then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`). Watch CI; `gh pr ready <n>` once via Bash with the cd prefix. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Notes
> - Another child (Q15) edits `.claude/hooks/*`, `docs/campaign/integration.md` and the ship skill in parallel; do not edit those.
> - Use `python`, not `python3`. Scratch files under `-x3` paths. Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`. If writing review markers is denied, report the exact `printf` lines.
>
> Return a HANDOFF BLOCK like X2's: branch; PR URL; head; base tip; conflict table; invariants; guard output; regenerated artifacts and matrix rows added; review verdicts with URLs; markers; CI; ready accepted or denied; any human_decision; the coordinator's next action.

> Attributed quotation: the coordinator's mid-task message, verbatim.

> Coordinator FYI for X3: PR #446 (Q15, campaign-aware `mission_ship_gate.sh`; hooks, `docs/campaign/integration.md` and the ship skill only, no crate files) just merged into `campaign/lean-a-plus`. Before opening your PR (or now, if you are past the main merge), fetch and merge the new campaign tip into `sync/main-5c7b7d1a` as a normal merge commit, and rerun `sh .claude/hooks/guardrails_test.sh`. With it, your sync PR can also run `sh .claude/hooks/mission_ship_gate.sh ship <n>` from your worktree and should get `MISSION-COMPLETION:PASS` as a main sync (zero or one own archive); do that as the last step after CI and ready, and report the output. Everything else stands.

> Attributed quotation: the coordinator's second mid-task message (decision D28), verbatim.

> Coordinator decision D28 for X3: KEEP the four repairs of #433's code (6b181df5, ba900a24), each with its failing-first test. Reasons: they are confirmed correctness defects (one is a panic), the consolidated campaign PR carries them to main anyway, so no separate main port is needed and the divergence lasts only until that PR merges; reverting them would ship known defects through a reviewed sync. Record D28 in your goal archive with this message as its source (replacing assumption S4 as the authority), name the four fixes and D28 in the PR body under their own heading, and file ONE follow-up issue for the four deferred round-three findings (labels area:app type:fix, milestone "v0.1 - Engine core", body lists each with its evidence, "Found by campaign #367 sync X3 (PR #445)"). Delivery-review's completeness line for S4 is then satisfied by D28. Also merge the new campaign tip (#446, hooks only) into your branch as I wrote earlier. Proceed.
