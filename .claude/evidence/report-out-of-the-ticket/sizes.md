# A12 / R13 / R11 — the accounting

Production lines as `crates/guards/src/size.rs` counts them (test modules
excluded), from `crates/guards/size-baseline.txt`.

| file | before | after | change |
| --- | ---: | ---: | ---: |
| `crates/app/src/paper_trading.rs` | 9,238 | **6,396** | **−2,842** |
| `crates/app/src/paper_report.rs` | — | **3,300** | +3,300 |
| **total production** | 9,238 | 9,696 | **+458** |
| `!budget` | 60,939 | 61,397 | **+458** |

**Under 7,000: yes**, at 6,396 — the criterion asked for below 7,000.

## Not the largest file any more, and it already was not

| file | production lines |
| --- | ---: |
| `crates/app/src/app.rs` | 9,362 |
| `crates/app/src/pane.rs` | 7,771 |
| `crates/app/src/paper_trading.rs` | **6,396** ← was 9,238, second largest |
| `crates/app/src/tab.rs` | 4,470 |
| `crates/app/src/control/gateway.rs` | 4,142 |
| `crates/app/src/paper_report.rs` | **3,300** (new) |

Worth stating plainly rather than claiming the criterion outright:
`paper_trading.rs` was the **second** largest file on `origin/main`, not
the largest — `app.rs` at 9,362 was ahead of it by 124 lines. It is now
third, behind `app.rs` and `pane.rs`, and the new module enters at sixth.
A parallel session is shrinking `app.rs`; had this branch not run, that
work would have made `paper_trading.rs` the largest file in the workspace.

## The +425, stated rather than buried

The request asked for this to be honest, because the last extraction moved
854 lines out of a tracked file into an untracked one and the budget fell
528 while total production rose ~326 — a reader of the budget alone would
not have known.

Here it is the other way round and just as visible: the budget **rises**
by 458. What it bought is the seam — `ReportEnv`, `OpenRow`,
`ReportResponse`, the `ReportState` struct and its hand-written `Default`,
the module header, the eleven one-line wrappers, the `report_env!` macro,
`open_row`, the test-only `report_parts` split, and the two review fixes
(forwarding the report's toast, and the `is_open` guard that keeps the
per-trade path from gathering an env it would throw away). That is the price of
the report no longer being able to reach into the host, and it is paid on
the `!budget` line where a reviewer watches one number move, not hidden
inside a signed per-file entry.
