# Mission: `app.rs` sheds its sidecars

Move four cohesive `impl QuantickApp` groups out of `crates/app/src/app.rs`
into sibling modules under `crates/app/src/app/`, bodies unchanged, so the
file that the size ratchet exists for finally falls below 8,000 lines and its
ceiling and the size budget come down with it.

**Why it matters.** `app.rs` is the first entry in
`crates/guards/size-baseline.txt` and the one file the sprint's merges left
the same size or larger — 9,181 production lines at `bc39248`, 9,239 today.
Every mission so far extracted from *around* it. This is a read-cost cut: the
sidecars become sibling files holding `impl QuantickApp`, the shape
`app/layout_wiring.rs` already has. Turning them into ports is the mission
after, once the paper seam has landed and `draw_menu_bar` is free to move.

**Tier:** `medium`. A ~1,300-line move with a hard number to hit (`app.rs`
at most 8,000 lines) and a ratchet to re-sign, but no new behaviour, no new
surface and no trader-facing decision. It earns the full ledger, the criteria
and both reviews; it does not earn the four-question round or `max`'s
re-check against the plan.

## Request ledger

The request is the brief at `C:\src\mission-app-rs-sidecars.md`, quoted in
full at the end of this file.

| # | Ask |
| --- | --- |
| R1 | Move the harness **demo appliers** into a sibling module under `crates/app/src/app/`. |
| R2 | Move the **drawing-input handlers** — `handle_drawing_keys`, `apply_drawing_chrome` — into a sibling module. |
| R3 | Move the **health summary** — `maybe_emit_summary`, `status_model` — into a sibling module. |
| R4 | Move the **workspace restore** — `restore_workspace` — into a sibling module. |
| R5 | **Bodies unchanged.** Visibility widens only where a move requires it, and the PR body says what was widened. |
| R6 | **Every hook and control name kept**; each moved read stays reachable by the generated-registry guard, a new module declaring its own `declare_hooks!` slice for any `QUANTICK_*` it reads. |
| R7 | Any inline test exercising a moved method goes to that module's own `#[cfg(test)]`; `app/tests/*.rs` change nothing but paths. |
| R8 | **Ceiling tightened**: `--tighten` run, `app.rs`'s ceiling at its new size, and the four new files under the 1,500-line threshold. |
| R9 | The size **`!budget` falls by at least 1,100**. |
| R10 | `app.rs` **at most 8,000 lines**. |
| R11 | `cargo run -q -p quantick-guards -- --report` before and after, diffed: **only `app.rs`-related lines move**. |
| R12 | The **generated hook registry and capability inventory are unchanged**, and `cargo test -p quantick-guards` is green. |
| R13 | `git diff --color-moved=zebra` shows **moves, not edits**; every non-move hunk quoted and explained. |
| R14 | The **four checks** green, and `cargo test -p quantick-app` runs the **same number of tests**. |
| R15 | **Verify the brief's evidence ledger (#1–#9) before acting** rather than trusting it. |
| R16 | **Respect the parallel branches**: prove no overlap with `refactor/paper-policy-out-of-the-ticket` and the indicator branch before the first move and again before the PR. |
| R17 | **Stay out of** `draw_menu_bar`, `draw_toolbar`, `apply_toolbar_action`, `draw_frame`, `new_with_workspace`, `adopt_tab`, `arm_strategy_instance`; turn no sidecar into a port or a surface; touch none of `QuantickApp`'s fields. |
| R18 | *(purpose)* `app.rs` **finally shrinks this sprint**, and the guard report is where that is read. |

## Decisions taken by the trader

None. Step 3 raised no question: the one contradiction in the brief is `S6`,
and measurement resolved it without needing an answer.

## Assumptions

- **S1** — The demo group takes the brief's four named appliers *plus* the
  demo-shaped neighbours in the same region that only moved code calls:
  `apply_drawing_draft`, `apply_venue_history_demo`,
  `deliver_synthetic_prefix`, `apply_avwap_demo`, `seed_band_demo`,
  `apply_drawing_demo_recut` and `carry_inspector_across_selection`. The brief
  told the mission to *"measure the region, not just the four"*; these are the
  members of `7195-8283` that are demos. Safe to assume rather than ask: the
  8,000-line target is met by the four named alone, so this widens the cut
  without putting the criterion at risk, and it is reversible by moving a
  function back.
- **S2** — `arm_strategy_instance`, `duplicate_selected_drawing`,
  `carry_strategy_to_duplicate`, `apply_replay_restart`,
  `play_pending_alarms`, `report_alert_attempt`, `apply_scripted_view`,
  `apply_load_older`, `apply_load_older_candles` and
  `apply_history_note_hook` stay in `app.rs` though they sit inside the same
  region: the first is named out of scope by R17 and is touched by the open
  paper branch, and the rest are not demos.
- **S3** — Module names are `demo_hooks.rs`, `drawing_input.rs`, `health.rs`
  and `workspace_restore.rs`, the brief's own suggestion; it said the names
  were the mission's to adjust and no better ones presented themselves.
- **S4** — Moved methods gain `pub(super)` where a caller stays behind, the
  visibility `app/layout_wiring.rs` already uses for exactly this. Every call
  site measured sits in `app.rs` or under `app/tests/`, both of which
  `pub(super)` reaches, so nothing needs `pub(crate)`.
- **S5** — Each new module carries its own `use super::{…}` header rather than
  a glob, following `layout_wiring.rs:53`.
- **S6** — *Wanted to ask, and measurement answered it.* The brief's ledger #7
  and scope #3 assert both that a moved read must carry its `declare_hooks!`
  slice with it *and* that the generated registry stays byte-identical. Those
  two cannot both hold: the registry's `Declared in` cell is built from
  `hooks::OWNERS` (`crates/app/src/hooks.rs:99`, fused at `hooks.rs:337`), so
  moving a declaration rewrites that cell. It does not have to be resolved,
  because the premise is false for these ranges — every `std::env::var` read
  of an `app.rs`-declared hook sits in `new_with_workspace` (`app.rs:825`) or
  in `harness.rs`, not in a moved body; the moved regions name `QUANTICK_*`
  only in doc comments and log messages. So no declaration moves, `OWNERS` is
  untouched, and R6 and R12 hold together. Recorded here rather than asked
  because the reading is checkable and the mission checks it (`A6`).
- **S7** — `refactor/native-indicator-docking`, the second open branch in the
  brief's ledger #8, is already merged (`d3cf317`, PR #293), so only
  `refactor/paper-policy-out-of-the-ticket` is still open against `app.rs`;
  R16's overlap proof is run against both regardless.

## Acceptance criteria

- [x] **A1** — The demo appliers, the drawing-input handlers, the health
      summary and the workspace restore each live in their own sibling module
      under `crates/app/src/app/`, declared by a `mod` line beside
      `app/layout_wiring.rs`'s, each opening with `use super::{…}` and holding
      `impl QuantickApp`.
      *Evidence:* the four files, `git diff --stat`, and the `mod` lines quoted.
      → `.claude/evidence/app-rs-sidecars/module-shape.log`. *(R1, R2, R3, R4)*
- [x] **A2** — `crates/app/src/app.rs` is at most **8,000** lines.
      *Evidence:* `wc -l` before and after.
      → `.claude/evidence/app-rs-sidecars/line-count.log`. *(R10, R18)*
- [x] **A3** — `app.rs`'s baseline ceiling is tightened to its new size and
      the size `!budget` falls by at least **1,100**; none of the four new
      files reaches the 1,500-line threshold.
      *Evidence:* the `size-baseline.txt` diff, quoted.
      → `.claude/evidence/app-rs-sidecars/baseline-diff.log`. *(R8, R9)*
- [x] **A4** — The guards report before and after differs only in
      `app.rs`-related lines.
      *Evidence:* the `diff` of the two `--report` runs.
      → `.claude/evidence/app-rs-sidecars/guards-report-{before,after}.log` and `guards-report-diff.log`. *(R11)*
- [x] **A5** — Every moved body is byte-identical modulo relocation: the
      branch diff under `--color-moved=zebra` is moves, with each non-move
      hunk quoted and explained (the `pub(super)` widenings of `S4`, the
      `use` headers, the `mod` lines).
      *Evidence:* the command's output summarised with every non-move hunk listed.
      → `.claude/evidence/app-rs-sidecars/moved-diff.log`. *(R5, R13)*
- [x] **A6** — `.claude/skills/ui-harness/references/hook-registry.md` and
      `docs/control-plane/capability-inventory.md` are untouched by the
      branch, and no hook or control name changed.
      *Evidence:* `git diff --stat origin/main...HEAD` showing neither file,
      plus the before/after set of `QUANTICK_*` names in `crates/app/src`.
      → `.claude/evidence/app-rs-sidecars/hooks-unchanged.log`. *(R6, R12)*
- [x] **A7** — Every test that exercised a moved method still runs, from
      wherever it now lives, and `cargo test -p quantick-app` reports the same
      totals as before the move.
      *Evidence:* the two runs' `test result:` lines.
      → `.claude/evidence/app-rs-sidecars/app-tests-{before,after}.log`. *(R7, R14)*
- [x] **A8** — No hunk of the branch's `app.rs` diff falls inside a region
      `refactor/paper-policy-out-of-the-ticket` edits, and none of the ranges
      named out of scope by R17 is touched.
      *Evidence:* the two branches' `app.rs` hunk headers, compared, taken
      before the first move and again before the PR.
      → `.claude/evidence/app-rs-sidecars/overlap.log`. *(R16, R17)*
- [x] **A9** — Each of the brief's evidence-ledger claims #1–#9 is
      re-measured, with the result — confirmed, or corrected — written down.
      *Evidence:* one line per claim.
      → `.claude/evidence/app-rs-sidecars/brief-ledger-recheck.log`. *(R15)*
- [ ] **G1** — Every artifact on the branch is in English.
      *Evidence:* `arch-review` dimension 8 and `cargo test -p quantick-guards`
      (`language.rs`). → the arch-review verdict in this session, and the PR body.
- [x] **G2** — The four checks are green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`, each run on its own.
      *Evidence:* the four exit codes and tails.
      → `.claude/evidence/app-rs-sidecars/four-checks.log`.
- [ ] **G3** — Performance impact declared. Every touched path classified by
      rate; a pure relocation within one crate changes no path's rate, and the
      PR body says so.
      *Evidence:* the classification, in the PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, with its
      step-0 bug pass, and every Blocker and Should-fix resolved or deferred
      in the PR body.
      *Evidence:* the verdict in this session, and the PR body.

## Verification results

Recorded at `c265313`, the branch's last code commit, before either review
ran. Every path below is under `.claude/evidence/app-rs-sidecars/`.

| | Result | Where |
| --- | --- | --- |
| **A1** | MET — `demo_hooks.rs` (1,010), `drawing_input.rs` (335), `health.rs` (327), `workspace_restore.rs` (157), four `mod` lines beside `mod layout_wiring;`, each file `use super::{…}` + `impl QuantickApp` | `module-shape.log` |
| **A2** | MET — `app.rs` 9,241 → **7,501** lines (7,499 production), against a target of 8,000 | `line-count.log` |
| **A3** | MET — ceiling 9,362 → 7,499, `!budget` 61,410 → **59,547**, a fall of **1,863** against a floor of 1,100; no new file reaches 1,500 | `baseline-diff.log` |
| **A4** | MET — nine numbers move and all nine are the cut: `app.rs`, the two ratchet numbers, and the crate/total counts, which rise **+89** for the four files' headers. Stated, not smoothed | `guards-report-diff.log` |
| **A5** | MET — 1,717 added / 1,726 removed lines paired as moves; 113 + 19 unpaired, every one enumerated: 4 `mod` lines, 1 trimmed import, 12 `pub(super)` signatures, the four files' headers, the tests binding, 4 stray blanks | `moved-diff.log` |
| **A6** | MET — neither generated file is in the diff, `hooks.rs` untouched, and the sorted set of `QUANTICK_*` names hashes identically before and after (`53b36ed6…`) | `hooks-unchanged.log` |
| **A7** | MET — 1,899 test names, identical sets, `diff` exit 0; 1894 passed / 4 ignored both sides | `app-tests-compared.log`, `app-tests-{before,after}.log` |
| **A8** | MET — no hunk of the open paper branch falls in a moved range (nearest: five lines), and `git merge-tree` auto-merges `app.rs` cleanly. One conflict, in `size-baseline.txt`, with a mechanical resolution | `overlap.log` |
| **A9** | MET — claims #1, #2, #8 and #9 confirmed; #3–#6 confirmed as regions and corrected upward in size; #7's premise corrected (no hook declaration needs to move) | `brief-ledger-recheck.log` |
| **G2** | MET — `fmt --check`, `clippy --workspace --all-targets`, `build --workspace`, `test --workspace`, each run separately, each exit 0 | `four-checks.log` |
| **G1** | Pending `arch-review` dimension 8. `cargo test -p quantick-guards` (which runs `language.rs`) is green. | the review verdict |
| **G3** | Pending: the classification lands in the PR body. | the PR body |
| **G4** | Pending: `arch-review` runs after this commit. | the review verdict |

## Not applicable, and why

- **Hot path** — no path's rate changes: the same methods run in the same
  order from the same callers, in the same crate. `G3` declares it anyway.
- **User-visible surfaces** (`ui-harness`, `visual-qa`, `trader-ux-review`) —
  no surface is added, moved or redrawn, and every hook and control name is
  kept identical, which `A6` proves. There is nothing for a capture to
  photograph that a `main` capture would not photograph identically.
- **Adds a capability** (`new-extension`) — nothing is added. This is a
  read-cost cut, and the brief says so: the sidecars become sibling modules,
  not ports. Docking them is the mission after.
- **Adds something a trader does** — nothing new is drivable, and nothing
  drivable stops being drivable.
- **Engine / determinism, test-first** — no engine code, no bar building and
  no behaviour at all; the tests that exist already cover the moved methods
  and must keep passing unchanged (`A7`).
- **Docs/skills only** — no. This is code, so the full shape pass applies.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the tier, with the evidence in its body.

## The request as received

Quoted verbatim and unedited, because a ledger that paraphrases its source
becomes its own source of truth and an ask dropped while writing it would be
unfindable afterwards. The request is in English; nothing here is translated.

The `/mission` line:

> /mission medium refactor/app-rs-sidecars — crates/app/src/app.rs has not
> shrunk since the sprint began: 9,181 lines at bc39248, 9,239 today, because
> every mission so far extracted from around it and PR #287 added hook
> declarations into it. Move four cohesive `impl QuantickApp` groups that no
> open branch touches — the harness demo appliers, the drawing-input handlers,
> the health summary, and the workspace restore, about 1,290 lines — into
> sibling modules under crates/app/src/app/, the way app/layout_wiring.rs
> already holds one. Bodies unchanged, every hook and control name kept,
> ceiling tightened, budget lowered. Read C:\src\mission-app-rs-sidecars.md in
> full before anything else and build the request ledger from it.

The brief that line points at, `C:\src\mission-app-rs-sidecars.md`, in full:

> # Mission brief: `app.rs` sheds its sidecars — the first cut that lowers its ceiling this sprint
>
> Paste the `/mission` line below into a fresh session in `C:\src\quantick`. Every
> claim was measured against `origin/main` at `2254039` on 2026-09-04; each carries
> its `file:line` so the mission re-checks it instead of trusting it.
>
> ## The paste-able invocation
>
> ```
> /mission medium refactor/app-rs-sidecars — crates/app/src/app.rs has not shrunk since the sprint began: 9,181 lines at bc39248, 9,239 today, because every mission so far extracted from around it and PR #287 added hook declarations into it. Move four cohesive `impl QuantickApp` groups that no open branch touches — the harness demo appliers, the drawing-input handlers, the health summary, and the workspace restore, about 1,290 lines — into sibling modules under crates/app/src/app/, the way app/layout_wiring.rs already holds one. Bodies unchanged, every hook and control name kept, ceiling tightened, budget lowered. Read C:\src\mission-app-rs-sidecars.md in full before anything else and build the request ledger from it.
> ```
>
> ## Why this mission
>
> The trader's own words: *"app.rs continua com mais de 9k de linhas… nunca caiu
> desde que começamos o refactor."* True, and measured. `app.rs` is the file
> the size guard exists for (`crates/guards/size-baseline.txt`, first entry),
> and it is the one file the sprint's eleven merges left untouched or slightly
> larger. The two branches open right now edit it at the paper fan-out and the
> indicator sites; this mission takes only what lies away from both, so it can
> run beside them.
>
> This is a read-cost cut, not a docking cut, and the brief says so: the
> sidecars become sibling files with `impl QuantickApp`, exactly the shape
> `app/layout_wiring.rs` already has (`layout_wiring.rs:53-64`). Turning them
> into ports is the mission after, once the paper and indicator seams have
> landed and `draw_menu_bar` (633 lines, 11 paper and 13 indicator mentions) is
> free to move.
>
> ## Evidence ledger — verify each before acting
>
> | # | Claim | Where |
> | --- | --- | --- |
> | 1 | `app.rs` line count across the sprint: 9,181 → 9,234 → 9,239 | `git show bc39248:crates/app/src/app.rs`, `d551813`, `2254039` |
> | 2 | The sibling-module precedent: `app/layout_wiring.rs` (1,557 production lines) opens with `use super::{…}` and three `impl QuantickApp` blocks; `app.rs:21` declares it with one `mod` line | `crates/app/src/app/layout_wiring.rs:53,64,1345,1481` |
> | 3 | **Demo hooks**, 559 lines, zero paper/indicator/toolbar mentions: `apply_control_evidence_hook` `:2096-2209` (`QUANTICK_CONTROL_EVIDENCE`), `apply_drawing_demo` `:7202-7322`, `apply_strategy_demo` `:7783-7960`, `apply_frvp_demo` `:7967-8112` (`QUANTICK_FRVP_FOLD_BUDGET`) | `app.rs` at those lines; helpers between `:7202` and `:8112` likely belong with them — measure the region, not just the four |
> | 4 | **Drawing input**, 306 lines: `handle_drawing_keys` `:6168-6314` (three `paper` mentions, all guards on the ticket being armed), `apply_drawing_chrome` `:6460-6618` | same |
> | 5 | **Health**, 294 lines: `maybe_emit_summary` `:5065-5307` (four hook reads), `status_model` `:5315-5365` | same |
> | 6 | **Workspace restore**, 127 lines: `restore_workspace` `:3993-4119` | same |
> | 7 | Hook declarations for the file are one `declare_hooks!` at `app.rs:9185`; a moved read must keep its declaration reachable by the generated-registry guard (`crates/guards/src/generated.rs`) — a sibling module declares its own slice, as the feed crate's adapters do | `app.rs:9185`; `crates/feed/src/hooks.rs` |
> | 8 | The two open branches' regions of `app.rs`: paper fan-out (`set_cmd_trading`, risk and strategy setters, `adopt_tab` `:…` with 11 paper mentions, `new_with_workspace` with 49) and the indicator sites (`add_native_indicator`, `apply_toolbar_action`, `:1172`, `:1726`, `:2787`, `:3515`). None of the ranges in #3–#6 overlap them | `refactor/paper-policy-out-of-the-ticket`, PR #293 |
> | 9 | Ceiling today 9,362 production lines; budget 61,410; the guard forbids sitting more than 200 below a ceiling, so `--tighten` is mandatory here | `crates/guards/size-baseline.txt` |
>
> ## Scope
>
> 1. **Four sibling modules** under `crates/app/src/app/`: `demo_hooks.rs`,
>    `drawing_input.rs`, `health.rs`, `workspace_restore.rs`, each `use super::…`
>    plus one or more `impl QuantickApp` blocks, declared by four `mod` lines
>    beside `app.rs:21`. Names are the mission's to adjust; the split is not.
> 2. **Bodies unchanged.** Visibility widens only where a moved method calls a
>    private free function or field that stays; prefer moving the helper with
>    its caller, and say in the PR body what was widened.
> 3. **Hooks travel with their reads** (ledger #7): each new module carries its
>    own `declare_hooks!` slice for the `QUANTICK_*` it reads, and `app.rs`'s
>    slice loses them, so the generated registry is byte-identical.
> 4. **Tests:** any inline test that exercises a moved method goes to the
>    module's own `#[cfg(test)]`; `app/tests/*.rs` change nothing but paths.
> 5. **Baselines:** `--tighten`; the four new files stay under the 1,500
>    production-line threshold so the budget falls by what `app.rs` lost.
>
> ## Acceptance criteria
>
> - `app.rs` at most 8,000 lines and its baseline ceiling tightened to the new
>   size. *Evidence: `wc -l` and the baseline diff.*
> - Size `!budget` lower by at least 1,100. *Evidence: the baseline diff.*
> - `cargo run -q -p quantick-guards -- --report` before and after, diffed:
>   only `app.rs`-related lines move. *Evidence: the diff.*
> - The generated hook registry and capability inventory are unchanged
>   (`cargo test -p quantick-guards` green, registry diff empty).
> - Every moved function's body is identical to its original modulo
>   indentation-free relocation: `git diff --color-moved=zebra` shows moves,
>   not edits. *Evidence: the command's output summarised, with any non-move
>   hunk quoted and explained.*
> - The four-check loop green; `cargo test -p quantick-app` runs the same
>   number of tests.
>
> ## Out of scope, deliberately
>
> - `draw_menu_bar`, `draw_toolbar`, `apply_toolbar_action`, `draw_frame`,
>   `new_with_workspace`, `adopt_tab`, `arm_strategy_instance` — they mention
>   paper or indicators and belong to the mission after the two open seams.
> - Turning any sidecar into a port or surface.
> - `QuantickApp`'s fields.
>
> ## Parallel work to respect
>
> - `refactor/paper-policy-out-of-the-ticket` (open): `app.rs` at the paper
>   receivers. Ledger #8 says the ranges do not overlap; verify with
>   `git diff origin/main..refactor/paper-policy-out-of-the-ticket -- crates/app/src/app.rs`
>   before the first move, and again before the PR.
> - `refactor/native-indicator-docking` (PR #293): `app.rs` at the indicator
>   sites; same check.
> - `refactor/reach-and-download-into-feed` (parallel, proposed): `app.rs`
>   `use` lines only.
>
> ## Housekeeping
>
> ```sh
> git worktree list                       # a worktree may already exist
> git fetch origin
> git worktree add -b refactor/app-rs-sidecars ../quantick-worktrees/refactor-app-rs-sidecars origin/main
> cd ../quantick-worktrees/refactor-app-rs-sidecars && cargo build -p quantick-guards
> ```
>
> Tier `medium`: a 1,300-line move with a hard number to hit.
