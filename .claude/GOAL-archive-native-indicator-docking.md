# Mission: a native indicator docks as a new file plus one line — in `app` too

Replace the per-indicator native variants in `crates/app` with a catalog the
app iterates, so that adding a fourth native touches the `indicators` crate
only, while existing state and preset files still load and every hook, control
call and toolbar entry keeps its name.

**Why it matters.** `CLAUDE.md` *Keeping the trunk small* promises that a
capability docks as a new file plus one registration line. For native
indicators that promise holds below `app` and fails inside it: the same native
is a variant in two enums, a toolbar action, and match arms in the worker, the
app and the layout wiring. The audit behind PR #287 counted this as one of four
falsified recipes and fixed the prose; this mission fixes the code so the prose
can say what it originally promised. The cost is paid on every indicator
addition and on every review of one.

**Tier:** `medium`. The change is behaviour-preserving, but it rewrites the
on-disk vocabulary of the trader's workspace file and redraws a trader-facing
menu, so `delivery-review` runs in full and the toolbar rows get a
`trader-ux-review` pass. It is not `small`: the diff spans six production files
plus a serde compatibility path, and D1 below is a one-way change to a file the
trader owns.

## Request ledger

Derived from `C:\src\mission-native-indicator-docking.md`, read in full before
any work started. Every claim in its evidence table was re-verified against
`origin/main` at `d551813` before this file was written; the three corrections
are recorded as `S` lines.

### The catalog

- **R1** — Add a native catalog to `crates/indicators/src/native/mod.rs`: one
  entry per native carrying a stable id (`native.ema`, `native.cvd`), a display
  label, default inputs and a constructor `fn() -> Box<dyn Indicator>`. The
  existing `pub use` lines stay; the catalog entry is *"the one registration
  line the recipe promises"*.
- **R2** — Anchored VWAP is **not** re-docked: it is a drawing today and stays
  one. Out of scope, and the branch must not touch `avwap` docking.

### Collapsing the app-side variants

- **R3** — `SavedKind::Native { id }` replaces `NativeEma` / `NativeCvd`.
- **R4** — `IndicatorSource::Native { id, values }` replaces the two worker
  variants.
- **R5** — `ToolbarAction::AddNative(id)` replaces `AddEmaIndicator` and
  `AddCvdIndicator`.
- **R6** — The toolbar indicator rows are drawn by iterating the catalog, not
  written out one per native.
- **R7** — `add_native_indicator` (`app.rs`) and the layout wiring
  (`layout_wiring.rs`) look the id up in the catalog instead of matching on a
  variant.
- **R8** — The `_ =>` arm that today silently turns any unknown kind into an
  EMA becomes an error path. *Verbatim:* "becomes an error path, never a silent
  EMA."

### Compatibility and names

- **R9** — The state file and the preset file still load a workspace saved by
  today's code, covered by a test that loads a fixture written in today's
  on-disk spelling.
- **R10** — Every `QUANTICK_*` hook, control capability id, harness registry
  row and toolbar label that mentions EMA or CVD keeps its **exact** string.
- **R11** — The recipe row in `.claude/skills/new-extension/SKILL.md` (the
  table row and the *"The indicator port is only half a port"* section it
  points at) is rewritten to the new truth and names the one file and one line
  a fourth native touches.

### The proof, and the checks

- **R12** — A fourth native costs exactly one new file under
  `crates/indicators/src/native/` and one line in `native/mod.rs`, and from
  those two edits alone it appears in the toolbar, saves and restores through a
  layout. *(Proof shape set by D2 — a permanent test-only fake native, not the
  throwaway SMA the brief proposed.)*
- **R13** — `grep -rn 'NativeEma\|NativeCvd' crates/app/src` returns only the
  compatibility path and its test.
- **R14** — Every existing test in `indicators_tests.rs`, `state_file.rs`,
  `preset_file.rs`, `layouts.rs` and `indicator_worker.rs` is green, unchanged
  except for the variant spelling.
- **R15** — No hook, capability id, registry row or UI string changed: the
  generated hook registry and the capability inventory show an empty diff.
- **R16** — The four-check loop and `cargo test -p quantick-guards` are green,
  and the size baseline is tightened where `app.rs` and `layout_wiring.rs`
  shrank.
- **R18** — Respect the parallel branches: `refactor/feed-crate` (touches
  `app.rs` and `toolbar.rs` at `use` lines only) and
  `fix/tests-own-their-scratch` (edits `app/tests/*.rs` scratch helpers,
  `indicators_tests.rs` among them) — this mission's changes to
  `indicators_tests.rs` stay limited to variant spelling.

### The judging ask

- **R17** — *Verbatim:* "so that adding a fourth native touches the indicators
  crate only, existing state and preset files still load, and every hook,
  control call and toolbar entry keeps its name." Three conditions, all three
  required; R17 is the ask that judges the rest.

## Decisions taken by the trader

- **D1** — After the change the app **writes the new format**
  (`native = { id = "native.ema" }`); reading the old spelling keeps working
  permanently. Accepted consequence: a workspace file saved by this build is
  not readable by a build older than this PR — a one-way migration.
- **D2** — The proof that a fourth native costs one file plus one line is a
  **permanent fake native registered behind `#[cfg(test)]`**, exercising
  catalog, toolbar, save and restore. The brief's throwaway SMA is **not**
  added: the fake is the "second implementation tested" that `new-extension`
  already asks for, and unlike a quoted diff it is re-run by every CI.

## Assumptions

- **S1** — *Correction to the brief's ledger #9.* `SavedKind` carries
  `#[serde(rename_all = "snake_case")]`, so the on-disk spelling is
  `native_ema` / `native_cvd`, not `NativeEma` / `NativeCvd`. The compatibility
  path in R9 is written against the real spelling, and the fixture is generated
  by today's serializer rather than hand-typed. Safe to assume: it is a fact
  the code states, not a choice.
- **S2** — *Correction to the brief's ledger #7.* `preset_file.rs` (13
  matches) and `layouts.rs` (2 matches) mention the variants **only in their
  test modules**; their production cost is zero. Their production code is
  generic over `SavedKind` and needs no change beyond compiling. Scope is
  therefore narrower than the brief stated, not wider.
- **S3** — *Correction to the brief's ledger #7.* `app.rs` has 11 matching
  sites and `state_file.rs` 7, not the 8 and 5 the brief counted. No design
  consequence; recorded so the count is not re-derived.
- **S4** — `IndicatorSource::Native { id, values }` carries the EMA's `len` and
  `source` as `values: Vec<InputValue>`, not as typed fields, with an empty
  `values` meaning "declared defaults". This is already how the settings path
  binds them (`rebind`, brief ledger #8), so no second mechanism is invented.
  Conventional default; not a trader-facing call.
- **S5** — The toolbar keeps the literal strings "Add EMA(9) on close" and
  "Add CVD pane" by storing each native's full menu label in its catalog entry,
  rather than composing `"Add " + label`. R10 demands the exact string, and
  composition is what silently changes one.
- **S6** — `.claude/evidence/generated-truth/recipes.md` records what PR #287
  *measured* and is a historical artifact, so its body is not rewritten; a
  single "superseded by" pointer line is added so a later reader is not misled
  by a claim this branch falsifies. *Wanted to ask* — the `medium` budget of
  two questions went to D1 and D2, which cost more if wrong.
- **S7** — The catalog lives in `crates/indicators/src/native/mod.rs` as the
  brief says, and `app` consumes it through the crate's public surface. The
  catalog returns `Box<dyn Indicator>`; it does not know about `SavedKind`,
  `IndicatorSource` or egui, so no reverse dependency edge is created.
  Conventional; the dependency direction rule in `CLAUDE.md` decides it.

## Acceptance criteria

- [ ] **A1** — `crates/indicators/src/native/mod.rs` exposes a catalog of
      native entries, each with a stable id, a display label, default inputs
      and a `fn() -> Box<dyn Indicator>` constructor; the two existing
      `pub use` lines are still there.
      *Evidence:* the catalog quoted, plus a test in the `indicators` crate
      asserting the catalog's ids are exactly `native.ema` and `native.cvd`.
      → `crates/indicators/src/native/mod.rs` tests. *(R1)*
- [ ] **A2** — Anchored VWAP is untouched: no change to `avwap.rs`, to the
      drawing-tool path, or to how AVWAP docks.
      *Evidence:* `git diff origin/main...HEAD --stat` naming no `avwap` file.
      → PR body. *(R2)*
- [ ] **A3** — `SavedKind::Native { id }`, `IndicatorSource::Native { id,
      values }` and `ToolbarAction::AddNative(id)` each replace their two
      per-native variants, and no per-native variant remains in any of the
      three enums.
      *Evidence:* the three enum definitions quoted from the branch.
      → PR body. *(R3, R4, R5)*
- [ ] **A4** — The toolbar's native rows are produced by iterating the catalog:
      one loop, no per-native `if ui.button(...)`.
      *Evidence:* the `draw_indicators_menu` diff quoted. → PR body. *(R6)*
- [ ] **A5** — `add_native_indicator` and `add_indicator_at` resolve a native
      by catalog lookup, and an id no catalog knows produces a visible error
      slot rather than an EMA.
      *Evidence:* a test that asks both paths for an unknown id and asserts an
      error, not an EMA. → `crates/app/src/app/tests/indicators_tests.rs`.
      *(R7, R8)*
- [ ] **A6** — A workspace state file and a preset file written in today's
      `native_ema` / `native_cvd` spelling still load and restore both natives
      on the branch's code.
      *Evidence:* a test loading a fixture in the old spelling, asserting both
      indicators come back. → `crates/app/src/indicators/state_file.rs` and
      `preset_file.rs` tests. *(R9, D1)*
- [ ] **A7** — Files this build writes use the new spelling
      (`native = { id = "native.ema" }`).
      *Evidence:* a round-trip test asserting the serialized text.
      → `crates/app/src/indicators/state_file.rs` tests. *(D1)*
- [ ] **A8** — Every `QUANTICK_*` hook name, control capability id, harness
      registry row and toolbar label mentioning EMA or CVD is byte-identical to
      `origin/main`.
      *Evidence:* `git diff origin/main...HEAD` over the hook registry and the
      capability inventory is empty, and the two menu strings are grepped from
      the branch. → PR body. *(R10, R15)*
- [ ] **A9** — A fake native registered behind `#[cfg(test)]` reaches the
      toolbar, saves and restores through a layout, with its registration being
      one catalog line and its implementation one file.
      *Evidence:* the test and the line count of its two edits, quoted.
      → `crates/indicators/src/native/` + an app test. *(R12, D2)*
- [ ] **A10** — `grep -rn 'NativeEma\|NativeCvd' crates/app/src` returns only
      the compatibility path and its test.
      *Evidence:* the command's full output. → PR body. *(R13)*
- [ ] **A11** — The existing tests in `indicators_tests.rs`, `state_file.rs`,
      `preset_file.rs`, `layouts.rs` and `indicator_worker.rs` are green with
      no change beyond variant spelling, and the `indicators_tests.rs` diff
      contains nothing but spelling plus the new tests A5 and A9 name.
      *Evidence:* the per-file diff and the test run. → PR body. *(R14, R18)*
- [ ] **A12** — The `new-extension` recipe row and its *"only half a port"*
      section state the new truth and name the one file and one line, and the
      section does not grow the context budget.
      *Evidence:* the diff, plus `cargo test -p quantick-guards` green on the
      context ratchet. → PR body. *(R11)*
- [ ] **A13** — The size baseline is tightened wherever `app.rs`,
      `layout_wiring.rs`, `indicator_worker.rs` or `toolbar.rs` shrank.
      *Evidence:* the `size-baseline.txt` diff, and guards green.
      → PR body. *(R16)*
- [ ] **A14** — All three conditions of the judging ask hold together: a fourth
      native touches the `indicators` crate only, old state and preset files
      still load, and no hook, control call or toolbar entry changed name.
      *Evidence:* A9, A6 and A8 cited together. → PR body. *(R17)*

### Injected gates

- [ ] **G1** — Every artifact this branch authors is in English, per
      `CLAUDE.md`.
      *Evidence:* `arch-review` dimension 8 with no finding, and
      `cargo test -p quantick-guards` green. → arch-review verdict.
- [ ] **G2** — The four checks are green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`, each run on its own.
      *Evidence:* the four exit codes, pasted separately. → PR body. *(also R16)*
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate (per-trade / per-depth / per-frame / rare).
      *Evidence:* the classification table. → PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, every
      Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the verdict and the `arch-review-ok` marker.
      → PR body.
- [ ] **G5** — The catalog is a docking port per `new-extension`: port named,
      registration-only edits, defaults preserving today's behaviour, a fake
      second implementation tested (A9), blast radius stated.
      *Evidence:* the `new-extension` checklist answered. → PR body.
- [ ] **G6** — The redrawn indicator menu passes `trader-ux-review` with no
      unresolved Blocker, and the surface stays reachable through the existing
      `QUANTICK_INDICATORS_AUTOSTART` hook with no new hook needed.
      *Evidence:* the review verdict and a `visual-qa` capture of the menu.
      → PR body.

### Not applicable, and why

- **Hot path evidence** — the catalog is consulted when an indicator is
  *added* or *restored*, never per trade, per depth update or per frame. The
  per-frame toolbar loop iterates two entries where it previously ran two
  `if`s. G3 declares this; a benchmark would measure nothing.
- **Engine / determinism territory** — nothing under `crates/engine` is
  touched, and no bar-building code changes. The `indicators` crate change is
  additive and does not alter any kernel.
- **`ui-harness` new-hook rule** — no new UI surface appears. The menu that
  changes is already reachable through `QUANTICK_INDICATORS_AUTOSTART`, and R10
  forbids adding or renaming a hook. G6 covers the surface that exists.
- **Docs/skills waiver** — not claimed. The branch is code first, so the full
  `arch-review` shape pass applies to all of it, `SKILL.md` included.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS over the branch as shipped.
- [ ] **C2** — The PR is open, naming the `medium` tier beside the four
      verification boxes.

## The request as received

Quoted verbatim and untranslated: this is the marked, attributed quotation
`CLAUDE.md`'s English rule exempts, kept word-for-word because the ledger above
must remain auditable against the words that produced it. The request arrived
in English; the brief it points at is reproduced in the branch's history rather
than here.

> medium refactor/native-indicator-docking — `crates/indicators/src/native/mod.rs` promises that a third native indicator is "a new file plus one line", and inside that crate it is; in `crates/app` the same indicator is a variant in two enums, a toolbar action, and match arms in the worker, the app and the layout wiring, across six files. Replace the per-indicator variants with a native catalog the app iterates — one entry per native (id, label, constructor) — so that adding a fourth native touches the indicators crate only, existing state and preset files still load, and every hook, control call and toolbar entry keeps its name. Read C:\src\mission-native-indicator-docking.md in full before anything else and build the request ledger from it.
