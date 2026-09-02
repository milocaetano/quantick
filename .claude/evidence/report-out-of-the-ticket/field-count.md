# A3 / R14 - what `PaperTrading` stopped holding

Counted from the struct body in each revision, not from the prose.

| | fields |
| --- | --- |
| `origin/main` | **75** |
| this branch | **55** |
| net | **-20** |

The criterion asked for at least 20, and the honest reading of the number
is this: **twenty-one fields' worth of state left**, and one field stayed
behind holding all of it.

Twenty names disappeared outright:

- `report_open`
- `report_symbol`
- `report_period`
- `report_source`
- `report_custom_text`
- `report_symbols`
- `report_generation`
- `report_view`
- `calendar`
- `report_days`
- `report_days_key`
- `report_list_open`
- `ledger_scope`
- `ledger_symbols`
- `collapsed_days`
- `ledger_tz`
- `ledger_pages`
- `history_cache`
- `saved_totals`
- `selected_trade`

The twenty-first is `report`, which did not disappear - it changed type,
and kept its name because the name was already right:

| | type |
| --- | --- |
| before | `Option<LoadedHistory>` |
| after | `crate::paper_report::ReportState` |

So 75 - 21 + 1 = 55. No field was added under a new name; the state that
left is the state `ReportState` now owns.

Reproduce:

```sh
git show origin/main:crates/app/src/paper_trading.rs |
  awk '/^pub struct PaperTrading/,/^}/' | grep -cE '^[[:space:]]+(pub )?(pub\(crate\) )?[a-z_]+:'
awk '/^pub struct PaperTrading/,/^}/' crates/app/src/paper_trading.rs |
  grep -cE '^[[:space:]]+(pub )?(pub\(crate\) )?[a-z_]+:'
```
