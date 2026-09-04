# Mission — `app.rs` sheds the workspace store and the indicator manager

Move the workspace store, the indicator manager and the chart-layer
maintenance out of `crates/app/src/app.rs` into sibling modules under
`crates/app/src/app/`, bodies unchanged and method names kept, with the size
ceiling tightened and the budget lowered.

**Why it matters.** `app.rs` is 7,499 production lines after PR #295 — still
the largest ceiling in `crates/guards/size-baseline.txt`, and still the first
file every agent has to read to change anything in the window. These three
groups are cohesive, and they lie away from every region the open
`refactor/paper-policy-out-of-the-ticket` branch edits, so the cut can run
beside it. What stays afterwards is the constructor, the menu bar, the frame,
the toolbar and the tab plumbing — the port-shaped work for a later mission.

**Tier:** `medium`. A ~2,000-line mechanical move with a hard number to hit
and a parallel branch to stay clear of. It is far past the `small` diff
ceiling, and the number in the acceptance criteria is exactly the kind of ask
`delivery-review` exists to grade; but it invents no behaviour and touches no
surface, so it does not earn `high`.

## Request ledger

| # | Ask |
| --- | --- |
| R1 | Move the **workspace store** — brief ledger #3, `app.rs:3657-4754`, ~1,100 lines from `capture_workspace` to `note_workspace` — into a sibling module under `crates/app/src/app/`. |
| R2 | Move the **indicator manager** — brief ledger #4, `app.rs:2666-3400` plus `attach_script_indicator`/`detach_script_indicator` at `:1676-1740`, ~800 lines — into a sibling module. |
| R3 | Move the **chart-layer maintenance** — brief ledger #5, `app.rs:3401-3657`, ~250 lines — into a sibling module, named so it does not shadow `crate::chart_layers`. |
| R4 | Declare the new modules with `mod` lines beside the existing five at `app.rs:21-25`, the way PR #295 did. |
| R5 | **Bodies unchanged**: `git diff --color-moved=zebra` shows moves, not edits. Every non-move hunk is quoted and explained in the PR body. |
| R6 | The one constant that travels — `SCRIPT_RELOAD_POLL_INTERVAL` (`app.rs:75`) — moves with its only user. Anything else widened in visibility is named in the PR body with why. |
| R7 | **Hooks: nothing moves** (brief ledger #10). The generated hook registry and the capability inventory stay byte-identical, proved with the guard rather than by assertion. |
| R8 | **Tests unchanged**: `crates/app/src/app/tests/*.rs` change nothing. An inline `#[cfg(test)]` test of a moved method travels with it (brief ledger #8 says there are none). |
| R9 | Run `--tighten`. Each new file stays under the 1,500 production-line threshold; if the workspace module would cross it, split the named-workspace functions from the cockpit-store writers rather than sign a new baseline entry. |
| R10 | `app.rs` at most **5,600 lines**, its ceiling tightened to the new size. |
| R11 | The size `!budget` lower by **at least 1,800**. |
| R12 | `cargo run -q -p quantick-guards -- --report` before and after, diffed: only `app.rs`-related lines and the new files move. |
| R13 | The four-check loop green, and `cargo test -p quantick-app` running the **same number of tests** as before. |
| R14 | Verify each claim in the brief's evidence ledger before acting on it, rather than trusting it. |
| R15 | Respect the parallel branch: run the brief's ledger-#6 diff before the first move and again before the PR. Whichever of the two lands second re-runs `--tighten` and resolves the `!budget` line by hand (brief ledger #7). |
| R16 | Stay out of the declared out-of-scope: `new_with_workspace`, `draw_menu_bar`, `draw_frame`, `draw_toolbar`, `apply_toolbar_action`, `adopt_tab`, `arm_strategy_instance` and every `persist_*`; turning any sidecar into a port or surface; `QuantickApp`'s fields; the menu-bar constants and shortcuts at `app.rs:4757-4860`. |
| R17 | *(purpose — the ask that judges the others)* `app.rs` stops being the first and largest file every agent must read, and what remains is the port-shaped core for the mission after this one. |

## Decisions taken by the trader

- **D1** — The workspace store becomes its **own module**, separate from the
  existing `crates/app/src/app/workspace_restore.rs`. The brief permitted
  merging the read and write halves under one name, and
  `workspace_restore.rs:1-8` argues for it; the trader chose two smaller files
  over one ~1,260-line one. `workspace_restore.rs` is left untouched. *(R1)*

## Assumptions

- **S1** — Module names: `workspace_save.rs` (R1), `indicator_manager.rs`
  (R2), `chart_layers_wiring.rs` (R3). The brief hands the mission the naming
  call explicitly ("Names are the mission's to adjust; the split is not"), and
  the third name is the one it suggested to avoid shadowing
  `crate::chart_layers`. The first was to be `workspace_store.rs` until
  measurement found `crate::workspace_store` already exists and is imported by
  `app.rs` — the same shadow the brief warned about for the layers, one module
  over. `workspace_save` names the write half against `workspace_restore`'s
  read half, which is what the file is.
- **S2** — A moved method that `app.rs` still calls becomes `pub(super)`,
  which is the visibility the five existing siblings already use
  (`workspace_restore.rs:29`). A method called only from inside its own new
  module stays private. `attach_surface` keeps its `pub`. This is the minimum
  change that compiles and is not a body edit, so it is not counted against
  R5 — but every such line is listed in the PR body under R6.
- **S3** — The region ends are read from the file, not from the brief's
  numbers: the workspace store ends at `:4754` (`note_workspace`'s closing
  brace), and `:4755` is the closing brace of the `impl QuantickApp` that
  began at `:500`. That brace stays in `app.rs`, moved up to close the impl
  after the last method that remains. Cuts are taken at doc-comment
  boundaries, so each method travels with the comment above it.
- **S4** — `git diff --color-moved=zebra` is read as evidence by capturing its
  output to a file; "shows moves, not edits" is graded as: every hunk in the
  three new files is either zebra-coloured moved code or appears in the
  explicit non-move list (module doc comment, `use` lines, `impl QuantickApp {`
  wrapper, the `pub(super)` lines of S2).
- **S5** — *wanted to ask, at the `medium` two-question budget* — whether the
  chart-layer region should fold into the existing
  `crates/app/src/app/layout_wiring.rs` instead of taking a third file. Went
  with a separate module: `layout_wiring.rs` is already 1,557 lines and is one
  of only two `crates/app/src/app/` entries in the baseline, so folding 250
  more lines into it would push a signed ceiling up in a mission whose whole
  point is pushing ceilings down.
- **S6** — "the same number of tests" (R13) is measured as the total test
  count reported by `cargo test -p quantick-app`, captured before the first
  edit and after the last.

## Acceptance criteria

- [ ] **A1** — The workspace store's ~1,100 lines live in
      `crates/app/src/app/workspace_store.rs`, and no method named in the
      brief's ledger #3 remains in `app.rs`.
      *Evidence:* a grep for each of the 24 method names showing one
      definition each, in the new file.
      → `docs/evidence/app-rs-workspace-and-indicators/moved-methods.txt`
      *(R1, D1)*
- [ ] **A2** — The indicator manager's ~800 lines, including
      `attach_script_indicator` and `detach_script_indicator`, live in
      `crates/app/src/app/indicator_manager.rs`, and none remains in `app.rs`.
      *Evidence:* the same grep.
      → `docs/evidence/app-rs-workspace-and-indicators/moved-methods.txt`
      *(R2)*
- [ ] **A3** — The chart-layer maintenance's ~250 lines live in
      `crates/app/src/app/chart_layers_wiring.rs`, whose name shadows nothing:
      `crate::chart_layers` still resolves.
      *Evidence:* the same grep, plus `cargo check -p quantick-app` exit 0.
      → `docs/evidence/app-rs-workspace-and-indicators/moved-methods.txt`
      *(R3)*
- [ ] **A4** — Three `mod` lines sit beside the existing five in `app.rs`.
      *Evidence:* the quoted `mod` block from the final `app.rs`.
      → the PR body *(R4)*
- [ ] **A5** — The move is a move: `git diff --color-moved=zebra` over the
      branch, with every hunk that is not moved code listed and explained.
      *Evidence:* the captured diff, and the explicit non-move list.
      → `docs/evidence/app-rs-workspace-and-indicators/colour-moved.txt` and
      the PR body *(R5, R6)*
- [ ] **A6** — `SCRIPT_RELOAD_POLL_INTERVAL` is defined in
      `indicator_manager.rs` and nowhere else; every other visibility change
      is a `pub(super)` on a moved method, each one listed.
      *Evidence:* a repo-wide grep for the constant, and the listed lines.
      → `docs/evidence/app-rs-workspace-and-indicators/widened.txt` and the
      PR body *(R6)*
- [ ] **A7** — The generated hook registry and the capability inventory are
      byte-identical to `origin/main`.
      *Evidence:* `git diff --stat origin/main...HEAD` over the two generated
      files, empty; `cargo test -p quantick-guards` green.
      → `docs/evidence/app-rs-workspace-and-indicators/generated.txt` *(R7)*
- [ ] **A8** — No test is added, removed, renamed or altered: the twelve test
      files under `crates/app/src/app/tests/` are byte-identical to
      `origin/main`, and the suite runs the same tests.
      *Evidence:* `git diff --stat origin/main...HEAD -- crates/app/src/app/tests/`
      naming only `mod.rs`, plus the count in A13.
      → `docs/evidence/app-rs-workspace-and-indicators/generated.txt` *(R8)*

      **Restated once, under measurement, and narrower than R8 asked.** R8
      wanted `app/tests/*.rs` to change *nothing*. Five lines were added to
      `tests/mod.rs`: two `use` statements and a three-line comment. The cause
      is mechanical and was not foreseeable from the brief — `CandlePreset` and
      `IndicatorEvent` reached the twelve test files through `app.rs`'s own
      imports via their one `use super::*`, the indicator manager took the last
      *production* reader of each out of `app.rs`, and `warnings = "deny"`
      (`Cargo.toml:174`) makes an import with no production reader a hard build
      error. The name has to be bound somewhere the tests can still see it.
      `tests/mod.rs:30-33` already binds `DrawingsDemo` for exactly this reason
      after PR #295's cut, with a comment saying so; the two new bindings sit
      beside it under the same comment. This is recorded as a deviation rather
      than folded into the criterion silently: R8 stands as written, and the
      branch met its intent but not its letter. `ledger-check.md` carries the
      full account.
- [ ] **A9** — Every new file is under 1,500 production lines, so none takes
      a `size-baseline.txt` entry.
      *Evidence:* the `--report` output's per-file lines for the three new
      files. → `docs/evidence/app-rs-workspace-and-indicators/report-after.txt`
      *(R9)*
- [ ] **A10** — `app.rs` is at most 5,600 lines and its baseline ceiling
      equals its new production-line count.
      *Evidence:* `wc -l crates/app/src/app.rs` and the `size-baseline.txt`
      diff. → `docs/evidence/app-rs-workspace-and-indicators/baseline.txt`
      *(R10)*
- [ ] **A11** — The `!budget` line falls by at least 1,800 from 59,547.
      *Evidence:* the same baseline diff.
      → `docs/evidence/app-rs-workspace-and-indicators/baseline.txt` *(R11)*
- [ ] **A12** — The `--report` diff, before against after, moves only
      `app.rs`-related lines and the three new files.
      *Evidence:* the two reports and the diff between them.
      → `docs/evidence/app-rs-workspace-and-indicators/report-diff.txt`
      *(R12)*
- [ ] **A13** — `cargo test -p quantick-app` reports the same total test
      count before and after the branch.
      *Evidence:* the two captured counts.
      → `docs/evidence/app-rs-workspace-and-indicators/tests.txt` *(R13)*
- [ ] **A14** — Every claim in the brief's ten-row evidence ledger was
      re-measured against `origin/main` at `e0ae2ac` before the first edit,
      with each confirmation or correction recorded.
      *Evidence:* the verification table.
      → `docs/evidence/app-rs-workspace-and-indicators/ledger-check.md`
      *(R14)*
- [ ] **A15** — The paper branch's `app.rs` hunks were mapped to functions
      before the first move and again before the PR, and none falls inside a
      moved region.
      *Evidence:* the two hunk-to-function mappings, dated.
      → `docs/evidence/app-rs-workspace-and-indicators/paper-overlap.md`
      *(R15)*
- [ ] **A16** — No file or region listed as out of scope is touched by the
      branch.
      *Evidence:* `git diff --stat origin/main...HEAD` naming only `app.rs`,
      the three new files, `size-baseline.txt`, the goal archive and the
      evidence directory; plus a statement that no out-of-scope method in
      `app.rs` appears in the diff.
      → the PR body *(R16)*
- [ ] **A17** — The remaining `app.rs` is the constructor, the menu bar, the
      frame, the toolbar and the tab plumbing, stated as a one-paragraph
      account of what is left and what the next mission would take.
      *Evidence:* that paragraph, against the final file's method list.
      → the PR body *(R17)*

### Injected gates

- [ ] **G1** — Every artifact the branch authors is in English — code,
      comments, doc comments, commit messages, the PR title and body, and the
      evidence files. Graded by `arch-review` dimension 8.
      *Evidence:* the `arch-review` verdict. → the PR body
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind an `||`.
      *Evidence:* the four exit codes and tail output.
      → `docs/evidence/app-rs-workspace-and-indicators/four-checks.txt`
- [ ] **G3** — Performance impact declared. Expected classification: the moved
      methods keep their call sites and their bodies, so every touched path
      keeps its existing rate — `poll_script_files` and
      `maintain_chart_layers` are per-frame, `draw_indicator_legends` and
      `draw_indicator_settings` per-frame, the workspace writers rare and
      event-driven — and nothing changes rate or work per call. Stated as a
      claim about the diff, with the per-frame methods named.
      *Evidence:* the declaration. → the PR body
- [ ] **G4** — `arch-review` run over the final branch, with every Blocker and
      Should-fix resolved or deferred in the PR body.
      *Evidence:* the verdict and the deferral list. → the PR body

### Not applicable, and why

- **Hot path evidence** (`APP_HEALTH_SUMMARY` under a dense tape, or a bench).
  Not run: the diff moves bodies between files without changing a call site,
  a rate or the work done per call, so there is no mechanism by which the
  frame budget could move. G3 declares the classification; measuring a move
  would be measuring the compiler. If `arch-review` disputes the "bodies
  unchanged" claim, this stops being inapplicable.
- **`ui-harness` / `visual-qa` / `trader-ux-review`.** No surface is added or
  changed, no hook is added, moved or removed (R7). The rendering methods
  that move keep their bodies and their callers, so nothing a trader sees can
  differ.
- **`new-extension`.** No capability is added. This is the opposite motion:
  existing code moving to where it already belonged.
- **The second operator / drivable without a mouse.** No new act, tool, trade
  or lock. The existing control-plane methods (`control_*`,
  `attach_script_indicator`, `detach_script_indicator`) keep their names and
  their `pub(crate)` visibility, which A2 and A6 check.
- **Engine test-first.** Nothing under `crates/engine/` is touched and no
  behaviour is authored, so there is no fixture to write first. The
  behaviour-preservation proof is A5 plus the unchanged test suite (A8, A13).
- **Docs/skills waiver.** Not claimed — this is a code change and takes the
  full shape pass.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the tier beside the four verification boxes.

## The request as received

Quoted verbatim and untranslated, as `CLAUDE.md`'s exemption for a marked,
attributed quotation allows: the ledger above is a reading of these words, and
a reviewer needs the words themselves to check the reading. The request was in
English already.

> /mission medium refactor/app-rs-workspace-and-indicators — crates/app/src/app.rs is 7,499 production lines after PR #295 and still holds two cohesive `impl QuantickApp` groups no open branch touches: the workspace store (named workspaces, export/import pickers, cockpit-store writes and the maximise hook, `app.rs:3657-4757`, about 1,100 lines) and the indicator manager (legends, settings, presets, script files, attach/detach, `app.rs:2666-3400` plus `:1676-1740`, about 800 lines), with the chart-layer maintenance between them (`:3401-3657`, about 250 lines). Move them into sibling modules under crates/app/src/app/, the way PR #295 moved the demo hooks, drawing input, health and workspace restore. Bodies unchanged, method names kept, ceiling tightened, budget lowered. Read C:\src\mission-app-rs-workspace-and-indicators.md in full before anything else and build the request ledger from it.

The brief it points at, `C:\src\mission-app-rs-workspace-and-indicators.md`,
is the rest of the request: its ten-row evidence ledger (R14), its five-point
scope (R1–R9), its acceptance criteria (R10–R13) and its two out-of-scope
sections (R15, R16). That file lives outside the repository and is not
committed; every line of it that carries an ask appears above.
