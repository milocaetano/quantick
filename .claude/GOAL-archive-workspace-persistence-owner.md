# Mission — give workspace persistence one owner

**Objective.** Collapse the twenty-one `QuantickApp` fields that describe where
the workspace lives on disk and whether it has been saved into a single owner
that *holds* the debounce and save-blocked rule instead of exposing its flags,
with no change to what is written, when, or where.

**Tier:** `high`. The diff crosses ~100 call sites in the largest file in the
repository, touches the four modules that own the cockpit's files, and its
whole promise is *no behaviour change* — a claim only a full bug pass and a
conformance review can hold. It is not `max`: nothing here is new behaviour to
get wrong, and no decision in it is the trader's money or safety.

**Why it matters.** The field count is the symptom. The disease is that
`layouts_dirty`, `last_layout_change` and `layouts_save_blocked` are three
independent flags carrying one rule between them: any method on the trunk can
set the first and forget to stamp the second, and the save condition is
re-derived at every call site. A rule nothing guards is a rule that will
eventually be got wrong somewhere nobody is looking.

## Where this already lives — the verdict (R4)

Four modules in `crates/app/src/` already know about persistence, not three.
The mission named `ui_state.rs` (1,910), `workspace_bundle.rs` (820) and
`layouts.rs` (964); `store_home.rs` (687) is the fourth and the closest thing
to an existing common owner.

All four are **file** modules. Each owns a format and the IO for it:

- `ui_state.rs` — the shape of `ui-state.toml`, plus `load` / `save` /
  `forget` / `load_for_edit` / `validate`.
- `layouts.rs` — the shape of `layouts.toml`, plus `load` / `save` /
  `validate`.
- `workspace_bundle.rs` — the shape of a `.qws.toml` spanning every store,
  plus `capture` / `apply` / `remember_recent` / `existing_recent`.
- `store_home.rs` — where each store's file resolves this run
  (`COCKPIT_STORES`, `home`, `resolve`), and the one-time rescue of the
  cwd-relative era.

**Not one of them holds session state.** There is no mutable type in any of
them that the app carries across frames. `store_home::COCKPIT_STORES` comes
closest to owning the six paths, but it is deliberately a `const` registry of
`fn` pointers — its own doc comment defends that shape ("a ninth store is one
entry here plus the one line in its own module that calls `resolve`"). The
twenty-one trunk fields are the opposite kind of thing: mutable state carried
between the file and the frame.

**Verdict: a new type in a new module — `crates/app/src/workspace_store.rs`,
holding `WorkspaceStore`.**

It is *not* the fourth overlapping persistence module the mission warns
against, and the test for that is mechanical: **it defines no file format and
serialises nothing.** Every write it authorises is a call into one of the four
modules above. What it adds is the one layer none of them has — the state
*between* the file and the frame: which path each store resolved to this run,
whether the layout book has unsaved edits and when the last one landed,
whether a store may be written at all, and what the Workspace menu already
knows about disk so it need not ask the filesystem at 60 Hz.

**Why not a home inside one of them.** The six `*_path` fields span six
different stores. Housing them in `ui_state.rs` or `layouts.rs` would make
either module the owner of five stores that are not its own — precisely the
"one field, one file" discipline `ui_state.rs`'s own header defends ("two
stores describing one switch would eventually disagree about a pixel").
Housing them in `store_home.rs` would put a mutable session type beside a
`const` registry and a one-shot rescue: three responsibilities in one file,
and the registry is the one thing in this area that is currently clean.

**Why not a merge.** `ui_state.rs` and `workspace_bundle.rs` do overlap, and
merging them may well be the honest long answer — R13 forbids doing it here.
It ships as a recorded PR follow-up.

**Shape**, per the trader's decision D1:

```rust
struct QuantickApp {
    workspace: WorkspaceStore,   // was 21 fields
    ...
}

pub(crate) struct WorkspaceStore {
    paths: StorePaths,           // the six resolved paths, handed in
    layouts: LayoutStore,        // the book + the rule that guards it
    layers: SavedLayers,         // saved_layer_mask + saved_layer_tab
    session: WorkspaceSession,   // save_on_exit, bookmarks, recent, saved-ness
    // the in-flight OS dialogs and the trades folder
}
```

**The rule, in one place.** Today it is spread across
`layout_wiring.rs:108-150` — `mark_layouts_dirty`, `save_layouts_now`,
`maintain_layouts` and `flush_layouts` — and it reads `Instant::now()` inside
itself, which is why no test can reach it without a window. After, it is one
`&mut self` method on `LayoutStore` taking the clock as a parameter:

```rust
/// What the frame owes the layouts file. `Wait` leaves the change pending;
/// `Write` and `Blocked` both consume it, exactly as `save_layouts_now` does
/// today.
pub(crate) enum LayoutSave { Wait, Write, Blocked }

impl LayoutStore {
    pub(crate) fn mark_changed(&mut self, now: Instant);             // both flags, or neither
    pub(crate) fn take_save(&mut self, now: Instant) -> LayoutSave;  // debounced
    pub(crate) fn take_flush(&mut self) -> LayoutSave;               // exit path
}
```

`dirty` and `last_change` become private, so setting one and forgetting the
other stops being expressible. `take_save` and `take_flush` share one private
`take`, so `Blocked` can never diverge from `Write` about what it consumed.

## Decisions taken by the trader (2026-09-02)

- **D1** — One trunk field, sub-structs private inside it, accessors out.
  Trunk goes 21 → 1, a net drop of 20. The literal reading of "one owner",
  accepted with its known cost: borrow-checker friction wherever a UI closure
  needs the store and another trunk field in the same expression.
- **D2** — All twenty-one fields are in scope, `trades_dir`,
  `trades_dir_picker` and `workspace_picker` included. `paper_trading.rs` has
  a `trades_dir` of its own; only the `QuantickApp` copy moves, and the
  `tab.paper.set_trades_dir(...)` call site stays where it is, reading from
  the new owner. `paper_trading.rs` itself is untouched (R3).
- **D3** — `visual-qa` runs twice: a control capture built from `origin/main`,
  then the branch, compared surface by surface. "Identically before and after"
  is the only phrasing that can falsify a behaviour change, and a branch-only
  run would prove "it works" rather than "it is unchanged".

## Request ledger

| | Ask |
| --- | --- |
| **R1** | Give workspace persistence in `QuantickApp` one owner. |
| **R2** | Do not reorganise `app.rs` ~1555-1620 gratuitously — the parallel branch touches five harness call sites there and nowhere else. |
| **R3** | Do not touch `paper_trading.rs` *"in any form"*. |
| **R4** | Read `ui_state.rs`, `workspace_bundle.rs` and `layouts.rs` first and say in the plan whether the owner is a new type, a home inside one of them, or a merge — and why. The first question is *"where does this already live?"*, and the named failure mode is *"creating a fourth persistence module beside three that overlap"*. |
| **R5** | The owner holds the rule rather than exposing the flags: something the trunk *asks* (`should_persist(now)`), never something it sets. |
| **R6** | Take the clock as a parameter rather than reading it, the way `SurfaceEnv` takes `now` — *"that is what makes the rule testable without a window"*. |
| **R7** | No behaviour change: what is written to disk, when, and where, is identical after — *"including the debounce timing and the save-blocked condition"*. |
| **R8** | Nothing about paths may become implicit. A path that comes from `QUANTICK_*` or config still comes from there; *"the owner receives paths, it does not resolve them"*. |
| **R9** | The trunk loses net field count; state before and after in the PR body. |
| **R10** | State the net production line change across every file touched, in the PR body. |
| **R11** | Re-run `cargo run -p quantick-guards -- --tighten` immediately before pushing, not earlier — a parallel branch is also moving the budget. |
| **R12** | Leave the other `QuantickApp` clusters alone: control plane, tabs, indicators, perf counters. |
| **R13** | Do not merge `ui_state.rs` with `workspace_bundle.rs` here; if that is the honest answer, say so in the PR body as a follow-up. |
| **R14** | `QuantickApp` drops at least 15 fields. |
| **R15** | The debounce and save-blocked rule exists in exactly one place. |
| **R16** | A test that a blocked store never asks to write. |
| **R17** | A test that a change inside the debounce window does not ask to write either. |
| **R18** | `visual-qa` proves a save, a rename, a delete and a reopen of a workspace behave identically before and after. |

## Assumptions

- **S1** — The owner lives at `crates/app/src/workspace_store.rs`, a sibling of
  `store_home.rs` and `ui_state.rs`, not under `src/app/`. `src/app/` today
  holds impl blocks on `QuantickApp` (`layout_wiring.rs`) and the test tree; a
  standalone state type with its own tests belongs beside the other state
  modules. Reversible in one `git mv`.
- **S2** — `LayoutBook` itself moves inside the owner. Keeping the book on the
  trunk while the rule that decides when to write it lives elsewhere would
  leave the rule unable to name what it writes, which is the split this
  mission exists to close. Cost: `self.layouts` becomes an accessor at ~22
  sites plus `layout_wiring.rs`.
- **S3** — The six paths are handed to `WorkspaceStore::new` as already
  resolved `PathBuf`s, from the same `<module>::default_path()` calls the
  constructor makes today. The owner never calls `store_home::resolve` and
  never reads a `QUANTICK_*` variable. This is R8 read literally, and it is
  also why `COCKPIT_STORES` is *not* folded in: a lookup keyed off the
  registry would move when paths resolve.
- **S4** — `take_save`/`take_flush` returning a three-way decision preserves
  today's semantics exactly, including the `LAYOUTS_SAVE_BLOCKED` warning
  firing once per consumed change rather than once per session. A
  `should_persist` that simply returned `false` while blocked would silence
  that log line, which is a behaviour change under R7.
- **S5** — *Wanted to ask.* Whether `store_home.rs` should absorb the new type
  rather than a new module being created. Went with a new module for the
  cohesion reason argued in the verdict above; if the trader disagrees this is
  a `git mv` and a header rewrite, not a redesign.
- **S6** — *Wanted to ask.* Whether `trader-ux-review` is owed. Recorded as
  not-applicable below with its reason rather than run: a refactor with a
  no-behaviour-change constraint changes nothing a persona could react to, and
  `visual-qa` (R18, explicitly asked for) is the gate that would catch it if
  the constraint were broken.

## Acceptance criteria

*Ticked below as the evidence landed; the verdicts are collected in the PR body.*

- [x] **A1** — `crates/app/src/workspace_store.rs` exists, holds exactly one
      crate-visible owner type `WorkspaceStore`, and defines no `Serialize` or
      `Deserialize` type and calls no serialisation function: every write it
      authorises is a call into `ui_state` / `layouts` / `chart_layers` /
      `symbols_file` / `footprint_config` / `preset_file`.
      *Evidence:* the module header stating the verdict, plus
      `grep -n "Serialize\|Deserialize\|toml::" crates/app/src/workspace_store.rs`
      returning nothing. → PR body, "Where this already lives". *(R1, R4)*
- [x] **A2** — `QuantickApp` holds one field where it held twenty-one, and the
      total field count drops by at least 15.
      *Evidence:* field count on `origin/main` and on `HEAD`, both counted the
      same way, both printed. → PR body. *(R1, R9, R14)*
- [x] **A3** — The debounce and the save-blocked condition are decided in
      exactly one function, which takes the clock as a parameter and never
      calls `Instant::now()`.
      *Evidence:* `grep -rn "Instant::now()" crates/app/src/workspace_store.rs`
      returns nothing outside `#[cfg(test)]`; `grep -rn "LAYOUTS_SAVE_DEBOUNCE"`
      across `crates/app/src/` returns exactly one non-test production site.
      → PR body. *(R5, R6, R15)*
- [x] **A4** — `dirty`, `last_change` and `blocked` are private to
      `LayoutStore`; no code outside the module can set one without the other.
      *Evidence:* the fields carry no visibility modifier; `cargo build -p
      quantick-app` succeeds, which it could not if a call site still reached
      them. → build output. *(R5)*
- [x] **A5** — Three tests in `workspace_store.rs`: a blocked store returns
      `Blocked` and never `Write`; a change read back inside the debounce
      window returns `Wait`; the same change at the window's edge returns
      `Write`. Each runs with an injected `Instant`, no window, no filesystem.
      *Evidence:* `cargo test -p quantick-app workspace_store` output.
      → PR body. *(R15, R16, R17)*
- [x] **A6** — No behaviour change: the full existing `quantick-app` suite
      passes unmodified except where a test names a moved field, and every
      such edit is a rename of the access path with no change to what is
      asserted.
      *Evidence:* `cargo test --workspace` green, plus a diff review of every
      touched test asserting only path renames. → PR body. *(R7)*
- [x] **A7** — The owner receives paths and never resolves them.
      *Evidence:* `grep -n "store_home::resolve\|env::var\|env::var_os" crates/app/src/workspace_store.rs`
      returns nothing; `QUANTICK_UI_STATE` and its siblings still redirect
      their stores, proven by the existing store-override tests staying green.
      → PR body. *(R8)*
- [ ] **A8** — The PR body states the `QuantickApp` field count before and
      after, and the net production line change across every file touched.
      *Evidence:* the PR body itself. → PR body. *(R9, R10)*
- [x] **A9** — `cargo run -p quantick-guards -- --tighten` is run immediately
      before the push, after the last code commit.
      *Evidence:* the commit ordering in `git log`, with the tighten commit
      last. → `git log --oneline`. *(R11)*
- [x] **A10** — Out of scope stayed out: `app.rs` lines ~1555-1620 are
      untouched, `paper_trading.rs` has no diff, and no field belonging to the
      control-plane, tab, indicator or perf clusters moved.
      *Evidence:* `git diff origin/main...HEAD -- crates/app/src/paper_trading.rs`
      empty, and the `app.rs` hunk list showing no hunk in that range.
      → PR body. *(R2, R3, R12)*
- [ ] **A11** — The `ui_state.rs` / `workspace_bundle.rs` merge question is
      recorded as a follow-up rather than acted on.
      *Evidence:* a "Follow-ups" section in the PR body naming it.
      → PR body. *(R13)*
- [x] **A12** — `visual-qa` captures save, rename, delete and reopen of a
      workspace on a build from `origin/main` and on this branch, and reports
      them identical.
      *Evidence:* the two capture sets and the visual-qa verdict.
      → `.claude/evidence/workspace-persistence-owner/` and the PR body.
      *(R18, D3)*

### Injected gates

- [ ] **G1** — Every artifact English throughout, per `CLAUDE.md`.
      *Evidence:* `cargo test -p quantick-guards` green (language scan) and
      `arch-review` dimension 8 clean. → PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace` — each run on its own,
      never chained behind an `||`.
      *Evidence:* four separate command outputs. → PR body.
- [x] **G3** — Performance impact declared. `maintain_layouts` is a
      **per-frame** path and the only touched one that is not rare; every
      other site in the diff (save, load, menu draw, picker poll) is rare or
      per-interaction. The change replaces two field reads with one call on an
      owned sub-struct, so the claim is *flat*, and the two `visual-qa` builds
      supply `APP_HEALTH_SUMMARY` fps/frame_avg on both sides for free.
      *Evidence:* the classification above plus both health summaries.
      → PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, every
      Blocker and Should-fix resolved or deferred into the PR body with its
      severity.
      *Evidence:* the `arch-review` verdict and the `arch-review-ok` marker.
      → PR body.
- [ ] **G5** — `ui-harness`: no new surface is added, so no new hook is owed;
      every existing hook the diff passes through still reaches its surface.
      *Evidence:* the `visual-qa` runs of A12 driving the workspace flows
      through the existing hooks. → PR body.
- [x] **G6** — `visual-qa` returns PASS with every surface PASS or the defect
      explicitly accepted.
      *Evidence:* the visual-qa report. → PR body.

### Not applicable, and why

- **`new-extension`** — nothing is added. A refactor registers no feed, bar
  type, indicator, layer, panel or crate, and the one new module is a move of
  existing state rather than a new capability. Its carve-the-port rule is
  answered anyway: `store_home::COCKPIT_STORES` is already the port for "a
  store the cockpit keeps", and this mission adds no entry to it.
- **The second operator / drivable without a mouse** — no action is added, so
  there is nothing new to reach by name. The existing control-plane surfaces
  over this state (`control/workspace.rs`) are in the blast radius and must
  keep working; that is covered by G2's test run rather than by a new
  registry entry.
- **Engine / determinism territory** — no engine, `sim`, `strategy` or
  `indicators` code is touched. The test-first rule still applies inside A5,
  which is written before the rule moves.
- **`trader-ux-review`** — R7 forbids any behaviour change, so there is no
  changed flow for a persona to react to. `visual-qa` (A12) is the gate that
  catches the case where that constraint was broken, and it runs on both
  sides. Recorded as S6 rather than silently dropped.
- **Hot-path *measurement* beyond the two health summaries** — a dedicated
  bench would measure a struct field access. G3 declares the classification
  and takes the numbers the visual-qa builds produce anyway.

### Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the `high` tier beside the four
  verification boxes.

## The request as received

Quoted verbatim and in full, in the trader's own words, so a reviewer can
re-derive the asks above without trusting this file's summary of them. The
words are not translated because the point of the section is that they are the
ones actually said; every other line in this file is English, as `CLAUDE.md`
requires. Received 2026-09-02.

> high Give workspace persistence one owner.
>
> Running in parallel with a second session working on `crates/app/src/paper_trading.rs`.
> You own `app.rs`. Expect the other branch to touch it at five harness call sites around
> lines 1555-1620 and nowhere else; do not reorganise that region gratuitously.
>
> After the harness extraction, `QuantickApp` is down to 76 fields. Twenty-one of them are
> one subject: where the workspace lives on disk and whether it has been saved.
>
>   symbols_path, layouts, layouts_path, layouts_dirty, last_layout_change,
>   layouts_save_blocked, chart_layers_path, saved_layer_mask, saved_layer_tab,
>   footprint_settings_path, indicator_presets_path, ui_state_path, save_on_exit,
>   favorites_are_staged, bookmarks, workspace_saved, recent_workspaces, recent_on_disk,
>   workspace_picker, trades_dir, trades_dir_picker
>
> Six `*_path` fields with no common owner, and a trio — `layouts_dirty`,
> `last_layout_change`, `layouts_save_blocked` — that carries a debounce rule nothing
> guards. Any of 144 methods can set `layouts_dirty` and forget to stamp
> `last_layout_change`, and the save rule itself is rewritten at every call site. That is
> the invariant this mission is really about; the field count is the symptom.
>
> The first question is not "what shall I build" but **"where does this already live?"**
> `ui_state.rs` (1,910 lines), `workspace_bundle.rs` (820) and `layouts.rs` (964) already
> exist and already know about persistence. The failure mode here is creating a fourth
> persistence module beside three that overlap. Read all three first and say in the plan
> whether the owner is a new type, a home inside one of them, or a merge — and why.
>
> Whatever the shape, it must hold the rule rather than expose the flags: something the
> trunk *asks* (`should_persist(now)`), never something it sets. Take the clock as a
> parameter rather than reading it, the way `SurfaceEnv` takes `now` and `replay` is told
> how much time passed — that is what makes the rule testable without a window.
>
> Constraints:
> - No behaviour change. What is written to disk, when, and where, is identical after —
>   including the debounce timing and the save-blocked condition.
> - Nothing about paths may become implicit. A path that today comes from `QUANTICK_*` or
>   from config still comes from there; the owner receives paths, it does not resolve them.
> - The trunk loses net field count. State before and after in the PR body, and the net
>   production line change across every file you touched.
> - Re-run `cargo run -p quantick-guards -- --tighten` immediately before pushing, not
>   earlier: a parallel branch is also moving `!budget`.
>
> Non-goals: the remaining clusters in `QuantickApp` (control plane, tabs, indicators,
> perf counters) — each is its own mission; `paper_trading.rs` in any form; and merging
> `ui_state.rs` with `workspace_bundle.rs` if that turns out to be the honest answer —
> say so in the PR body as a follow-up rather than growing this mission into it.
>
> Acceptance beyond the standard gates:
> - `QuantickApp` drops at least 15 fields.
> - The debounce and save-blocked rule exists in exactly one place, with a test that a
>   blocked store never asks to write and that a change inside the debounce window does not
>   either.
> - `visual-qa` proves a save, a rename, a delete and a reopen of a workspace behave
>   identically before and after.
