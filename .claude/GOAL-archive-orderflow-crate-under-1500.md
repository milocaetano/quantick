# Mission: split the orderflow projection and config files under 1,500 production lines

**Objective.** Split `crates/orderflow/src/projection.rs` (1,850 production
lines) and `crates/orderflow/src/config.rs` (1,523) into owned sibling modules
under each file's own directory, every file at most 1,500 production lines,
with no behaviour change and determinism intact — so that an agent asked to
change one phase of the heatmap projection or one config family reads one
owner and not the whole subsystem.

**Tier:** `high`. Campaign child S8 of #367 (issue #375), assigned at `high`
by the coordinator: the two files are the headless, deterministic engine of
the heatmap — the per-depth and per-trade projection paths and the sanitised
configuration every consumer reads — and a regression there changes what
every chart, backtest and bot draws from the same trades. `high` buys the
full interrogation (answered by the coordinator's decisions D1–D8), the full
gate table, `code-review` at `medium` inside `arch-review`, and
`delivery-review` in full.

## Request ledger

- **R1** — Split the two files "into owned sibling modules, each at most
  1,500 production lines": `config.rs` "one file per config family" with
  "the shared validation and the parent `Config` in `config.rs`";
  `projection.rs` "fold versus read model (or per layer) under
  `projection/`"; split further if any file would still exceed 1,500.
  *(A1, A7)*
- **R2** — "Pure moves only. Every body moves unchanged; the only widenings
  are `pub(super)` marks." No change to "any projection output, config
  default, validation rule, serialisation or public type path"; every public
  path stays stable through re-exports "so no consumer changes". Proven by a
  "statement-line multiset before/after with command and result in the PR
  body". *(A2, A6)*
- **R3** — "No behaviour change and determinism intact": every golden, parity
  and determinism test in `orderflow` and `engine` passes unchanged
  (`cargo test -p quantick-orderflow`, `cargo test -p quantick-engine`,
  "including `profile_fold_parity`"), counts named; "grep the diff for
  `HashMap` to show none was introduced". *(A3)*
- **R4** — Every pinned or golden test in the touched crates passes
  unchanged and "no test expectation was edited". *(A3)*
- **R5** — After the moves `cargo run -p quantick-guards -- --tighten`; "both
  entries must leave `size-baseline.txt`" and "`!budget` must fall by exactly
  3,373"; `cargo test -p quantick-guards` passes; `cycle.rs` sees no new
  module cycle and `graph.rs` no new edge. *(A4)*
- **R6** — Performance: "state per touched path (per-trade / per-depth /
  per-frame / rare) in the PR body"; if any call shape changes, bench
  before/after and report. *(A5, G4)*
- **R7** — The four checks green on the final head, "run one at a time", CI
  green at the exact PR head. *(A8, G2)*
- **R8** — Draft PR against exactly `campaign/lean-a-plus` with the
  prescribed title and body sections; `arch-review` with `code-review` at
  `medium` and the full shape pass; `ai-review` completion; `delivery-review`
  in full; markers with the shared campaign key; at most two step-0 rounds;
  one `gh pr ready` through the Bash tool; never merge, never touch `main`.
  *(G3, C1, C2, C3)*
- **R9** — Write only inside the owned surface: the two named files, new
  files under `crates/orderflow/src/projection/` and
  `crates/orderflow/src/config/`, this mission's entries in
  `crates/guards/size-baseline.txt`, and `mod` lines if strictly needed; no
  edit to `crates/app` or any sibling-owned file. *(A9)*
- **R10** — Every artifact English; conventional commits ending with the two
  attribution lines. *(G1)*
- **R11** — Archive `GOAL.md` as the last commit before reviews; return a
  handoff block to the coordinator: issue, branch, worktree, PR, head, base
  tip, per-file line table, verdicts, markers, CI, findings, batches, ready
  outcome, any human decision, next action. *(C4, C5)*

## Decisions (from the coordinator; step 3 answered before work started)

- **D1** — Pure moves only; widenings limited to `pub(super)`; a move needing
  `pub` beyond `pub(super)`/`pub(crate)`, a new field, a consumer edit in
  `crates/app`, or a change of iteration order stops and is reported as a
  human decision.
- **D2** — The ceiling is production lines as `crates/guards/src/size.rs`
  measures them; inline tests may move to sidecars, never must
  (`projection/tests/mod.rs` already is one).
- **D3** — Seams: `config.rs` one file per config family with the shared
  validation and the parent config in `config.rs`; `projection.rs` fold
  versus read model (or per layer) under `projection/`; split further if any
  file would still exceed 1,500.
- **D4** — A2 is proven by a statement-line multiset before/after, command
  and result in the PR body; the compiler drives the `pub(super)` widenings.
  A1 is proven by the `orderflow` and `engine` suites passing unchanged with
  counts named, and a `HashMap` grep over the diff.
- **D5** — Performance: per-depth and per-trade paths; moving methods across
  `impl` blocks adds no work. State the rate per touched path in the PR body;
  bench before/after only if a call shape changes.
- **D6** — After the moves `--tighten`; both entries leave the baseline;
  `!budget` falls by exactly 3,373 from the base's value; guards pass; no new
  module cycle, no new crate edge. No rebase if the campaign tip moves.
- **D7** — Tier `high`: `arch-review` with `code-review` at `medium` and the
  full shape pass; `ai-review` completion; `delivery-review` in full; at most
  two step-0 rounds; remaining minor findings ship as named follow-ups.
- **D8** — `gh pr ready` runs once, cd-prefixed, through the Bash tool after
  reviews, markers and green CI. Never PowerShell for `gh pr`, never merge,
  never touch `main`.

## Assumptions

- **S1** — `config.rs` has three families in the code, not the four the
  issue's parenthesis lists: the bubble style (`BubbleStyle` with its size
  reference, consumption mark, render mode, the φ ladder and its defaults),
  the live lane (`LaneWindow`, `LiveLaneStyle`, its disk representation and
  the lane labels) and the parent `HeatmapConfig` with `DisplayGrouping`,
  `HeatmapTheme`, `IntensityMode` and the retention/cluster bounds it
  sanitises. There is no footprint or health config in this file — the
  footprint's settings live in `crates/app`, the health counters in
  `engine.rs` — so D3's "others as found" resolves to `config/bubbles.rs` and
  `config/lane.rs`. `finite_clamp` is the shared validation and stays in the
  parent.
- **S2** — `projection.rs` splits into the read model (`projection/model.rs`:
  the primitives and the two projection halves as data, `with_live` included),
  the print tiers (`projection/tiers.rs`: cutting, clustering, refining and
  placing one stretch of the tape) and the budget fold
  (`projection/fold.rs`: pane budgets, ordering and merge). The pipeline —
  `project_settled`, `project_live`, the reduction-marker helpers, the lane
  grouping, the percentile and the two normalisers other crates call — stays
  in the root. The reduction helpers stay in the root rather than forming a
  fourth sibling because `with_live` (read model) calls `cap_events` while
  `event_primitives` builds a read-model type: as siblings the two would form
  a cycle, and a child reaching its parent's private function is not one.
- **S3** — Moved `pub` items are re-exported from each root with `pub use`
  lines, and the child modules stay private (`mod bubbles;`, not `pub mod`),
  so the public path set of the crate is byte-identical: `config::X`,
  `projection::X` and the `lib.rs` facade all resolve as before and no
  consumer changes. A `pub use` is a `use` line, not a widening.
- **S4** — Test files change only by `use` lines: the inline `mod tests` of
  `config.rs` gains imports for the two private names it reads that no
  longer live in its parent (`SERIALIZED_FLOAT_PLACES`, `TapeAge`); the
  projection sidecar gains one import for the three lane-share constants the
  root no longer imports (only the fold reads them now), and reaches the
  sibling functions it calls through `use super::*`, which sees the root's
  own imports of them. No assertion or expectation changes.
- **S5** — Two intra-doc links inside moved doc comments name items relative
  to the old parent (`super::reserved_span_ms`, `HeatmapConfig::…`) and
  would dangle from `config/lane.rs`; their link targets are retargeted in
  place (comment lines, not statements). `cargo doc` already fails on the
  base with three unresolved links (`engine.rs:1015`, `projection.rs:212`,
  `projection.rs:298`) that are not this mission's to fix; the split adds
  none.
- **S6** — `--tighten` removes an entry once its file is under the threshold
  and lowers `!budget` by the removed ceilings. If it also tightens an entry
  outside this mission's ownership (a file more than 200 lines below its
  ceiling on the base), that line is reverted and reported rather than
  shipped, because R9 forbids touching sibling entries.

## Acceptance criteria

- [ ] **A1** — `projection.rs`, `config.rs` and every new sibling are at most
      1,500 production lines as the size guard counts them.
      *Evidence:* the per-file table (before/after) in the PR body;
      `cargo test -p quantick-guards` passing with no entry for either file.
      → PR body, size section. *(R1)*
- [ ] **A2** — Every production move is a pure move: the statement-line
      multiset of each file before equals that of its root plus siblings
      after, with `pub(super) ` stripped; the whole-line residue is `use`,
      `mod` and `pub use` lines, the new module headers and the two doc-link
      retargets, and nothing else.
      *Evidence:* the proof command and its result in the PR body. → PR body,
      A2 section. *(R2)*
- [ ] **A3** — Every golden, parity and determinism test in `orderflow` and
      `engine` passes unchanged, `profile_fold_parity` included, with the
      counts named; no test expectation was edited; `git diff` contains no
      added `HashMap`.
      *Evidence:* the two test runs' result lines, the `HashMap` grep, and
      the `git diff -U0` of the test files showing only `use` lines. → PR
      body, A3 section; CI run. *(R3, R4)*
- [ ] **A4** — `crates/guards/size-baseline.txt` carries no entry for either
      file; `!budget` is exactly 3,373 lower than on the base (20,025 →
      16,652); `--tighten` reports nothing left; `cargo test -p quantick-guards`
      passes; `cycle.rs` and `graph.rs` are unchanged and green.
      *Evidence:* the baseline diff and the guards run. → PR body, size
      section. *(R5)*
- [ ] **A5** — Every touched path is classified by rate in the PR body, and
      no call shape changed: every call that was a direct call to a private
      function in one module is a direct call to a `pub(super)` function in a
      sibling of the same crate.
      *Evidence:* the rate table in the PR body. → PR body, performance
      section. *(R6)*
- [ ] **A6** — No public path changed: `lib.rs` is untouched, every
      `quantick_orderflow::config::…` and `quantick_orderflow::projection::…`
      import in the workspace compiles unchanged, and no file under
      `crates/app` is in the diff.
      *Evidence:* `cargo build --workspace` exit 0 with `git diff --name-only`
      naming no consumer. → PR body, scope section. *(R2)*
- [ ] **A7** — Each sibling owns one family or one phase and says so in its
      module doc.
      *Evidence:* the moved-item table in the PR body. → PR body. *(R1)*
- [ ] **A8** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES
      cargo test --workspace` are green on the final head, each run on its
      own, and CI is green at the exact PR head.
      *Evidence:* local exit codes labelled local; the CI run URL and
      conclusion labelled CI. → PR body, verification section. *(R7)*
- [ ] **A9** — `git diff --name-only <base>...HEAD` names only the two roots,
      the new siblings under their two directories, the size baseline and
      this archive.
      *Evidence:* the path list in the PR body. → PR body, scope section.
      *(R9)*

## Injected gates

- [ ] **G1** — Every artifact in English; conventional commits with the two
      attribution lines. *Evidence:* `crates/guards/src/language.rs` passing
      in the guards run; the commit log. → guards run; `git log`.
- [ ] **G2** — The four checks, one at a time, on the final head; performance
      impact declared per touched path by rate. *Evidence:* exit codes and
      the rate table. → PR body.
- [ ] **G3** — `arch-review` over the exact diff against `campaign/lean-a-plus`
      with `code-review` at `medium` as step 0; every Blocker and Should-fix
      resolved or deferred in the PR body; `ai-review` completion recorded
      with zero unresolved threads. *Evidence:* the review reports and
      markers. → PR body, review section; git dir markers.
- [ ] **G4** — Hot path: the per-depth and per-trade paths keep their
      complexity and call shape; a bench runs only if a shape changes (D5).
      *Evidence:* as A5. → PR body.
- [ ] **G5** — Engine / determinism territory: no new fixture is owed because
      no behaviour is added; the existing golden, parity and determinism
      suites are the net and run unchanged (A3). *Evidence:* as A3.
- [ ] **G6** — CI at the PR head is watched bounded; a failure on one of the
      known load flakes is reported and the job rerun at most once per head.
      *Evidence:* the CI section of the PR body. → PR body.

## Not applicable, and why

- *Touches anything user-visible* — no surface, string, colour or layout
  changes; the crate is headless and nothing in `crates/app` is edited.
- *Adds a capability* — nothing new is added; five files dock as `mod` lines.
- *Adds something a trader does* — no action, tool or lock is added.
- *Docs/skills only* — this is runtime code; the full shape pass applies.

## Closing steps

- **C1** — `arch-review` resolved and `arch-review-ok` recorded with the
  shared campaign key.
- **C2** — `delivery-review` returns PASS and `delivery-review-ok` is
  recorded with the shared key; `ai-review-complete` recorded.
- **C3** — Draft PR open against `campaign/lean-a-plus`; CI green at the
  head; one `gh pr ready` attempt.
- **C4** — Handoff block returned to the coordinator.
- **C5** — `GOAL.md` archived as the mission's last commit before either
  review runs.

## The request as received

Quoted verbatim, an attributed quotation from the campaign coordinator under
`CLAUDE.md`'s language exemption:

> You are executing campaign child mission **S8** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: split `crates/orderflow/src/projection.rs` (1,850 production lines) and `crates/orderflow/src/config.rs` (1,523) into owned sibling modules, each at most 1,500 production lines, with no behaviour change and determinism intact. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision is reported back, never guessed.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (English everywhere; determinism rules: no wall clock, no randomness, `BTreeMap`/`Vec` over `HashMap` on output paths; the size ratchet and `--tighten` under *Keeping the trunk small*; `orderflow` is headless).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-orderflow --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` (base `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`, review key from `sh .claude/hooks/campaign_context.sh key "$WT"`), `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #375 (`gh issue view 375`): scope, A1..A6, gates.
> 5. How siblings did the same kind of cut and proved it: PR #388 body (`gh pr view 388`), the purity proof (statement-line multiset with only `pub(super)` pairs differing) and the size table.
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\refactor-orderflow-crate-under-1500` (Git Bash `/c/src/quantick-worktrees/refactor-orderflow-crate-under-1500`), branch `refactor/orderflow-crate-under-1500`, cut from `origin/campaign/lean-a-plus` at `793d6400`. Every command starts with `cd /c/src/quantick-worktrees/refactor-orderflow-crate-under-1500 &&`. Never write to `C:\src\quantick` or other worktrees.
> - Siblings running in parallel own `crates/app/src/control/*`, `crates/feed-mt5/*` and `bridge/mt5/*`; you own only the two named files, new files under `crates/orderflow/src/projection/` and `crates/orderflow/src/config/`, your entries in `crates/guards/size-baseline.txt`, and `mod` lines if strictly needed. Do not edit any other file (including `crates/app`, which consumes these types through their current paths; keep every public path stable with re-exports so no consumer changes).
>
> ## Decisions
> - D1: pure moves only. Every body moves unchanged; the only widenings are `pub(super)` marks. No change to any projection output, config default, validation rule, serialisation or public type path. If a move would need `pub` beyond `pub(super)`/`pub(crate)`, a new field, a consumer edit in `crates/app`, or would change iteration order anywhere, stop and report it as a human_decision.
> - D2: the ceiling is production lines as `crates/guards/src/size.rs` measures them; moving inline tests to sidecars is allowed, never required (`projection/tests/mod.rs` already exists as a sidecar).
> - D3: seams: `config.rs`: one file per config family (heatmap, bubbles, footprint, health, others as found) with the shared validation and the parent `Config` in `config.rs`; `projection.rs`: fold versus read model (or per layer) under `projection/`. If any file or sibling would still exceed 1,500, split further.
> - D4: proof of A2: statement-line multiset before/after with command and result in the PR body; the compiler drives the `pub(super)` widenings. Proof of A1: every golden, parity and determinism test in `orderflow` and `engine` passes unchanged (`cargo test -p quantick-orderflow`, `cargo test -p quantick-engine`, including `profile_fold_parity`); name the counts; grep the diff for `HashMap` to show none was introduced.
> - D5: performance: these are per-depth and per-trade paths of the heatmap engine; moving methods across `impl` blocks adds no work. State per touched path (per-trade / per-depth / per-frame / rare) in the PR body; if you change any call shape, run `cargo bench -p quantick-engine --bench hot_path` (or the nearest orderflow bench you find) before/after and report.
> - D6: after the moves, `cargo run -p quantick-guards -- --tighten`; both entries must leave `size-baseline.txt` and `!budget` must fall by exactly 3,373 from the value on your base; `cargo test -p quantick-guards` passes; `crates/guards/src/cycle.rs` must see no new module cycle and `graph.rs` no new edge. If the campaign tip moves before you open the PR, do not rebase; the coordinator does that at integration time.
> - D7: tier `high`: `arch-review` with `code-review` at `medium`, full shape pass; `ai-review` completion; `delivery-review` in full. At most two step-0 rounds; remaining minor findings ship as PR follow-ups named in the PR body.
> - D8: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`, for scripted edits. If `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all -- --check`. Run the four checks one at a time, never `||` or `| head`; read whole failures.
> - Check `df -h /c` before the first build; if under 15 GB free, `cargo clean` only in your own worktree and report.
> - Splitting: never de-indent moved items; child modules need `pub(super)` on moved helpers; a new test module must be suffixed `_tests`.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Known CI load flakes: `gateway_a_client_that_never_reads...`, `a_retry_that_races_its_own_first_call...`, `layers_tests::the_trade_paint_layer_switch_stops_the_marks`; report, rerun the job once at most.
>
> ## Delivery
> 1. Implement with the verification loop (`cargo check -p quantick-orderflow`, `cargo test -p quantick-orderflow`, `cargo test -p quantick-engine`, `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately. Conventional English commits ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 3. Archive `GOAL.md` per mission step 8 (slug `orderflow-crate-under-1500`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/refactor-orderflow-crate-under-1500 && gh pr create --draft --base campaign/lean-a-plus --title "refactor(orderflow): split the projection and config files under 1,500 production lines" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (S8); closes on integration: #375", moved-item table, purity proof, determinism evidence, size report, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D8).
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base tip; production-line table before/after per file; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; repair batches; ready accepted or denied; any human_decision; the coordinator's next action.
