# A8 / A14 / R9 / R16 — the money path is untouched

## Which files changed at all

```
$ git diff --stat origin/main...HEAD
 crates/app/src/main.rs          |  1 +
 crates/app/src/paper_report.rs  | (new file)
 crates/app/src/paper_trading.rs |
 crates/guards/size-baseline.txt |
 .claude/GOAL-archive-report-out-of-the-ticket.md
 .claude/evidence/report-out-of-the-ticket/…
```

`crates/app/src/risk_sizing.rs` — **unchanged**. `crates/app/src/app.rs` —
**unchanged**. Nothing under `crates/app/src/control/` — **unchanged**, so
the control plane learned nothing new and lost nothing. No crate outside
`quantick-app` changed except the size baseline.

## What changed inside `paper_trading.rs`, by section

Every hunk outside the moved region, classified:

| site | what changed | why it is not a behaviour change |
| --- | --- | --- |
| imports | dropped `PerformanceReport`, `SideReport`, four `paper_calendar` names; added the `paper_report` re-exports | the types moved; the re-export keeps `LedgerAction`, `LedgerScope` and `HistoryRow` reachable at their old paths |
| `report_env!` macro | new | builds the borrowed env; a macro and not a method because a `&self` method borrows the whole host and collides with `&mut self.report` |
| struct field | 21 fields → `report: ReportState` | same state, one owner |
| `PaperTrading::new` | 21 initialisers → `ReportState::default()` | the `Default` impl carries the same three non-default values: list open, one page revealed, scope follows the chart |
| `set_trades_dir` | 6 lines → `self.report.trades_dir_changed(&env)` | the method body **is** those six lines, in the same order, including the reload-if-open |
| symbol change | 2 lines → `self.report.symbol_changed()` | same two assignments; the comment about deliberately leaving the revealed page alone travelled with them |
| `clear` (Escape) | `self.selected_trade.take().is_some()` → `self.report.clear_selected_trade()` | identical expression, one field deeper |
| `selected_trade_index` | reads through `self.report.selected_trade()` | same `.filter` on the same bound |
| ticket's "Report…" button | `self.open_report()` → `self.report.open(&env)` | same path |
| `start_export` | reads `self.report.saved_rows(&env)` | `saved_rows` is the old `if history_cache.is_none() { reload_ledger() }` plus the read, in one named call |
| `poll_import` | 5 lines → `self.report.history_imported(&env)` | same five lines |
| `handle_events` | `if report_open { reload_report() }` → `self.report.journal_changed(&env)` | the flag test moved inside the method that owns the flag |
| `open_row`, `report_parts`, wrappers | new | the seam |

**Untouched entirely**: order entry, `market`/`limit`/`stop` placement,
`aim_bracket`, bracket projection and dragging, the cmd preview and its
layout, the ruler in all its methods, risk sizing and its lock, the
journal writer, `export_csv`, the chart layer and every paint helper.

## The suite agrees

```
$ cargo test --workspace
test result: ok. 2175 passed; 0 failed; 6 ignored   (quantick-app)
```

Not one order-entry, bracket, ruler, risk or journal test needed editing.
The ten report tests that drive a real journal stayed in this file and
still pass; they now read the state through `saved_rows_loaded`,
`revealed_pages` and `view_rows` instead of through fields.
