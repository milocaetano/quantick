# Mission — move `pane.rs`'s test module out of the file

Move the trailing `#[cfg(test)] mod tests` of `crates/app/src/pane.rs`
(`pane.rs:7772` to the end of the file, 2,707 lines) into
`crates/app/src/pane/tests/`, the way `app.rs`'s tests already live in
`crates/app/src/app/tests/`. A pure move: no production line of `pane.rs`
changes, every test keeps its name and body, and the 21 indented
`#[cfg(test)]` sites inside the production code stay where they are.

**Why it matters.** `pane.rs` is the largest file in the repository (10,478
lines) and the most edited one. An agent opening it to change one gesture
reads 2,707 lines of tests it did not ask for. The size ratchet counts
production lines only, so this mission does not move the ceiling: the gain is
the read cost per session, and the precedent for `paper_trading.rs`, which
cannot move now because `fix/generated-truth` is editing it.

**Tier:** `medium`. A 2,707-line move is past the `small` tier's diff ceiling,
so `delivery-review` runs (completeness pass). It is not `high`: behaviour is
unchanged by construction, no UI surface moves, and every acceptance criterion
is mechanical (a line count, a diff, a test count, a guard exit code).
`arch-review` waives nothing.

## Request ledger

Built from `C:\src\mission-pane-tests-out.md` (the brief) and the `/mission`
line. Every brief claim was re-measured against `origin/main` at `8329c39`
before acting: `pane.rs` is byte-identical to the brief's `bc39248`.

- **R1** — Move the trailing `#[cfg(test)] mod tests` of
  `crates/app/src/pane.rs` (`:7772` to the end, 2,707 lines) out of the file
  into `crates/app/src/pane/` test modules, *"the way app.rs's tests already
  live in crates/app/src/app/tests/"*.
- **R2** — `pane.rs` *"keeps one line where the module was"*: the
  `#[cfg(test)] mod tests;` declaration.
- **R3** — *"A pure move: no production line of pane.rs changes."*
- **R4** — *"every test keeps its name and body"*; the only permitted edit is
  the `use super::*` header line, if the new location needs a different path.
- **R5** — *"the 21 test-only helpers inside the production code stay where
  they are"*; if a moved test reaches a private item, widen it to `pub(crate)`
  with a one-line comment rather than moving the helper.
- **R6** — Read the brief in full before anything else and build the ledger
  from it.
- **R7** — The mission picks `pane/tests.rs` versus `pane/tests/mod.rs`, and
  splits into `pane/tests/<topic>_tests.rs` only if the lines *"fall into
  natural topics"*, otherwise one file.
- **R8** — *"No `--tighten` expected for pane.rs; run it anyway and commit
  whatever it writes."*
- **R9** — Acceptance: `cargo test -p quantick-app pane::` runs the same
  number of tests before and after, all green; evidence is both counts.
- **R10** — Acceptance: `git diff origin/main...HEAD -- crates/app/src/pane.rs`
  shows deletions plus one `mod tests;` line and no other insertion.
- **R11** — Acceptance: `pane.rs` is at most 7,772 lines; no moved item was
  de-indented; every new module name ends in `_tests` or is `tests`.
- **R12** — Acceptance: the four-check loop green; `cargo test -p
  quantick-guards` green.
- **R13** — *(purpose; judges the others)* *"read cost, not ceiling"*: a
  session opening `pane.rs` no longer loads the tests, the `pane.rs` ceiling
  (7,771) does not change, and the recipe is reusable for `paper_trading.rs`.
- **R14** — Out of scope, deliberately: splitting the production half of
  `pane.rs` or `ChartPane`; the 21 inline `#[cfg(test)]` sites;
  `paper_trading.rs`, `app.rs`, `tab.rs`; a `lib.rs` target for `crates/app`.
- **R15** — Parallel work to respect: `fix/generated-truth` adds hook lines in
  the production half of `pane.rs`, `refactor/orderflow-crate` touches its
  `use` lines; this branch must not overlap either, and whoever merges second
  rebases.
- **R16** — Verify each evidence-ledger claim of the brief before acting.
- **R17** — Tier `medium`: `delivery-review` runs; `arch-review` waives
  nothing.

## Decisions taken by the trader

None. Nothing in the request earned a question at `medium`: the one open
choice (R7) was delegated to the mission by the brief itself, and its no-regret
reading (S1) throws no work away if the trader later wants the other one.

## Assumptions

- **S1 — One file, `crates/app/src/pane/tests/mod.rs`, holding the whole
  module body; no topical split.** Measured before deciding: the 61 tests are
  interleaved with 27 helper functions and 3 constants, with no section
  markers, and the shared helpers span the whole module (`painted` is used
  from line 7890 to 10096, `test_areas` from 8697 to 10187, `drive_navigation`
  from 8871 to 10160, `TEST_PLOT` 52 times across 1,500 lines). A topical
  split would hoist a dozen shared items out of sequence rather than cut at
  boundaries, which is no longer the pure move R3 asks for. R7 offers exactly
  this outcome, and a later split builds on this file without undoing it.
- **S2 — The directory form, not `pane/tests.rs`.** The size guard's
  `tracked()` excludes a path only when a *directory component* is named
  `tests`; a file named `tests.rs` under `pane/` would be counted as 2,707
  production lines, need a signed ceiling, and blow the budget. R7 leaves the
  choice to the mission and the guard decides it.
- **S3 — `use super::*` stays as written.** In `pane/tests/mod.rs`, `super`
  is still `crate::pane`, so the header line the brief allowed to change does
  not need to. The module body is therefore moved with zero edits.
- **S4 — No helper is widened to `pub(crate)`.** `pane::tests` is a child of
  `pane` and sees its private items, exactly as it did inline; R5's fallback
  is never reached. Confirmed by the build, not assumed.
- **S5 — The moved lines are written at their original indentation and
  `rustfmt` re-indents them.** Stripping four spaces by hand also strips them
  from inside multi-line string literals and doc comments; rustfmt re-indents
  code and never touches a string's contents. This is the `app.rs` split's
  own post-mortem, and it is what makes the byte-level proof in A2 possible.
- **S6 — The declaration keeps rustfmt's two-line form**, `#[cfg(test)]` on
  its own line above `mod tests;`, as `app.rs:9180-9181` has it. The first of
  the two lines already exists at `pane.rs:7772`, so the diff's only insertion
  is `mod tests;`, which is R10 exactly.
- **S7 — `mod.rs` opens with a short header comment** naming the layout and
  the one rule it depends on (a child module sees its ancestor's private
  items). It is the only text in the new file that did not come from
  `pane.rs`, and A2's proof lists it as such.
- **S8 — Evidence lands in the PR body**, as every archived mission's does;
  the raw test logs live in the session scratchpad and are quoted from there.
- **S9 — rustfmt's re-wrap is part of the move, not an edit to a test.** With
  the items four columns further left, 28 lines at eight sites now fit inside
  rustfmt's width and it joined them into 14; `cargo fmt --check` leaves no
  other state to ship. That is why A2's proof is the rustfmt round trip rather
  than a line multiset: the round trip reproduces `origin/main`'s module byte
  for byte, which is the definition of "same tokens" that rustfmt itself uses.
  The one multi-line string literal in the module (`pane.rs:9471-9472`, a
  backslash continuation) kept its content untouched, as S5 predicted.

## Acceptance criteria

- [x] **A1** — `crates/app/src/pane/tests/mod.rs` exists and is the only file
      under `crates/app/src/pane/`; `pane.rs` ends with `#[cfg(test)]` then
      `mod tests;` and nothing after.
      *Evidence:* `ls crates/app/src/pane/` and `tail -3 crates/app/src/pane.rs`,
      quoted. → PR body. *(R1, R2, R7, R11)*
- [x] **A2** — Every moved item is the original item, token for token:
      wrapping `pane/tests/mod.rs` (minus its header comment) back into
      `#[cfg(test)] mod tests { … }` and running rustfmt over it reproduces
      `pane.rs:7772-10478` of `origin/main` byte for byte, and the `#[test]`
      count is 61 on both sides. Lines rustfmt re-wrapped because they now fit
      at four fewer columns are listed by original line number; nothing else
      differs, and no string literal's content moved (S9).
      *Evidence:* the round-trip script's output and the re-wrapped-site list,
      quoted. → PR body. *(R3, R4, R11)*
- [x] **A3** — `git diff origin/main...HEAD -- crates/app/src/pane.rs` is
      deletions plus exactly one inserted line, `mod tests;`.
      *Evidence:* `--stat` and every `^+` line of that diff, quoted.
      → PR body. *(R3, R10)*
- [x] **A4** — `wc -l crates/app/src/pane.rs` reports at most 7,772 lines, the
      21 indented `#[cfg(test)]` sites are still at `:1157`-`:5051`, and no
      production item's visibility changed.
      *Evidence:* `wc -l`, `grep -n 'cfg(test)'`, and A3's diff having no
      `pub(crate)` insertion. → PR body. *(R5, R11)*
- [x] **A5** — `cargo test -p quantick-app pane::` runs the same number of
      tests on `origin/main` and on the branch, all green.
      *Evidence:* both summary lines, quoted. → PR body. *(R9)*
- [x] **A6** — `cargo run -p quantick-guards -- --tighten` was run; its
      result (nothing written, or the lines it wrote) is committed and named.
      *Evidence:* the command's output and the resulting diff of the baseline
      files. → PR body. *(R8)*
- [x] **A7** — The `pane.rs` ceiling in `crates/guards/size-baseline.txt` is
      unchanged at 7,771 and `cargo test -p quantick-guards` is green.
      *Evidence:* `grep pane.rs size-baseline.txt` and the guards test
      summary. → PR body. *(R12, R13)*
- [x] **A8** — The branch touches no line that `fix/generated-truth` or
      `refactor/orderflow-crate` touches: its only production edits are at
      `pane.rs:7773` and below.
      *Evidence:* A3's diff hunk headers. → PR body. *(R14, R15)*
- [x] **G1** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace` all
      green after rebasing on latest `main`, each run on its own.
      *Evidence:* the four commands' summary output. → PR body.
- [x] **G2** — Performance impact declared: every touched path classified by
      rate. *Evidence:* the classification. → PR body.
- [ ] **G3** — `arch-review` run over `git diff origin/main...HEAD`, step 0
      included, every Blocker and Should-fix resolved or deferred with its
      severity. *Evidence:* the verdict and the deferral list. → PR body.
- [ ] **G4** — Every artifact English, per `CLAUDE.md`.
      *Evidence:* `arch-review` dimension 8 and the language guard, both
      clean. → PR body.

### Not applicable, and why

- **Hot path** — no production line changes; nothing that runs per trade,
  per depth update or per frame is touched. The test binary is the only thing
  that changes shape, and it compiles the same items.
- **User-visible surfaces** (`ui-harness`, `visual-qa`, `trader-ux-review`) —
  no surface is added, removed or changed.
- **Adds a capability** (`new-extension`) — nothing docks; a test module
  moves.
- **Something a trader *does*** — no action, tool, trade or lock is added.
- **Engine / determinism** — no crate below `app` is touched.
- **Docs/skills only** — this is code, so no shape dimension is waived.

### Closing steps

- **C1** — `delivery-review` (completeness pass) returns PASS.
- **C2** — The PR is open, its body naming the tier.

## Rebase after PR #289

`refactor/orderflow-crate` merged as `a5bf3b9` while this PR was open and
edited four lines *inside* the test module (`crate::orderflow::` became
`quantick_orderflow::` at the old `pane.rs:7934`, `:8047-8055` and `:8153`),
so the branch conflicted. Resolved by rebasing and re-running the move from
`origin/main`'s new `pane.rs` rather than merging hunks by hand: the same
script, the same header, rustfmt again. A2's round trip was re-proven against
`a5bf3b9`, and A5's before-count stands at 61 from the `8329c39` run — the
module differs between the two bases only by those four path renames, which
the round trip covers. The review markers were re-recorded only after both
reviews ran again over the rebased head.

## The request as received

Quoted verbatim and untranslated: this is the marked, attributed quotation
`CLAUDE.md`'s English rule exempts, and the ledger above is the operative
English statement of it. The brief it points at is reproduced in full below
it, for the same reason.

> /mission medium refactor/pane-tests-out — move the trailing `#[cfg(test)]
> mod tests` of crates/app/src/pane.rs (pane.rs:7772 to the end, 2,707 lines)
> out of the file into crates/app/src/pane/ test modules, the way app.rs's
> tests already live in crates/app/src/app/tests/. A pure move: no production
> line of pane.rs changes, every test keeps its name and body, the 21
> test-only helpers inside the production code stay where they are. Read
> C:\src\mission-pane-tests-out.md in full before anything else and build the
> request ledger from it.

### The brief, `C:\src\mission-pane-tests-out.md`, as received

> # Mission brief: `pane.rs` tests leave the file — read cost, not ceiling
>
> Paste the `/mission` line below into a fresh session in `C:\src\quantick`. Every
> claim was measured against `main` at `bc39248` on 2026-09-04; each carries its
> `file:line` so the mission re-checks it instead of trusting it.
>
> ## The paste-able invocation
>
> ```
> /mission medium refactor/pane-tests-out — move the trailing `#[cfg(test)] mod tests` of crates/app/src/pane.rs (pane.rs:7772 to the end, 2,707 lines) out of the file into crates/app/src/pane/ test modules, the way app.rs's tests already live in crates/app/src/app/tests/. A pure move: no production line of pane.rs changes, every test keeps its name and body, the 21 test-only helpers inside the production code stay where they are. Read C:\src\mission-pane-tests-out.md in full before anything else and build the request ledger from it.
> ```
>
> ## Why this mission
>
> `pane.rs` is the largest file in the repository at 10,478 lines and the most
> edited one: 212 commits since June. An agent opening it to change one gesture
> reads 2,707 lines of tests it did not ask for. The size ratchet counts production
> lines only, so **this mission does not move the ceiling** — the gain is the read
> cost per session and the precedent for `paper_trading.rs` (which cannot move now:
> `fix/generated-truth` is editing it). `app.rs` already went through this in
> `refactor/app-rs-first-split`, and its recipe is the one to reuse.
>
> ## Evidence ledger — verify each before acting
>
> | # | Claim | Where |
> | --- | --- | --- |
> | 1 | The trailing test module starts at `pane.rs:7772` and runs to the end: 2,707 lines | `grep -n 'cfg(test)' crates/app/src/pane.rs` — last hit at column 0 |
> | 2 | 21 other `#[cfg(test)]` sites between `:1157` and `:5051` are indented: test-only fields, helpers and branches on production items. They stay | same grep; every other hit is indented |
> | 3 | Size-baseline ceiling for `pane.rs` is 7,771 production lines and will not change | `crates/guards/size-baseline.txt` |
> | 4 | Precedent: `app.rs:9181` ends with `mod tests;`, resolved to `crates/app/src/app/tests/mod.rs`, which declares topical submodules at `mod.rs:32-40` and holds the shared `test_app()` at `:239` | `crates/app/src/app/tests/` |
> | 5 | `app/tests/panes_layout_tests.rs` (3,013 lines) already drives panes *through* `QuantickApp`; the `pane.rs` module tests drive `ChartPane` and helpers directly. They are different layers and must not be merged | both files |
> | 6 | The move's known trap: never de-indent the moved items, and suffix every new module `_tests` so it cannot shadow a real crate module | the `app.rs` split's own post-mortem |
>
> ## Scope
>
> 1. **`pane.rs` keeps one line** where the module was: `#[cfg(test)] mod tests;`
>    (Rust resolves it to `crates/app/src/pane/tests.rs` or `pane/tests/mod.rs`
>    without renaming `pane.rs`). The mission picks which; if the 2,707 lines fall
>    into natural topics, split them into `pane/tests/<topic>_tests.rs` as `app`
>    did, otherwise one `pane/tests.rs`.
> 2. **Bodies unchanged.** `use super::*` becomes the explicit `use crate::pane::…`
>    the new location needs; nothing else in a test changes.
> 3. **Test-only production helpers stay** in `pane.rs`; if a moved test reaches a
>    private item, widen it to `pub(crate)` with a one-line comment rather than
>    moving the helper.
> 4. **No `--tighten` expected** for `pane.rs`; run it anyway and commit whatever it
>    writes.
>
> ## Acceptance criteria
>
> - `cargo test -p quantick-app pane::` before and after runs the same number of
>   tests, all green. *Evidence: both counts.*
> - `git diff origin/main...HEAD -- crates/app/src/pane.rs` shows deletions plus
>   one `mod tests;` line and no other insertion. *Evidence: the diff stat and the
>   insertion lines, quoted.*
> - `pane.rs` is ≤ 7,772 lines. No moved item was de-indented; every new module
>   name ends in `_tests` or is `tests`. *Evidence: `wc -l`, `ls crates/app/src/pane/`.*
> - The four-check loop green; `cargo test -p quantick-guards` green.
>
> ## Out of scope, deliberately
>
> - Splitting the production half of `pane.rs` or the 77-field `ChartPane`.
> - The 21 inline `#[cfg(test)]` sites.
> - `paper_trading.rs`, `app.rs`, `tab.rs` — the first is being edited by
>   `fix/generated-truth`; the others are not this mission.
> - Adding a `lib.rs` target to `crates/app` so tests stop being one 3m15s
>   binary. That is the mission after this one and `refactor/orderflow-crate` land.
>
> ## Parallel work to respect
>
> - `fix/generated-truth` (open): adds two hook-declaration lines near
>   `QUANTICK_CONTEXT_MENU`, `QUANTICK_DRAWINGS_DEMO`, `QUANTICK_DRAWING_DRAFT`
>   in the production half of `pane.rs`. No overlap with the test module.
> - `refactor/orderflow-crate` (open, parallel): touches `pane.rs` at its `use`
>   lines only. Whoever merges second rebases.
>
> ## Housekeeping
>
> ```sh
> git worktree list                       # a worktree may already exist
> git fetch origin
> git worktree add -b refactor/pane-tests-out ../quantick-worktrees/refactor-pane-tests-out origin/main
> cd ../quantick-worktrees/refactor-pane-tests-out && cargo build -p quantick-guards
> ```
>
> Tier `medium`: a 2,700-line move is past the `small` ceiling, so
> `delivery-review` runs. `arch-review` waives nothing.
