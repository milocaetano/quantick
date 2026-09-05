# Mission — group ChartPane's fields, and take the bar-spec parameters off the pane

Group `ChartPane`'s 77 fields into owned sub-structs, one commit per group, and
redesign the bar-spec parameters so they are reached through `BarSpec` rather
than held one-per-variant on the struct every variant shares.

**Why it matters.** `arch-review` opens on a bar with two halves: *a standard
port, and no modification to the station*. The port half became dimension 1 and
the `new-extension` skill. The station half was never made countable, so debt
accrued where nothing rations it: the size ratchet rations lines per file, and a
field costs no lines. `struct.wide.app::ChartPane 77` has been reported with no
ceiling since the report existed, and nobody acted. This mission pays part of
that back on the widest struct in the app, and hands the future type-level shape
ratchet a much smaller number to freeze.

**Tier:** `high`. The work rewrites the field layout of the app's widest struct
and touches every call site in `app`, and one of its six commits is a design
change to how bar parameters are stored, not a move. That earns the full
interrogation round, the full injected gate table, a `medium` bug pass and
`delivery-review` in full. It is far past `small`'s changed-line ceiling and was
never a candidate for it.

## Measured before any work (do not re-derive)

Against `origin/main` at `0c3431d`, in this worktree:

- `cargo run -p quantick-guards -- --report` → `struct.wide.app::ChartPane 77`,
  `file.largest.crates/app/src/pane.rs 5618`.
- `crates/guards/size-baseline.txt` holds `crates/app/src/pane.rs 5618`.
- `crates/app/src/pane.rs` is 5,620 lines on disk and already carries sidecars
  (`pane/axes_and_chrome.rs`, `drawing_gestures.rs`, `menus.rs`,
  `strategy_badges.rs`, `tests/`). Sidecars move methods, not fields; another
  sidecar pass is not this work.
- **Correction to the request:** `deals_n` is *not* on `origin/main`. PR #306
  (`feat/trades-bars-b3`) is open, not merged. The bar-spec group is therefore
  **8 fields over 5 `BarKind` variants**, not 9 over 6. PR #306 is a third live
  sibling that does touch `pane.rs`; whichever of the two lands second rebases.
- `refactor/tab-rs-sidecars` merged as PR #307 and is in this branch's base.
  `refactor/gateway-rs-sidecars` is still live and does not touch `pane.rs`.

## Request ledger

- **R1** — Group `ChartPane`'s fields into owned sub-structs, one commit per
  group, each independently compiling and green.
- **R2** — Commit order: `last_*` first ("the purest mechanical win, and it
  proves the pattern for the rest"), then drawing gestures, footprint, context
  menu, strategy.
- **R3** — The bar-spec group last, and as a redesign, not a grouping: "the
  parameters belong with the variants, reached through `BarSpec`, so the eighth
  kind adds no field here".
- **R4** — Each commit message states the field count before and after.
- **R5** — No behaviour change in any commit except the bar-spec one, "whose
  behaviour change is that a bar kind's parameters no longer live on the pane".
  The existing app test suite is the proof; add no new behaviour.
- **R6** — Let the compiler drive. Prove nothing was lost per commit with a line
  multiset over the moved code, as the earlier split missions did.
- **R7** — Do not open a second sidecar pass on `pane.rs`. Fields are the work.
- **R8** — Do not raise `pane.rs`'s size ceiling. If grouping pushes it up, the
  sub-structs go into `pane/` modules of their own "and the ceiling comes down,
  not up".
- **R9** — Cut from `origin/main`. Do not touch `refactor/tab-rs-sidecars` or
  `refactor/gateway-rs-sidecars`; neither touches `pane.rs` today, "keep it that
  way".
- **R10** — `cargo run -p quantick-guards -- --report` shows
  `struct.wide.app::ChartPane` at 35 or below, from 77.
- **R11** — Adding a hypothetical eighth bar kind requires no new field on
  `ChartPane`; the PR body states which line proves it.
- **R12** — `crates/app/src/pane.rs` is no larger than today; the four
  verification-loop commands pass; the app suite is green "with no test
  rewritten to accommodate a move".
- **R13** — *(purpose)* Make `arch-review`'s "no modification to the station"
  countable on the widest struct in the app, and give the type-level shape
  ratchet a much smaller number to freeze.

Explicitly out of scope, stated by the request: the type-level shape ratchet
itself (its own mission), and `ChartState`'s `deal_samples` deposit found by
`/ai-review` on PR #306 (same defect class, different struct, separate branch).

## Decisions taken by the trader

- **D1** — The bar-spec parameters become a single `spec: SpecSelector` field on
  `ChartPane`, replacing `kind` plus the eight parameters. `SpecSelector` holds
  the active kind, one retained `BarSpec` per `BarKind::ALL` variant, and the
  pending spec. An eighth kind is then a variant in `BarSpec`/`BarKind` and a
  line in `ALL` — no field here. (Rejected: hoisting retention to the tab or
  workspace, which would make two panes share one set of parameters; and
  dropping retention for `BarKind` defaults.)
- **D2** — The per-kind retention behaviour is preserved *exactly*: switching
  tick → volume → tick still returns the trader's previous tick N. The only
  declared behaviour change stays "the parameters no longer live on the pane".
- **D3** — Sub-structs expose public fields with each field's visibility
  mirrored from today (`pub`, `pub(crate)` or private), not accessors on
  `ChartPane`. Accessors would add three to five lines per field to precisely
  the file whose ceiling may not rise (R8), and pub fields let the compiler
  drive the whole move.
- **D4** — The delivered groups are the six named in R2 and R3 only. The
  arithmetic lands the target without the small groups: 77 − 50 + 6 = 33, at or
  below the 35 of R10. `price_*` (3), `pending_*` (3), `layout_*` (2) and
  `drawings_*` (2) are reached only if the count fails to land at 35, and are
  otherwise left to the shape-ratchet mission.

## Assumptions

- **S1** — Sub-structs live in `pane/` modules of their own from the start, not
  only as a remedy if `pane.rs` grows. R8 names this as the response to growth;
  doing it up front is the same placement and keeps R12 satisfied by
  construction rather than by luck. This is not the sidecar pass R7 forbids:
  those modules carry *fields*, and no method moves with them.
- **S2** — "No test rewritten to accommodate a move" (R12) means no test's
  *assertions or intent* change. A test that names a moved field must still say
  `pane.frame.chart_rect` where it said `pane.last_chart_rect`; that is the
  rename the compiler forces, not a rewrite. Any test whose logic would have to
  change is a finding, not an edit.
- **S3** — Field names lose their now-redundant group prefix inside the
  sub-struct (`last_chart_rect` → `frame.chart_rect`), since the prefix was the
  grouping being made structural. Where dropping it would collide or read worse,
  the original name is kept.
- **S4** — The line-multiset proof (R6) is run per commit over the doc comments
  and field declarations moved, sorted and diffed, exactly as the earlier
  `app.rs` and `pane.rs` split missions did.
- **S5** — *(wanted to ask, budget spent)* If PR #306 merges before this branch,
  this branch rebases onto it and folds `deals_n` into `SpecSelector` rather than
  asking #306 to rebase. Chosen because this branch's whole point is that the
  sixth kind should cost no field, so absorbing it here is the cheaper merge and
  it demonstrates R11 on a real kind instead of a hypothetical one.
- **S6** — Branch `refactor/pane-field-groups`, worktree
  `../quantick-worktrees/refactor-pane-field-groups`, archive slug
  `pane-field-groups`. Repo convention, reversible in one edit.

## Acceptance criteria

- [ ] **A1** — Six commits land, in the order of R2 and R3: `last_*`, drawing
      gestures, footprint, context menu, strategy, bar-spec. Each compiles and
      is green on its own.
      *Evidence:* `git log --oneline origin/main..HEAD`, plus
      `cargo check -p quantick-app --all-targets` and
      `cargo test -p quantick-app` run at each commit.
      → PR body, per-commit table. *(R1, R2, R3)*
- [ ] **A2** — Every one of the six commit messages states the `ChartPane` field
      count before and after that commit.
      *Evidence:* `git log origin/main..HEAD` quoted.
      → PR body. *(R4)*
- [ ] **A3** — `cargo run -p quantick-guards -- --report` shows
      `struct.wide.app::ChartPane` at **35 or below**, down from the measured 77.
      *Evidence:* the report line, before and after.
      → PR body. *(R10, R13)*
- [ ] **A4** — The bar-spec parameters are reached through `BarSpec`: `kind` and
      the eight parameter fields are gone, replaced by one `spec: SpecSelector`
      whose retained parameters are keyed by `BarKind::ALL`. Per-kind retention
      behaves exactly as today.
      *Evidence:* the struct definition; the existing app tests covering kind
      switching, green and unmodified in intent.
      → PR body. *(R3, D1, D2)*
- [ ] **A5** — A hypothetical eighth bar kind adds **no** field to `ChartPane`,
      and the PR body names the line that proves it.
      *Evidence:* the cited line (the `BarKind::ALL`-driven construction of the
      retained set), quoted with its path and line number.
      → PR body. *(R11)*
- [ ] **A6** — `crates/app/src/pane.rs` is no larger than today (5,618 tracked
      production lines) and its `size-baseline.txt` ceiling is **not raised**.
      Sub-structs live in `pane/` modules of their own.
      *Evidence:* `file.largest.crates/app/src/pane.rs` from the report, and the
      `git diff` of `crates/guards/size-baseline.txt` showing no raise.
      → PR body. *(R8, R12, S1)*
- [ ] **A7** — No behaviour change in the first five commits; the sixth's only
      behaviour change is where the parameters live. No new behaviour, no new
      behavioural test.
      *Evidence:* the app suite green at every commit with no assertion changed;
      the diff of the app's test modules showing renames only.
      → PR body. *(R5, R12, S2)*
- [ ] **A8** — Nothing was lost in any move: a sorted line multiset over the
      moved declarations and their doc comments matches before and after, per
      commit.
      *Evidence:* the multiset diff output per commit.
      → PR body. *(R6, S4)*
- [ ] **A9** — No second sidecar pass: no method moves out of `pane.rs` on this
      branch. The new `pane/` modules declare fields and their construction only.
      *Evidence:* `git diff origin/main...HEAD -- crates/app/src/pane/` reviewed
      for moved `impl` bodies; stated in the PR body.
      → PR body. *(R7)*
- [ ] **A10** — The branch touches neither sibling refactor's territory:
      `crates/app/src/tab/` beyond forced field renames, and
      `crates/app/src/control/gateway.rs` not at all.
      *Evidence:* `git diff --stat origin/main...HEAD`.
      → PR body. *(R9)*

## Injected gates

- [ ] **G1** — Every artifact on this branch is in English: identifiers,
      comments, doc comments, commit messages, PR title and body, branch name.
      *Evidence:* `arch-review` dimension 8 verdict; `cargo test -p
      quantick-guards` (which runs `language.rs`).
      → review output, PR body.
- [ ] **G2** — The four verification-loop commands pass after rebasing on latest
      `main`, each run on its own (a chained `||` has printed a false all-clear
      here before): `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets`, `cargo build --workspace`,
      `cargo test --workspace`.
      *Evidence:* the four exit codes and tails, separately.
      → PR body's verification boxes.
- [ ] **G3** — Performance impact declared. Every touched path classified by
      rate (per-trade / per-depth / per-frame / rare) as part of the plan.
      `pane.rs` is squarely per-frame, so the classification is part of the work
      and not an afterthought; a pure field regrouping should be a no-op at
      every rate, and the declaration says so and why.
      *Evidence:* the classification, written in the PR body.
      → PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, its step 0
      bug pass at `medium` effort, with every Blocker and Should-fix resolved or
      deferred with its severity in the PR body.
      *Evidence:* the review verdict and the `arch-review-ok` marker.
      → review output, PR body.
- [ ] **G5** — `cargo test -p quantick-guards` green, including the size ratchet
      (which must not need a raise) and the cycle guard (new `pane/` modules
      must introduce no module cycle).
      *Evidence:* command output.
      → PR body.

- [x] **G6** — *(promoted out of the exclusions below, during delivery-review,
      when its own trigger fired.)* The bar-spec commit turned a direct field
      read into more than a struct field access on a per-frame path, in
      `ChartPane::future_slot_at_time` and `pane/drawing_gestures.rs`. The
      exclusion said that made this gate applicable **and measured**, so it is
      measured rather than argued.
      *Evidence:* a release-mode timing harness over 20,000,000 iterations of
      the old and new expressions against the real types — `new=1.72ns/call
      old=0.48ns/call delta=1.24ns` — costed at the worst rate the product
      permits (512 freehand anchors x 2 panes = 1.27 microseconds per frame,
      0.015% of a 8.3ms frame). The harness was deleted and is not in the diff.
      → PR body, and `delivery-review/performance.txt`.

### Not applicable, and why
- **A full `APP_HEALTH_SUMMARY` run** against a `main` control — the measured
  delta is 1.24ns per call on a path bounded to about a microsecond per frame,
  which is below what an fps comparison between two launches can resolve. The
  direct measurement in G6 is reported in its place, and says so.
- **`ui-harness` / `visual-qa` / `trader-ux-review`** — nothing user-visible
  changes. No surface is added, moved or restyled; the pixels are identical by
  construction (R5), and that is what the existing app suite proves. Had any
  commit changed a pixel it would have broken R5 first.
- **`new-extension`** — no capability is added. The mission makes the *next*
  capability cheaper; it ships none.
- **Something a trader does** — no new action, tool, trade or lock. Every
  operable surface keeps its existing hook and registry entry unchanged.
- **Engine / determinism test-first** — nothing under `crates/engine` is
  touched. `BarSpec` lives in `crates/app/src/state.rs` and is app-side
  selection state; the engine's builders behind `BarSpec::build` are untouched.
- **Docs/skills waiver** — does not apply; this is a code change and takes the
  full shape pass.

## Closing steps

- **C1** — `delivery-review` returns PASS over the branch as shipped.
- **C2** — The PR is open, with the tier named beside the four verification
  boxes, and every criterion's evidence in its body.

## The request as received

> Quoted verbatim and in full, as an attributed quotation under `CLAUDE.md`'s
> language rule. The trader's words; every other line of this file is English by
> the same rule.

```
high Group ChartPane's 77 fields into owned sub-structs, one commit per group, and take the bar-spec parameters off the shared pane entirely.

Measured this session — do not re-derive:
- crates/app/src/pane.rs is 5,620 lines and already carries sidecars (pane/axes_and_chrome.rs, drawing_gestures.rs, menus.rs, strategy_badges.rs, tests/). The extraction recipe has already been applied here and the shape survived it: sidecars move methods, not fields. Another sidecar pass is not the work.
- `cargo run -p quantick-guards -- --report` prints `struct.wide.app::ChartPane 77`. It is reported with no ceiling, so it costs nothing and nobody acted.
- The size ratchet rations lines per file; a field costs nothing, which is why the debt accrued in shape rather than in size.
- arch-review's opening line is the bar: "A new feature should dock like a spacecraft to the ISS: a standard port, no modification to the station." Half of it — the port — became dimension 1 and the new-extension skill. The other half, no modification to the station, was never made countable. This mission pays part of that back on the widest struct in the app.

The groups, measured:
- last_* frame measurements (12): last_lane_divider_x, last_chart_rect, last_area, last_price_gutter, last_time_strip, last_lane_reference_ms, last_auto_range, last_chart_height, last_chart_top, last_chart_area, last_bands, last_plot_area.
- drawing gesture and drag state (~15): drawing_menu_rects, drawing_hover, content_editing, drawing_band_hint, drawing_press_position, drawing_press_started_empty, parked_hand, freehand_last_position, drawing_press_pick, drawing_drag_pending_from, drawing_drag, shared_drag, shared_drag_owner, shared_drag_pending_from, shared_pointer_mark. Its methods already live in pane/drawing_gestures.rs; the state stayed behind.
- footprint_* (5), context_menu_* (5), strategy_* (4), price_* (3), pending_* (3), layout_* / drawings_* (2 each).
- bar-spec parameters (8): kind, pending_spec, tick_n, volume_units, dollar_notional, time_interval_ms, imbalance_target, imbalance_unit — plus deals_n, which PR #306 added. This is one field per bar variant on the struct every variant shares, and it grows by one with every new kind. Treat this group as a redesign, not a grouping: the parameters belong with the variants, reached through BarSpec, so the eighth kind adds no field here.

Deliver, one commit per group, each independently compiling and green:
- Start with last_* — the purest mechanical win, and it proves the pattern for the rest.
- Then drawing gestures, footprint, context menu, strategy.
- Then the bar-spec group, last, because it is the only one that changes a design rather than a layout of fields.
- Each commit states in its message the field count before and after.

Constraints:
- No behaviour change in any commit except the bar-spec one, whose behaviour change is that a bar kind's parameters no longer live on the pane. The existing app test suite is the proof; add no new behaviour.
- Let the compiler drive. Prove nothing was lost per commit with a line multiset over the moved code, as the earlier split missions did.
- Do not open a second sidecar pass on pane.rs. Fields are the work; the methods already moved.
- Do not raise pane.rs's size ceiling. If grouping pushes it up, the sub-structs go into pane/ modules of their own and the ceiling comes down, not up.
- Cut from origin/main; the main checkout is behind. Two sibling refactors are live and must not be touched: refactor/tab-rs-sidecars (crates/app/src/tab.rs into tab/) and refactor/gateway-rs-sidecars (crates/app/src/control/gateway.rs). Neither touches pane.rs today; keep it that way.

Out of scope:
- The type-level shape ratchet that would ration fields per struct the way size.rs rations lines per file. It is the mechanism that stops this recurring, and it is its own mission; this one gives it a much smaller number to freeze.
- ChartState's deal_samples deposit, found by /ai-review on PR #306. Same class of defect, different struct, separate branch.

Acceptance:
- `cargo run -p quantick-guards -- --report` shows `struct.wide.app::ChartPane` at 35 or below, from 77.
- Adding a hypothetical eighth bar kind requires no new field on ChartPane. State in the PR body which line proves it.
- crates/app/src/pane.rs is no larger than today, and its size-baseline ceiling is not raised.
- The four verification-loop commands pass, and the app test suite is green with no test rewritten to accommodate a move.
```
