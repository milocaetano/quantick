# Mission: split the order-flow and footprint renderers under 1,500 production lines

**Objective.** Split `crates/app/src/orderflow_render.rs` (3,075 production
lines), `crates/app/src/orderflow_view.rs` (2,485) and
`crates/app/src/footprint_render.rs` (2,236) into owned sibling modules by
layer and by phase, every file at most 1,500 production lines, with no
behaviour change and no per-frame cost added — so that an agent asked to
change one layer reads one owner and not the whole subsystem.

**Tier:** `high`. Campaign child S3 of #367 (issue #370), assigned at `high`
by the coordinator: the three files are the per-frame projection and painting
of the heatmap, bubbles and footprint, and a regression there is a frame-time
regression on every dense tape. `high` buys the full interrogation (answered
by the coordinator's decisions D1–D8), the full gate table, `code-review` at
`medium` inside `arch-review`, and `delivery-review` in full.

## Request ledger

- **R1** — Split the three files into owned sibling modules under each file's
  own directory, "each at most 1,500 production lines", by layer and by phase
  ("projection versus painting per layer"); the frame loop keeps calling "the
  same sequence with the same borrowed inputs". *(A1, A8)*
- **R2** — "Pure moves only": every body moves unchanged; the only widenings
  are `pub(super)` marks; proven by a "statement-line multiset before/after
  with the command and result in the PR body". *(A2)*
- **R3** — "No per-frame cost added": frame timing before and after "within
  noise", measured with the same method and stated in the PR; "any measurable
  regression is a Blocker; a change in call shape that could add an allocation
  per frame is a Blocker even without a number". *(A1, A7, G4)*
- **R4** — Every pinned or golden test in the touched crates passes unchanged;
  "no test expectation was edited". *(A3)*
- **R5** — After the moves `cargo run -p quantick-guards -- --tighten`; the
  three entries "must leave `size-baseline.txt`"; "`!budget` must not rise";
  `cargo test -p quantick-guards` passes, including the hook-declaration guard
  (`declare_hooks!` follows the `QUANTICK_FOOTPRINT_DEBUG` read). *(A4, A6)*
- **R6** — The four checks green on the final head, "run one at a time", and
  CI green at the exact PR head. *(A5, G2)*
- **R7** — Draft PR against exactly `campaign/lean-a-plus` with the prescribed
  title and body sections; `arch-review` (with `code-review` at `medium` and
  the full shape pass), `ai-review` completion, `delivery-review` in full;
  markers recorded with the shared campaign key; at most two step-0 rounds;
  one `gh pr ready` attempt through the Bash tool and no other route; never
  merge, never touch `main`. *(G3, C1, C2, C3)*
- **R8** — Write only inside the owned surface: the three renderer files, new
  files under their three directories, this mission's entries in
  `crates/guards/size-baseline.txt`, and `mod` lines if strictly needed; no
  edit to `pane.rs`, the hooks or any sibling-owned file. *(A9)*
- **R9** — Every artifact English; conventional commits ending with the two
  attribution lines. *(G1)*
- **R10** — Return a handoff block to the coordinator: issue, branch, worktree,
  PR, head, base tip, per-file line table, frame measurements, verdicts,
  markers, CI, findings, batches, ready outcome, any human decision, next
  action. *(C4)*

## Decisions (from the coordinator; step 3 answered before work started)

- **D1** — Pure moves only; widenings limited to `pub(super)`; a move needing
  `pub` beyond `pub(super)`/`pub(crate)` or a new trunk field stops and is
  reported as a human decision.
- **D2** — The ceiling is production lines as `crates/guards/src/size.rs`
  measures them; inline tests may move to sidecars, never must.
- **D3** — Split by layer and by phase (projection versus painting per layer:
  heatmap, bubbles, footprint columns, imbalance marks, value area, chrome),
  each sibling under the owning file's directory; split further by phase if a
  file would still exceed 1,500.
- **D4** — A2 is proven by a statement-line multiset before/after, command and
  result in the PR body (S2's method); the compiler drives the widenings.
- **D5** — Performance is the acceptance criterion that matters most: measure
  before and after with the same method, preferably `APP_HEALTH_SUMMARY`
  fps/frame_avg on a dense replay through the ui-harness hooks with every
  `QUANTICK_*` store variable pointed at a scratch directory, or the nearest
  bench/test over these layers if a live run is impractical; state exactly
  what ran, on which fixture, and the numbers.
- **D6** — After the moves `--tighten`; the three entries leave the baseline;
  `!budget` does not rise; `cargo test -p quantick-guards` passes; the
  `--report` "largest" list is a fixed top-N, so an untouched file appearing
  in its diff is explained, not "fixed".
- **D7** — Tier `high`: `arch-review` with `code-review` at `medium` and the
  full shape pass; `ai-review` completion; `delivery-review` in full; at most
  two step-0 rounds; remaining minor findings ship as named follow-ups.
- **D8** — `gh pr ready` is tried exactly once through the Bash tool; if the
  gate denies it, stop at the draft PR with reviews, markers and green CI and
  report. Never PowerShell for `gh pr`, never merge, never touch `main`.

## Assumptions

- **S1** — `orderflow_render.rs` splits into five siblings (`layout`,
  `heatmap`, `bubbles`, `legend`, `preview`), `orderflow_view.rs` into two
  (`frame`, `settings`) and `footprint_render.rs` into two (`bar`, `heat`).
  D3 names the layers; which phase boundary becomes a file is a conventional
  placement call the code answers — the shared colour arithmetic and the
  render style stay in each root because every sibling reads them.
- **S2** — Names other modules reach through `crate::orderflow_render::…` are
  re-exported from the root with `pub(crate) use` lines rather than having
  their callers (`pane.rs`, the control plane) edited: R8 forbids touching
  those files, and a re-export is a `use` line, not a widening.
- **S3** — The test sidecar `orderflow_render/tests/mod.rs` and the two
  inline `mod tests` blocks gain `use` lines for the items that moved (glob
  imports of the siblings plus the external names the roots no longer
  import). No assertion or expectation changes; this is the same shape S2's
  PR #384 recorded.
- **S4** — The one-line `# +2: the QUANTICK_FOOTPRINT_DEBUG declaration`
  pointer in the baseline leaves with the `footprint_render.rs` entry it
  titled; the budget note it points at stays and already records the raise.
- **S5** — *Wanted to ask.* The build host has no room for a workspace build:
  `C:` holds under 1.5 GB free with some fifty worktree `target/` directories
  of 15 GB each. `cargo check --all-targets`, `clippy`, `fmt` and the guards
  fit; `cargo build --workspace`, `cargo test --workspace` and a release build
  for the `APP_HEALTH_SUMMARY` measurement do not. Reading taken: run every
  check that fits locally, let CI at the PR head run the build and the
  workspace tests, and report the frame measurement as blocked by the
  environment for a human decision rather than fabricate or skip it.

## Acceptance criteria

- [ ] **A1** — Each of the three renderer files and every new sibling is at
      most 1,500 production lines as the size guard counts them.
      *Evidence:* the per-file table (before/after) in the PR body, and
      `cargo test -p quantick-guards` passing with no entry for the three.
      → PR body, size table. *(R1)*
- [ ] **A2** — Every production move is a pure move: the statement-line
      multiset of the three files before equals that of the roots plus
      siblings after, the only differing lines being struct-field lines with
      and without `pub(super)`.
      *Evidence:* the proof command and its result in the PR body; the raw
      whole-file multiset taken at cut time. → PR body, A2 section. *(R2)*
- [ ] **A3** — Every pinned or golden test in `quantick-app` passes unchanged
      and no test expectation was edited: the only test-file changes are
      `use` lines.
      *Evidence:* `git diff` of the test files showing `use` lines only; the
      workspace test run at the PR head. → PR body, A3 section; CI run. *(R4)*
- [ ] **A4** — `crates/guards/size-baseline.txt` carries no entry for the
      three files and `!budget` did not rise.
      *Evidence:* the baseline diff (three entries removed, `!budget`
      45,739 → 37,709) and `--tighten` reporting nothing left. → PR body,
      size section. *(R5)*
- [ ] **A5** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace` are
      green on the final head, each run on its own, and CI is green at the
      exact PR head; `cargo test -p quantick-guards` passes.
      *Evidence:* local exit codes for every check that ran locally, labelled
      local; the CI run URL and conclusion at the head for the rest,
      labelled CI. → PR body, verification section. *(R6)*
- [ ] **A6** — `declare_hooks!["QUANTICK_FOOTPRINT_DEBUG"]` stays in the file
      that reads the variable, and the hook-declaration guard passes.
      *Evidence:* `grep -n QUANTICK_FOOTPRINT_DEBUG crates/app/src/footprint_render.rs`
      showing read and declaration in one file; the guards run. → PR body,
      reviewer notes. *(R5)*
- [ ] **A7** — Frame time on a dense fixture is within noise of the base, and
      no call shape changed in a way that could add an allocation per frame.
      *Evidence:* `APP_HEALTH_SUMMARY` fps / frame_avg / frame_cpu before and
      after on the same fixture with the method stated — or, if the host
      cannot build a release binary, the per-path rate table plus an explicit
      "measurement pending" line and a human decision request. → PR body,
      performance section. *(R3)*
- [ ] **A8** — Each sibling owns one layer or one phase and says so in its
      module doc; the frame loop in `orderflow_view` calls the same four
      renderer passes in the same order with the same borrowed inputs.
      *Evidence:* the moved-item table in the PR body, and `git diff` of
      `draw_background` / `draw_aggressions` / `draw_legend` showing bodies
      unchanged. → PR body, moved-item table. *(R1)*
- [ ] **A9** — `git diff --name-only <base>...HEAD` names only the three
      roots, the new siblings, the three test files inside those trees, the
      baseline and this archive.
      *Evidence:* the path list in the PR body. → PR body, scope section.
      *(R8)*

## Injected gates

- [ ] **G1** — Every artifact in English; conventional commits with the two
      attribution lines. *Evidence:* `crates/guards/src/language.rs` passing
      in the guards run; the commit log. → guards run; `git log`.
- [ ] **G2** — The four checks, one at a time, on the final head after
      rebasing on the current campaign base; performance impact declared per
      touched path by rate. *Evidence:* exit codes and the rate table.
      → PR body.
- [ ] **G3** — `arch-review` over the exact diff against `campaign/lean-a-plus`
      with `code-review` at `medium` as step 0; every Blocker and Should-fix
      resolved or deferred in the PR body; `ai-review` completion recorded
      with zero unresolved threads. *Evidence:* the review reports and
      markers. → PR body, review section; git dir markers.
- [ ] **G4** — Hot path: evidence that performance is flat, not a belief
      (see A7). *Evidence:* as A7. → PR body.

## Not applicable, and why

- *Touches anything user-visible* — no surface, string, colour or layout
  changes; every painter body is byte-identical, so `visual-qa` and
  `trader-ux-review` have nothing to grade. The env hooks are unchanged.
- *Adds a capability* — nothing new is added; nine files dock as `mod` lines.
- *Adds something a trader does* — no action, tool or lock is added.
- *Engine / determinism territory* — no engine code is touched; the diff is
  confined to `crates/app` and the guards' data file.
- *Docs/skills only* — this is runtime code; the full shape pass applies.

## Closing steps

- **C1** — `arch-review` resolved and `arch-review-ok` recorded with the
  shared campaign key.
- **C2** — `delivery-review` returns PASS and `delivery-review-ok` is
  recorded with the shared key; `ai-review-complete` recorded.
- **C3** — Draft PR open against `campaign/lean-a-plus`; CI green at the head;
  one `gh pr ready` attempt.
- **C4** — Handoff block returned to the coordinator.

## The request as received

Quoted verbatim, an attributed quotation from the campaign coordinator under
`CLAUDE.md`'s language exemption:

> You are executing campaign child mission **S3** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: split `crates/app/src/orderflow_render.rs` (3,075 production lines), `crates/app/src/orderflow_view.rs` (2,485) and `crates/app/src/footprint_render.rs` (2,236) into owned sibling modules, each at most 1,500 production lines, with no behaviour change and no per-frame cost added. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision is reported back, never guessed.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (English everywhere; the size ratchet and `--tighten` under *Keeping the trunk small*).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-app --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` (base `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`, review key from `sh .claude/hooks/campaign_context.sh key "$WT"`), `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #370 (`gh issue view 370`): scope, A1..A6, gates.
> 5. How the sibling S2 did the same kind of cut and proved it, for the shape of the proof: PR #384 body (`gh pr view 384`), in particular the purity proof (statement-line multiset with only `pub(super)` pairs differing) and the size table.
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\refactor-orderflow-render-under-1500` (Git Bash `/c/src/quantick-worktrees/refactor-orderflow-render-under-1500`), branch `refactor/orderflow-render-under-1500`, from `origin/campaign/lean-a-plus` at `e8eb23e5`. Every command starts with `cd /c/src/quantick-worktrees/refactor-orderflow-render-under-1500 &&`. Never write to `C:\src\quantick` or other worktrees.
> - Siblings running in parallel own `pane.rs` and the hooks; you own only the three renderer files, new files under `crates/app/src/orderflow_render/`, `crates/app/src/orderflow_view/`, `crates/app/src/footprint_render/`, your entries in `crates/guards/size-baseline.txt`, and `mod` lines if strictly needed. Do not edit any other file.
>
> ## Decisions
> - D1: pure moves only. Every body moves unchanged; the only widenings are `pub(super)` marks. No rendering, projection, colour, layout or input change. If a move would need `pub` beyond `pub(super)`/`pub(crate)` or a new field on a trunk struct, stop and report it as a human_decision.
> - D2: the ceiling is production lines as `crates/guards/src/size.rs` measures them; moving inline tests to sidecars is allowed, never required.
> - D3: split by layer and by phase: projection versus painting per layer (heatmap, bubbles, footprint columns, imbalance marks, value area, chrome), each sibling under the owning file's directory; the frame loop calls the same sequence with the same borrowed inputs. If any file or sibling would still exceed 1,500, split further by phase.
> - D4: proof of A2: statement-line multiset before/after with the command and result in the PR body (S2's method is a good template); the compiler drives the `pub(super)` widenings.
> - D5: performance is the acceptance criterion that matters most here (A1's "within noise"): these are per-frame paths. Measure before and after with the same method: preferably `APP_HEALTH_SUMMARY` fps and frame_avg on a dense replay via the ui-harness env hooks (`.claude/skills/ui-harness/SKILL.md`; capture-window and QUANTICK_* notes apply: point every QUANTICK_* store variable at a scratch dir so the trader's live workspace is not overwritten, and clear them between runs), or the nearest existing bench/test that exercises `draw`/`project` of these layers if a live run is impractical; state exactly what you ran, on which fixture, and the numbers. Any measurable regression is a Blocker; a change in call shape that could add an allocation per frame is a Blocker even without a number.
> - D6: after the moves, `cargo run -p quantick-guards -- --tighten`; the three entries must leave `size-baseline.txt`; `!budget` must not rise; `cargo test -p quantick-guards` passes (the hook-declaration guard: `footprint_render.rs` declares `QUANTICK_FOOTPRINT_DEBUG` beside its read, so `declare_hooks!` must follow the read if that read moves). The `--report` "largest" list is a fixed top-N; an untouched file may appear in its diff, explain, do not "fix".
> - D7: tier `high`: `arch-review` with `code-review` at `medium` and the full shape pass; `ai-review` completion; `delivery-review` in full. At most two step-0 rounds; remaining minor findings ship as PR follow-ups named in the PR body.
> - D8: `gh pr ready` will be denied by the current gate for a cd-prefixed command (a sibling mission Q7 is fixing that). Try it exactly once through the Bash tool; if denied, stop at the draft PR with reviews, markers and green CI, and report. **Never use the PowerShell tool or any other route for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`, for scripted edits of large Rust files. If `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all -- --check`. Run the four checks one at a time, never `||` or `| head`; read whole failures.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`.
> - Splitting: never de-indent moved items; child modules need `pub(super)` on moved helpers; a new test module must be suffixed `_tests`; `crates/guards/src/cycle.rs` fails the build on a new module cycle.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Known CI load flakes being fixed by siblings: `delayed_producer_bookkeeping...`, `gateway_client_reads...`, `gateway_a_client_that_never_reads...`, `a_retry_that_races_its_own_first_call...`; report, rerun the job once at most.
>
> ## Delivery
> 1. Implement with the verification loop (`cargo check -p quantick-app`, targeted render tests, `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately. Conventional English commits ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 3. Archive `GOAL.md` per mission step 8 (slug `orderflow-render-under-1500`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/refactor-orderflow-render-under-1500 && gh pr create --draft --base campaign/lean-a-plus --title "refactor(app): split the order-flow and footprint renderers under 1,500 production lines" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (S3); closes on integration: #370", moved-item table, purity proof, frame numbers before/after, size report, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. One `gh pr ready` attempt per D8.
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base tip; production-line table before/after per file; frame measurements; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; repair batches; ready accepted or denied; any human_decision; the coordinator's next action.
