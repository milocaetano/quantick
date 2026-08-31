# Mission

Move a second batch of six floating surfaces off `app.rs` onto the `Surface`
port, and converge `paper_trading.rs`'s private toast onto the window's one
`ToastSurface`, so the port carries an implementation written outside `app.rs`
and the window has a single acknowledgement lane again.

## Why

PR #264 carved the port and moved three surfaces that already owned all their
state. The open question it left is whether the port generalises: every
surface below edits state the *host* owns, and the toast in `paper_trading.rs`
is a second acknowledgement lane sitting in the same screen position on a
different clock. If `SurfaceResponse` cannot carry those asks as added fields,
the port is a filing cabinet rather than an abstraction.

## In scope

Six functions, with the coupling measured on today's `app.rs` (fields touched
plus host methods called, then production lines):

| function | coupling | lines |
| --- | --- | --- |
| `draw_indicator_preview_watermark` | 3 | 36 |
| `draw_style_panel` | 6 | 15 |
| `draw_footprint_settings` | 8 | 42 |
| `draw_source_picker` | 5 | 46 |
| `draw_alarm_controls` | 5 | 211 |
| `draw_strategy_popup` | 6 | 227 |

Plus the toast convergence: `paper_trading.rs`'s `struct Toast`, `draw_toast`,
`show_toast`, `TOAST_MS = 4_000` against `UNDO_MS = 8_000`, and
`TOAST_LIFT_PX = 96.0` against `BOTTOM_MARGIN_PX = 44.0` — two lanes, same
`Align2::CENTER_BOTTOM` anchor, two clocks. ~25 `show_toast` call sites.

## Out of scope, deliberately

`draw_frame` (23 fields, 60 host methods) and `draw_menu_bar` (17 fields, 22
methods). Those are entangled design rather than misfiled wiring; folding them
into an extraction turns a safe change into an unsafe one. Stated in the PR
body, not quietly widened.

## Acceptance criteria

### Mission-specific

1. All six functions are gone from `app.rs`; each lives in its own module
   under `crates/app/src/surfaces/`, implements `Surface`, and is registered
   as one field plus one line in `Surfaces`. `draw_alarm_controls` moves with
   the strategy popup it is a section of.
2. `SurfaceEnv` stays **read-only**: every host edit a surface asks for
   travels back as an added `SurfaceResponse` field, never as a `&mut` into
   the trunk and never as a `match` arm. Where the host must answer a surface
   (an arm that failed, a symbol the catalog refused), it answers through a
   named method on the surface, not by handing over itself.
3. `paper_trading.rs` no longer owns a toast type, a toast clock or a toast
   anchor. `show_toast` posts to an outbox the host drains into
   `Surfaces::toast`, so one message at one height on one clock. A test proves
   a paper-trading acknowledgement reaches `ToastSurface`, and the per-tab
   question (a message raised by a tab that is not on screen) has a documented
   answer rather than an accidental one.
4. `crates/app/tests/size_guard.rs` records the new, smaller `app.rs` ceiling
   in the same commit that shrinks it — the ratchet tightened, not loosened.
   `paper_trading.rs`'s entry likewise.
5. A test drives a surface written outside `surfaces/mod.rs` through the port
   and proves the new response fields survive the fold, extending the existing
   `a_second_implementation_needs_only_the_trait` proof rather than restating
   it.

### Standard gates

6. **English throughout** — every identifier, comment, UI string, test name
   and commit message. `CLAUDE.md` owns the rule and its exemptions;
   `arch-review` dimension 8 and `language_guard.rs` grade it.
7. **Four checks green** on latest `main`: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo build --workspace`, `cargo test --workspace` — each run on its own,
   never chained behind an `echo`.
8. **Performance declared and measured.** `Surfaces::draw_all` is a
   **per-frame** path: this change adds three surfaces' worth of virtual calls
   and the construction of a wider `SurfaceEnv` to every frame. Anything that
   allocates for a surface (the open-markets list, the counted-bar sides) is
   gated on that surface being open, so a closed surface costs a branch. Proof
   is a measurement, not a belief: `APP_HEALTH_SUMMARY` fps/frame_avg under a
   dense tape against a `main` control run, numbers in the PR body.
9. **ui-harness**: every moved surface is reachable by a `QUANTICK_*` env
   hook, declared through `Surface::apply_env_hook` in the same change.
   Existing hooks (`QUANTICK_STYLE_PANEL`, `QUANTICK_FOOTPRINT_PANEL`) move
   with their surface; surfaces that had none get one.
10. **visual-qa**: every moved surface captured and read against the defect
    checklist — all PASS, or a defect explicitly accepted in the PR body. The
    converged toast is captured in both lanes it replaces.
11. **trader-ux-review** with no unresolved Blocker: the arm dialog, the
    footprint panel and the acknowledgement lane are all mid-session surfaces.
12. **arch-review** run over `git diff main...HEAD`, every Blocker and
    Should-fix resolved or deferred in writing in the PR body. Its step 0
    (`code-review`) runs first.
13. **PR opened** with the evidence in its body and CI green. Merging is not
    part of this mission.
