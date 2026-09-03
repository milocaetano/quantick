# A9 / R10 / R16 — `app.rs` was not touched at all

The mission allowed edits at five harness call sites. None were needed.

```
$ git diff origin/main...HEAD -- crates/app/src/app.rs
$ git diff origin/main...HEAD --numstat -- crates/app/src/app.rs
(no output — the file is byte-identical to origin/main)
```

The reason is the wrappers. `autostart_report`, `autostart_calendar`,
`set_ledger_scope`, `autostart_folded_days`, `autostart_ledger_pages` and
`set_report_list_open` all still exist on `PaperTrading` with the same
names and the same signatures; each is now one line that hands the call to
`ReportState`. A name the operator already knows does not move because the
code behind it did.

## The one file outside `paper_trading.rs` that did change

```
$ git diff origin/main...HEAD -- crates/app/src/main.rs
+mod paper_report;
```

One line, in alphabetical order among its siblings. That is module
registration, not a restructure, and there is no way to add a file to a
Rust binary without it.

## The non-goals stayed out

```
$ git diff --stat origin/main...HEAD
 .claude/GOAL-archive-report-out-of-the-ticket.md   | (the mission record)
 .claude/evidence/report-out-of-the-ticket/…        | (this evidence)
 crates/app/src/main.rs                             |   1 +
 crates/app/src/paper_report.rs                     | (new)
 crates/app/src/paper_trading.rs                    |
 crates/guards/size-baseline.txt                    |
```

`risk_sizing.rs` is untouched. So is every file under `crates/app/src/control/`,
so the control plane learned nothing new. No crate outside `quantick-app`
changed except the size baseline.
