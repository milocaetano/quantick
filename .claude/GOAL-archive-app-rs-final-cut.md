# Mission: `app.rs` final cut

**Objective.** Cut the remaining seven `impl QuantickApp` groups out of
`crates/app/src/app.rs` into sibling modules under `crates/app/src/app/`,
leaving at most 2,300 lines — bodies unchanged, method names kept, ceiling
tightened and budget lowered.

**Why it matters.** Two cuts took `app.rs` from 9,241 to 5,350 without one body
changing. This finishes it: afterwards the file is the window's *definition* —
its fields, how it is built, how `eframe` drives it — and nothing else. Every
group has a sibling precedent (`app/health.rs`, `app/indicator_manager.rs`,
`app/workspace_save.rs`), and only one line of the moved code is touched by the
open paper branch.

**Tier:** `medium`. A ~3,260-line move across seven modules with one named
merge hazard against an open branch. It earns more than `small` — the diff is
far past the `small` ceiling and the paper-branch seam needs a delivery pass —
but no part of it is a design change, a user-visible surface or a call that is
the trader's, so it does not need `high`.

## Request ledger

| # | Ask | Verbatim fragment where the wording carries it |
| --- | --- | --- |
| R1 | Move the control host accessors and agent actions (`app.rs:1602-2064`) into a sibling module. | "the control host accessors and agent actions" |
| R2 | Move the tab lifecycle (`:2068-2109`, `:2233-2309`, `:5085-5298`) into a sibling module. | "the tab lifecycle" |
| R3 | Move the toolbar wiring (`:2314-2595`) into a sibling module. | "the toolbar wiring" |
| R4 | Move the menu bar with its shortcut constants (`:2610-3364`) into a sibling module. | "the menu bar with its shortcut constants" |
| R5 | Move the drawing chrome and text notes (`:3369-3708`) into a sibling module, or into the existing `drawing_input.rs` if the result stays under 1,500 production lines. | "the drawing chrome and text notes" |
| R6 | Move the replay, history and alarm appliers (`:3714-3806`, `:3903-4262`, `:4380-4395`) into a sibling module; the alarms may be their own if they are a different reader. | "the replay, history and alarm appliers" |
| R7 | Move `draw_frame` (`:4406-5076`) into a sibling module. | "`draw_frame`" |
| R8 | The result is about 3,260 lines in seven modules under `crates/app/src/app/`. | "about 3,260 lines in seven modules" |
| R9 | What remains in `app.rs` is the struct, `new_with_workspace`, the paper fan-out, the `eframe::App` impl and the hook declarations — at most 2,300 lines. | "at most 2,300 lines" |
| R10 | Bodies unchanged — the diff shows moves, not edits. | "Bodies unchanged" |
| R11 | Method names kept. | "method names kept" |
| R12 | The one paper-branch line is carried: rebased in, or quoted with its new `file:line` in the PR body. | "one paper-branch line carried" |
| R13 | The size ceiling is tightened. | "ceiling tightened" |
| R14 | The size `!budget` is lowered — by at least 2,900. | "budget lowered" |
| R15 | Free items travel with their only users; test-visible names get `pub(super) use` re-exports in `app.rs`, listed in the PR body. | "re-exports for the test-visible constants … are `pub(super) use` lines" |
| R16 | No `declare_hooks!` line moves; the generated registry and capability inventory stay byte-identical. | "Registry and inventory byte-identical" |
| R17 | `app/tests/*.rs` change only for the test-visible constants, and only if imports are chosen over re-exports; nothing else. | "nothing else" |
| R18 | Each new file stays under 1,500 production lines. | "each new file under 1,500 production lines" |
| R19 | `--report` before and after, diffed: only `app.rs`-related lines and the new files move; no new file appears under `file.largest`. | "no new file appears under `file.largest`" |
| R20 | `cargo test -p quantick-app` runs the same number of tests. | "runs the same number of tests" |
| R21 | Respect what is deliberately out of scope: `new_with_workspace`, `adopt_tab`, `arm_strategy_instance`, the pickers and `persist_*`, the constructor's hook reads, `QuantickApp`'s fields, the `eframe::App` impl, and anything a capture could see. | "Out of scope, deliberately" |
| R22 | **(Purpose)** After this mission `app.rs` is the window's definition and nothing else. | "the file is the window's *definition* … and nothing else" |

## Decisions taken by the trader

None. Nothing in the brief qualified for a question: every ambiguity that would
cost work is settled in its evidence ledger, and the two calls it explicitly
delegates to the mission are recorded as `S2` and `S3` below.

## Assumptions

- **S1** — The mission re-measures every ledger claim in the brief before
  acting rather than trusting it, as the brief itself instructs. Claims 1, 5,
  6, 7, 8, 10 and 12 were verified against `origin/main` at `de9ee04` before
  work started; the rest were verified as their code was touched. Three
  corrections were found and are recorded as S5, S6 and A14.
- **S2** — *Wanted to ask, decided instead:* the drawing chrome became its own
  `drawing_chrome_wiring.rs` rather than joining `drawing_input.rs`. The brief
  delegates this ("may instead join"); the name follows the repository's
  existing `chart_layers_wiring` / `layout_wiring` idiom for the window side of
  a surface module, and avoids reading as a second `surfaces::drawing_chrome`.
- **S3** — *Wanted to ask, decided instead:* the alarms stayed inside
  `replay_and_history.rs` rather than becoming their own `alarms.rs`.
  `play_pending_alarms` and `report_alert_attempt` are about 60 lines and are
  reached from the same per-frame path as the history appliers; a 60-line
  module is a worse reader than a section.
- **S4** — Test-visible names use gated re-exports in `app.rs` rather than
  edits to `app/tests/*.rs`, keeping R17's "nothing else" strictly true.
- **S5** — *Correction to the brief's ledger #6.* `DEMO_VISIBLE_SLOTS` stays in
  `app.rs`: `app/demo_hooks.rs` already imports it through `use super::`, so
  the moving code is not its only user. `drawing_chrome_wiring` reaches it
  through `super::`.
- **S6** — `saved_time_interval` and `saved_context_intervals` travelled with
  the tab code, and `saved_context_intervals` is re-exported from `app.rs`, so
  `workspace_restore.rs` and `workspace_save.rs` keep their existing
  `use super::saved_context_intervals` line unchanged.
- **S7** — Module names are the mission's to set (the brief says so); the split
  itself is not negotiable.
- **S8** — *Circumstance that changed mid-mission.* `origin/main` moved from
  `de9ee04` to `d2ba64a` during the work: the paper-policy branch merged as
  PR #301 and `chore/public-surface` as PR #302. Rather than rebase a
  3,500-line move textually, the cut was re-derived on the new `main` by an
  extractor that names each group by its first and last item instead of by
  line number. This is the brief's own scope 3, first branch: the paper line is
  now simply in `menu_bar.rs`, carried by the rebase. See A16.

## Acceptance criteria

- [x] **A1** — The control host group lives in
      `crates/app/src/app/control_host.rs` (496 production lines), declared
      beside the existing `mod` lines.
      *Evidence:* the file, and the per-function identity check under A9.
      → PR body. *(R1, R8)*
- [x] **A2** — The tab lifecycle lives in `app/tabs.rs` (375); `adopt_tab` and
      `TabSlot` stayed in `app.rs`.
      *Evidence:* the identity check reports `adopt_tab` byte-identical in
      `app.rs`; `struct TabSlot` is still declared there. → PR body.
      *(R2, R8, R21)*
- [x] **A3** — The toolbar wiring lives in `app/toolbar_wiring.rs` (307).
      *Evidence:* the file; the identity check. → PR body. *(R3, R8)*
- [x] **A4** — The menu bar and its shortcut constants live in
      `app/menu_bar.rs` (778).
      *Evidence:* the file; the identity check. → PR body. *(R4, R8)*
- [x] **A5** — The drawing chrome lives in `app/drawing_chrome_wiring.rs`
      (430), and `DrawingRead` and `drawing_env` travelled with it.
      *Evidence:* neither name occurs in `app.rs` any more. → PR body.
      *(R5, R8)*
- [x] **A6** — The replay, history and alarm appliers live in
      `app/replay_and_history.rs` (511), with `DUPLICATE_OFFSET_BARS`.
      *Evidence:* the file; the identity check. → PR body. *(R6, R8)*
- [x] **A7** — `draw_frame` lives in `app/frame.rs` (732), with
      `indicator_preview_area`.
      *Evidence:* the file; the identity check. → PR body. *(R7, R8)*
- [x] **A8** — `crates/app/src/app.rs` is 1,932 production lines, against the
      2,300 asked for.
      *Evidence:* the guards ratchet recorded 1,932. → PR body. *(R9, R22)*
- [x] **A9** — Of the 92 moved functions, 90 are byte-identical to their
      originals once the `pub(super)` prefix is removed. The two exceptions are
      `apply_layout_preset` and `drawing_bbox_on_screen`, whose signatures
      rustfmt reflowed because the prefix pushed them past the column limit;
      their parameters, types, names and bodies are unchanged. No method was
      renamed.
      *Evidence:* the moved-body comparison, and the two signatures quoted.
      → PR body. *(R10, R11)*
- [x] **A10** — The `app.rs` ceiling is tightened from 5,388 to 1,932; the
      largest new file is `menu_bar.rs` at 778, well under 1,500, so none takes
      a baseline entry.
      *Evidence:* `cargo run -p quantick-guards -- --tighten`; the baseline
      diff. → PR body. *(R13, R18)*
- [x] **A11** — The size `!budget` falls 55,595 to 52,139, a drop of 3,456
      against the 2,900 asked for, with the arithmetic signed in the baseline.
      *Evidence:* the baseline diff. → PR body. *(R14)*
- [x] **A12** — `--report` before and after: only `app.rs`-related lines move.
      `app.rs` leaves `file.largest` and its slot is taken by the pre-existing
      `crates/app/src/drawings/mod.rs` (2,283) — no new file appears there.
      `crate.lines.app` rises 173, the cost of seven headers and import blocks.
      *Evidence:* the diff of the two reports. → PR body. *(R19)*
- [x] **A13** — No `declare_hooks!` line moved, none of the seven new files
      contains one, and no generated file is in the diff.
      *Evidence:* `cargo test -p quantick-guards` green, including
      `the_generated_indexes_match_the_code_they_describe`; the diff is nine
      files. → PR body. *(R16)*
- [x] **A14** — Free items travelled with their only users, except
      `DEMO_VISIBLE_SLOTS` (S5). The names kept reachable are one plain import
      (`saved_context_intervals`) and twelve `#[cfg(test)]` imports, each
      listed in the PR body.
      *Evidence:* the import block, quoted. → PR body. *(R15)*
- [x] **A15** — `app/tests/*.rs` are unchanged, and the `#[test]` count across
      `crates/app` is 1,904 on both `origin/main` and the branch.
      *Evidence:* the tests directory is absent from `git status`; the two
      counts. → PR body. *(R17, R20)*
- [x] **A16** — The paper-branch line merged as PR #301 before this branch
      shipped, so it is carried by the rebase rather than quoted for a manual
      carry: `.account_mut()` in the `PAPER_CANCEL_SHORTCUT` handler, now at
      `menu_bar.rs:206`, exactly 77 lines into `draw_menu_bar` as the brief
      predicted.
      *Evidence:* the one `account` occurrence in `menu_bar.rs`. → PR body.
      *(R12)*
- [x] **A17** — Every out-of-scope region is byte-identical: `new`,
      `new_with_workspace` (930 lines), `adopt_tab`, `arm_strategy_instance`,
      the two pickers, the five `persist_*` and `control_persist_*` methods,
      `attach_surface`, `fmt_progress`, `parse_tape_window`, and the whole
      93-line `eframe::App` impl.
      *Evidence:* fourteen functions plus the impl, all reported identical.
      → PR body. *(R21)*
- [x] **G1** — Every artifact in English.
      *Evidence:* `tracked_files_are_written_in_english` green in
      `cargo test -p quantick-guards`; `arch-review` dimension 8. → PR body.
- [x] **G2** — The four checks, each run on its own after rebasing onto
      `d2ba64a`: `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace
      --all-targets` exit 0; `cargo build --workspace` exit 0;
      `cargo test --workspace` exit 0, with 1,899 app tests passed and 4
      ignored.
      *Evidence:* the four exit codes. → PR body.
- [x] **G3** — Performance impact declared: no path changes rate. Every moved
      body is byte-identical, every call site keeps its name and its caller,
      and inherent-method dispatch is static, so the per-frame path
      (`draw_frame` and its appliers), the per-trade path and the per-depth
      path each execute the same instructions from the same call sites.
      Visibility prefixes and module boundaries are compile-time only.
      *Evidence:* the identity checks under A9 and A17. → PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, its step 0
      bug pass included, with every Blocker and Should-fix resolved or deferred
      with its severity.
      *Evidence:* the review verdict. → PR body.

## Not applicable, and why

- **Hot path** — no body changed and no call site changed rate, so there is
  nothing to measure beyond G3's declaration.
- **User-visible surface** (`ui-harness`, `visual-qa`, `trader-ux-review`) —
  the brief puts any change to what a capture could see out of scope, and
  moving private methods between a module and its child changes no pixel.
- **Adds a capability** (`new-extension`) — nothing is added; seven groups
  move.
- **Adds something a trader does** — no new action, tool, trade or lock, and no
  registry entry changes (A13).
- **Engine / determinism** — the diff is confined to `crates/app` and
  `crates/guards/size-baseline.txt`.
- **Docs/skills only** — this is a code change, so no shape dimension is
  waived.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, its body naming the tier beside the four
  verification boxes.

## The request as received

> *Attributed quotation of the trader's request, reproduced verbatim under
> `CLAUDE.md`'s exemption for a marked quotation.*

> /mission medium refactor/app-rs-final-cut — crates/app/src/app.rs is 5,350
> production lines after PRs #295 and #300, and every `impl QuantickApp` group
> still in it has a sibling-module shape waiting: the control host accessors
> and agent actions (`app.rs:1602-2064`), the tab lifecycle (`:2068-2109`,
> `:2233-2309`, `:5085-5298`), the toolbar wiring (`:2314-2595`), the menu bar
> with its shortcut constants (`:2610-3364`), the drawing chrome and text notes
> (`:3369-3708`), the replay, history and alarm appliers (`:3714-3806`,
> `:3903-4262`, `:4380-4395`) and `draw_frame` (`:4406-5076`), about 3,260
> lines in seven modules under crates/app/src/app/. What remains is the struct,
> `new_with_workspace`, the paper fan-out the open paper branch is editing, the
> `eframe::App` impl and the hook declarations: at most 2,300 lines. Bodies
> unchanged, method names kept, one paper-branch line carried, ceiling
> tightened, budget lowered. Read C:\src\mission-app-rs-final-cut.md in full
> before anything else and build the request ledger from it.

The referenced brief, `C:\src\mission-app-rs-final-cut.md`, is the request's
full text; its evidence ledger (#1-#12), scope (1-6), acceptance criteria, "Out
of scope, deliberately" and "Parallel work to respect" sections are quoted and
decomposed into R1-R22 above.
