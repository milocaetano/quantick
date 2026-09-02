# A5 / R6 — the module never reaches back

```
$ grep -n PaperTrading crates/app/src/paper_report.rs
15://! - **It never reaches back.** Nothing here names `PaperTrading`. What
3293:    use crate::paper_trading::PaperTrading;
3325:        let mut paper = PaperTrading::new();
…nine more, all inside `#[cfg(test)] mod tests` (first `mod tests` line: 3288)
```

**Production references: zero.** Line 15 is the module header stating the
rule; every other hit is a test that drives a real host because it is about
the journal on disk rather than the arithmetic.

## What it is handed instead

```rust
pub(crate) struct ReportEnv<'a> {
    pub symbol: &'a str,
    pub dir: &'a Path,
    pub session_journal_paths: &'a [PathBuf],
    pub session_trades: &'a [ClosedTrade],
    pub open: Option<OpenRow>,
}
```

Borrowed for the call, the way `SurfaceEnv` hands a floating surface what
its host knows. `OpenRow` gathers the three venue reads the ledger's top
row needs — the summary, the mark it is valued against, how long it has
been held — into one value, so a caller cannot supply two of the three.

## What leaves

```rust
#[derive(Default)]
pub(crate) struct ReportResponse {
    pub start_import: bool,
    pub toast: Option<String>,
}
```

The import button decides a folder picker should open; the host opens it,
because the import copies into *its* journal. The typed-period field can
refuse an entry; the message goes to the same outbox every other paper
acknowledgement uses, rather than this module growing a second toast lane
on a second clock — which is the divergence the panel's private toast was
converged away from in the first place.

## The two entry points

```rust
pub(crate) fn draw_window(&mut self, ctx, tz, env: &ReportEnv<'_>) -> ReportResponse
pub(crate) fn draw_trades_tab(&mut self, ui, tz, env: &ReportEnv<'_>) -> Option<LedgerAction>
```

Everything under them — the cut, the equity walk, the day index, the paging
arithmetic, the row layout — is plain functions over plain values, which is
what lets the tests below them run without a window.
