# Mission — break the paper module cycle, and guard against the next one

Break the app crate's remaining module cycle (`paper_report` ↔ `paper_trading`)
by moving the shared presentation vocabulary into a neutral module below both,
then add a cycle guard to `crates/guards` as a third instance of the ratchet
`Policy`, so the next refactor that welds two modules together fails the build
instead of being found by hand months later.

**Why it matters.** This is the second module cycle in the app crate in three
days. The first was broken by hand (PR #285); this one was *introduced* by
PR #282, a well-reviewed refactor, and nothing in the repository saw it.
Reviews caught neither. A rule enforced only by review is a rule that drifts —
`CLAUDE.md` already says so about worktrees, and `size`, `language` and
`encoding` are the repo's answer. Cycles get the same answer.

**Tier:** `medium`. Two crates change, one of them headless-adjacent; the diff
is a module extraction plus a new guard with its own baseline, which is more
than a `small` mission's exemption is meant to cover, and `delivery-review`'s
completeness pass is worth running over a request with nine distinct asks. It
is not `high`: behaviour is unchanged by construction, no UI surface moves, and
the acceptance evidence is all mechanical (tests, guard exit codes).

## Request ledger

- **R1** — Break the app crate's remaining dependency cycle: `paper_report`
  imports thirteen items from `paper_trading` while `paper_trading` re-exports
  three back.
- **R2** — Move the shared helpers down into a neutral module below both, the
  way PR #285 moved the plot geometry into `plot_area`, so *"`paper_report`
  stops importing `paper_trading` at all"*. The thirteen: `PositionSummary`,
  `caption`, `fmt_decimal`, `fmt_duration_ms`, `fmt_offset_minute`,
  `fmt_points`, `fmt_signed_points`, `list_symbol_folders`, `pill_toggle`,
  `points_color`, `position_word`, `sanitize_symbol`, `today`.
- **R3** — The `HistoryRow` / `LedgerAction` / `LedgerScope` re-export in
  `paper_trading` *"may stay, since it is a deliberate facade with callers in
  app.rs and dock.rs and it is one-directional once the other edge is gone"*.
- **R4** — Behaviour unchanged.
- **R5** — *"tests travel with the items they cover"*.
- **R6** — Add a cycle guard to `crates/guards` as *"a third instance of the
  ratchet Policy that size and context already share"*.
- **R7** — It *"reads the `use crate::` edges of a crate's modules, reports any
  strongly connected component larger than one"*.
- **R8** — …*"and records the count in a baseline that starts at zero"*.
  Amended by **D1**: the baseline records what exists, signed.
- **R9** — *(purpose; judges the others)* *"So that a refactor that welds two
  modules together fails the build instead of being found by someone measuring
  months later."*

## Decisions taken by the trader

- **D1 — The baseline records what exists, signed, rather than starting at
  zero.** Measured before any code was written: the repository has three
  production `use crate::` cycles today, not one — `app`
  [`paper_report`, `paper_trading`], `app` [`app`, `surfaces`, `control`]
  (`app.rs` → `surfaces::MarketRequest` → `control::AgentPopup` →
  `app::QuantickApp`), and `trading` [`events`, `position`]. A baseline
  literally starting at zero would require breaking all three, which reaches
  into the app/control/surfaces architecture and into a headless crate nobody
  asked to touch. So the baseline is an entry per crate, signed with its
  reason, exactly like `size-baseline.txt` and `context-baseline.txt`: this
  branch drives `crates/app` from 2 to 1, `crates/trading` stays at a signed 1,
  every other crate is 0, and a new cycle anywhere fails the build — which is
  R9. `--tighten` writes the lower number when a signed cycle is later broken.

## Assumptions

- **S1 — Production edges only; `#[cfg(test)]` modules and `*/tests/*` files
  are not measured.** The size guard already rations production lines only and
  leaves `tests/` untracked, for a stated reason; a cycle guard that counted
  test edges would fire on `paper_report`'s own test module reaching for
  `PaperTrading` to build a fixture, which is not the defect being guarded.
  Safe to assume rather than ask: the repo has already made this exact call
  once, in writing.
- **S2 — Nodes are a crate's top-level modules.** `control::contract` counts as
  `control`. It is what R7's "a crate's modules" reads as against a crate root
  that declares its modules in one list, it is the granularity the cycle in the
  request is stated at, and a finer one would need parent/child edges invented
  to be meaningful.
- **S3 — Only `use crate::` statements are edges, not inline `crate::foo::bar`
  paths.** R7 says so explicitly, and measuring inline paths as well was tried
  in the prototype: it reports the `guards` crate itself as a three-way cycle
  purely from `[`size`](crate::size)` doc-comment links, which is a false
  positive that would make the guard untrustworthy on day one.
- **S4 — The new module is `crates/app/src/paper_chrome.rs`.** Naming and file
  placement have a conventional default here. "Chrome" is already the word
  `PositionSummary`'s own doc comment uses for the surfaces that share these
  items.
- **S5 — Every caller of a moved helper is repointed at its new home; no
  helper is re-exported from `paper_trading`.** R3 grants the facade to the
  three ledger types by name and by reason; extending it to the helpers would
  leave `paper_calendar` and `paper_home` still naming `paper_trading` for a
  formatter, which is the edge the mission exists to delete.
- **S6 — `--tighten` covers the cycle baseline too.** `main.rs` runs both
  existing ratchets and says in its own doc comment why tightening only one is
  a trap; a third that silently stayed stale would be that trap.
- **S7 — Wanted to ask, went with a reading:** whether the guard should also
  fail on a *new* signed entry appearing (a crate gaining its first cycle) as
  opposed to an existing count rising. Reading taken: both fail, because the
  ratchet's untracked-file rule already works this way — a file with no entry
  is capped at the threshold, and here the threshold is 0.
- **S8 — `fmt_offset_minute` and `today` land in `paper_calendar`, not in
  `paper_chrome`.** Both need `CivilDate` and `civil_utc`, which
  `paper_calendar` owns, while `paper_calendar` paints its day cells with
  `points_color` and `fmt_signed_points`. Putting all thirteen in one module
  would therefore have made `paper_chrome` and `paper_calendar` import each
  other — trading the cycle the mission removes for an identical one, in the
  module built to prevent it. `paper_calendar` is itself a neutral module
  below both halves, and these two are date law, so R2 is satisfied by both
  homes. Recorded rather than asked because it was discovered while writing
  the code, after step 3 had closed, and no reading of the request makes the
  new cycle acceptable.
- **S9 — A fourth cycle, `crates/control`, is signed alongside the two D1
  names.** The guard found `error` ↔ `wire` on its first run; the throwaway
  prototype that measured the repository before D1 was asked had missed it,
  because both imports are written as grouped `use crate::{...}` trees and
  the prototype read only the first segment after `crate::`. D1's answer —
  record what exists, signed — decides this one too, so it is an assumption
  rather than a second question.

## Acceptance criteria

- [ ] **A1** — `crates/app/src/paper_report.rs` names `paper_trading` nowhere
      in its production code: no `use crate::paper_trading`, no inline
      `crate::paper_trading::` path.
      *Evidence:* `grep -n 'paper_trading' crates/app/src/paper_report.rs`
      returns only lines inside the `#[cfg(test)]` module, quoted in the PR.
      → PR body. *(R1, R2)*
- [ ] **A2** — Eleven of the thirteen named items live in
      `crates/app/src/paper_chrome.rs`, which imports neither `paper_trading`
      nor `paper_report`; `fmt_offset_minute` and `today` land in
      `paper_calendar` instead, per **S8**. Both homes are neutral modules
      below `paper_report` and `paper_trading`, which is what R2 asks for.
      *Evidence:* each file's `use crate::` block, quoted; `grep` for each of
      the thirteen showing its definition at the new address.
      → PR body. *(R2)*
- [ ] **A3** — `paper_trading`'s `pub(crate) use crate::paper_report::{HistoryRow,
      LedgerAction, LedgerScope}` survives unchanged, with its comment.
      *Evidence:* the line, quoted from the branch. → PR body. *(R3)*
- [ ] **A4** — Behaviour unchanged: `cargo test --workspace` green, and
      `the_report_numbers_are_fixed` — the byte-for-byte pin on a fixed
      journal's whole report — passes untouched.
      *Evidence:* the test run's summary lines, and the diff showing that test
      unedited. → PR body. *(R4)*
- [ ] **A5** — Every test covering a moved item moved with it: no test naming
      only `paper_chrome` items is left behind in `paper_trading`.
      *Evidence:* the moved test names listed with their new file.
      → PR body. *(R5)*
- [ ] **A6** — `crates/guards/src/cycle.rs` is a third `ratchet::Policy` — it
      reuses the shared baseline parser, `!budget` and `tighten` rather than
      copying them — and is registered in `GUARDS` and in `main.rs`'s tighten
      list.
      *Evidence:* the `Policy` construction quoted, plus the two registration
      lines. → PR body. *(R6)*
- [ ] **A7** — The guard reads production `use crate::` edges over a crate's
      top-level modules and reports every strongly connected component larger
      than one, naming the modules and the edges that close the loop.
      *Evidence:* unit tests over synthetic sources, named in the PR; plus the
      guard's real output on `origin/main`, which must name both app cycles. → PR body. *(R7)*
- [ ] **A8** — `crates/guards/cycle-baseline.txt` carries a signed entry per
      crate with a cycle (`crates/app` 1, `crates/control` 1, `crates/trading` 1)
      and a `!budget`; every other crate is capped at 0 with no entry.
      *Evidence:* the baseline file, quoted whole. → PR body. *(R8, D1)*
- [ ] **A9** — `cargo run -p quantick-guards` exits 0 on this branch, and would
      have exited 1 on `origin/main`.
      *Evidence:* both exit codes and both outputs. → PR body. *(R9)*
- [ ] **A10** — A test proves the guard catches the exact defect PR #282 landed:
      a two-module `use crate::` cycle raises a finding rather than passing.
      *Evidence:* the test name and its assertion. → PR body. *(R9)*
- [ ] **G1** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace` all
      green after rebasing on latest `main`, each run on its own.
      *Evidence:* the four commands' output. → PR body.
- [ ] **G2** — Performance impact declared: every touched path classified by
      rate (per-trade / per-depth / per-frame / rare).
      *Evidence:* the classification. → PR body.
- [ ] **G3** — `arch-review` run over `git diff origin/main...HEAD`, step 0
      included, every Blocker and Should-fix resolved or deferred with its
      severity.
      *Evidence:* the verdict and the deferral list. → PR body.
- [ ] **G4** — Every artifact English, per `CLAUDE.md`.
      *Evidence:* `arch-review` dimension 8 and `quantick-guards`' language
      guard, both clean. → PR body.
- [ ] **G5** — The new guard docks per `new-extension`: port named (`GUARDS` and
      `ratchet::Policy`), registration-only edits to existing files, and the
      third `Policy` instance is itself the proof that the abstraction holds a
      second implementation.
      *Evidence:* the diff of `lib.rs` and `main.rs`, which must be additions
      to a list and nothing else. → PR body.

### Not applicable, and why

- **Hot path** — nothing here runs per trade, per depth update or per frame.
  The moved helpers are the same functions at a new address, called from the
  same places; the guard runs in `cargo test`, never in the app.
- **User-visible surfaces** (`ui-harness`, `visual-qa`, `trader-ux-review`) —
  no surface is added, removed or changed. The moved items render exactly what
  they rendered, which is what A4 pins.
- **Something a trader *does*** — no action, tool, trade or lock is added.
- **Engine / determinism** — no crate below `app` changes behaviour; the
  `trading` crate is only *measured* by the new guard, not edited.
- **Docs/skills only** — this is code, so no shape dimension is waived.

### Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open.

## The request as received

Quoted verbatim and untranslated: this is the marked, attributed quotation
`CLAUDE.md`'s English rule exempts, and the ledger above is the operative
English statement of it. It is preserved so no reviewer has to trust the
ledger as its own source of truth.

> /mission medium Break the app crate's remaining dependency cycle, and stop the
> next one from landing unseen. `paper_report` imports thirteen items from
> `paper_trading` — PositionSummary plus the presentation helpers caption,
> fmt_decimal, fmt_duration_ms, fmt_offset_minute, fmt_points, fmt_signed_points,
> list_symbol_folders, pill_toggle, points_color, position_word, sanitize_symbol
> and today — while `paper_trading` re-exports HistoryRow, LedgerAction and
> LedgerScope back, so the two form a cycle. Move the shared helpers down into a
> neutral module below both, the way PR #285 moved the plot geometry into
> plot_area, so `paper_report` stops importing `paper_trading` at all; the
> re-export may stay, since it is a deliberate facade with callers in app.rs and
> dock.rs and it is one-directional once the other edge is gone. Behaviour
> unchanged, tests travel with the items they cover. Then add a cycle guard to
> crates/guards as a third instance of the ratchet Policy that size and context
> already share: it reads the `use crate::` edges of a crate's modules, reports
> any strongly connected component larger than one, and records the count in a
> baseline that starts at zero. This cycle was introduced by PR #282, a
> well-reviewed refactor, and nothing in the repository saw it — the previous
> cycle was broken by hand three days ago and this is the second. So that a
> refactor that welds two modules together fails the build instead of being found
> by someone measuring months later.
