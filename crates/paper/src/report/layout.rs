//! Shared report layout reference dimensions, in logical pixels.
/// Fixed ledger row height — two lines plus their padding, held constant so
/// the trade list can virtualise through `ScrollArea::show_rows`.
pub const LEDGER_ROW_HEIGHT_PX: f32 = 34.0;

/// Height reserved under the ledger for the pinned totals strip.
pub const TOTALS_STRIP_PX: f32 = 26.0;

/// Saved trades one ledger page reveals. A folder holding a year of
/// sessions must not paint a year of rows to show today's; the "show
/// older" control adds another page and says how many are left.
pub use super::ledger_rows::LEDGER_PAGE_TRADES;

/// The ledger's instrument picker. Wide enough for "This chart · BTCUSDT"
/// without pushing the refresh and fold controls off the row.
pub const LEDGER_SCOPE_COMBO_PX: f32 = 150.0;

/// Gap between a detail line and the date stamp anchored opposite it.
pub const DETAIL_GAP_PX: f32 = 8.0;

/// Right margin the detail line and its stamp both respect.
pub const DETAIL_RIGHT_PAD_PX: f32 = 6.0;

/// How far a day header's tinted band sits below the row's top edge — the
/// gap is what separates one day's block from the previous day's last row.
pub const DAY_HEADER_INSET_PX: f32 = 6.0;

/// Where a day header's date starts, clear of its fold caret.
pub const DAY_HEADER_TEXT_X_PX: f32 = 20.0;
/// The report's floor while the month grid is expanded: the grid's own
/// six rows plus a weekday rule, on top of what the collapsed report had
/// to fit. Kept under the app's own 560 px minimum height so the report
/// can never be taller than the window it lives in.
pub const REPORT_MIN_HEIGHT_CALENDAR_PX: f32 = 540.0;

/// The size the report opens at. The trade list is eleven columns wide and
/// the curve wants room above it; opening cramped and making the trader
/// drag the corner every session is not a default.
pub const REPORT_DEFAULT_W_PX: f32 = 900.0;
/// See [`REPORT_DEFAULT_W_PX`].
pub const REPORT_DEFAULT_H_PX: f32 = 720.0;

/// The floor the equity curve shrinks to while the month grid is open. The
/// curve is a shape to glance at; the list under it is the answer to
/// "which trades", so when the two compete for a short window the curve is
/// what gives way.
pub const CURVE_MIN_H_CALENDAR_PX: f32 = 104.0;

/// One calendar day cell. Seven of them plus the report's own margins fit
/// inside `REPORT_MIN_WIDTH_PX`, so the grid never forces the window wider
/// than the trader sized it.
pub const CALENDAR_CELL_W_PX: f32 = 34.0;

/// See [`CALENDAR_CELL_W_PX`]. Tall enough for the day number and the
/// trade count under it.
pub const CALENDAR_CELL_H_PX: f32 = 28.0;

/// The tallest the report's trade list grows before it scrolls inside
/// itself. Bounded so a thousand-trade window still leaves the metric
/// grids under it reachable by scrolling the page rather than the list.
pub const REPORT_LIST_MAX_H_PX: f32 = 220.0;
/// Headline tile geometry: a caption, a 22 px value, a sub-line.
pub const TILE_HEIGHT_PX: f32 = 62.0;
/// Gap between headline tiles.
pub const TILE_GUTTER_PX: f32 = 8.0;
/// The report's hero numbers — the one type size above 16 in the app.
pub const HEADLINE_FONT_PX: f32 = 22.0;
/// The equity curve's readable floor.
pub const CURVE_MIN_H_PX: f32 = 160.0;
/// Stops the curve from eating a resized window.
pub const CURVE_MAX_H_PX: f32 = 240.0;
/// Alpha of the equity area fill — ground under the line, not a mark.
pub const CURVE_FILL_ALPHA: u8 = 31;
/// Alpha of the equity curve's gridlines — the chart's own grid token at
/// half strength, recessive under one quiet line.
pub const CURVE_GRID_LINE_ALPHA: u8 = 128;
/// At most this many curve points are drawn; the *drawing* downsamples
/// past it (and says so), the metrics always use every trade.
pub const CURVE_MAX_POINTS: usize = 1000;
/// Space kept under the curve for the metric grids before the curve
/// height clamps.
pub const CURVE_GRID_RESERVE_PX: f32 = 230.0;
/// The report window's smallest usable size — everything scrolls or
/// clamps below its defaults, so the window resizes freely down to this.
pub const REPORT_MIN_WIDTH_PX: f32 = 440.0;
/// See [`REPORT_MIN_WIDTH_PX`].
pub const REPORT_MIN_HEIGHT_PX: f32 = 360.0;
/// Height the report keeps for its honesty footer under the grids.
pub const REPORT_FOOTER_RESERVE_PX: f32 = 64.0;
/// Width of the typed-period field beside the pills — room for "999h".
pub const CUSTOM_PERIOD_FIELD_PX: f32 = 44.0;
/// The grids' readable floor inside a squeezed window.
pub const REPORT_GRID_MIN_H_PX: f32 = 80.0;
/// Width of the equity curve's y-tick gutter.
pub const CURVE_GUTTER_PX: f32 = 52.0;

pub const TRADE_LIST_COLUMNS: [(&str, f32); 11] = [
    ("#", 38.0),
    ("DATE", 70.0),
    ("TIME", 58.0),
    ("SYMBOL", 66.0),
    ("SIDE", 56.0),
    ("ENTRY → EXIT", 122.0),
    ("HELD", 52.0),
    ("EXIT", 74.0),
    ("P&L", 196.0),
    ("EQUITY", 96.0),
    // Not a hover: under Source "Both" a practice trade and a real one
    // are otherwise the same row, and a replay result readable as a real
    // one is the worst thing this window could do.
    ("SOURCE", 58.0),
];
