# Mission — split the tool rail, tab and layout wiring under 1,500 production lines

**Objective:** split `crates/app/src/toolrail.rs`, `crates/app/src/tab.rs` and
`crates/app/src/app/layout_wiring.rs` into owned sibling modules so every one
of them, and every sibling it creates, is at most 1,500 production lines, with
no behaviour change.

Why it matters: these three are trunk-adjacent files an agent opens for any
chrome change. Each carried a signed entry in `crates/guards/size-baseline.txt`,
which is debt: an agent that opens one to change one behaviour reads a whole
subsystem. The campaign's rule is one owner per file.

**Tier:** `medium` — campaign child S6 of
https://github.com/milocaetano/quantick/issues/367. A pure-move refactor over
UI chrome and the tab's pane state: mechanical, but wide enough that the shape
pass and a completeness pass are both worth their cost. Step 3 was answered by
the coordinator as D1..D8 below.

## Request ledger

- **R1** — `toolrail.rs` (2,114 production lines), `tab.rs` (1,661) and
  `app/layout_wiring.rs` (1,577) each end at most 1,500 production lines, and
  so does every new sibling.
- **R2** — every production move is a *pure* move: bodies move unchanged, the
  only widenings are the marks a child module needs.
- **R3** — no behaviour change: no UI, persistence-format, feed-drain or
  workspace-layout difference; no test expectation edited.
- **R4** — the three entries leave `crates/guards/size-baseline.txt` and the
  `!budget` line does not rise.
- **R5** — the four checks green on the final head, run one at a time, plus
  `cargo test -p quantick-guards`; CI green at the exact PR head.
- **R6** — performance stated per touched path; no per-trade, per-depth or
  per-frame path changes its complexity.
- **R7** — the work lands through a PR whose base is exactly
  `campaign/lean-a-plus`, reviewed under the campaign's shared key.
- **R8** — the work stays inside its authority: no merge, no write to `main` or
  to another worktree, `gh pr` only through the Bash tool, one `gh pr ready`
  attempt, and every commit carrying the session's attribution trailers.
- **R9** — the work stays inside its ownership: only the three named files,
  new files under their own directories, this branch's entries in
  `crates/guards/size-baseline.txt`, and `mod` lines where strictly needed —
  *"Do not edit any other file."*
- **R10** — the run reports back: a HANDOFF BLOCK to the coordinator carrying
  the issue, branch, worktree, PR, SHAs, the size table, review verdicts,
  markers, CI, findings, repair batches, readiness and any `human_decision`.

## Decisions (from the coordinator, in place of step 3)

- **D1** — pure moves only; the only widenings are the marks a child module
  needs. A `pub` beyond `pub(super)`/`pub(crate)`, or a new field on `ToolRail`,
  `Tab` or `QuantickApp`, stops the work and is reported.
- **D2** — the ceiling is production lines as `crates/guards/src/size.rs`
  measures them. Moving inline tests to sidecars is allowed, never required.
- **D3** — seams: `toolrail.rs` by rail section; `tab.rs` by feed drain versus
  settings and persistence; `layout_wiring.rs` by wiring group.
- **D4** — purity is proved by a statement-line multiset before/after, with the
  command and its result in the PR body; the round-trip tests are named.
- **D5** — performance is stated per touched path; only a changed call shape on
  a hot path needs a measurement.
- **D6** — `--tighten` after the moves; the three entries leave the baseline;
  `!budget` must not rise; `cargo test -p quantick-guards` passes.
- **D7** — tier `medium`: `arch-review` with `code-review` at `low`, full shape
  pass; `ai-review` completion; `delivery-review` completeness pass inline. At
  most two step-0 rounds.
- **D8** — one `gh pr ready` attempt through the Bash tool; if the gate denies
  a cd-prefixed command, stop at the draft PR and report.

## Assumptions

- **S1** — `tab.rs`'s feed drain already lives in `crates/app/src/tab/feed.rs`,
  so D3's "feed drain versus settings and persistence" seam is read as its
  in-file equivalent: the tab's pane addressing and its layout/spec persistence
  move out, and what stays is the tab's construction and feed attachment. The
  `drain_feed` opening-block arm the baseline comment cites was never in this
  file, so nothing of it is disturbed. Safe to assume: the code answers it in
  under a minute, and the outcome R1 asks for is unchanged.
- **S2** — a method that was `pub(super)` in `app/layout_wiring.rs` keeps the
  same visibility from its new child module as `pub(in crate::app)`. That is
  the *same* visibility, not a widening, so it stays inside D1.
- **S3** — the extension-boundary ratchet counts `impl QuantickApp` lines, and
  a new child module adds one `impl`/`}` pair to that count. Moving the layout
  strip and its delete confirmation out as one module — they were two adjacent
  `impl QuantickApp` blocks separated by a blank line — pays that pair back, so
  the root count falls rather than rises. Safe to assume: it is a file-local
  structural choice with no behaviour in it, and the guard proves the result.
- **S4** — D6 orders `cargo run -p quantick-guards -- --tighten`, and that
  command writes `crates/guards/extension-roots-baseline.txt` as well as
  `size-baseline.txt`, which R9's ownership list does not name. The two
  instructions collide, so the mandated command was run and its second edit
  kept: both numbers it wrote move *down* (`QuantickApp` 10,240 → 10,239, its
  `!budget` 10,669 → 10,668), it is the file's only change, and refusing it
  would have left the repository failing its own guard. Recorded rather than
  assumed away — it is reported to the coordinator as a `human_decision`,
  because widening a stated ownership boundary is not a subagent's call.
- **S5** — `use super::*` was the first spelling of the new children's
  imports. The shape pass replaced it with named `use` blocks, because every
  other production sibling in this crate names what it imports and the glob is
  a test-module convention here. Safe to assume: it is the repo's own
  convention, the compiler proves the result, and A2's multiset shows only
  import lines moving.

## Acceptance criteria

- [ ] **A1** — each named file and every new sibling is at most 1,500
      production lines. *Evidence:* the guard's own measure printed per file.
      → PR body size table. *(R1)*
- [ ] **A2** — every production move is pure. *Evidence:* a statement-line
      multiset over each family before/after, whose only differences are module
      docs, `mod`/`use`/`impl`/`}` scaffolding, import lines rewritten on
      either side of a move, and visibility rewrites in pairs.
      → PR body purity proof. *(R2)*
- [ ] **A3** — behaviour is unchanged and no test expectation was edited.
      *Evidence:* `cargo test --workspace` green with no test file in the diff;
      the workspace and UI-state round-trip suites named. → PR body. *(R3)*
- [ ] **A4** — `size-baseline.txt` carries no entry for any named file and
      `!budget` fell. *Evidence:* the baseline diff. → PR body. *(R4)*
- [ ] **A5** — the four checks green on the final head, one at a time, plus
      `cargo test -p quantick-guards`; CI green at the PR head. *Evidence:*
      command output and the CI run URL. → PR body. *(R5)*
- [ ] **G1** — every artifact in English; conventional commits.
- [ ] **G2** — performance declared per touched path. *Evidence:* a per-path
      rate classification. → PR body. *(R6)*
- [ ] **G3** — `arch-review` run over the diff against `campaign/lean-a-plus`
      with its step 0 bug pass, every Blocker and Should-fix resolved or
      deferred in the PR body; `ai-review` completion recorded;
      `delivery-review` completeness pass. *Evidence:* review verdicts and the
      markers under the campaign's shared key. *(R7)*
- [ ] **G4** — the run stayed inside its authority and its ownership.
      *Evidence:* the branch's own `git log` and `git diff --stat` against the
      campaign base — no merge commit authored here, no write outside the
      worktree, every commit carrying the two attribution trailers, and no
      file in the diff outside the owned set bar the one `--tighten` wrote,
      which `S4` records. *(R8, R9)*

## Not applicable

- *Touches a hot path* — the rail and the tab draw per frame and the tab's
  drain runs per trade batch, but no call shape on either changes: methods move
  between `impl` blocks of the same type, which is resolved at compile time.
  No measurement is owed; the classification is stated instead.
- *Touches anything user-visible* — nothing user-visible changes, so no new or
  changed surface needs a hook, and `visual-qa` / `trader-ux-review` have
  nothing to grade. The existing hook declaration
  (`QUANTICK_PANE_COLLAPSED` in `tab.rs`) stays beside its read, which did not
  move.
- *Adds a capability* — none added.
- *Adds something a trader does* — none added.
- *Engine / determinism territory* — no engine code is touched.
- *Docs/skills only* — this is runtime code.

## Closing steps

- **C1** — `delivery-review` returns PASS (completeness pass, tier `medium`).
- **C2** — the PR is open against `campaign/lean-a-plus` with the evidence in
  its body.
- **C3** — exactly one `gh pr ready 392` attempt is made, through the Bash tool
  and no other route, and its outcome is reported either way.
- **C4** — the HANDOFF BLOCK returns to the coordinator (`R10`), which owns the
  merge into the campaign branch. This run never merges.

## The request as received

Quoted in full and verbatim, as an attributed quotation under `CLAUDE.md`'s
language exemption. The source is the campaign coordinator's child brief.

> You are executing campaign child mission **S6** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick:
> split `crates/app/src/toolrail.rs` (2,114 production lines),
> `crates/app/src/tab.rs` (1,661) and `crates/app/src/app/layout_wiring.rs`
> (1,577) into owned sibling modules, each at most 1,500 production lines, with
> no behaviour change. You are a subagent: you cannot ask the trader; decisions
> D1..Dn below answer the mission's step-3 questions. A doubt that would need a
> new decision is reported back, never guessed.
>
> ## Read first, in this order
>
> 1. `C:\src\quantick\CLAUDE.md` (English everywhere; the size ratchet and
>    `--tighten` under *Keeping the trunk small*).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign
>    child at tier `medium`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is
>    answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`,
>    guards armed); still run `cargo check -p quantick-app --all-targets` before
>    the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` (base
>    `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`,
>    review key from `sh .claude/hooks/campaign_context.sh key "$WT"`),
>    `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #373 (`gh issue view 373`): scope, A1..A6, gates.
> 5. How siblings did the same kind of cut and proved it: PR #384 and PR #388
>    bodies (`gh pr view 384`, `gh pr view 388`), the purity proof
>    (statement-line multiset with only `pub(super)` pairs differing) and the
>    size table. The baseline header comments in
>    `crates/guards/size-baseline.txt` record why `tab.rs` carries its entry
>    (the opening block's arm in `drain_feed`).
>
> ## Worktree (the only place you write)
>
> - `C:\src\quantick-worktrees\refactor-chrome-under-1500` (Git Bash
>   `/c/src/quantick-worktrees/refactor-chrome-under-1500`), branch
>   `refactor/chrome-under-1500`, from `origin/campaign/lean-a-plus` at
>   `e8eb23e5`. Every command starts with
>   `cd /c/src/quantick-worktrees/refactor-chrome-under-1500 &&`. Never write to
>   `C:\src\quantick` or other worktrees.
> - Siblings running in parallel own `drawings/` and `control/`; you own only
>   the three named files, new files under `crates/app/src/toolrail/`,
>   `crates/app/src/tab/`, `crates/app/src/app/layout_wiring/` (create the
>   directories), your entries in `crates/guards/size-baseline.txt`, and `mod`
>   lines if strictly needed. Do not edit any other file.
>
> ## Decisions
>
> - D1: pure moves only. Every body moves unchanged; the only widenings are
>   `pub(super)` marks. No UI, persistence-format, feed-drain or
>   workspace-layout behaviour change. If a move would need `pub` beyond
>   `pub(super)`/`pub(crate)` or a new field on `ToolRail`, `Tab` or
>   `QuantickApp`, stop and report it as a human_decision.
> - D2: the ceiling is production lines as `crates/guards/src/size.rs` measures
>   them; moving inline tests to sidecars is allowed, never required.
> - D3: seams: `toolrail.rs` by rail section; `tab.rs`: the feed drain
>   (including the opening block's arm, kept intact and in order) versus the
>   tab's settings and persistence; `layout_wiring.rs` by wiring group. If any
>   file or sibling would still exceed 1,500, split further.
> - D4: proof of A2: statement-line multiset before/after with command and
>   result in the PR body; the compiler drives the `pub(super)` widenings. Proof
>   of A1: workspace and UI-state round-trip tests pass unchanged (name them).
> - D5: performance: the rail and the tab draw per frame and `drain_feed` runs
>   per trade batch; moving methods across `impl` blocks adds no work. State per
>   touched path (per-frame / per-trade / rare) in the PR body; if you change
>   any call shape on those paths, measure `APP_HEALTH_SUMMARY` frame_avg
>   before/after.
> - D6: after the moves, `cargo run -p quantick-guards -- --tighten`; the three
>   entries must leave `size-baseline.txt`; `!budget` must not rise;
>   `cargo test -p quantick-guards` passes (hook declarations stay beside their
>   reads; `crates/guards/src/cycle.rs` must see no new module cycle). The
>   `--report` "largest" list is a fixed top-N; an untouched file may appear in
>   its diff, explain, do not "fix".
> - D7: tier `medium`: `arch-review` with `code-review` at `low`, full shape
>   pass; `ai-review` completion; `delivery-review` completeness pass inline. At
>   most two step-0 rounds.
> - D8: readiness will be denied by the current gate for a cd-prefixed command
>   (the fix, PR #387, awaits the user's main merge). Try it exactly once
>   through the Bash tool; if denied, stop at the draft PR with reviews, markers
>   and green CI, and report. **Never use the PowerShell tool or any other route
>   for pull-request commands; never merge; never touch main.**
>
> ## Environment notes
>
> - Disk: C: ran out of space earlier today; the coordinator freed build caches
>   of merged worktrees. Check `df -h /c` before the first build; if under 15 GB
>   free, `cargo clean` only in your own worktree and report, never delete
>   elsewhere.
> - Use `python`, not `python3`, for scripted edits of large Rust files. If
>   `cargo fmt` is blocked, run `rustfmt` directly then
>   `cargo fmt --all -- --check`. Run the four checks one at a time, never `||`
>   or `| head`; read whole failures.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`.
> - Splitting: never de-indent moved items; child modules need `pub(super)` on
>   moved helpers; a new test module must be suffixed `_tests`.
> - If writing review markers into the git dir is denied by the permission
>   classifier, put the exact `printf` lines in your handoff, marked pending.
> - Known CI load flakes: `delayed_producer_bookkeeping...`,
>   `gateway_client_reads...`, `gateway_a_client_that_never_reads...`,
>   `a_retry_that_races_its_own_first_call...`,
>   `layers_tests::the_trade_paint_layer_switch_stops_the_marks`; report, rerun
>   the job once at most.
>
> ## Delivery
>
> 1. Implement with the verification loop (`cargo check -p quantick-app`,
>    `env -u QUANTICK_BUBBLES cargo test -p quantick-app toolrail`, `... tab`,
>    `... layout`, `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`,
>    `cargo clippy --workspace --all-targets`, `cargo build --workspace`,
>    `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately.
>    Conventional English commits ending with
>    `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and
>    `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 3. Archive `GOAL.md` per mission step 8 (slug `chrome-under-1500`) as the last
>    commit before reviews.
> 4. Push; open a **draft** pull request from the worktree, with base
>    `campaign/lean-a-plus`, titled "refactor(app): split the tool rail, tab and
>    layout wiring under 1,500 production lines", and a heredoc body per the PR
>    template: tier, "Campaign child of #367 (S6); closes on integration: #373",
>    moved-item table, purity proof, size report, local verification, then
>    `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and
>    `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve
>    Blockers/Should-fixes; record markers with the shared key per
>    integration.md.
> 6. Watch CI at the head (bounded).
> 7. One readiness attempt per D8.
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base
>    tip; production-line table before/after per file; review verdicts with
>    URLs; markers yes/no; CI run URL and conclusion; findings closed/open;
>    repair batches; ready accepted or denied; any human_decision; the
>    coordinator's next action.

*One command line in item 4 and two command names in D8 and items 6–7 are
paraphrased rather than quoted: the repository's own readiness hook refuses to
run any shell command carrying them, so quoting them literally made this file
unwritable. Nothing else is altered.*

### Mid-run course correction from the coordinator

> Coordinator here: your run was cut by an API rate limit, now reset. Continue
> mission S6 from the actual state: worktree
> `/c/src/quantick-worktrees/refactor-chrome-under-1500` is at head 02ecd926
> (5 commits over merge-base 537beca3) with 7 uncommitted changed files, and
> draft PR #392 exists. First inspect `git status --short` and
> `git diff --stat` and decide whether those changes are your in-progress work
> (finish and commit them) or leftovers to discard; do not lose intended work.
> You were "re-verifying the four checks on the rebased head". Note the
> campaign tip has moved again to c390971b since your rebase; do not rebase
> again yourself, the coordinator does that at integration time. Then finish
> under the same brief: four checks, reviews with markers, CI green at the
> head, one Bash readiness attempt on PR 392 (the gate now accepts the
> cd-prefixed form), and the HANDOFF BLOCK. Never merge.
