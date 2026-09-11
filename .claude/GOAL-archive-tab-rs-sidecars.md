# Mission: `tab.rs` sheds its sidecars

Move the four cohesive groups that lie away from `Tab`'s struct and layout code
— venue-history paging, feed lifecycle, strategy runner, canvas painter — plus
the four inline test modules, out of `crates/app/src/tab.rs` and into sibling
modules under `crates/app/src/tab/`, the way `app/` and `pane/` already hold
theirs. Bodies unchanged, method names kept, ceiling tightened, budget lowered.

Why it matters: `Tab` is the second-widest struct in the repository and its
file the second-largest. Everything a tab *does* lives in one `impl Tab` block
spanning 3,700 lines, so a history-paging bug is read through the canvas
painter and the canvas is read through the paging state machine. Each of the
four groups has a reader that never needs the other three.

**Tier:** `medium`. A ~2,700-line move plus 685 lines of tests, with two `pub`
types (`OlderCandles`, `CanvasChrome`) whose paths other files import and must
keep. Mechanical in kind but large in blast radius, and the privacy widening a
split `impl` block forces is easy to get subtly wrong — more than `small`
buys, short of the design judgement `high` is for.

## Request ledger

Sourced from `C:\src\mission-tab-rs-sidecars.md`, read in full before any work.

| # | Ask |
| --- | --- |
| R1 | Move the venue-history group (free fns `merge_older_candles`/`trim_to_seam`/`trim_borrowed_to_seam`/`seam_bucket_ms`, `OlderCandles`, the `:1111-1662` methods and the `:2196-2457` trade-history page path) into `crates/app/src/tab/history.rs`. |
| R2 | Move the feed lifecycle group (`:2470-2790`, `:3019-3282`, `:3538-3922`) into `crates/app/src/tab/feed.rs`. |
| R3 | Move the strategy runner (`:3295-3534`) into `crates/app/src/tab/strategies.rs`. |
| R4 | Move the canvas painter (`CanvasChrome`, `trading_pane`, `:3928-4464`) into `crates/app/src/tab/canvas.rs`. |
| R5 | Move the four inline test modules (`:4469-5157`) into `crates/app/src/tab/tests/mod.rs`, `tab.rs` ending with `#[cfg(test)] mod tests;`; keep the four `_tests` names; never de-indent a moved body. |
| R6 | "**Bodies unchanged**", method names kept — `git diff --color-moved=zebra` shows moves, not edits; every non-move hunk quoted and explained. |
| R7 | `OlderCandles` and `CanvasChrome` stay reachable at their current `crate::tab::` paths via `pub use`, "so no other file changes an import". |
| R8 | Every `pub(super)` and every re-export listed in the PR body. |
| R9 | Hooks: nothing to move; generated hook registry byte-identical, capability inventory unchanged. |
| R10 | `tab.rs` at most 1,900 lines, ceiling tightened via `--tighten`. |
| R11 | Size `!budget` lower by at least 2,400. |
| R12 | Each new file under 1,500 production lines, `feed.rs` (~960) and `history.rs` (~900) re-measured with their doc comments. |
| R13 | `--report` before and after: only `tab.rs`-related lines and the new files move; no new file appears under `file.largest`. |
| R14 | `cargo test -p quantick-app` runs the same number of tests; the four-check loop green; `cargo test -p quantick-guards` green. |
| R15 | Respect the out-of-scope list: no change to `Tab`'s 62 fields; none to `set_layout`, `apply_pending_layout`, `restore_canvas` or the accessors; none to paging rules, drain budgets, strategy arming or what a capture could see. |
| R16 | Re-check every evidence-ledger claim against the branch point rather than trusting it. |
| R17 | *Purpose, and the ask that judges the others:* each of the four concerns becomes readable on its own — the reader of a paging bug no longer reads the canvas painter to reach it. |
| R18 | Say whether `BOOK_DRAIN_BUDGET` travels with the feed drain or is widened in place (ledger #11). |
| R19 | Respect parallel `refactor/gateway-rs-sidecars`: the two meet only at the `!budget` line; whoever lands second re-runs `--tighten`. |

## Decisions taken by the trader

None. At `medium` the budget is two questions and nothing reached the bar: the
brief settles file layout, the test destination and the two re-exports, and the
one genuinely open call (R18) is reversible in a single edit, so it is recorded
as `S3` rather than asked.

## Assumptions

- **S1** — The branch is cut from `origin/main` at `62c8730`, not the brief's
  `cc4c92f`; `refactor/app-rs-launch-hooks` (#305) landed in between. Safe:
  every `file:line` in the brief was re-verified against `62c8730` and all
  match exactly — that PR touched `app.rs`, not `tab.rs`.
- **S2** — R11's "`!budget` lower by at least 2,400" is measured against the
  branch point's **50996**, not the brief's stale 52139. The ask is a
  *reduction of 2,400*, not an absolute target, so the moved base does not
  change what is owed.
- **S3** — *(R18)* The consts at `:52-62` **stay in `tab.rs` and are widened**
  to `pub(super)` rather than travelling. Ledger #6 lists "consts `:54-62`" on
  the what-stays side, and `BOOK_GENERATION_STRIDE` is `pub` and imported by
  `crate::tab::BOOK_GENERATION_STRIDE` from `app/tests/mod.rs` — moving it
  would either break that path or need a re-export for no gain. One home for
  the file's constants; reversible in one edit if a reviewer prefers otherwise.
- **S4** — Test modules stay **nested inside `tests/mod.rs`** (ledger #9's
  first option), not split one file per module, because scope §2 names
  `tab/tests/mod.rs` and its 685-line count as a single destination.
- **S5** — *Wanted to ask, went with this reading:* every moved private method
  called from outside its new module gets `pub(super)` — the narrowest
  widening that keeps it visible across `tab`'s subtree — rather than
  `pub(crate)`. The compiler drives which ones need it.
- **S7** — `history_reach_running` (`:2459-2466`) travels to `history.rs`
  although the brief's ledger #2 stops its range at `expire_history_note`. It
  sits between that method and `next_book_generation`, and reads nothing but
  `self.campaign` — the paging run's own state. Leaving it behind would split
  one concern across two files for no reason and weaken R17. A move, not an
  edit; called out in the PR body.
- **S6** — The brief's ledger #3 claim that the eight `QUANTICK_*` reads live
  in the feed and history groups is **incorrect**: `tab.rs` has exactly one
  real env read (`QUANTICK_PANE_COLLAPSED`, `:945`, inside `new`), and the
  other eight occurrences are doc-comment mentions. Both the read and the
  `declare_hooks!` site stay in `tab.rs`, so R9 holds trivially.

## Acceptance criteria

- [ ] **A1** — `crates/app/src/tab/history.rs` holds the venue-history group;
      `tab.rs` no longer defines any of its items. *Evidence:* the file exists,
      `grep` shows the moved symbols only there. → PR body. *(R1)*
- [ ] **A2** — `crates/app/src/tab/feed.rs` holds the feed lifecycle group.
      *Evidence:* as A1. → PR body. *(R2)*
- [ ] **A3** — `crates/app/src/tab/strategies.rs` holds the strategy runner.
      *Evidence:* as A1. → PR body. *(R3)*
- [ ] **A4** — `crates/app/src/tab/canvas.rs` holds the canvas painter,
      `CanvasChrome` and `trading_pane`. *Evidence:* as A1. → PR body. *(R4)*
- [ ] **A5** — The four test modules live in `crates/app/src/tab/tests/mod.rs`
      under their existing `_tests` names, bodies at their original
      indentation; `tab.rs` ends with `#[cfg(test)] mod tests;`. *Evidence:*
      the file, and a diff showing no de-indentation. → PR body. *(R5)*
- [ ] **A6** — `git diff --color-moved=zebra` renders the change as moves; every
      hunk that is not a move is quoted in the PR body with the reason it was
      needed. *Evidence:* the quoted list. → PR body. *(R6, R15)*
- [ ] **A7** — No file outside `crates/app/src/tab*` changes an import:
      `crate::tab::OlderCandles` and `crate::tab::CanvasChrome` still resolve.
      *Evidence:* `git diff --stat` shows no other file touched for imports;
      the `pub use` lines quoted. → PR body. *(R7)*
- [ ] **A8** — Every `pub(super)` added and every re-export introduced is listed
      in the PR body. *Evidence:* the list, checked against `grep`. → PR body.
      *(R8)*
- [ ] **A9** — The generated hook registry and capability inventory are
      byte-identical to the branch point. *Evidence:* `git diff` over the
      generated files is empty; `cargo test -p quantick-guards` green.
      → PR body. *(R9, S6)*
- [ ] **A10** — `wc -l crates/app/src/tab.rs` is at most 1,900, and its
      `size-baseline.txt` ceiling is tightened to match. *Evidence:* `wc -l`
      plus the baseline diff. → PR body. *(R10)*
- [ ] **A11** — `!budget` falls by at least 2,400 from 50996. *Evidence:*
      baseline diff. → PR body. *(R11, S2)*
- [ ] **A12** — Each new file under `tab/` is below 1,500 production lines.
      *Evidence:* `--report` output. → PR body. *(R12)*
- [ ] **A13** — `--report` before vs. after differs only in `tab.rs`-related
      lines and the new files; no new file appears under `file.largest`.
      *Evidence:* a diff of the two reports. → PR body. *(R13)*
- [ ] **A14** — `cargo test -p quantick-app` reports the same test count as the
      branch point. *Evidence:* both counts quoted. → PR body. *(R14)*
- [ ] **A15** — Behaviour is untouched: no edit to `Tab`'s fields, to
      `set_layout`/`apply_pending_layout`/`restore_canvas`/the accessors, or to
      paging rules, drain budgets, strategy arming or capture visibility.
      *Evidence:* A6's non-move hunk list contains none. → PR body. *(R15)*
- [ ] **A16** — Every evidence-ledger claim re-checked against `62c8730`, with
      each correction recorded. *Evidence:* `S1`, `S2` and `S6` above.
      → this file. *(R16)*
- [ ] **A17** — Each of the four concerns is readable alone: no sidecar module
      needs another sidecar's body to be understood, and `tab.rs` retains only
      the struct, its layout and its accessors. *Evidence:* the `mod` lines and
      each file's `use super::…` header quoted. → PR body. *(R17)*
- [ ] **A18** — The `BOOK_DRAIN_BUDGET` call is stated and carried out.
      *Evidence:* `S3`, and the const's final location. → PR body. *(R18)*
- [ ] **A19** — `--tighten` is re-run against `origin/main` immediately before
      the PR opens, so a `gateway-rs-sidecars` landing first is absorbed.
      *Evidence:* the rebase and the final baseline diff. → PR body. *(R19)*

### Injected gates

- [ ] **G1** — Every artifact in English per `CLAUDE.md`. *Evidence:*
      `arch-review` dimension 8; `cargo test -p quantick-guards`. → PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace` — each run on its
      own, never chained behind `||`. *Evidence:* four exit codes. → PR body.
- [ ] **G3** — Performance impact declared. Classified in advance: this change
      moves bodies between modules without altering them, so every touched
      path — per-trade (`ingest_live_trade_at`, `drain_feed`), per-depth
      (`drain_book_feed`), per-frame (`draw_canvas`, `run_strategies`) and rare
      (history paging, feed switching) — keeps its instruction sequence.
      Rust module boundaries carry no runtime cost and nothing crosses a crate
      boundary, so the expectation is *exactly* flat, not merely no worse.
      *Evidence:* this declaration plus A6's move-only diff. → PR body.
- [ ] **G4** — `arch-review` run over the final branch, every Blocker and
      Should-fix resolved or deferred in the PR body. *Evidence:* its verdict.
      → PR body.

### Not applicable, and why

- **Hot path evidence** — G3's row asks for measurement when a hot path
  *changes*. Nothing here changes one: the diff is move-only, verified by A6,
  and a body that is byte-identical in a new module cannot run differently.
  Declared rather than measured.
- **User-visible surfaces** (`ui-harness`, `visual-qa`, `trader-ux-review`) —
  no surface changes. `draw_canvas` moves file without a byte changing, so
  there is nothing new or altered for a capture to photograph.
- **Adds a capability** (`new-extension`) — nothing is added. This mission
  subtracts from a file; no new port, feed, bar type, indicator or panel.
- **Something a trader does** — no new action, tool, trade or lock; every
  existing one keeps its name and its call path.
- **Engine / determinism** — `crates/app` is not the engine, and no bar-building
  or aggregation code is touched.
- **Docs/skills only** — this is code, so the full shape pass applies; no
  dimension is waived.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, with the tier named beside the verification boxes.

## The request as received, verbatim

> *(Attributed quotation, reproduced under `CLAUDE.md`'s language exemption.)*

> medium refactor/tab-rs-sidecars — crates/app/src/tab.rs is 5,157 lines (4,472
> production) with one `impl Tab` block from line 757 to 4,468 and four inline
> test modules after it. Four cohesive groups lie away from the struct and its
> layout code: the venue-history paging (`tab.rs:194-331` free helpers and
> `OlderCandles`, `:1111-1662`, `:2196-2457`, about 900 lines), the feed
> lifecycle (`:2470-2790`, `:3019-3282`, `:3538-3922`, about 960 lines), the
> strategy runner (`:3295-3534`, about 240 lines) and the canvas painter
> (`CanvasChrome` `:164-192`, `trading_pane` `:737-755`, `:3928-4464`, about 590
> lines). Move them into sibling modules under crates/app/src/tab/ the way app/
> and pane/ hold theirs, and move the four test modules (`:4469-5157`, 685
> lines) into tab/tests/mod.rs. Bodies unchanged, method names kept, ceiling
> tightened, budget lowered. Read C:\src\mission-tab-rs-sidecars.md in full
> before anything else and build the request ledger from it.
