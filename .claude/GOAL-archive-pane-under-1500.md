# Mission: split `crates/app/src/pane.rs` into owned pane modules under 1,500 production lines

Campaign child of #367 (task key S1), issue #368. Base `origin/campaign/lean-a-plus`
at `e8eb23e543dcc827c16c7c552478e7ebf2150a4f`.

**Objective.** Move every function body of `crates/app/src/pane.rs` (5,384
production lines) into owned sibling modules under `crates/app/src/pane/`, each
file at most 1,500 production lines as `crates/guards/src/size.rs` measures
them, with no behaviour change — so that an agent that opens a file to change
one behaviour reads one owner and not the whole subsystem.

**Tier:** `high` — set by the coordinator. The pane hosts navigation and the
chart painter, both per-frame paths; a mistaken move there changes what the
trader sees or how the chart answers the hand, so the full shape pass, the
`medium` bug pass, `ai-review` completion and a full `delivery-review` are
earned, not optional.

## Request ledger

- **R1** — `pane.rs` and every new sibling are at most 1,500 production lines
  ("split `crates/app/src/pane.rs` … into owned sibling modules, each at most
  1,500 production lines"). Source: mission brief §1, issue A1.
- **R2** — pure moves: "Every function body moves unchanged; the only
  widenings are `pub(super)` marks a child module needs. No behaviour,
  rendering, input, persistence or contract change." Source: D1, issue Scope.
- **R3** — a shape for `draw_chart` and `handle_navigation`: "extract per-layer
  painters and gesture arms into siblings … passing the pane's state as a narrow
  borrowed context … not a clone and not a new struct field. Frame-time work
  must not grow: no extra allocation, no extra pass over trades per frame."
  Source: D3, issue Scope.
- **R4** — split further by phase when any file would still exceed 1,500.
  Source: D4.
- **R5** — proof of the pure move: "a sorted line multiset of the moved bodies
  before and after … record the command and result in the PR body". Source:
  D5, issue A2.
- **R6** — performance evidence before and after on the per-frame path:
  "`APP_HEALTH_SUMMARY` fps/frame_avg on a dense replay … A regression is a
  Blocker." Source: D6, issue G4.
- **R7** — the size ratchet: "`cargo run -p quantick-guards -- --tighten`; the
  `pane.rs` entry must disappear from `size-baseline.txt` … and `!budget` must
  not rise; `cargo test -p quantick-guards` passes." Source: D7, issue A4.
- **R8** — every pinned or golden test in the touched crates passes unchanged;
  no test expectation edited. Source: issue A3.
- **R9** — the four checks green on the final head, run one at a time; CI green
  at the exact PR head. Source: issue A5, G2, delivery step 2.
- **R10** — tier `high` reviews: `arch-review` with `code-review` at `medium`,
  full shape pass; `ai-review` completion; `delivery-review` in full; at most
  two step-0 rounds, minor leftovers as named PR follow-ups; markers with the
  shared campaign key. Source: D8, issue G3, delivery step 5.
- **R11** — a draft PR whose base is exactly `campaign/lean-a-plus`, with the
  body the brief prescribes (tier, campaign line without a `Closes` keyword,
  moved-item table, multiset proof, frame timing, size report before/after,
  local verification, attribution lines); `gh pr ready` once green; never
  merge; never touch main. Source: delivery steps 4 and 7.
- **R12** — write only in this worktree and only to the files this mission
  owns: `crates/app/src/pane.rs`, new files under `crates/app/src/pane/`,
  the `pane.rs` entry of `crates/guards/size-baseline.txt` and, if strictly
  needed, `mod` lines. Source: "Your worktree" section.
- **R13** — a HANDOFF BLOCK back to the coordinator with the fields the brief
  lists. Source: delivery step 8.
- **R14** — A6 (merged into `campaign/lean-a-plus`, merge read back and
  recorded on the issue) is the coordinator's action after readiness; this
  mission reports it as pending, never performs it. Source: issue A6, "Never
  merge".

## Decisions (from the coordinator)

- **D1** — pure moves only; the only widenings are `pub(super)`. A move that
  would need `pub` beyond `pub(super)`/`pub(crate)` or a new `ChartPane` field
  stops and is reported as a human_decision.
- **D2** — the ceiling counts production lines as `size.rs` measures them
  (inline tests excluded). Moving inline tests to sidecars is allowed, never
  required.
- **D3** — `draw_chart` and `handle_navigation` get a shape: per-layer painters
  and gesture arms in `crates/app/src/pane/<name>.rs`, as `&self`/`&mut self`
  methods in `impl ChartPane` blocks, narrow borrowed context, no clone, no new
  struct field, no extra allocation or pass over trades per frame.
- **D4** — if any file would still exceed 1,500 production lines, split
  further by phase (compute versus paint, or by layer) until every file is
  under.
- **D5** — the A2 proof is a sorted line multiset of the moved bodies before and
  after, command and result in the PR body; the compiler drives the
  `pub(super)` widenings.
- **D6** — record `APP_HEALTH_SUMMARY` fps/frame_avg before and after on a
  dense replay (or the nearest benchmark); a regression is a Blocker.
- **D7** — after the moves, `--tighten`; the `pane.rs` entry disappears;
  `!budget` does not rise; the `--report` "largest" list is a fixed top-N, so
  an untouched file may enter that diff: explain, do not "fix".
- **D8** — tier `high`: `arch-review` with `code-review` at `medium`, full
  shape pass; `ai-review` completion; `delivery-review` in full; at most two
  step-0 rounds; remaining minor findings ship as named PR follow-ups.

## Assumptions

- **S1** — *wanted to ask.* The issue expects "three to four siblings" and that
  "`pane.rs` keeps the struct, its constructor and the frame entry points".
  The arithmetic does not allow both: 5,384 lines must fall to at most 1,500,
  and `draw_chart` holds field borrows of `self.history_prefix` and
  `self.state` (`prefix`, `closed`, `partial`) alive across its whole body, so
  its arms cannot be `&mut self` methods; only its read-only painters can move
  out as `&self` methods. The paint frame therefore moves whole to
  `pane/draw_chart.rs` under D4 (a split by phase: input frame in `pane.rs`,
  paint frame in its own file), and more than four siblings are cut. Reading
  taken: D4 authorises it; the count of siblings is a shape expectation, not a
  criterion. Reported in the handoff for the coordinator to confirm.
- **S2** — a body is "unchanged" when its lines are byte-identical at the same
  indentation. Blocks extracted from the two frame functions are top-level
  statements of a method body, so they land at the same 8-space depth inside
  the new methods and rustfmt re-wraps nothing. The seam lines a move adds
  (module header, imports, `impl ChartPane {`, the new signature, the call,
  a destructuring line) are listed in the PR as the multiset residual.
- **S3** — passing a small `Copy` context struct by reference to the paint
  painters (built once per frame on the stack from values `draw_chart` already
  holds) is the "narrow borrowed context" D3 asks for: it is not a `ChartPane`
  field, nothing is cloned, no trade is walked twice. The precedent is the
  existing `SharedPointer`, `footprint_render::LayerFrame` and
  `indicator_render::PaneFrame`.
- **S4** — `#[allow(clippy::too_many_arguments)]` on an extracted arm whose
  inputs are the frame locals it read is acceptable; `pane/drawing_gestures.rs`
  and `pane/axes_and_chrome.rs` already carry the same allowance for the same
  reason.
- **S5** — the free functions `paint_placement_hint`, `snap_bar_to_tape` and
  `magnet_price_of` stay in `pane.rs`: `axes_and_chrome.rs`'s header names
  them as staying there and `drawing_gestures.rs` and the tests reach them
  through `super`.
- **S6** — the D6 measurement is a replay of a recorded B3 session already on
  this machine (`Documents/Quantick/replay/WINV26`) played through the app's
  own replay hooks with every store variable pointed at the scratchpad, the
  same binary layout before and after; it is a per-frame smoke, not a
  benchmark, and the PR reads it as the gate does — "nothing fell off the
  frame cap" — with the positive claim resting on the moved bodies being
  byte-identical.
- **S7** — the tests in `pane/tests/mod.rs` keep working through `use super::*`
  once moved methods carry `pub(super)`; no test file is edited.
- **S8** — where a moved body passed `&x` and `x` became a reference
  parameter of the new method (`bands`, `areas`, `clip`, `plot_x`, `levels`,
  `time_claims`, `carved`), clippy's `needless_borrow` fires; the workspace
  denies warnings and `Cargo.toml` says a lint is fixed, never allow-ed. The
  fifteen call arguments were changed by `cargo clippy --fix` to the bare name
  — the same reference, the same callee, verified by the compiler — and are
  listed one by one in the PR body. The price scale was instead stored by
  value in `DrawFrame` (it is `Copy`) so its `&scale` uses stayed verbatim.
  Read as within D1: no behaviour, rendering, input or contract changes, and
  the alternative was an `allow` the repository forbids.

## Acceptance criteria

- [x] **A1** — `crates/app/src/pane.rs` and each sibling under
      `crates/app/src/pane/` measure at most 1,500 production lines.
      *Evidence:* `cargo run -p quantick-guards -- --report` before and after,
      per-file table in the PR body. → PR body, "Size report". *(R1, R4)*
- [x] **A2** — every production move is a pure move: the sorted line multiset
      of the moved bodies is identical before and after; the residual is only
      seam lines, each named.
      *Evidence:* the script and its output in the PR body. → PR body,
      "Multiset proof". *(R2, R3, R5)*
- [x] **A3** — every pinned or golden test in `quantick-app` and
      `quantick-guards` passes unchanged; `git diff --stat` shows no edit under
      `crates/app/src/pane/tests/`, `crates/app/tests/` or any expectation.
      *Evidence:* `env -u QUANTICK_BUBBLES cargo test --workspace` exit 0 and
      the name-status diff in the PR body. → PR body, "Local verification".
      *(R8)*
- [x] **A4** — `crates/guards/size-baseline.txt` carries no `pane.rs` entry
      and the `!budget` line did not rise; `cargo test -p quantick-guards`
      passes.
      *Evidence:* the baseline diff and the guards test exit code in the PR
      body. → PR body, "Size report". *(R7)*
- [x] **A5** — *(local four checks green at 4e8218dc; CI at the PR head pending at archive time, reported in the handoff)* the four checks are green on the final head, run one at a
      time, and CI is green at the exact PR head.
      *Evidence:* four exit codes in the PR body; `gh pr checks <n>` all
      passing at the head SHA. → PR body, "Local verification"; the PR checks
      tab. *(R9)*
- [x] **A6** — `draw_chart` and `handle_navigation` have a shape: each is an
      orchestrator that calls named per-layer painters / gesture arms living
      in siblings, the state passed as `&self`/`&mut self` plus borrowed frame
      values, with no new `ChartPane` field, no clone and no added per-frame
      allocation or pass over trades.
      *Evidence:* the moved-item table naming each arm and its file; the
      `ChartPane` struct diff empty; `arch-review` dimension 3 (performance)
      verdict. → PR body, "Moved items" and "Performance". *(R3)*
- [x] **A7** — frame timing before and after on the same replay:
      `APP_HEALTH_SUMMARY` fps / frame_avg / frame_cpu, with no regression.
      *Evidence:* the two log excerpts and the table in the PR body. → PR
      body, "Performance". *(R6)*
- [x] **A8** — the draft PR exists against `campaign/lean-a-plus` with the
      prescribed body (tier, campaign line without a `Closes` keyword,
      moved-item table, multiset proof, frame timing, size report, local
      verification, attribution). *Amended at review time:* "`gh pr ready`
      ran alone once everything was green" was a closing step misfiled as
      a criterion — it is **C2**, where it already stood; the reviews'
      verdicts and CI are **G3** and **A5**.
      *Evidence:* `gh pr view 383 --json baseRefName,isDraft,body`. → PR #383.
      *(R10, R11)*
- [x] **A9** — every write landed in this worktree and only in the files the
      mission owns.
      *Evidence:* `git diff --name-status <base>...HEAD` in the PR body lists
      only `crates/app/src/pane.rs`, `crates/app/src/pane/*`,
      `crates/guards/size-baseline.txt` and `.claude/GOAL-archive-*.md`.
      → PR body, "Local verification". *(R12)*
- ~~**A10** — the HANDOFF BLOCK is returned with every field the brief
      lists, A6-of-the-issue (the campaign merge) marked pending for the
      coordinator.~~ *Struck at review time, never renumbered:* the handoff
      is how control returns to the coordinator — closing step **C3**, not a
      product outcome — and the campaign merge is **C4** below, the
      coordinator's action. R13 and R14 are discharged by C3 and C4.
      *(R13, R14)*

## Gates

- [x] **G1** — every artifact English; conventional commits ending with the
      attribution lines. *Evidence:* `arch-review` dimension 8;
      `cargo test -p quantick-guards` (language guard). → review report.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo
      test --workspace`, each run alone before every commit and on the final
      head. *Evidence:* exit codes in the PR body. → PR body.
- [x] **G3** — `arch-review` over `origin/campaign/lean-a-plus...HEAD` with
      step 0 at `medium`, every Blocker/Should-fix resolved or deferred in the
      PR body; `ai-review` completion recorded. (`delivery-review` PASS is
      **C1**, the closing step, not a gate this review can grade about
      itself.) Markers written with `sh .claude/hooks/campaign_context.sh
      key "$WT"`.
      *Evidence:* the arch-review verdict comment on PR #383
      (issuecomment-5642365308: step 0 `medium`, two rounds, 8+8 findings,
      0 correctness, no Blocker/Should-fix open); the AI-review report
      (issuecomment-5642384918) with its two WEAK threads; `arch-review-ok`
      and `ai-review-complete` in the worktree git dir, each holding the
      shared key for the reviewed diff. → PR #383, the worktree git dir.
- [x] **G4** — performance declared per touched path by rate: `draw_chart`
      and its painters (per-frame), `handle_navigation` and its arms
      (per-frame), the series methods (per-trade / rare) — every one relocated
      verbatim, complexity unchanged. *Evidence:* the rate table and A7's
      numbers in the PR body. → PR body, "Performance".

## Not applicable, and why

- *Touches anything user-visible* (`ui-harness` hook, `visual-qa`,
  `trader-ux-review`): no surface, pixel, hotkey or string changes; every
  painter and gesture body moves byte-identical. The D6 replay run doubles as
  a smoke that the chart still presents; no new surface needs a hook.
- *Adds a capability* / *adds something a trader does*: nothing is added.
- *Engine / determinism territory*: no engine code is touched.
- *Docs/skills only*: this is a code change; the full shape pass applies.

## Evidence at archive time

Recorded here so the archive says what was proven before the reviews ran;
the PR body carries the full tables and the review verdicts and CI land in
the handoff.

- Split commit `4e8218dc`: `pane.rs` 5,376 -> 1,256 production lines
  (guard's own law); largest sibling `draw_chart.rs` at 769; eleven new
  siblings, two extended. `cargo run -p quantick-guards -- --report`:
  `ratchet.size.budget 40121`, `ratchet.size.recorded 40121`, no `pane.rs`
  entry.
- Multiset proof (`scratchpad/multiset.py`, in the PR body): 5,210 non-blank
  lines cancel; the old-side residual is 55 lines — the fifteen `&x -> x`
  call arguments, the `pub(super)` widenings and the pruned `pane.rs`
  imports — and nothing else.
- Four checks at `4e8218dc`, each run alone: `cargo fmt --all -- --check`
  exit 0; `cargo clippy --workspace --all-targets` exit 0;
  `cargo build --workspace` exit 0; `env -u QUANTICK_BUBBLES cargo test
  --workspace` exit 0 (1,983 app tests, 252 in the pane modules).
  `cargo test -p quantick-guards` exit 0.
- No file under `crates/app/src/pane/tests/` or `crates/app/tests/` changed.
- Frame timing: `APP_HEALTH_SUMMARY` on the WINV26 2026-08-21 replay at 50x,
  before/after binaries interleaved; the paired runs sit on the same cap
  (59 fps, 16.7 ms, frame_cpu 2.65 vs 2.72 ms median) — table in the PR.

## Repair batches after the initial review

- **Batch 1** (`d14a7afc`, after step 0 `code-review medium 383` on
  `2d29ab3a` returned 8 findings, 0 correctness): `TimePaneAreas` kept
  nameable via `pub(crate) mod canvas_split` (SF-1, closed); `axis_claims`
  returns a named `AxisChips` (C-3, closed); `handle_context_menu`,
  `handle_paper_input`, `handle_axis_gestures` and
  `handle_indicator_pane_gestures` derive `price_band`/`drawing_scale`/
  `total`/`auto` from the receiver, two `too_many_arguments` allowances
  dropped (C-4, C-6, closed); four stale location comments and the
  `drawing_gestures.rs` header fixed (C-7, closed); baseline note names PR
  #383 and 769 (SF-8, closed); five constants followed their only reader
  (cut candidate, closed). Deferred to a named follow-up under D1: folding
  `pointer_delta` into `SharedPointer` with `#[derive(Clone, Copy)]` (C-2
  and the verifier's candidate 1) and slimming `DrawFrame` / carrying
  `axis_x`, `clip`, `half`, `content_half`, `candles` in it (C-5).
  Four checks green at `d14a7afc`, each run alone.
- **Batch 2** (after step 0 round 2, `code-review medium 383` on `4c6bccbb`:
  8 findings, 0 correctness, count flat — the last round D8 allows):
  `DrawFrame` and `AxisChips` moved to `pane/draw_frame.rs` so
  `draw_chart.rs` and `layer_painters.rs` no longer import each other
  (F-3, closed); the baseline note names `drawing_gestures.rs` (800) as the
  largest sibling (F-8, closed). Named follow-ups, not repaired here:
  per-sibling test modules replacing the `#[cfg(test)]` re-import shim in
  `pane.rs` (F-1; D2 made test moves optional, and `CLAUDE.md`'s "a surface
  that moves out takes its tests with it" is the rule the follow-up serves);
  an index-only `DrawFrame` so the writing steps of `draw_chart` can become
  `&mut self` methods (F-2, with F-7's derivable fields and `axis_x` /
  `nothing_in_view` accessors); a `CandleDressing` value and the
  under-candles carve beside its over-candles twin (F-4);
  `SharedPointer: Copy` carrying `pointer_delta` and the paper claim (F-5,
  F-6). Four checks green after batch 2 (`e8a7432d`), each run alone;
  `pane.rs` measures 1,212 production lines at the final head.

## Closing steps

- **C1** — `delivery-review` returns PASS (tier `high`).
- **C2** — the PR is open as a draft, then `gh pr ready <n>` once reviews,
  markers, AI completion and CI are green.
- **C3** — control returns to the campaign coordinator with the handoff
  block (every field the brief lists; R13).
- **C4** — the coordinator merges into `campaign/lean-a-plus` through this PR
  and records the merge on #368 (issue A6; R14). Never this mission's action.

## The request, as received

Quoted verbatim from the coordinator's brief (an attributed quotation under
`CLAUDE.md`'s language exemption; it is English throughout):

> You are executing campaign child mission **S1** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for the repository
> milocaetano/quantick: split `crates/app/src/pane.rs` (5,384 production
> lines) into owned sibling modules, each at most 1,500 production lines, with
> no behaviour change. You are a subagent: you cannot ask the trader anything;
> the coordinator has answered the mission's step-3 questions below as
> decisions D1..Dn. A doubt that would need a new decision is reported back,
> never guessed.
>
> ## Ground rules (read these files first, in this order)
> 1. `C:\src\quantick\CLAUDE.md` (all rules apply; English everywhere in the
>    repo; the size ratchet and `--tighten` are described under *Keeping the
>    trunk small*).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign
>    child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9 exactly. Step 3
>    is answered below. Step 6 is already done (worktree, `mission-base`,
>    `mission-tier`, guards armed) but you still run `cargo check -p
>    quantick-app --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` — base is
>    `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`,
>    review keys from `sh .claude/hooks/campaign_context.sh key "$WT"`.
> 4. `C:\src\quantick\docs\workflow\delivery.md`.
> 5. The task: issue #368 (`gh issue view 368`), which is your scope,
>    acceptance criteria A1..A6 and gates.
> 6. The history of earlier cuts of this file, in the header comments of
>    `crates/guards/size-baseline.txt` (what moved to `crates/app/src/pane/`
>    already and why `handle_navigation` and `draw_chart` stayed).
>
> ## Your worktree (the only place you write)
> - Worktree: `C:\src\quantick-worktrees\refactor-pane-under-1500` (Git Bash
>   `/c/src/quantick-worktrees/refactor-pane-under-1500`), branch
>   `refactor/pane-under-1500`, cut from `origin/campaign/lean-a-plus` at
>   `e8eb23e543dcc827c16c7c552478e7ebf2150a4f`.
> - Never write to `C:\src\quantick` or any other worktree. Every command
>   starts with `cd /c/src/quantick-worktrees/refactor-pane-under-1500 &&`.
> - Two sibling missions run in parallel on this machine (paper trading
>   files, and two test files under `worker_progress` and
>   `app/tests/control_plane_tests.rs`); you own only
>   `crates/app/src/pane.rs`, new files under `crates/app/src/pane/`,
>   `crates/guards/size-baseline.txt` (your entry only) and, if strictly
>   needed, `mod` lines. Do not edit any other file.
>
> ## Decisions from the coordinator (record as D1..Dn in GOAL.md)
> - D1: pure moves only. Every function body moves unchanged; the only
>   widenings are `pub(super)` marks a child module needs. No behaviour,
>   rendering, input, persistence or contract change. If a move would require
>   making a private item `pub` beyond `pub(super)`/`pub(crate)`, or adding a
>   field to `ChartPane`, stop and report it in the handoff as a
>   human_decision instead of doing it.
> - D2: the ceiling is production lines as `crates/guards/src/size.rs`
>   measures them (inline tests excluded). Moving inline tests to sidecars is
>   allowed when it helps legibility, never required.
> - D3: give `draw_chart` and `handle_navigation` a shape: extract per-layer
>   painters and gesture arms into siblings (`crates/app/src/pane/<name>.rs`),
>   passing the pane's state as a narrow borrowed context (`&self`/`&mut self`
>   methods in `impl ChartPane` blocks in the child modules, as the previous
>   four cuts did), not a clone and not a new struct field. Frame-time work
>   must not grow: no extra allocation, no extra pass over trades per frame.
> - D4: if `pane.rs` or any new sibling would still exceed 1,500 production
>   lines, split further by phase (compute versus paint, or by layer) until
>   every file is under.
> - D5: proof of A2: for the moved items, produce a sorted line multiset of
>   the moved bodies before and after (for example `git show
>   origin/campaign/lean-a-plus:crates/app/src/pane.rs | sed -n 'A,Bp' | sort`
>   versus the new file's lines, or a script over the diff) and record the
>   command and result in the PR body; the compiler drives the `pub(super)`
>   widenings.
> - D6: performance: it is a per-frame path. Before and after, record
>   `APP_HEALTH_SUMMARY` fps/frame_avg on a dense replay or the existing
>   benchmark you find nearest to `draw_chart` (see `docs/quality/` and
>   `.claude/skills/ui-harness/SKILL.md` for how to run the app headlessly
>   with env hooks; if a live capture is impractical, say so and give the
>   benchmark/timing you could run). A regression is a Blocker.
> - D7: after the moves, `cargo run -p quantick-guards -- --tighten`; the
>   `pane.rs` entry must disappear from `size-baseline.txt` (under threshold)
>   and `!budget` must not rise; `cargo test -p quantick-guards` passes. The
>   `--report` "largest" list is a fixed top-N, so an untouched file may
>   appear in the diff of that report; explain, do not "fix".
> - D8: tier `high`: `arch-review` with `code-review` at `medium`, full shape
>   pass; `ai-review` completion; `delivery-review` runs in full. At most two
>   step-0 rounds; remaining minor findings ship as PR follow-ups named in the
>   PR body.
>
> ## Environment notes
> - Use `python`, not `python3`, for scripted edits of large Rust files. If
>   `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all --
>   --check`.
> - Run the four checks one at a time, never chained with `||` or `| head`;
>   read the whole failure output.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`.
> - When splitting: never de-indent moved items; child modules of `pane` need
>   `pub(super)` on moved helpers; let the compiler drive; a new test module
>   must be suffixed `_tests` or it shadows a real crate module. Cargo cannot
>   see module cycles but `crates/guards/src/cycle.rs` fails the build on a
>   new one; do not create one.
> - If writing the review markers into the git dir is denied by the
>   permission classifier, do not work around it: put the exact `printf` lines
>   in your handoff and mark the step pending.
>
> ## Delivery
> 1. Implement with the verification loop between edits (`cargo check -p
>    quantick-app`, targeted pane tests, `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy
>    --workspace --all-targets`, `cargo build --workspace`, `env -u
>    QUANTICK_BUBBLES cargo test --workspace`, each separately. Conventional
>    English commits ending with `Co-Authored-By: Claude Fable 5.1
>    <noreply@anthropic.com>` and `Claude-Session:
>    https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 3. Archive `GOAL.md` per mission step 8 (slug `pane-under-1500`) as the last
>    commit before reviews.
> 4. Push and open a **draft** PR: `cd
>    /c/src/quantick-worktrees/refactor-pane-under-1500 && gh pr create
>    --draft --base campaign/lean-a-plus --title "refactor(app): split pane.rs
>    into owned pane modules under 1,500 production lines" --body-file -` with
>    a heredoc body following the PR template: mission tier, "Campaign child
>    of #367 (S1); closes on integration: #368" (no `Closes` keyword), the
>    moved-item table, the multiset proof, the frame-timing numbers, the size
>    report before/after, local verification, then `🤖 Generated with [Claude
>    Code](https://claude.com/claude-code)` and
>    `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. Run `/arch-review` (step 0 bundles `code-review` against the campaign
>    base), then `/ai-review`, then `/delivery-review`. Resolve Blockers and
>    Should-fixes; record markers with the shared key exactly as
>    `docs/campaign/integration.md` shows.
> 6. Watch CI at the exact PR head (`gh pr checks <n> --watch`, bounded). A
>    failure of `delayed_producer_bookkeeping...` or
>    `gateway_client_reads_the_running_application...` is a known load flake
>    being fixed by a sibling mission: report it, do not rerun blindly more
>    than once.
> 7. When reviews, markers, AI completion and CI are green: `cd
>    /c/src/quantick-worktrees/refactor-pane-under-1500 && gh pr ready <n>` as
>    its own command. **Never merge.** Never touch main.
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; final head SHA;
>    campaign base tip at review time; production-line table before/after per
>    file; review verdicts with URLs; markers recorded yes/no; CI run URL and
>    conclusion; findings closed/open with IDs; repair batches used; anything
>    pending/blocked (including any human_decision) and the exact next action
>    for the coordinator.
