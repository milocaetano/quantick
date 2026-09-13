# Goal: synchronize main d8de9a1b into campaign/lean-a-plus

Bring `origin/main` at `d8de9a1b` (#393 mission completion gates, #306 trades
bars for MetaTrader B3, #420 copy and paste chart drawings) into
`campaign/lean-a-plus` through one reviewed merge commit, preserving every
behaviour both sides added and every campaign invariant, so that the campaign
keeps building on the trunk the trader uses and its consolidated PR does not
inherit a stale base (integration contract step 6).

**Tier:** high — the coordinator assigned it: a merge across the engine, the
MetaTrader bridge and feed, the app's pane/tab/state owners, the control plane
and the guard baselines, where a wrong resolution silently drops behaviour.

Campaign synchronization X2 of #367. Base: `campaign/lean-a-plus` at
`6ba1a7f3`. Source: `origin/main` at `d8de9a1b`.

## Request ledger

- **R1** — Bring main at `d8de9a1b` into the campaign with a `--no-ff` merge
  commit, first parent the campaign tip, conflicts resolved in that commit, no
  rebase and no force push; the message names the source SHA and the PRs.
- **R2** — Preserve both sides' behaviour: where main changed code the
  campaign moved into siblings, apply main's change to the sibling that now
  owns it ("re-home, don't reintroduce"); #306's deal-counter state composes
  with Q12's chunked tape (#431) and Q3's envelope counters (#405).
- **R3** — Keep the campaign invariants: every production file at or under
  1,500 production lines with the size baseline empty and `!budget 0`; the
  chunked tape (no append copies more than one chunk; counts-based
  session-length tests); the retry and UI behaviour matrices with a row for
  every new capability; released-schema compatibility green without
  regenerating released baselines; generated artifacts regenerated, never
  hand-merged; extension baselines at measured numbers.
- **R4** — Deliver it as a draft PR against `campaign/lean-a-plus`, reviewed
  at tier high, with CI watched and readiness requested; never merge, never
  touch main.

## Decisions (from the coordinator's brief)

- **D1** — `git merge --no-ff origin/main`; resolve in the merge commit.
- **D2** — Re-home into the owning sibling; combine #306's state with #431 and
  #405 with no contiguous-slice assumption.
- **D3** — Size baseline stays empty with `!budget 0`; discard main's raised
  ceilings; extension baselines via `--tighten`, any raise carries a reason.
- **D4** — Regenerate `capability-inventory.md`, the observer capability
  catalog, the UI behaviour matrix and the retry matrix through their
  documented commands; new capabilities get matrix rows.
- **D5** — Validation: the four checks each alone (tests with
  `env -u QUANTICK_BUBBLES`), guards, `guardrails_test.sh`, the bridge Python
  checks, and #306's own key tests by name.
- **D6** — Reviews: arch-review (step 0 code-review at `medium`, at most three
  rounds), ai-review, delivery-review.
- **D7** — Draft PR with base exactly `campaign/lean-a-plus`; `gh pr ready`
  once, via Bash with the cd prefix.

## Assumptions

- **S1** — `layout.pane.set_bar_spec` (new in #306) answers with the v1
  `LayoutResult`, whose float `fraction` the control wire refuses; the
  campaign's rule for every such layout call is a version 2 that answers with
  an exact decimal (`control/layout/v2.rs`). It is docked into v2 as the
  eighth call. Safe: additive, v1 unchanged, and it is the campaign's own
  established fix for the same defect class.
- **S2** — `feed.deal_recording.set`'s retry-matrix row is proven by the
  every-optional-row transport test on a tab whose feed declares no counter
  (`Stays`), the way `feed.reconnect` and `feed.reload` are: a test app has
  no MetaTrader venue. Safe: the row's field exists in the published
  `feed.status` schema, which the matrix's drift check verifies.
- **S3** — The UI behaviour matrix needs no new rows: its generator and drift
  guard pass unchanged after the merge, and #306/#420 registered their own
  operability entries on main. Safe: the guard is the authority D4 names.

## Acceptance criteria

- [ ] **A1** — The merge commit's first parent is `6ba1a7f3`, its second
      `d8de9a1b`, and its message names `d8de9a1b`, #393, #306 and #420.
      *Evidence:* `git log -1 --format='%P %s%n%b'` quoted in the PR body.
      → PR body. *(R1)*
- [ ] **A2** — Every conflicted hunk is listed with both sides and its
      resolution, each re-homed change naming the sibling that owns it.
      *Evidence:* the conflict table. → PR body. *(R1, R2)*
- [ ] **A3** — #306's deal bars cut on the chunked tape exactly what a
      contiguous tape cut, on every way in (backfill, live interleaved,
      prepended history, seeded, spec switch, refold, keeping reset), across
      three chunk boundaries.
      *Evidence:* `state::tape_identity_tests::deal_bars_on_a_chunked_tape_show_what_a_contiguous_tape_showed`
      passes. → PR body, verification section. *(R2, R3)*
- [ ] **A4** — #306's and #420's key tests pass by name on the merged head:
      `another_n_recuts_the_same_day`, `interleaved_samples_cut_where_a_rebuild_cuts`,
      `a_reading_corrects_the_estimate_without_cutting_twice`,
      `golden_cuts_at_the_sessions_multiples_of_n`, `same_input_same_bars`,
      `a_deal_count_change_recuts_the_bars`,
      `a_reset_drops_the_readings_unless_the_market_is_the_same`, the bridge
      `test_deals.py` suite, and the drawing clipboard tests.
      *Evidence:* named test runs. → PR body. *(R2)*
- [ ] **A5** — The size guard passes with `crates/guards/size-baseline.txt`
      holding no entry and `!budget 0`; extension roots at the measured
      585/10241 with the reason recorded.
      *Evidence:* `cargo test -p quantick-guards` and `--report`.
      → PR body. *(R3)*
- [ ] **A6** — The chunked tape's invariants hold:
      `building_the_tape_live_never_copies_more_than_one_chunk` and the
      counts-based session-length tests pass.
      *Evidence:* named test runs. → PR body. *(R3)*
- [ ] **A7** — Generated artifacts are regenerated by their commands, the
      retry matrix has rows for `feed.deal_recording.set` and
      `layout.pane.set_bar_spec`, and the released-schema compatibility tests
      pass with no released baseline regenerated.
      *Evidence:* dump commands, `retry_matrix` tests, schema compatibility
      tests, and `git diff` showing no released baseline rewritten.
      → PR body. *(R3)*
- [ ] **A8** — The campaign's fixed version of #361's test (#382) is kept.
      *Evidence:* `git diff 6ba1a7f3 -- crates/app/src/worker_progress/` is
      empty. → PR body. *(R2)*
- [ ] **A9** — A draft PR with base `campaign/lean-a-plus` exists, readiness
      requested once, never merged. *Evidence:* PR URL and state.
      → handoff. *(R4)*

## Injected gates

- [ ] **G1** — Every artifact in English. *Evidence:* arch-review dimension 8
      and the language guard. → PR review report.
- [ ] **G2** — Four checks green, each run alone. *Evidence:* local logs and
      final-head CI. → PR body.
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate. *Evidence:* the PR body's performance section. → PR body.
- [ ] **G4** — `arch-review` run, every Blocker/Should-fix resolved or
      deferred in the PR body. *Evidence:* its report URL. → PR body.
- [ ] **G5** — Adds something a trader does (#306's REC and bar rule, #420's
      copy/paste): drivable without a mouse. *Evidence:* the capability
      inventory rows and retry-matrix rows for the two new capabilities; #420's
      shortcut is keyboard-first. → PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open against `campaign/lean-a-plus`.
- **C3** — The final verifier (`mission_ship_gate.sh mission <pr>`) PASS, or
  its refusal reported to the coordinator.

## Not applicable

- *Touches a hot path*: the per-print paths changed are #306's, measured on
  main in #306 (fps 59-60, frame cpu 5.5-6.3 ms); this merge adds no per-print
  work of its own beyond composing them, and the chunked tape's counts tests
  (A6) are the campaign's hot-path proof.
- *Touches anything user-visible*: the surfaces are #306's and #420's, each
  visual-QA'd and trader-UX-reviewed on main; this merge re-homes their code
  and changes no surface.
- *Adds a capability* / *Engine determinism*: the capabilities and engine code
  arrive already reviewed from main; the new-extension recipe and the
  test-first rule were graded there. This mission's own new code is a test
  (A3), a v2 docking (S1) and matrix rows.

## The request as received

> Attributed quotation: the coordinator's brief for task X2, verbatim.

> You are executing campaign synchronization task **X2** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: merge `origin/main` at `d8de9a1b` into `campaign/lean-a-plus` through a reviewed PR, preserving every behaviour both sides added and every campaign invariant. You are a subagent: you cannot ask the trader; the decisions below answer the questions. A doubt that would need a new decision becomes a `human_decision` in your handoff, never a guess.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (the size rule: no production file over 1,500 production lines; the campaign branch has `crates/guards/size-baseline.txt` empty with `!budget 0`).
> 2. `C:\src\quantick\docs\campaign\integration.md` (step 6: synchronization branch/PR, merge commit, no history rewrite, validate conflicts, the same merge gate) and `C:\src\quantick\docs\workflow\delivery.md`.
> 3. `C:\src\quantick\.claude\skills\mission\SKILL.md` — treat this as a tier `high` mission (steps 1, 2, 4, 5, 7, 8, 9; step 3 answered here). Step 6 is done: worktree, `mission-base`, `mission-tier`, guards armed.
> 4. What main brought (`git log --oneline origin/campaign/lean-a-plus..origin/main`): PR #306 (trades bars for MetaTrader B3: a deal counter beside prints, REC control, recut paths; it edits `pane.rs`, `tab.rs`, `state.rs`, `crates/feed-mt5/src/stream.rs`, `bridge/mt5/quantick_bridge.py`, `drawings/mod.rs`, schemas and the capability inventory, and raises size ceilings on main for `pane.rs`, `tab.rs` and `stream.rs`), PR #393 (mission completion gates in the hooks), PR #420 (copy and paste chart drawings). Read each PR body (`gh pr view 306`, `393`, `420`).
> 5. What the campaign did to the same files (read `git log origin/main..origin/campaign/lean-a-plus --stat -- <file>` for each conflicted file, and the PR bodies of #383 pane split, #392 tab/chrome split, #395 MT5 stream and bridge split, #390 drawings split, #405 live envelope, #431 chunked tape in state/tab, #421 capability inventory v2 rows, #406 size baseline to zero).
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\sync-main-d8de9a1b` (Git Bash `/c/src/quantick-worktrees/sync-main-d8de9a1b`), branch `sync/main-d8de9a1b`, at the campaign tip `6ba1a7f3`. Every command starts with `cd /c/src/quantick-worktrees/sync-main-d8de9a1b &&`. Never write to `C:\src\quantick` or other worktrees. A sibling (Q13, `fix/gateway-request-id-ordering`) is working on `crates/app/src/control/gateway/server.rs` and control-plane test modules; your merge may touch those only as main's changes require.
>
> ## Decisions
> - D1: `git merge --no-ff origin/main` (a merge commit, first parent the campaign tip); resolve conflicts in the merge commit itself; no rebase, no force push of the campaign branch. Commit message states source main SHA `d8de9a1b` and the PRs it brings.
> - D2: re-home, don't reintroduce: where main changed code that the campaign moved into siblings (for example `pane.rs` → `pane/*.rs`, `tab.rs` → `tab/*.rs`, `stream.rs` → `stream/*.rs`, the bridge → `bridge/mt5/quantick_bridge/` modules or `_history/_core/_ticks/_rates/_transport`, `drawings/mod.rs` → `drawings/{tool,collection,placement}.rs`), apply main's change to the sibling that now owns the code. `state.rs` and `tab/*`: combine #306's deal-counter state with Q12's chunked tape (#431) and Q3's envelope counters (#405): every new field or path of #306 must work with the chunked tape (no contiguous-slice assumption), and every campaign invariant (no append copies more than one chunk; the counts-based session-length tests) must hold.
> - D3: size: every production file at or under 1,500 production lines; the size baseline stays empty with `!budget 0` (discard main's raised ceilings for `pane.rs`, `tab.rs`, `stream.rs`; if a re-homed change pushes a sibling over 1,500, split it further by owner). Extension baselines (`extension-roots-baseline.txt`, `extension-shapes-baseline.txt`): take the correct measured numbers via `cargo run -p quantick-guards -- --tighten` after the merge; never raise a budget to make a guard pass without a reason recorded in the file.
> - D4: generated artifacts (`docs/control-plane/capability-inventory.md`, `schemas/control/observer-capability-catalog-v1.json`, the UI behaviour matrix, the retry matrix): regenerate through their documented commands after the code merge, never hand-merge them; the released-schema compatibility tests must stay green without regenerating released baselines; any new capability from #306/#420 must get a row in the retry matrix and the UI behaviour matrix (their drift guards will tell you).
> - D5: validation: the full ordered loop (fmt check, clippy, build, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately), `cargo test -p quantick-guards`, `sh .claude/hooks/guardrails_test.sh`, the bridge Python checks (`ruff check --select F bridge/mt5 tools/mt5`, `python bridge/mt5/tests/test_*.py`, `python tools/mt5/test_export_session.py`), and #306's own key tests by name.
> - D6: reviews: tier `high`: `arch-review` over the full sync diff against the campaign base with step 0 `code-review` at `medium` (focus on the conflict resolutions: list every conflicted hunk and how it was resolved), `ai-review` completion, `delivery-review` (the ledger: R1 bring main d8de9a1b into the campaign, R2 preserve both sides' behaviour, R3 keep the campaign invariants: size 0, chunked tape, matrices, schema compatibility). At most three step-0 rounds.
> - D7: open a **draft** PR with base exactly `campaign/lean-a-plus`: `cd /c/src/quantick-worktrees/sync-main-d8de9a1b && gh pr create --draft --base campaign/lean-a-plus --title "chore(campaign): synchronize main d8de9a1b into campaign/lean-a-plus" --body-file -` with a heredoc body: tier, "Campaign synchronization X2 of #367 (integration.md step 6); source main d8de9a1b (#393, #306, #420)", the conflict table (file, both sides, resolution, owning sibling), invariants checked, regenerated artifacts, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`. Watch CI; `gh pr ready <n>` once via Bash with the cd prefix. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`. Run checks one at a time, never `||` or `| head`; read whole failures. Foreground commands, adequate timeouts.
> - Keep every scratchpad file of yours under a `-x2` path.
> - Check `df -h /c` before the first build (about 259 GB free now).
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> - Known CI load flakes still open: #427, #428, #429, #430, #432 (Q13 is fixing them); report, rerun the job once at most. Main's own CI at d8de9a1b was red on #361, which the campaign already fixed in #382: confirm your merge keeps the campaign's fixed version of that test.
>
> ## Handoff
> Return a HANDOFF BLOCK: branch; worktree; PR URL; head SHA; base tip; the conflict table; how #306's deal counter composes with the chunked tape; size and extension guard output; regenerated artifacts; matrices and schema compatibility results; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; ready accepted or denied; any human_decision; the coordinator's next action.
