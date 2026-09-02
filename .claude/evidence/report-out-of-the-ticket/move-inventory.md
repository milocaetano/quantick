# A1 / A2 / A15 - what moved, and what is left

## Types, constants and free functions (72)

| item | `paper_trading.rs` @ origin/main | `paper_report.rs` now |
| --- | ---: | ---: |
| `CALENDAR_CELL_H_PX` | 242 | 176 |
| `CALENDAR_CELL_W_PX` | 238 | 172 |
| `CURVE_FILL_ALPHA` | 259 | 193 |
| `CURVE_GRID_LINE_ALPHA` | 262 | 196 |
| `CURVE_GRID_RESERVE_PX` | 268 | 202 |
| `CURVE_GUTTER_PX` | 313 | 215 |
| `CURVE_MAX_H_PX` | 257 | 191 |
| `CURVE_MAX_POINTS` | 265 | 199 |
| `CURVE_MIN_H_CALENDAR_PX` | 233 | 167 |
| `CURVE_MIN_H_PX` | 255 | 189 |
| `CUSTOM_PERIOD_FIELD_PX` | 277 | 211 |
| `DAY_HEADER_INSET_PX` | 211 | 146 |
| `DAY_HEADER_TEXT_X_PX` | 214 | 149 |
| `DETAIL_GAP_PX` | 204 | 139 |
| `DETAIL_RIGHT_PAD_PX` | 207 | 142 |
| `EquityWalk` | 711 | 437 |
| `HEADLINE_FONT_PX` | 253 | 187 |
| `HistoryRow` | 748 | 474 |
| `LEDGER_PAGE_TRADES` | 197 | 132 |
| `LEDGER_ROW_HEIGHT_PX` | 188 | 123 |
| `LEDGER_SCOPE_COMBO_PX` | 201 | 136 |
| `LedgerAction` | 845 | 550 |
| `LedgerPage` | 918 | 623 |
| `LedgerRow` | 854 | 559 |
| `LedgerRowResponse` | 939 | 644 |
| `LedgerScope` | 494 | 220 |
| `LedgerTotals` | 878 | 583 |
| `LoadedHistory` | 759 | 485 |
| `REPORT_DEFAULT_H_PX` | 227 | 161 |
| `REPORT_DEFAULT_W_PX` | 225 | 159 |
| `REPORT_FOOTER_RESERVE_PX` | 275 | 209 |
| `REPORT_GRID_MIN_H_PX` | 311 | 213 |
| `REPORT_LIST_MAX_H_PX` | 247 | 181 |
| `REPORT_LIST_ROW_H_PX` | 7476 | 2330 |
| `REPORT_MIN_HEIGHT_CALENDAR_PX` | 220 | 154 |
| `REPORT_MIN_HEIGHT_PX` | 273 | 207 |
| `REPORT_MIN_WIDTH_PX` | 271 | 205 |
| `ReportPeriod` | 530 | 256 |
| `ReportSnapshot` | 670 | 396 |
| `ReportView` | 638 | 364 |
| `ReportWindow` | 689 | 415 |
| `RowLines` | 8648 | 3072 |
| `SIDE_RAIL_WIDTH_PX` | 190 | 125 |
| `SourceFilter` | 772 | 498 |
| `TILE_GUTTER_PX` | 251 | 185 |
| `TILE_HEIGHT_PX` | 249 | 183 |
| `TOTALS_STRIP_PX` | 192 | 127 |
| `TRADE_LIST_CELL_PAD_PX` | 7479 | 2333 |
| `TRADE_LIST_COLUMNS` | 7457 | 2311 |
| `draw_day_header` | 8478 | 2902 |
| `draw_equity_curve` | 7167 | 2021 |
| `draw_exit_reason_grid` | 7889 | 2743 |
| `draw_group_header` | 8455 | 2879 |
| `draw_hover_card` | 7416 | 2270 |
| `draw_ledger_row` | 8757 | 3181 |
| `draw_more_row` | 8558 | 2982 |
| `draw_open_row` | 8593 | 3017 |
| `draw_report_grid` | 7699 | 2553 |
| `draw_report_tiles` | 7059 | 1913 |
| `draw_row_lines` | 8666 | 3090 |
| `draw_side_grid` | 7839 | 2693 |
| `draw_tile` | 7105 | 1959 |
| `draw_trade_list` | 7520 | 2374 |
| `elide_tail` | 8741 | 3165 |
| `fmt_period_ms` | 618 | 344 |
| `ledger_detail` | 8854 | 3278 |
| `load_history` | 7915 | 2769 |
| `paint_list_row` | 7488 | 2342 |
| `parse_period` | 598 | 324 |
| `push_by_day` | 8423 | 2847 |
| `report_from_history` | 7989 | 2843 |
| `trade_list_width` | 7482 | 2336 |

## Methods (53), now on `ReportState`

| method | `paper_trading.rs` @ origin/main | `paper_report.rs` now |
| --- | ---: | ---: |
| `a_detail_line_too_long_for_its_row_is_elided_not_clipped` | 10341 | 3730 |
| `a_ledger_row_names_its_market_its_clock_its_age_and_its_ending` | 10304 | 3693 |
| `a_picked_calendar_range_cuts_the_report_and_the_pills_stand_down` | 10466 | 3855 |
| `a_picked_day_with_no_trades_reports_an_honest_empty` | 10527 | 3924 |
| `admits` | 807 | 533 |
| `all_days_collapsed` | 5570 | 900 |
| `all_symbols_hides_older_markets_but_says_so_and_scope_restores_them` | 10630 | 4513 |
| `autostart_calendar` | 5969 | 1294 |
| `autostart_folded_days` | 5990 | 1315 |
| `autostart_ledger_pages` | 6001 | 1326 |
| `autostart_report` | 5926 | 1251 |
| `cutoff_ms` | 579 | 305 |
| `days_fold_shut_one_at_a_time_or_all_at_once` | 10157 | 3557 |
| `default` | 1144 | 725 |
| `draw_ledger_disclosure` | 5902 | 1227 |
| `draw_report_calendar` | 6478 | 1823 |
| `draw_report_filters` | 6300 | 1645 |
| `draw_trades_tab` | 5615 | 945 |
| `ensure_report_view` | 6033 | 1358 |
| `folder` | 506 | 232 |
| `hover` | 796 | 522 |
| `label` | 515 | 241 |
| `ledger_days` | 5582 | 912 |
| `ledger_rows_open_a_new_day_caption_when_the_day_changes` | 10389 | 3778 |
| `of` | 721 | 447 |
| `open_report` | 6013 | 1338 |
| `opt_plain` | 7842 | 2696 |
| `phrase` | 563 | 289 |
| `pick_report_dates` | 5935 | 1260 |
| `plus` | 899 | 604 |
| `reload_ledger` | 5547 | 877 |
| `reload_report` | 6023 | 1348 |
| `report_periods_anchor_to_the_given_trade_never_a_clock` | 9966 | 3378 |
| `rescope_ledger` | 5607 | 937 |
| `row` | 9451 | 3313 |
| `set_day_collapsed` | 5560 | 890 |
| `set_ledger_scope` | 5982 | 1307 |
| `set_report_list_open` | 6008 | 1333 |
| `show_report_month` | 5943 | 1268 |
| `show_toast` | 6608 | 767 |
| `the_day_index_follows_the_source_filter_and_the_timezone` | 10551 | 3949 |
| `the_equity_walk_is_cut_once_and_both_readers_share_it` | 10109 | 3509 |
| `the_ledger_builds_a_bounded_number_of_rows_however_deep_the_history` | 10243 | 3632 |
| `the_ledger_lists_the_chart_a_named_market_or_all_of_them` | 10133 | 3533 |
| `the_ledger_reveals_saved_history_one_page_at_a_time` | 10358 | 3747 |
| `the_report_opens_on_real_and_keeps_replay_out_until_asked` | 9855 | 3322 |
| `the_report_reports_itself_as_data` | 10040 | 3438 |
| `the_report_view_filters_by_period_from_the_newest_trade` | 10594 | 4467 |
| `today_breaks_at_the_displayed_midnight_not_utc` | 9988 | 3400 |
| `toggle_all_days` | 5595 | 925 |
| `trade_at` | 9459 | 3296 |
| `typed_periods_parse_strictly_and_format_back` | 10006 | 3418 |
| `win_rate` | 908 | 613 |

## Nothing of the report is left behind

```
$ grep -nE 'draw_report|draw_trade_list|draw_equity_curve|draw_trades_tab|draw_ledger|REPORT_|CALENDAR_|CURVE_|TILE_' crates/app/src/paper_trading.rs
5038:    pub fn draw_trades_tab(&mut self, ui: &mut egui::Ui, tz: TzOffset) -> Option<LedgerAction> {
5040:        self.report.draw_trades_tab(ui, tz, &env)
5044:    pub fn draw_report_window(&mut self, ctx: &egui::Context, tz: TzOffset) {
5054:    /// Open the report window (`QUANTICK_PAPER_REPORT_AUTOSTART`).
5082:    /// Open or collapse the report's trade list (`QUANTICK_PAPER_REPORT_LIST`).
9767:        // What the first `draw_trades_tab` does before painting a row.
```

Two wrapper signatures, three doc mentions and one test comment. **No
report or ledger body remains**, and not one `REPORT_*` / `CALENDAR_*` /
`CURVE_*` / `TILE_*` constant.

## Five wrappers that were not kept

`set_day_collapsed`, `toggle_all_days`, `pick_report_dates`,
`show_report_month` and `report_snapshot` had no caller outside the report
- on `origin/main` they were reached only from inside the code that just
moved, and from tests. A wrapper with no caller is not an API, so they are
`ReportState` methods now and nothing outside lost a name it was using.

`report_snapshot` deserves a word, because it is the operator's read. Its
one production caller was `ensure_report_view`, which logs the
`PAPER_REPORT_CUT` line - and that call, that log line and its fields moved
together and are unchanged. Nothing an operator could reach before is
unreachable now.

## What `paper_trading.rs` still holds

The money path, and only it - the sections that remain, in file order:

- **Chart layer** - the paper lines, tags, brackets and drag handles.
- **Dock tab** - the order ticket: quantity, type, offsets, strategies,
  the ruler, the risk block.
- **Report and ledger** - the wrappers above, `report_env!`, `open_row`,
  and the test-only seam. Nothing that draws.
- **End of frame** - `settle`, the toast outbox.
- **Import** / **Export** - the folder picker and the CSV writer. These
  stayed deliberately: they reach `venue`, `symbol`,
  `session_trade_sources` and `dir`, so moving them would widen four
  private items to a sibling module - a number in the ratchet traded for a
  hole in the type's encapsulation.
- **Events, journal, parsing** - the funnel every simulator event goes
  through, and the journal writer.

That is the starting point for the next extraction. On the request's own
count the two obvious candidates are risk sizing (21 methods, and
`risk_sizing.rs` already exists) and the cmd/bracket/drag path (32
methods, the hot path). Both were named non-goals here.
