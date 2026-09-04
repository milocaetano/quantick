# Mission — break the app crate's one dependency cycle

Move `PlotAreas`, `fmt_time_as`, `plot_split` and `split_time_strip` out of
`app` into a neutral module that `pane` and `bands` may depend on, so that the
strongly connected component `{app, pane, tab, bands}` dissolves and the two
largest files in the repository stop being welded to each other.

**Tier:** `medium`, raised from `small` after the branch was built.

It was scoped `small`: a behaviour-preserving move of five items and their
tests between modules in one crate, no signature changes beyond the path, no
user-visible surface touched, and no call the trader has to make. The finished
branch is 615 changed lines against `origin/main` — a pure move pays twice, in
the deletion and in the insertion — and that is past the 300 the `small`
exemption allows. The exemption lapsed, so the tier goes up and
`delivery-review` runs. Raising it is the honest option of the two on offer;
shrinking the diff to slip back under the ceiling is the dishonest one.

## Request ledger

- **R1** — Break the one dependency cycle in the `app` crate: the modules
  `app`, `pane`, `tab` and `bands` form its only strongly connected component.
- **R2** — Move `PlotAreas`, `fmt_time_as`, `plot_split` and
  `split_time_strip` out of `app` into "a neutral module that both sides may
  depend on".
- **R3** — Leave `app` importing `pane` and `tab` "in one direction only".
- **R4** — Behaviour is unchanged: "no signature changes beyond the path".
- **R5** — "the tests that cover them move with them."
- **R6** — Verify the cycle is gone.
- **R7** — Tighten the size baseline for whichever files shrank.
- **R8** — Purpose: "the two largest files in the repository stop being welded
  to each other."

## Assumptions

- **S1** — The four named items are not the whole `pane → app` edge:
  `pane.rs:4289` also calls `crate::app::gesture_hits_lane_divider`. Moving
  only the four leaves the cycle intact, so R6 cannot be satisfied without
  moving that fifth item too. It is the same layout/gesture family, declared
  between `plot_split` and `split_time_strip` in `app.rs`, so it travels with
  them. *Wanted to ask:* whether the trader would rather keep it in `app`; the
  reading taken is that R6 governs, since a cycle that survives the move makes
  the whole mission useless.
- **S2** — `AXIS_GUTTER` and `TIME_STRIP` are private constants read only by
  `plot_split`, so they move with it and stay private to the new module.
- **S3** — `fmt_time` is a one-line wrapper whose entire body calls
  `fmt_time_as`. It moves too rather than staying behind in `app.rs` as a
  function whose body is a hop into another module; its three non-test callers
  (`paper_report`, `trade_paint`) are outside the cycle and only change path.
- **S4** — The neutral module is `crates/app/src/plot_area.rs`, one module as
  the request words it. `chart.rs` was considered and rejected: it is
  deliberately egui-free and `PlotAreas` is built from `egui::Rect`.
- **S5** — The moved tests live in an inline `#[cfg(test)] mod tests` at the
  foot of `plot_area.rs`, which is how leaf modules in this crate (`chart.rs`,
  `bands.rs`) already carry their tests.

## Acceptance criteria

- [x] **A1** — No module in `crates/app/src/` reaches `crate::app::` from
      `pane.rs`, `tab.rs` or `bands.rs`; `app` reaches `pane` and `tab` and is
      not reached back.
      *Evidence:* a `grep -rn "crate::app" crates/app/src/{pane,tab,bands}.rs`
      returning nothing, quoted in the PR body. *(R1, R3, R6)*
- [x] **A2** — `PlotAreas`, `fmt_time_as`, `plot_split`,
      `split_time_strip` and `gesture_hits_lane_divider` are declared in
      `crates/app/src/plot_area.rs`, registered by one `mod plot_area;` line
      in `main.rs`, and `plot_area` imports none of `app`, `pane`, `tab` or
      `bands`.
      *Evidence:* the new file plus its `main.rs` registration line, and a grep
      for those four module names inside it returning nothing. *(R2)*
- [x] **A3** — Every moved item keeps its exact signature; the only change at
      each call site is the module path.
      *Evidence:* `git diff origin/main...HEAD` showing the moved `fn`/`struct`
      lines identical, quoted in the PR body. *(R4)*
- [x] **A4** — The tests covering the moved items live beside them in
      `plot_area.rs` and pass.
      *Evidence:* `cargo test -p quantick-app plot_area` output. *(R5)*
- [x] **A5** — The size ratchet is asked to tighten, and its verdict recorded.
      *Evidence:* `cargo run -p quantick-guards -- --tighten` printed "nothing
      to tighten in the size ratchet". Exactly one tracked file shrank —
      `app.rs`, by 124 production lines, from 9305 to 9181 against a ceiling of
      9362 — which leaves it 181 under, inside the ratchet's own `SLACK` of 200
      (`crates/guards/src/size.rs:97`), so no entry qualifies. `pane.rs` is
      unchanged at 7771: the longer `plot_area::` path first pushed it +4 over,
      and a `self` import brought it back to its original count rather than
      buying a ceiling raise. The baseline is therefore correct unedited, and
      an edit would have been a hand-tighten the tool declines to make. *(R7)*
- [x] **A6** — `pane.rs` and `app.rs`, the repository's two largest files, no
      longer reference each other in either direction.
      *Evidence:* the same grep as A1, plus `grep -n "crate::pane" app.rs`
      showing the surviving one-way edge. *(R8)*

## Injected gates

- [x] **G1** — Every artifact authored on this branch is in English.
      *Evidence:* `arch-review` dimension 8 verdict.
- [x] **G2** — The four checks pass after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
      -D warnings`, `cargo build --workspace`, `cargo test --workspace`, each
      run on its own.
      *Evidence:* the four exit codes, quoted in the PR body.
- [x] **G3** — Performance impact declared. Expected: none — the moved code is
      byte-identical and every touched path keeps its rate class.
      *Evidence:* the classification, stated in the PR body.
- [x] **G4** — `arch-review` run over `git diff origin/main...HEAD`, every
      Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the `arch-review` verdict.

Not applicable, and why: the hot-path row (nothing measurable changes — no
statement executes at a different rate), the user-visible row (no surface, no
string, no pixel moves), the capability rows (nothing is added), the
engine/determinism row (the `app` crate is the UI layer), and the docs-only
row (this is code).

## Closing steps

- **C1** — `delivery-review` returns PASS. Owed because the tier was raised to
  `medium`; the `small` mission this started as would not have run it.
- **C2** — The PR is open.

## The request as received

Quoted verbatim and untranslated because the ledger above must be auditable
against the exact words that produced it; this is the marked, attributed
quotation the language rule allows.

> small Break the one dependency cycle in the app crate. `pane` and
> `bands` import PlotAreas, fmt_time_as, plot_split and split_time_strip from
> `app`, which is the composition root importing them back — the four modules
> app, pane, tab and bands form the crate's only strongly connected component, so
> none of them can move without the others. Move those four items to a neutral
> module that both sides may depend on, and leave app importing pane and tab in
> one direction only. Behaviour is unchanged: no signature changes beyond the
> path, and the tests that cover them move with them. Verify the cycle is gone,
> and tighten the size baseline for whichever files shrank. So that the two
> largest files in the repository stop being welded to each other.
