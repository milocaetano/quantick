//! Paper trading: the app-side host of the deterministic `quantick-sim`
//! simulator. UX contract: `docs/ux/paper-trading.md`.
//!
//! Ownership rules this module enforces:
//!
//! - Simulated order state lives *here*, never in the drawings overlay — a
//!   bar-spec change clears annotations, but orders belong to the session.
//! - The simulator taps the exact per-trade ingestion point the bar engine
//!   uses, so live feeds and replay behave identically.
//! - Closed trades are journaled to the history folder the moment they
//!   close, one self-contained CSV row per trade (`quantick_sim::history`);
//!   nothing else survives a session, and every surface says "SIM".

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::{Side, Trade};
use quantick_sim::{
    Bracket, ClosedTrade, Command, EntryKind, OrderId, PerformanceReport, Position, QueuedAction,
    SideReport, SimEvent, Simulator, history,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::chart::PriceScale;
use crate::theme;
use crate::timezone::TzOffset;

/// `=1` runs a fixed sequence of ordinary sim commands driven by print
/// count, so a screenshot or demo run shows every trading surface without
/// a click — the autostart family's member for paper trading. The trades
/// are as real as any simulated trade (journaled, listed, painted); point
/// `QUANTICK_TRADES_DIR` somewhere scratch to keep a demo out of your
/// journal.
const PAPER_DEMO_ENV: &str = "QUANTICK_PAPER_DEMO";
/// `=<rungs>` rests entry orders around the mark as soon as the tape has
/// one, so the in-plot order tag can be photographed at all. The scripted
/// demo's own order is 220 prints away and sits 0.4 % out — far enough to
/// fall outside an autoscaled price range, and close enough that a lively
/// tape fills it before the shutter. Each rung is a **buy limit below and
/// a sell limit above**: a move in either direction can fill only one side
/// of it, so a resting tag always survives on screen.
const PAPER_ORDERS_ENV: &str = "QUANTICK_PAPER_ORDERS";
/// How far the first rung sits from the mark, as a fraction of it. Small
/// on purpose: a line outside the chart's autoscaled price range paints no
/// tag, so an order that cannot be reached also cannot be seen.
const PAPER_ORDERS_STEP_FRACTION: Decimal = Decimal::from_parts(6, 0, 0, false, 4);
/// Rungs past this are refused — a capture wants a tag or two, not a book.
const PAPER_ORDERS_MAX_RUNGS: u8 = 4;
/// How long a paper toast stays on screen.
const TOAST_MS: u64 = 4_000;
/// Grab distance for order lines — the drawings' select radius, so the two
/// grammars feel identical under the pointer.
const LINE_GRAB_RADIUS_PX: f32 = 10.0;
/// Dash geometry of a pending order's line (the last-price line's rhythm).
const ORDER_DASH_PX: f32 = 4.0;
/// Gap between dashes of a pending order's line.
const ORDER_GAP_PX: f32 = 4.0;
/// How far above the bottom chrome the paper toast floats — clear of the
/// drawings toast so the two never overlap.
const TOAST_LIFT_PX: f32 = 96.0;
/// Price precision for snapped drags before any print reveals the
/// instrument's own (two decimals, the crypto-major default).
const SNAP_FALLBACK_DECIMALS: u32 = 2;
/// The position's entry line leads the paper lines: it is the one that is
/// history rather than an order, and it matches the drawings' default width.
const POSITION_LINE_WIDTH_PX: f32 = 1.5;
/// Resting width of every other paper line (orders, stop, target).
const LINE_WIDTH_PX: f32 = 1.0;
/// Width of a paper line while the pointer is within grab range — the same
/// emphasis step the drawings use.
const LINE_HOVER_WIDTH_PX: f32 = 1.5;
/// Width of a paper line while it is being dragged.
const LINE_DRAG_WIDTH_PX: f32 = 2.0;
/// The drawings' selection-halo treatment, mirrored for a dragged paper
/// line so a grabbed stop feels identical to a grabbed drawing (the
/// originals are private to `drawings`).
const DRAG_HALO_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(40, 40, 40, 40);
/// How much wider than the line the halo pass paints.
const DRAG_HALO_EXTRA_WIDTH_PX: f32 = 3.5;
/// Height of an in-plot tag (fits mono 11 plus its padding).
const TAG_HEIGHT_PX: f32 = 20.0;
/// Gap between a tag's right edge and the plot's right edge — the inside
/// mirror of the gutter chips' `AXIS_LABEL_GAP_PX`.
const TAG_GAP_PX: f32 = 6.0;
/// Horizontal padding inside a tag.
const TAG_PAD_X: f32 = 6.0;
/// Width of the ✕ zone a hovered tag reveals. An overlay convenience —
/// every action here has a ≥ 28 px twin in the chrome.
const TAG_BUTTON_PX: f32 = 20.0;
/// How far around a tag the hover that reveals the bracket handles still
/// counts.
const TAG_HOVER_SLACK_PX: f32 = 4.0;
/// Alpha of the ink hairline between a chip tag's ✕ zone and its words.
const CLOSE_DIVIDER_ALPHA: u8 = 90;
/// Size of a labelled SL/TP bracket handle on the entry line.
const HANDLE_SIZE: egui::Vec2 = egui::vec2(20.0, 14.0);
/// Vertical clearance between the entry line and a bracket handle — past
/// the tag's half height, so handle and tag never overlap.
const HANDLE_CLEAR_PX: f32 = 12.0;
/// How far (in pixels) a press on the entry line must travel before it
/// commits to creating one bracket leg — the drawings' drag threshold.
const CREATE_DECIDE_THRESHOLD_PX: f32 = 4.0;
/// Fixed ledger row height — two lines plus their padding, held constant so
/// the trade list can virtualise through `ScrollArea::show_rows`.
const LEDGER_ROW_HEIGHT_PX: f32 = 34.0;
/// The side rail on ledger rows — the position HUD card's rail width.
const SIDE_RAIL_WIDTH_PX: f32 = 3.0;
/// Height reserved under the ledger for the pinned totals strip.
const TOTALS_STRIP_PX: f32 = 26.0;
/// Headline tile geometry: a caption, a 22 px value, a sub-line.
const TILE_HEIGHT_PX: f32 = 62.0;
/// Gap between headline tiles.
const TILE_GUTTER_PX: f32 = 8.0;
/// The report's hero numbers — the one type size above 16 in the app.
const HEADLINE_FONT_PX: f32 = 22.0;
/// The equity curve's readable floor.
const CURVE_MIN_H_PX: f32 = 160.0;
/// Stops the curve from eating a resized window.
const CURVE_MAX_H_PX: f32 = 240.0;
/// Alpha of the equity area fill — ground under the line, not a mark.
const CURVE_FILL_ALPHA: u8 = 31;
/// Alpha of the equity curve's gridlines — the chart's own grid token at
/// half strength, recessive under one quiet line.
const CURVE_GRID_LINE_ALPHA: u8 = 128;
/// At most this many curve points are drawn; the *drawing* downsamples
/// past it (and says so), the metrics always use every trade.
const CURVE_MAX_POINTS: usize = 1000;
/// Space kept under the curve for the metric grids before the curve
/// height clamps.
const CURVE_GRID_RESERVE_PX: f32 = 230.0;
/// The report window's smallest usable size — everything scrolls or
/// clamps below its defaults, so the window resizes freely down to this.
const REPORT_MIN_WIDTH_PX: f32 = 440.0;
/// See [`REPORT_MIN_WIDTH_PX`].
const REPORT_MIN_HEIGHT_PX: f32 = 360.0;
/// Height the report keeps for its honesty footer under the grids.
const REPORT_FOOTER_RESERVE_PX: f32 = 64.0;
/// Width of the typed-period field beside the pills — room for "999h".
const CUSTOM_PERIOD_FIELD_PX: f32 = 44.0;
/// How many `.rerun-N` session names one venue-time stamp may try — far
/// beyond any real journal, a backstop against a pathological folder.
const MAX_SESSION_RERUNS: usize = 999;
/// `=buy`/`=sell` forces the cmd-trading preview for a capture run, and an
/// optional `@<fraction>` parks the virtual pointer at that fraction of the
/// band's width (`buy@0.15` aims near the left edge). The held modifier and
/// the hand that moves the mouse are the two inputs a run with nobody at
/// the keyboard cannot supply (the ParkedHand rule) — and now that the
/// label rides the pointer, its x is a state of its own to capture.
const CMD_PREVIEW_ENV: &str = "QUANTICK_CMD_PREVIEW";
/// Forces every resting order's in-plot tag to its expanded form for a
/// capture run — the same ParkedHand problem: the compact pill opens under
/// a pointer no scripted run has.
const PAPER_ORDER_HOVER_ENV: &str = "QUANTICK_PAPER_ORDER_HOVER";
/// Shortest cmd-trading preview line: the pointer near the right edge
/// still gets a line long enough to read as one, by starting left of it.
const CMD_LINE_MIN_PX: f32 = 120.0;
/// Most dash segments the aim line is allowed to paint. It now runs from
/// the pointer all the way to the axis, which ties the label beside the
/// hand to the price on the gutter — but on a maximised chart that is
/// thousands of pixels, and `Shape::dashed_line` allocates one segment per
/// dash *every frame the modifier is held*. Past this the dash period
/// stretches instead, so the cost is bounded and the rhythm still reads.
const CMD_LINE_MAX_DASHES: f32 = 96.0;
/// The preview label's fixed width: paint and press share this exact
/// rect, so the two can never disagree (the overlay-controls rule).
const CMD_LABEL_WIDTH_PX: f32 = 116.0;
/// Clear space between the pointer and the label riding beside it. The
/// label must not sit under the crosshair it belongs to — the cursor and
/// the candle beneath it stay readable — while staying close enough to
/// read as one statement with the aim.
const CMD_LABEL_CURSOR_GAP_PX: f32 = 14.0;
/// The grids' readable floor inside a squeezed window.
const REPORT_GRID_MIN_H_PX: f32 = 80.0;
/// Width of the equity curve's y-tick gutter.
const CURVE_GUTTER_PX: f32 = 52.0;
/// Longest export path a toast prints whole; past it the folder elides
/// and the file name stays.
const EXPORT_PATH_ELIDE_CHARS: usize = 64;
/// Vertical clearance between a paper chip's centre and the last-price
/// chip's, in pixels — just over one chip height, so the two can never
/// overprint. At the instant a market order fills, the entry price *is* the
/// last price, and without this the one persistent "you are long" statement
/// is born unreadable.
const CHIP_CLEAR_PX: f32 = 16.0;

/// The next chart click places this entry (`Limit` or `Stop` only — a
/// market order needs no price and fires straight from its button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmedPlacement {
    side: Side,
    kind: EntryKind,
}

/// The open position, read-only, as every chrome surface reports it — the
/// HUD, the dock badge and the status cell all describe the same trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionSummary {
    /// Which way the position points.
    pub side: Side,
    /// Contracts/units held.
    pub quantity: Decimal,
    /// Average entry price.
    pub avg_price: Decimal,
    /// Open profit at the current mark; `None` before any mark exists.
    pub open_points: Option<Decimal>,
}

/// Which simulated line the pointer is dragging.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PaperDrag {
    #[default]
    None,
    StopLoss,
    TakeProfit,
    Order(OrderId),
    /// The press landed on the entry line: an average entry is history, not
    /// an order, so the geometry stays put — but the gesture still belongs
    /// to the line (the chart must not pan under it). This is the state for
    /// a fully bracketed position, whose legs are their own handles.
    Blocked,
    /// The press landed on the entry line and at least one bracket leg is
    /// missing: the first committed pull decides which leg the drag creates
    /// (profit side → take profit, losing side → stop loss).
    CreatePending,
    /// Dragging a stop loss into existence from the entry line or its
    /// handle; release submits it, exactly like repricing an existing one.
    CreateStopLoss,
    /// Dragging a take profit into existence (see `CreateStopLoss`).
    CreateTakeProfit,
}

/// A painted overlay control from the last paint pass: a tag's ✕ or a
/// bracket handle. Hit rects are cached one frame behind the paint — the
/// input pass runs before the draw, and an immediate-mode overlay control is
/// pressed against where it was actually painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperControl {
    /// ✕ on the position tag: exit at the next print.
    ClosePosition,
    /// ✕ on the stop-loss tag: clear the protective stop.
    ClearStopLoss,
    /// ✕ on the take-profit tag: clear the profit target.
    ClearTakeProfit,
    /// ✕ on a working order's tag: cancel it.
    CancelOrder(OrderId),
    /// Labelled `SL` handle on the entry line: press starts a create-drag.
    HandleStopLoss,
    /// Labelled `TP` handle on the entry line.
    HandleTakeProfit,
}

/// One transient message; rejections double as the tutorial.
struct Toast {
    message: String,
    shown_at: Instant,
}

/// The scripted demo's only state: how many prints it has seen.
struct PaperDemo {
    prints: u64,
}

/// Report scope: one symbol's history or the whole folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportScope {
    Symbol,
    All,
}

/// The report's period filter, measured back from the newest saved trade
/// in scope — never from a wall clock. The engine has no clock, and a
/// replayed session's trades may be years old; a wall-clock "7 days" would
/// report a perfectly good replay as empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportPeriod {
    Today,
    Week,
    Month,
    Quarter,
    All,
    /// A typed span (`2d`, `12h`…) in milliseconds back from the anchor —
    /// the text box beside the pills.
    Custom(i64),
}

impl ReportPeriod {
    /// Every fixed period, in pill order; `Custom` rides the text box.
    const PILLS: [Self; 5] = [
        Self::Today,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::All,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Quarter => "90d",
            Self::All => "All",
            Self::Custom(_) => "custom",
        }
    }

    /// The anchor-relative phrase the support line reads.
    fn phrase(self) -> String {
        match self {
            Self::Today => "the newest trade's day".to_owned(),
            Self::Week => "the last 7 days".to_owned(),
            Self::Month => "the last 30 days".to_owned(),
            Self::Quarter => "the last 90 days".to_owned(),
            Self::All => "everything saved".to_owned(),
            Self::Custom(period_ms) => format!("the last {}", fmt_period_ms(period_ms)),
        }
    }

    /// Oldest closing time still inside the period, measured back from
    /// `anchor_ms`; `None` keeps everything. "Today" is the anchor's civil
    /// day in the chart's display timezone — the ledger renders local
    /// times, so the day must break where the user sees midnight, not
    /// where UTC does.
    fn cutoff_ms(self, anchor_ms: i64, tz: TzOffset) -> Option<i64> {
        const DAY_MS: i64 = 86_400_000;
        match self {
            Self::Today => {
                let local = anchor_ms.saturating_add(tz.offset_ms());
                let local_day_start = local.div_euclid(DAY_MS) * DAY_MS;
                Some(local_day_start.saturating_sub(tz.offset_ms()))
            }
            Self::Week => Some(anchor_ms.saturating_sub(7 * DAY_MS)),
            Self::Month => Some(anchor_ms.saturating_sub(30 * DAY_MS)),
            Self::Quarter => Some(anchor_ms.saturating_sub(90 * DAY_MS)),
            Self::All => None,
            Self::Custom(period_ms) => Some(anchor_ms.saturating_sub(period_ms)),
        }
    }
}

/// Parse a typed period: a positive whole number and a unit — `m`
/// (minutes), `h`, `d`, `w` — any case, blanks tolerated. `None` is a
/// refusal the caller must say out loud: a silently-empty report is
/// exactly the confusion the typed field exists to end.
fn parse_period(text: &str) -> Option<i64> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let count = text.get(..text.len() - unit.len_utf8())?.trim_end();
    let count: i64 = count.parse().ok()?;
    if count <= 0 {
        return None;
    }
    let unit_ms: i64 = match unit.to_ascii_lowercase() {
        'm' => 60_000,
        'h' => 3_600_000,
        'd' => 86_400_000,
        'w' => 7 * 86_400_000,
        _ => return None,
    };
    count.checked_mul(unit_ms)
}

/// `45m`, `36h`, `2d`, `1w` — the canonical spelling of a custom period,
/// largest whole unit first.
fn fmt_period_ms(period_ms: i64) -> String {
    const MINUTE_MS: i64 = 60_000;
    const HOUR_MS: i64 = 3_600_000;
    const DAY_MS: i64 = 86_400_000;
    const WEEK_MS: i64 = 7 * DAY_MS;
    let (value, unit) = if period_ms % WEEK_MS == 0 {
        (period_ms / WEEK_MS, 'w')
    } else if period_ms % DAY_MS == 0 {
        (period_ms / DAY_MS, 'd')
    } else if period_ms % HOUR_MS == 0 {
        (period_ms / HOUR_MS, 'h')
    } else {
        (period_ms / MINUTE_MS, 'm')
    };
    format!("{value}{unit}")
}

/// The report as filtered for display: the period's trades in closing
/// order, their aggregation, and the anchor the period was measured from.
struct ReportView {
    period: ReportPeriod,
    source: SourceFilter,
    /// The display timezone the view was cut with — "Today" moves with it.
    tz: TzOffset,
    /// Newest closing time in scope — what the period counts back from.
    anchor_ms: Option<i64>,
    /// Saved trades older than the window — the honest answer to "where
    /// did my old trades go": they exist, the period just stops short.
    hidden_before: usize,
    /// Saved trades the Source filter keeps out of this view.
    hidden_by_source: usize,
    trades: Vec<ClosedTrade>,
    report: PerformanceReport,
}

/// One journal row loaded from disk: the trade, the symbol folder it came
/// from, and the session source its file recorded.
#[derive(Clone)]
struct HistoryRow {
    symbol: String,
    /// `None` — a file from before the source was recorded. The report's
    /// Real view includes it: that era *was* live trading, and hiding it
    /// would "lose" the user's history all over again.
    source: Option<history::SessionSource>,
    trade: ClosedTrade,
}

/// Journal rows loaded from disk, each remembering the symbol folder it
/// came from, merged into one closing-order timeline.
struct LoadedHistory {
    /// Rows in closing order across every file read.
    rows: Vec<HistoryRow>,
    files: usize,
    /// Files that were not readable quantick-trades files.
    unreadable_files: usize,
    /// Rows the parser had to report as unreadable (torn tails and such).
    problem_rows: usize,
}

/// The report's session-source filter. Default `Real`: practice runs must
/// never inflate the real track record unasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceFilter {
    /// Live sessions, plus files from before the source was recorded.
    Real,
    /// Replay-driven practice sessions only.
    Replay,
    /// Everything, mixed.
    All,
}

impl SourceFilter {
    /// Every filter, in pill order.
    const PILLS: [Self; 3] = [Self::Real, Self::Replay, Self::All];

    fn label(self) -> &'static str {
        match self {
            Self::Real => "Real",
            Self::Replay => "Replay",
            // Not "All": the period pills own that word on the same row,
            // and two identical pills a hand-width apart invite the wrong
            // click.
            Self::All => "Both",
        }
    }

    fn hover(self) -> &'static str {
        match self {
            Self::Real => {
                "live sessions - files saved before quantick recorded a source count as real"
            }
            Self::Replay => "practice sessions driven by a market-replay recording",
            Self::All => "live and replay together - mixed on purpose",
        }
    }

    /// Whether a row with this recorded source belongs to the filter.
    fn admits(self, source: Option<history::SessionSource>) -> bool {
        match self {
            // Exhaustive on purpose: a future source variant must not fall
            // into the real track record by default — adding one forces
            // this match to say where it belongs.
            Self::Real => match source {
                None | Some(history::SessionSource::Live) => true,
                Some(history::SessionSource::Replay) => false,
            },
            Self::Replay => source == Some(history::SessionSource::Replay),
            Self::All => true,
        }
    }
}

/// What the Trading tab asked of its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingTabAction {
    /// Open the folder picker for where trades are saved — the choice is
    /// app-wide (every tab journals there) and remembered, so the app
    /// owns the dialog and the fan-out.
    PickTradesDir,
    /// Cmd-trading settings changed — app-wide like the trades dir: the
    /// app persists them and fans them out to every tab.
    CmdTradingChanged,
}

/// What the Trades ledger asked of its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerAction {
    /// Center the chart on this round trip (`opened_ms`, `closed_ms`).
    Navigate(i64, i64),
    /// Switch the dock to the Trading tab — the empty state's call to
    /// action.
    OpenTicket,
}

/// One virtualised ledger line.
enum LedgerRow<'a> {
    /// A group caption and its count.
    Header(&'static str, usize),
    /// A closed trade from this session's simulator: selectable, and its
    /// round trip is on the current tape.
    Session(usize, &'a ClosedTrade),
    /// A row loaded from an earlier session's journal — display only; its
    /// tape is not the one on screen.
    Earlier(&'a str, &'a ClosedTrade),
}

/// What one painted ledger row reported back.
#[derive(Default)]
struct LedgerRowResponse {
    clicked: bool,
    navigate: bool,
}

/// Everything `handle_chart_input` needs from the frame, gathered by the
/// caller so this module never reads raw input state itself.
pub struct ChartInput<'a> {
    pub chart: egui::Rect,
    pub scale: Option<&'a PriceScale>,
    pub pointer: Option<egui::Pos2>,
    pub primary_pressed: bool,
    pub primary_down: bool,
    pub primary_released: bool,
    /// The frame's held modifiers — what the cmd-trading gesture reads.
    pub modifiers: egui::Modifiers,
    /// Whether something the pane owns already holds this pixel — a
    /// drawing a press would grab, or the canvas's own chrome (the tape
    /// chip, an indicator pane's header or its divider). The pane answers
    /// it, the same way it answers "a tool is armed": this module never
    /// reads the drawings itself. The aim-anywhere cmd gesture is the
    /// *last* claimant on the canvas, so it paints nothing and places
    /// nothing there; the buy modifier is Shift by default, which is also
    /// the key that levels a channel corner mid-drag.
    pub canvas_claimed: bool,
    /// Whether the paper layer is painted this frame. Switched off, its
    /// lines and tags are unpainted — so they take no press either. An
    /// invisible control is not a control, and the aim's target is now the
    /// whole plot rather than one small label.
    pub layer_visible: bool,
}

/// A modifier key the cmd-trading gesture can bind to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdModifier {
    Shift,
    Ctrl,
    Alt,
}

impl CmdModifier {
    /// Every binding the selectors offer.
    pub const ALL: [Self; 3] = [Self::Shift, Self::Ctrl, Self::Alt];

    /// Stable token for the state file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
        }
    }

    /// The inverse of [`Self::as_str`]; unknown tokens are refused.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "shift" => Some(Self::Shift),
            "ctrl" => Some(Self::Ctrl),
            "alt" => Some(Self::Alt),
            _ => None,
        }
    }

    /// Display label for the selector.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
        }
    }

    /// Whether the key is held. `Ctrl` reads the platform command key, so
    /// the binding keeps meaning "the control-ish key" on every OS.
    fn is_down(self, modifiers: egui::Modifiers) -> bool {
        match self {
            Self::Shift => modifiers.shift,
            Self::Ctrl => modifiers.command,
            Self::Alt => modifiers.alt,
        }
    }
}

/// Cmd trading: hold a key over the chart and a dashed line shows exactly
/// where the order will rest, with a label riding beside the cursor; the
/// click places it. Safer than the right-click menu because the price is
/// visible before anything commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmdTradingSettings {
    pub enabled: bool,
    pub buy: CmdModifier,
    pub sell: CmdModifier,
}

impl Default for CmdTradingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            buy: CmdModifier::Shift,
            sell: CmdModifier::Ctrl,
        }
    }
}

impl CmdTradingSettings {
    /// The settings the sidecar remembers, with the defaults wherever it
    /// never spoke — or spoke a token this build does not know.
    #[must_use]
    pub(crate) fn from_state(state: &crate::paper_state::PaperState) -> Self {
        let defaults = Self::default();
        Self {
            enabled: state.cmd_trading_enabled.unwrap_or(defaults.enabled),
            buy: state
                .cmd_buy_modifier
                .as_deref()
                .and_then(CmdModifier::parse)
                .unwrap_or(defaults.buy),
            sell: state
                .cmd_sell_modifier
                .as_deref()
                .and_then(CmdModifier::parse)
                .unwrap_or(defaults.sell),
        }
    }
}

/// The frame's cmd-trading preview: computed by `handle_chart_input`,
/// painted by `draw_layer`, clicked through the same geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CmdPreview {
    side: Side,
    kind: EntryKind,
    /// Snapped price, for the label and the gutter chip.
    price: Decimal,
    /// Raw pointer price — what a click hands to `place_resting`, which
    /// snaps for itself (the armed-click path's contract).
    raw_price: f64,
    /// The aiming pointer, both coordinates: y is the price, x is where
    /// the label rides. Stored whole so paint and press lay out from the
    /// very same position.
    pointer: egui::Pos2,
    /// This aim was invented by the capture hook, not by a held key. It
    /// paints, so a screenshot has something to show, and it never places:
    /// a run with nobody at the keyboard is holding no modifier, and a
    /// stray click during one must not write orders into a journal.
    forced: bool,
}

/// One frame's answer for one working order's in-plot tag: computed by
/// `handle_chart_input`, read by the paint *and* by the press.
///
/// A shared **value**, not a shared formula. The two sides are handed
/// different pointers (`hover_pos` for the paint, `latest_pos` for the
/// press) and different rects (the whole chart vs. the band left of the
/// tape lane), so asking them to recompute the same predicate is asking
/// them to disagree — and a ✕ that one side paints and the other side does
/// not is a cancel the trader never saw coming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenTag {
    id: OrderId,
    /// The ✕ is painted with the full statement, so a press may act on it.
    /// False while the order is being dragged: a moving order offers no
    /// cancel, and its tag is on a different row from its resting price.
    cancel: bool,
}

/// The `QUANTICK_CMD_PREVIEW` hook, parsed: which side to aim and, when
/// stated, where along the band to park the virtual pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CmdPreviewForce {
    side: Side,
    /// 0.0 is the band's left edge, 1.0 its right; `None` leaves the
    /// pointer mid-band, which is what the hook meant before the label
    /// followed it.
    x_fraction: Option<f32>,
}

impl CmdPreviewForce {
    /// `buy`, `sell`, `buy@0.15`. An unparseable fraction degrades to the
    /// mid-band park rather than killing the whole preview — a capture run
    /// that paints nothing is the hardest failure to read.
    fn parse(value: &str) -> Option<Self> {
        let (side, fraction) = match value.split_once('@') {
            Some((side, fraction)) => (side, Some(fraction)),
            None => (value, None),
        };
        let side = match side.trim().to_ascii_lowercase().as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return None,
        };
        Some(Self {
            side,
            x_fraction: fraction
                .and_then(|text| text.trim().parse::<f32>().ok())
                // `"NaN"` and `"inf"` parse, and `clamp` passes NaN
                // straight through — which would poison the pointer's x
                // and paint nothing at all, the one outcome this fallback
                // exists to rule out.
                .filter(|fraction: &f32| fraction.is_finite())
                .map(|fraction| fraction.clamp(0.0, 1.0)),
        })
    }
}

/// The app-side paper-trading host: simulator, order-entry form state,
/// chart-layer interaction, journal and report.
pub struct PaperTrading {
    sim: Simulator,
    /// Symbol the journal writes under; follows the app's active symbol.
    symbol: String,
    dir: PathBuf,
    /// Current session file, named after the first closed trade.
    journal_path: Option<PathBuf>,
    /// Every file this host has journaled to. The ledger excludes them
    /// all — their trades are still in the simulator, which keeps closed
    /// trades across every retarget; excluding only the current file
    /// double-counted after a symbol or source switch.
    session_journal_paths: Vec<PathBuf>,
    /// The session source each of the simulator's closed trades closed
    /// under, index-aligned with `sim.closed_trades()` — the export must
    /// not stamp a pre-switch trade with the current source.
    session_trade_sources: Vec<history::SessionSource>,
    /// A failed journal write warns once, not once per trade.
    journal_warned: bool,
    /// Where this session's trades come from — the tab's feed sets it,
    /// the journal header records it.
    session_source: history::SessionSource,
    /// Cmd trading: the toggle and its two key bindings (app-wide; the
    /// app persists and fans out changes).
    cmd_trading: CmdTradingSettings,
    /// This frame's cmd preview — input computes, paint reads, one
    /// geometry both sides.
    cmd_preview: Option<CmdPreview>,
    /// This frame's opened order tags — same contract as `cmd_preview`:
    /// input computes, paint and press both read. Empty is the common
    /// case, so this allocates nothing on an ordinary frame.
    open_tags: Vec<OpenTag>,
    /// Whether this frame paints the paper layer. Same contract again, and
    /// the gate lives *here* so that every reader honours it: an unpainted
    /// line offers no cursor, no control and no press, whoever asks.
    layer_visible: bool,
    /// Harness override: paint the preview for this side, optionally at a
    /// stated x, with nobody at the keyboard (`QUANTICK_CMD_PREVIEW`).
    cmd_preview_force: Option<CmdPreviewForce>,
    /// Harness override: every resting order's tag opens, with nobody at
    /// the mouse (`QUANTICK_PAPER_ORDER_HOVER`).
    order_hover_force: bool,
    /// Harness override: how many rungs of resting orders to place on the
    /// first mark (`QUANTICK_PAPER_ORDERS`); `None` once they are placed.
    orders_demo: Option<u8>,
    // Order-entry form.
    qty_text: String,
    order_type: EntryKind,
    stop_offset_text: String,
    profit_offset_text: String,
    armed: Option<ArmedPlacement>,
    // Chart-layer drag.
    drag: PaperDrag,
    drag_price: Option<f64>,
    /// The working order hovered in the dock this frame — its chart line
    /// lifts, so one hover reads on both surfaces. Cleared after the chart
    /// consumed it (`draw_toast` runs last in the frame).
    hovered_order: Option<OrderId>,
    /// The in-flight export, if any; resolved by `draw_toast`'s poll.
    export_rx: Option<std::sync::mpsc::Receiver<Result<(PathBuf, usize), String>>>,
    /// The in-flight history-folder import, if any; resolved by
    /// `draw_toast`'s poll. Imports copy — the picked folder keeps its
    /// files.
    import_rx: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    toast: Option<Toast>,
    report_open: bool,
    /// Report symbol filter: `None` is every symbol, `Some` one folder.
    report_symbol: Option<String>,
    report_period: ReportPeriod,
    /// The report's session-source scope; opens on `Real`.
    report_source: SourceFilter,
    /// What the typed-period field holds; applied on Enter, kept verbatim
    /// so a refused entry stays visible for fixing.
    report_custom_text: String,
    /// Symbol folders on disk, for the report's combo box.
    report_symbols: Vec<String>,
    /// The report's history in scope, loaded fresh from disk.
    report: Option<LoadedHistory>,
    /// The report filtered to the period — rebuilt when the period or the
    /// loaded history changes.
    report_view: Option<ReportView>,
    /// The scripted demo (`QUANTICK_PAPER_DEMO=1`), for screenshot and
    /// validation runs; `None` in normal use.
    demo: Option<PaperDemo>,
    // Trades ledger.
    ledger_scope: ReportScope,
    /// Earlier sessions' journal rows, read on first draw and on demand —
    /// the live session file is excluded (its trades are already in the
    /// simulator).
    history_cache: Option<LoadedHistory>,
    /// Index into the session's closed trades selected in the ledger; the
    /// chart emphasizes that round trip.
    selected_trade: Option<usize>,
}

impl Default for PaperTrading {
    fn default() -> Self {
        Self::new()
    }
}

impl PaperTrading {
    /// A host on the default journal folder (environment override, then
    /// `paper-trades`) — tests and standalone use. The app itself goes
    /// through [`Self::with_trades_dir`] with the configured folder.
    #[must_use]
    pub fn new() -> Self {
        if cfg!(test) {
            // Tests must never journal into a real documents folder — the
            // same scratch discipline `paper_state::default_path` applies.
            return Self::with_trades_dir(test_scratch_dir());
        }
        Self::with_trades_dir(Self::resolve_trades_dir(None, None))
    }

    /// The journal folder for this run: the environment override wins (an
    /// env var is an explicit request for one run, like every autostart
    /// hook), then `stored` — the folder the user last picked with the
    /// panel's button — then `configured`, the `[paper] trades_dir` key
    /// when the config carries one, and finally the documents home
    /// ([`crate::paper_home::default_trades_dir`]).
    #[must_use]
    pub fn resolve_trades_dir(configured: Option<&str>, stored: Option<&str>) -> PathBuf {
        crate::paper_home::resolve(configured, stored)
    }

    /// A host journaling to `dir`, already resolved from config and
    /// environment.
    #[must_use]
    pub fn with_trades_dir(dir: PathBuf) -> Self {
        Self {
            sim: Simulator::new(),
            symbol: String::new(),
            dir,
            journal_path: None,
            session_journal_paths: Vec::new(),
            session_trade_sources: Vec::new(),
            journal_warned: false,
            session_source: history::SessionSource::Live,
            cmd_trading: CmdTradingSettings::default(),
            cmd_preview: None,
            open_tags: Vec::new(),
            layer_visible: true,
            cmd_preview_force: std::env::var(CMD_PREVIEW_ENV).ok().and_then(|value| {
                CmdPreviewForce::parse(&value).or_else(|| {
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "CMD_PREVIEW_AUTOSTART_UNKNOWN",
                        value = %value,
                        "QUANTICK_CMD_PREVIEW wants `buy` or `sell`, optionally `@<0..1>`"
                    );
                    None
                })
            }),
            order_hover_force: std::env::var(PAPER_ORDER_HOVER_ENV).is_ok_and(|value| value == "1"),
            orders_demo: std::env::var(PAPER_ORDERS_ENV).ok().and_then(|value| {
                value
                    .trim()
                    .parse::<u8>()
                    .ok()
                    .filter(|rungs| (1..=PAPER_ORDERS_MAX_RUNGS).contains(rungs))
                    .or_else(|| {
                        // Refused, never guessed: a typo that silently
                        // photographed an orderless chart would read as a
                        // defect in the thing being photographed.
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "PAPER_ORDERS_AUTOSTART_UNKNOWN",
                            value = %value,
                            max = PAPER_ORDERS_MAX_RUNGS,
                            "QUANTICK_PAPER_ORDERS wants a rung count from 1 to the maximum"
                        );
                        None
                    })
            }),
            qty_text: "1".to_owned(),
            order_type: EntryKind::Market,
            stop_offset_text: String::new(),
            profit_offset_text: String::new(),
            armed: None,
            drag: PaperDrag::None,
            drag_price: None,
            hovered_order: None,
            export_rx: None,
            import_rx: None,
            toast: None,
            report_open: false,
            report_symbol: None,
            report_period: ReportPeriod::All,
            report_source: SourceFilter::Real,
            report_custom_text: String::new(),
            report_symbols: Vec::new(),
            report: None,
            report_view: None,
            demo: std::env::var(PAPER_DEMO_ENV)
                .is_ok_and(|value| value == "1")
                .then_some(PaperDemo { prints: 0 }),
            ledger_scope: ReportScope::Symbol,
            history_cache: None,
            selected_trade: None,
        }
    }

    /// Point the journal at a scratch folder (tests only).
    #[cfg(test)]
    pub(crate) fn redirect_history_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }

    /// Where trades save right now.
    #[must_use]
    pub fn trades_dir(&self) -> &Path {
        &self.dir
    }

    /// Point the journal somewhere new, in-session — the panel's folder
    /// picker. Files already written stay exactly where they are; the next
    /// close opens a new session file under the new folder, and the ledger
    /// and report re-read from the new home.
    pub fn set_trades_dir(&mut self, dir: PathBuf) {
        if self.dir == dir {
            return;
        }
        self.dir = dir;
        self.journal_path = None;
        self.journal_warned = false;
        self.history_cache = None;
        self.report = None;
        self.report_view = None;
        if self.report_open {
            // An open report must answer for the new folder now — cleared
            // caches alone left it claiming "no saved trades" until a
            // manual refresh.
            self.reload_report();
        }
        self.show_toast(format!("SIM: trades now save to {}", elide_path(&self.dir)));
    }

    /// Apply a raw simulator command through the normal event funnel
    /// (tests only) — for arranging sim state the UI would need gestures
    /// to reach.
    #[cfg(test)]
    pub(crate) fn apply_sim_command_for_tests(&mut self, command: Command) {
        let events = self.sim.apply(command);
        self.handle_events(events);
    }

    /// The cmd-trading settings, for the app to persist.
    #[must_use]
    pub fn cmd_trading(&self) -> CmdTradingSettings {
        self.cmd_trading
    }

    /// Install cmd-trading settings — the app's fan-out on boot and on a
    /// change made in any tab (one gesture, one meaning, everywhere).
    pub fn set_cmd_trading(&mut self, settings: CmdTradingSettings) {
        self.cmd_trading = settings;
        if !settings.enabled {
            self.cmd_preview = None;
        }
    }

    /// Drop the frame's preview — the pane calls this when a drawing tool
    /// owns the hand, so a stale line never keeps painting.
    pub fn clear_cmd_preview(&mut self) {
        self.cmd_preview = None;
    }

    /// Follow the tab's feed: a session is wholly live or wholly a
    /// replay, and the journal's header records which. A change retargets
    /// the journal so the next close opens a file honest about its
    /// source.
    pub fn set_session_source(&mut self, source: history::SessionSource) {
        if self.session_source != source {
            self.session_source = source;
            self.journal_path = None;
        }
    }

    /// Follow the app's active symbol. A change retargets the journal; the
    /// simulator itself was already flattened by the timeline reset that
    /// every switch performs.
    pub fn set_symbol(&mut self, symbol: &str) {
        if self.symbol != symbol {
            self.symbol = symbol.to_owned();
            self.journal_path = None;
            self.history_cache = None;
            self.selected_trade = None;
        }
    }

    /// Seed the mark from backfilled history — never fills (look-ahead).
    pub fn seed(&mut self, trade: &Trade) {
        self.sim.seed(trade);
    }

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
        let events = self.sim.on_trade(trade);
        self.handle_events(events);
        if self.orders_demo.is_some() {
            self.rest_capture_orders();
        }
        if self.demo.is_some() {
            self.run_demo_step();
        }
    }

    /// Place the capture run's resting orders, once, as soon as the tape
    /// has a mark to place them around. See [`PAPER_ORDERS_ENV`].
    ///
    /// The rung offsets snap to the instrument's own precision, and never
    /// to nothing: on a coarsely quoted mark 6 bp rounds to zero, both
    /// legs would price *at* the mark, the simulator would refuse every
    /// one of them and the hook would have disarmed itself already — an
    /// empty chart with nothing in the log to explain it, which is exactly
    /// the failure this hook exists to prevent. So the step floors at one
    /// unit of that precision, and the hook stays armed until at least one
    /// order is actually resting.
    fn rest_capture_orders(&mut self) {
        let Some(rungs) = self.orders_demo else {
            return;
        };
        let Some(mark) = self.sim.mark_price() else {
            return;
        };
        let tick = Decimal::ONE
            .checked_div(Decimal::from(10_u64.pow(mark.scale().min(18))))
            .unwrap_or(Decimal::ONE);
        for rung in 1..=u32::from(rungs) {
            let step = (mark * PAPER_ORDERS_STEP_FRACTION * Decimal::from(rung))
                .round_dp(mark.scale())
                .max(tick * Decimal::from(rung));
            for (side, price) in [
                (Side::Buy, mark.saturating_sub(step)),
                (Side::Sell, mark.saturating_add(step)),
            ] {
                let events = self.sim.apply(Command::PlaceLimit {
                    side,
                    quantity: Decimal::ONE,
                    price,
                    bracket: Bracket::none(),
                });
                self.handle_events(events);
            }
        }
        if self.sim.orders().is_empty() {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_ORDERS_HOOK_REJECTED",
                %mark,
                action = "retry_next_print",
                "QUANTICK_PAPER_ORDERS rested nothing around this mark"
            );
            return;
        }
        self.orders_demo = None;
    }

    /// One step of the scripted demo: a fixed command sequence by print
    /// count — an entry, brackets, a partial, a flatten, a resting order,
    /// a short round trip — so every surface has something honest to show.
    fn run_demo_step(&mut self) {
        let Some(demo) = &mut self.demo else { return };
        demo.prints += 1;
        let prints = demo.prints;
        let Some(mark) = self.sim.mark_price() else {
            return;
        };
        // Scale-free distance: 0.2% of the mark, snapped to its precision.
        let offset = (mark * Decimal::new(2, 3)).round_dp(mark.scale());
        let has_position = self.sim.position().is_some();
        let command = match prints {
            5 => Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: Bracket::none(),
            },
            12 => Command::SetBracket {
                stop_loss: Some(mark.saturating_sub(offset)),
                take_profit: Some(mark.saturating_add(offset.saturating_add(offset))),
            },
            80 if has_position => Command::ClosePartial {
                quantity: Decimal::new(5, 1),
            },
            160 => Command::Flatten,
            220 => Command::PlaceLimit {
                side: Side::Buy,
                quantity: Decimal::ONE,
                price: mark.saturating_sub(offset.saturating_add(offset)),
                bracket: Bracket::none(),
            },
            260 => Command::PlaceMarket {
                side: Side::Sell,
                quantity: Decimal::ONE,
                bracket: Bracket::none(),
            },
            340 if has_position => Command::ClosePosition,
            _ => return,
        };
        let events = self.sim.apply(command);
        self.handle_events(events);
    }

    /// The source rebuilt its timeline (replay seek, feed/symbol switch,
    /// restart): pending orders are swept and the position flattens at the
    /// last mark, labeled `reset` — never silently.
    pub fn on_timeline_reset(&mut self) {
        let had_position = self.sim.position().is_some();
        let had_orders = !self.sim.orders().is_empty() || !self.sim.queued().is_empty();
        let events = self.sim.reset();
        let mut all_saved = true;
        for event in &events {
            if let SimEvent::Closed(trade) = event {
                all_saved &= self.journal(&trade.clone());
            }
        }
        // A reset ends the tape session, so it ends the file session too:
        // the next close opens a fresh file (same venue stamp lands as
        // `.rerun-N`). Without this, replaying the same recording again
        // without leaving replay appended run 2 into run 1's file.
        self.journal_path = None;
        self.armed = None;
        self.drag = PaperDrag::None;
        self.drag_price = None;
        if had_position && all_saved {
            self.show_toast(
                "SIM position flattened - the timeline was rebuilt under it.".to_owned(),
            );
        } else if had_orders && all_saved {
            self.show_toast(
                "SIM orders cancelled - the timeline was rebuilt under them.".to_owned(),
            );
        }
    }

    /// Whether the simulator has a price to trade against — the toolbar
    /// buttons disable themselves (with the reason) until this is true.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.sim.mark_price().is_some()
    }

    /// A toolbar/panel market order using the form's quantity and offsets.
    pub fn market(&mut self, side: Side) {
        let Some(quantity) = self.parse_quantity() else {
            return;
        };
        let reference = self.sim.mark_price().unwrap_or_default();
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return;
        };
        let events = self.sim.apply(Command::PlaceMarket {
            side,
            quantity,
            bracket,
        });
        self.handle_events(events);
    }

    /// The status-bar cell, honest about open versus flat: `SIM LONG 1 ·
    /// +2 pts` while a position is open (side, size and its open profit),
    /// `SIM +7 pts · flat` otherwise (the session's realized points). `None`
    /// while the simulator has never been touched.
    #[must_use]
    pub fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        if let Some(position) = self.sim.position() {
            let open = self
                .sim
                .mark_price()
                .map(|mark| position.open_points(mark))
                .unwrap_or_default();
            return Some((
                format!(
                    "SIM {} {} · {} pts",
                    position_word(position.side),
                    fmt_decimal(position.quantity),
                    fmt_signed_points(open),
                ),
                open.cmp(&Decimal::ZERO),
            ));
        }
        let untouched = self.sim.closed_trades().is_empty()
            && self.sim.orders().is_empty()
            && self.sim.queued().is_empty();
        if untouched {
            return None;
        }
        let realized = self.sim.realized_points();
        Some((
            format!("SIM {} pts · flat", fmt_signed_points(realized)),
            realized.cmp(&Decimal::ZERO),
        ))
    }

    /// The open position as the chrome reports it: side, size, entry, and
    /// the open profit at the current mark. `None` while flat.
    #[must_use]
    pub fn position_summary(&self) -> Option<PositionSummary> {
        let position = self.sim.position()?;
        Some(PositionSummary {
            side: position.side,
            quantity: position.quantity,
            avg_price: position.avg_price,
            open_points: self.sim.mark_price().map(|mark| position.open_points(mark)),
        })
    }

    /// Exit the open position at the next print — the toolbar's close
    /// button, the HUD's, and the Trading tab's all funnel here.
    pub fn close_position(&mut self) {
        let events = self.sim.apply(Command::ClosePosition);
        self.handle_events(events);
    }

    /// Close the position and cancel every pending order.
    pub fn flatten(&mut self) {
        let events = self.sim.apply(Command::Flatten);
        self.handle_events(events);
    }

    /// Flip the open position: one market order for twice its size, which
    /// closes it and opens the opposite side at the same quantity. The
    /// form's protective offsets apply to the new entry, exactly as they do
    /// to any market order.
    pub fn reverse_position(&mut self) {
        let Some(position) = self.sim.position().cloned() else {
            return;
        };
        let side = match position.side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let reference = self.sim.mark_price().unwrap_or_default();
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return;
        };
        let events = self.sim.apply(Command::PlaceMarket {
            side,
            quantity: position.quantity.saturating_add(position.quantity),
            bracket,
        });
        self.handle_events(events);
    }

    /// The form quantity, parsed without side effects — the label builders
    /// peek at it every frame and must not toast.
    fn quantity_preview(&self) -> Option<Decimal> {
        match self.qty_text.trim().parse::<Decimal>() {
            Ok(quantity) if quantity > Decimal::ZERO => Some(quantity),
            _ => None,
        }
    }

    /// State-aware entry label: what pressing this side's button would do to
    /// the open position — `SELL 1 (closes)`, `SELL 5 (reverses to short
    /// 4)`. Whether a press closes or flips hangs on a quantity field the
    /// toolbar never shows, so the button itself must say. Falls back to the
    /// bare side word while the form quantity does not parse — the click
    /// will toast the correction.
    #[must_use]
    pub fn entry_label(&self, side: Side) -> String {
        let word = side_word_upper(side);
        let Some(qty) = self.quantity_preview() else {
            return word.to_owned();
        };
        let qty_text = fmt_decimal(qty);
        let Some(position) = self.sim.position() else {
            return format!("{word} {qty_text}");
        };
        if position.side == side {
            return format!(
                "{word} {qty_text} (adds to {})",
                fmt_decimal(position.quantity.saturating_add(qty)),
            );
        }
        match qty.cmp(&position.quantity) {
            std::cmp::Ordering::Less => format!(
                "{word} {qty_text} (closes {qty_text} of {})",
                fmt_decimal(position.quantity),
            ),
            std::cmp::Ordering::Equal => format!("{word} {qty_text} (closes)"),
            std::cmp::Ordering::Greater => format!(
                "{word} {qty_text} (reverses to {} {})",
                match side {
                    Side::Buy => "long",
                    Side::Sell => "short",
                },
                fmt_decimal(qty.saturating_sub(position.quantity)),
            ),
        }
    }

    /// The entry buttons' hover text: the quantity and protective offsets
    /// the press will use, which the toolbar itself has no widgets for.
    #[must_use]
    pub fn entry_hover(&self, side: Side) -> String {
        let quantity = match self.quantity_preview() {
            Some(qty) => format!("quantity {}", fmt_decimal(qty)),
            None => "the quantity is not a positive number".to_owned(),
        };
        let bracket = match (
            parse_offset(&self.stop_offset_text),
            parse_offset(&self.profit_offset_text),
        ) {
            (Ok(None), Ok(None)) => "no protective bracket set".to_owned(),
            (Ok(stop), Ok(profit)) => {
                let mut parts = Vec::new();
                if let Some(stop) = stop {
                    parts.push(format!("stop {} pts", fmt_decimal(stop)));
                }
                if let Some(profit) = profit {
                    parts.push(format!("target {} pts", fmt_decimal(profit)));
                }
                format!("{} on fill", parts.join(" / "))
            }
            _ => "an offset field needs fixing".to_owned(),
        };
        let hotkey = match side {
            Side::Buy => "Shift+B",
            Side::Sell => "Shift+S",
        };
        format!(
            "simulated market {} - fills at the next print; {quantity}, {bracket} \
             (Trading tab) · {hotkey}",
            side_word(side),
        )
    }

    /// `Close 1 LONG` while a position is open — the toolbar's exit button
    /// label. `None` while flat, which is what removes the button.
    #[must_use]
    pub fn close_button_label(&self) -> Option<String> {
        let position = self.sim.position()?;
        Some(format!(
            "Close {} {}",
            fmt_decimal(position.quantity),
            position_word(position.side),
        ))
    }

    // ------------------------------------------------------------------
    // Chart layer
    // ------------------------------------------------------------------

    /// Paint the simulated lines and their in-plot tags: pending orders
    /// (dashed, accent), then the position's entry / stop-loss / take-profit
    /// (solid, semantic colors). Gutter chips keep the last-price chip's
    /// geometry and carry *the price and nothing else*, so prices never
    /// disagree about their pixel; the words and the ✕s live in tags
    /// right-anchored inside the plot. Every interactive rect painted here
    /// comes from the same pure geometry the press-time hit-test computes
    /// (`close_button_rect`, `bracket_handle_rect`) — nothing is cached
    /// across frames, because a live chart autoscales between paint and
    /// press. `pointer` is `Some` only on the pane that owns paper input,
    /// so hover affordances paint nowhere else.
    #[expect(
        clippy::too_many_arguments,
        reason = "the pane hands over its frame geometry; bundling it here would rename, not simplify"
    )]
    pub fn draw_layer(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        tag_right: f32,
        axis_x: f32,
        scale: &PriceScale,
        reserved_chip_y: Option<f32>,
        pointer: Option<egui::Pos2>,
    ) {
        let ctx = PaintCtx {
            painter,
            chart_rect,
            tag_right,
            axis_x,
            scale,
            reserved_chip_y,
            pointer,
        };

        for order in self.sim.orders() {
            let Some(level) = order.price else { continue };
            let dragged = self.drag == PaperDrag::Order(order.id);
            let price = if dragged {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let y = ctx.scale.y(price);
            if !ctx.in_range(y) {
                continue;
            }
            // At rest the tag is a pill; it opens under the pointer. Read,
            // never recomputed — this frame's input already decided it,
            // from the pointer and the rect the *press* will use.
            let open = self.open_tag(order.id);
            let expanded = open.is_some();
            // The line's own emphasis keeps its own 10 px band: that band
            // is `line_at`'s, so a line that lights up is a line the press
            // can actually grab. The tag opens over a wider row and near a
            // chart edge over a different one, which is why the two
            // questions stayed separate.
            let hovered = ctx.hovers_line(y) || self.hovered_order == Some(order.id);
            let shown = if dragged { self.snap(price) } else { level };
            ctx.level_line(y, theme::ACCENT, true, LINE_WIDTH_PX, hovered, dragged);
            ctx.gutter_chip(y, theme::ACCENT, &fmt_decimal(shown));
            // Resting, the pill states what the gutter chip cannot — which
            // side and what kind of order waits there. The price is the
            // chip's job, and the id only matters once you mean to act on
            // it, so both wait for the open form.
            let text = if expanded {
                format!(
                    "#{} {} {} {} @ {}",
                    order.id.0,
                    side_word_upper(order.side),
                    kind_short(order.kind),
                    fmt_decimal(order.quantity),
                    fmt_decimal(shown),
                )
            } else {
                format!(
                    "{} {} {}",
                    side_word_upper(order.side),
                    kind_short(order.kind),
                    fmt_decimal(order.quantity),
                )
            };
            // The ✕ is painted exactly when the press-side offers it —
            // one value, read twice, never two formulas.
            ctx.chip_tag(y, theme::ACCENT, &text, open.is_some_and(|tag| tag.cancel));
        }

        if let Some(position) = self.sim.position().cloned() {
            let color = side_color(position.side);
            let entry_y = ctx.scale.y(position.avg_price.to_f64().unwrap_or_default());
            if ctx.in_range(entry_y) {
                ctx.level_line(entry_y, color, false, POSITION_LINE_WIDTH_PX, false, false);
                ctx.gutter_chip(entry_y, color, &fmt_decimal(position.avg_price));
                let side_text = format!(
                    "SIM {} {}",
                    position_word(position.side),
                    fmt_decimal(position.quantity),
                );
                let points = self
                    .sim
                    .mark_price()
                    .map(|mark| position.open_points(mark))
                    .map(|open| {
                        (
                            format!("{} pts", fmt_signed_points(open)),
                            points_color(open),
                        )
                    });
                let line_hovered = ctx.hovers_line(entry_y);
                let tag_rect = ctx.position_tag(entry_y, color, &side_text, points);

                // Labelled handles for the missing bracket legs, revealed
                // while the pointer is on the entry line, its tag, or the
                // handles themselves — the affordance behind "drag from the
                // position line to create". Their rects are the hit-test's
                // own geometry, so a revealed handle is always pressable.
                let entry_center =
                    clamp_tag_center(entry_y, ctx.chart_rect.top(), ctx.chart_rect.bottom());
                let legs = [
                    (
                        position.stop_loss.is_none(),
                        "SL",
                        position.side == Side::Sell,
                        theme::SELL,
                    ),
                    (
                        position.take_profit.is_none(),
                        "TP",
                        position.side == Side::Buy,
                        theme::BUY,
                    ),
                ];
                let over_tag = ctx
                    .pointer
                    .is_some_and(|pointer| tag_rect.expand(TAG_HOVER_SLACK_PX).contains(pointer));
                let over_handle = ctx.pointer.is_some_and(|pointer| {
                    legs.iter().any(|(absent, _, above, _)| {
                        *absent
                            && bracket_handle_rect(ctx.tag_right, entry_center, *above)
                                .contains(pointer)
                    })
                });
                if self.drag == PaperDrag::None && (line_hovered || over_tag || over_handle) {
                    for (absent, label, above, leg_color) in legs {
                        if !absent {
                            continue;
                        }
                        let rect = bracket_handle_rect(ctx.tag_right, entry_center, above);
                        ctx.bracket_handle(rect, label, leg_color);
                    }
                }
            }

            self.draw_bracket_leg(
                &ctx,
                &LegPaint {
                    position: &position,
                    level: position.stop_loss,
                    other_level: position.take_profit,
                    word: "SL",
                    color: theme::SELL,
                    amend: PaperDrag::StopLoss,
                    create: PaperDrag::CreateStopLoss,
                },
            );
            self.draw_bracket_leg(
                &ctx,
                &LegPaint {
                    position: &position,
                    level: position.take_profit,
                    other_level: position.stop_loss,
                    word: "TP",
                    color: theme::BUY,
                    amend: PaperDrag::TakeProfit,
                    create: PaperDrag::CreateTakeProfit,
                },
            );
        }

        if let Some(armed) = self.armed {
            let hint = format!(
                "click a price to place your {} {} - Esc cancels",
                side_word(armed.side),
                kind_word(armed.kind),
            );
            // Bottom-left: the top-left corner belongs to the position HUD,
            // and arming an entry with a position open is a normal flow.
            painter.text(
                chart_rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                hint,
                egui::FontId::proportional(12.0),
                theme::ACCENT,
            );
        }

        // On top of every order line: the cmd preview is the thing being
        // aimed right now.
        self.draw_cmd_preview(&ctx);
    }

    /// The cmd-trading preview: a dashed line at the pointer's price
    /// running out to the right edge, the label riding beside the cursor,
    /// and the exact price on the gutter — the trader reads what this
    /// click will place *before* it commits, which is the safety the
    /// right-click menu cannot offer. The line is what ties the label at
    /// the pointer to the price on the axis, so it spans the whole way
    /// rather than hugging the edge.
    fn draw_cmd_preview(&self, ctx: &PaintCtx<'_>) {
        let Some(preview) = self.cmd_preview else {
            return;
        };
        // Only the pane that owns paper input paints the aim — except in
        // a harness run, whose panes never own a pointer at all.
        if ctx.pointer.is_none() && self.cmd_preview_force.is_none() {
            return;
        }
        if !ctx.in_range(preview.pointer.y) {
            return;
        }
        let color = side_color(preview.side);
        // Lay out against the band the input hit-tests (its right edge is
        // the lane divider when the live tape lane is up), never the full
        // chart rect — a label painted right of the divider would be a
        // click target the press could not find (the overlay-controls
        // rule).
        let band = egui::Rect::from_min_max(
            ctx.chart_rect.min,
            egui::pos2(
                ctx.tag_right.min(ctx.chart_rect.right()),
                ctx.chart_rect.max.y,
            ),
        );
        // The aim belongs to the band it was aimed in. Both panes of a
        // split draw this layer from one simulator, and the label now
        // rides an x — so laying a flow-pane pointer out against the time
        // pane's band would paint a label off the end of it. Live, the
        // other pane holds no pointer and never reaches here; under the
        // capture hook it does, which is how this was seen at all.
        if preview.pointer.x < band.left() || preview.pointer.x > band.right() {
            return;
        }
        let (start, end, label) = cmd_preview_layout(band, preview.pointer);
        // The gesture in progress reads a step above a resting order's
        // line: this is the one thing on the chart the next click acts on.
        // The dash period stretches on a very wide plot rather than paying
        // for a segment every 8 px across it (see `CMD_LINE_MAX_DASHES`).
        let period = ((end.x - start.x) / CMD_LINE_MAX_DASHES).max(ORDER_DASH_PX + ORDER_GAP_PX);
        let dash = period * ORDER_DASH_PX / (ORDER_DASH_PX + ORDER_GAP_PX);
        ctx.painter.extend(egui::Shape::dashed_line(
            &[start, end],
            egui::Stroke::new(LINE_HOVER_WIDTH_PX, color),
            dash,
            period - dash,
        ));
        ctx.gutter_chip(preview.pointer.y, color, &fmt_decimal(preview.price));
        // Full fill, always: while it paints, the click is already live.
        // There is no resting state to distinguish — releasing the
        // modifier is what makes the aim go away.
        ctx.painter
            .rect_filled(label, egui::Rounding::same(3.0), color);
        let quantity = self
            .quantity_preview()
            .map_or_else(|| "?".to_owned(), fmt_decimal);
        let text = format!(
            "{} {} {}",
            side_word_upper(preview.side),
            kind_word(preview.kind),
            quantity,
        );
        let galley =
            ctx.painter
                .layout_no_wrap(text, egui::FontId::monospace(11.0), theme::CHIP_INK);
        ctx.painter.galley(
            egui::pos2(
                label.center().x - galley.size().x / 2.0,
                label.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme::CHIP_INK,
        );
    }

    /// One protective leg: its resting line and tag, the drag that reprices
    /// it, or — while a create-drag runs and the leg does not exist yet —
    /// the dashed preview of where release would put it. The tag gains the
    /// live R:R read once both legs are known, which is what turns the drag
    /// into a decision.
    fn draw_bracket_leg(&self, ctx: &PaintCtx<'_>, leg: &LegPaint<'_>) {
        let amending = self.drag == leg.amend;
        let creating = self.drag == leg.create && leg.level.is_none();
        let resting = leg.level.map(|level| level.to_f64().unwrap_or_default());
        let price = if amending || creating {
            self.drag_price.or(resting)
        } else {
            resting
        };
        let Some(price) = price else { return };
        let y = ctx.scale.y(price);
        if !ctx.in_range(y) {
            return;
        }
        let dragging = amending || creating;
        let hovered = ctx.hovers_line(y);
        let shown = if dragging {
            self.snap(price)
        } else {
            leg.level.unwrap_or_else(|| self.snap(price))
        };
        // A leg being created previews dashed — it is not an order yet.
        ctx.level_line(y, leg.color, creating, LINE_WIDTH_PX, hovered, dragging);
        ctx.gutter_chip(y, leg.color, &fmt_decimal(shown));
        let mut text = format!(
            "{} {} {} pts",
            leg.word,
            fmt_decimal(shown),
            fmt_signed_points(leg.position.open_points(shown)),
        );
        if dragging && let Some(ratio) = rr_ratio(leg, shown) {
            text.push_str(&format!(" · R:R {ratio}"));
        }
        ctx.chip_tag(y, leg.color, &text, !dragging);
    }

    /// Route pointer input to the simulated lines. Returns true when paper
    /// trading owns the gesture this frame — the chart must not pan and the
    /// drawings must not select under it.
    /// Cancel the transient chart interaction — an armed placement or a
    /// grabbed line (dropped without submitting). Called from the app's
    /// escape stack; returns true when there was something to cancel, so
    /// the stack spends exactly one layer on it.
    pub fn cancel_interaction(&mut self) -> bool {
        if self.armed.take().is_some() {
            return true;
        }
        if self.drag != PaperDrag::None {
            self.drag = PaperDrag::None;
            self.drag_price = None;
            return true;
        }
        if self.selected_trade.take().is_some() {
            return true;
        }
        false
    }

    /// This session's closed round trips, oldest first — the trades whose
    /// fills the current tape can prove, and so the only ones the chart
    /// paints marks for.
    #[must_use]
    pub fn session_trades(&self) -> &[ClosedTrade] {
        self.sim.closed_trades()
    }

    /// The resting entry orders, in placement order — the simulator's own
    /// view, read-only.
    #[must_use]
    pub fn working_orders(&self) -> &[quantick_sim::Order] {
        self.sim.orders()
    }

    /// Index (into [`Self::session_trades`]) of the ledger's selected
    /// trade, for the chart to emphasize; `None` while nothing is selected.
    #[must_use]
    pub fn selected_trade_index(&self) -> Option<usize> {
        self.selected_trade
            .filter(|index| *index < self.sim.closed_trades().len())
    }

    pub fn handle_chart_input(&mut self, input: &ChartInput<'_>) -> bool {
        // Three per-frame facts, in dependency order: whether the layer
        // paints at all, which tags are open (the aim yields to an open ✕,
        // so this comes before it), then the cmd preview. All three are
        // written here, and read by `draw_layer`, by the press below and
        // by `hover_cursor` — one value each, never a formula re-run
        // against a different pointer.
        self.layer_visible = input.layer_visible;
        self.refresh_open_tags(input);
        self.cmd_preview = self.compute_cmd_preview(input);

        // Nothing paper is painted while its layer is off, so nothing
        // paper takes the press: an invisible line is not a control.
        if !input.layer_visible {
            self.drag = PaperDrag::None;
            return false;
        }

        // An overlay control (a tag's ✕, a bracket handle) takes the press
        // before *everything*: before the armed click — arming an order must
        // never eat the ✕ under the pointer — and before the line grab,
        // which is what made "close this order" read as "drag this order".
        // The hit is geometric, from this frame's own scale and state: a
        // pixel rect cached from the last paint goes stale the moment a
        // live chart autoscales, and the press then slips onto the line.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(pointer) = input.pointer
            && let Some(scale) = input.scale
            && let Some(control) = self.control_at(pointer, input.chart, scale)
        {
            match control {
                PaperControl::ClosePosition => self.close_position(),
                PaperControl::ClearStopLoss => self.clear_bracket_leg(true),
                PaperControl::ClearTakeProfit => self.clear_bracket_leg(false),
                PaperControl::CancelOrder(id) => {
                    let events = self.sim.apply(Command::CancelOrder { id });
                    self.handle_events(events);
                }
                PaperControl::HandleStopLoss => {
                    self.drag = PaperDrag::CreateStopLoss;
                    self.drag_price = Some(scale.price_at(pointer.y));
                }
                PaperControl::HandleTakeProfit => {
                    self.drag = PaperDrag::CreateTakeProfit;
                    self.drag_price = Some(scale.price_at(pointer.y));
                }
            }
            return true;
        }

        // The aimed order. There is no separate target to hit: a label
        // that rides the pointer can never be landed on — move toward it
        // and it moves with you — so the *held modifier* is the deliberate
        // act and the label beside the cursor is the statement of what
        // this click will do. It is also the *last* claimant on the
        // canvas: `compute_cmd_preview` has already stood the aim down
        // wherever an overlay control, a paper line, an armed placement or
        // an annotation holds the pixel, so a preview existing here means
        // nothing else wanted this press. A forced aim is a capture
        // fixture with no hand behind it and never places.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(preview) = self.cmd_preview
            && !preview.forced
        {
            self.place_resting(preview.side, preview.kind, preview.raw_price);
            return true;
        }

        // An armed placement takes the next chart click.
        if let Some(armed) = self.armed
            && input.primary_pressed
            && let Some(pointer) = input.pointer
            && input.chart.contains(pointer)
            && let Some(scale) = input.scale
        {
            self.place_armed(armed, scale.price_at(pointer.y));
            return true;
        }

        // Grab a line.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(pointer) = input.pointer
            && input.chart.contains(pointer)
            && let Some(scale) = input.scale
            && let Some(target) = self.line_at(pointer, scale)
        {
            self.drag = target;
            self.drag_price = Some(scale.price_at(pointer.y));
            return true;
        }

        // Follow the pointer while dragging. A press that started on the
        // entry line commits to one bracket leg on its first real pull:
        // towards the profit side it creates the take profit, towards the
        // losing side the stop — and a side whose leg already exists stays
        // blocked (that leg's own line is its handle).
        if input.primary_down && self.drag != PaperDrag::None {
            if let (Some(pointer), Some(scale)) = (input.pointer, input.scale) {
                let y = pointer.y.clamp(input.chart.top(), input.chart.bottom());
                self.drag_price = Some(scale.price_at(y));
                if self.drag == PaperDrag::CreatePending {
                    self.decide_pending_leg(y, scale);
                }
            }
            return true;
        }

        // Drop: submit the new price; the simulator answers (a rejection
        // snaps the line back and the toast explains why). Creating a leg
        // and repricing it are the same command — the bracket is replaced
        // wholesale either way.
        if input.primary_released && self.drag != PaperDrag::None {
            let drag = std::mem::take(&mut self.drag);
            if let Some(price) = self.drag_price.take() {
                let price = self.snap(price);
                let command = match drag {
                    PaperDrag::StopLoss | PaperDrag::CreateStopLoss => {
                        self.sim.position().map(|position| Command::SetBracket {
                            stop_loss: Some(price),
                            take_profit: position.take_profit,
                        })
                    }
                    PaperDrag::TakeProfit | PaperDrag::CreateTakeProfit => {
                        self.sim.position().map(|position| Command::SetBracket {
                            stop_loss: position.stop_loss,
                            take_profit: Some(price),
                        })
                    }
                    PaperDrag::Order(id) => Some(Command::ModifyOrder { id, price }),
                    PaperDrag::None | PaperDrag::Blocked | PaperDrag::CreatePending => None,
                };
                if let Some(command) = command {
                    let events = self.sim.apply(command);
                    self.handle_events(events);
                }
            }
            return true;
        }

        self.drag != PaperDrag::None
    }

    /// Remove one protective leg, keeping the other — the tag ✕'s command.
    fn clear_bracket_leg(&mut self, stop: bool) {
        let Some(position) = self.sim.position() else {
            return;
        };
        let command = if stop {
            Command::SetBracket {
                stop_loss: None,
                take_profit: position.take_profit,
            }
        } else {
            Command::SetBracket {
                stop_loss: position.stop_loss,
                take_profit: None,
            }
        };
        let events = self.sim.apply(command);
        self.handle_events(events);
    }

    /// The overlay control under the pointer, computed from this frame's
    /// scale and simulator state — never a cached pixel rect, which goes
    /// stale between paint and press the moment a live chart autoscales.
    /// Priority follows the draw stack: working orders on top, then the
    /// take profit, the stop, the position's ✕, and last the bracket
    /// handles beside the entry line.
    fn control_at(
        &self,
        pointer: egui::Pos2,
        chart: egui::Rect,
        scale: &PriceScale,
    ) -> Option<PaperControl> {
        let tag_right = chart.right();
        // A line outside the visible price range paints no tag, so it
        // offers no control either — same gate the paint applies.
        let visible_center = |price: Decimal| {
            let y = scale.y(price.to_f64().unwrap_or_default());
            (y >= chart.top() && y <= chart.bottom())
                .then(|| clamp_tag_center(y, chart.top(), chart.bottom()))
        };
        for order in self.sim.orders().iter().rev() {
            // A tag that paints no ✕ offers none: the press reads the very
            // value the paint read, rather than recomputing a predicate
            // from a different pointer and a different rect.
            if let Some(level) = order.price
                && let Some(center_y) = visible_center(level)
                && self.open_tag(order.id).is_some_and(|tag| tag.cancel)
                && close_button_rect(tag_right, center_y).contains(pointer)
            {
                return Some(PaperControl::CancelOrder(order.id));
            }
        }
        let position = self.sim.position()?;
        if let Some(target) = position.take_profit
            && let Some(center_y) = visible_center(target)
            && close_button_rect(tag_right, center_y).contains(pointer)
        {
            return Some(PaperControl::ClearTakeProfit);
        }
        if let Some(stop) = position.stop_loss
            && let Some(center_y) = visible_center(stop)
            && close_button_rect(tag_right, center_y).contains(pointer)
        {
            return Some(PaperControl::ClearStopLoss);
        }
        let entry_center = visible_center(position.avg_price)?;
        if close_button_rect(tag_right, entry_center).contains(pointer) {
            return Some(PaperControl::ClosePosition);
        }
        if position.take_profit.is_none()
            && bracket_handle_rect(tag_right, entry_center, position.side == Side::Buy)
                .contains(pointer)
        {
            return Some(PaperControl::HandleTakeProfit);
        }
        if position.stop_loss.is_none()
            && bracket_handle_rect(tag_right, entry_center, position.side == Side::Sell)
                .contains(pointer)
        {
            return Some(PaperControl::HandleStopLoss);
        }
        None
    }

    /// The preview the pointer and the held key describe this frame;
    /// `None` hides the overlay. Both keys down at once is ambiguous and
    /// shows nothing — so a shared binding degrades to "off", never to a
    /// wrong side.
    ///
    /// **The aim is the last claimant on the canvas.** Its target is the
    /// whole plot, so anything already holding the pixel outranks it: an
    /// annotation or the canvas chrome (the pane says so), an armed
    /// placement the trader is in the middle of, an overlay ✕ or bracket
    /// handle, and this module's own draggable lines. Standing the aim
    /// *down* rather than merely refusing its press is what keeps the
    /// promise: no preview means nothing paints, no hand cursor, no place
    /// — the label can never advertise an order the press will not make.
    fn compute_cmd_preview(&self, input: &ChartInput<'_>) -> Option<CmdPreview> {
        if !self.cmd_trading.enabled || !input.layer_visible {
            return None;
        }
        // A drawing a press would grab, or the canvas's own chrome. The
        // buy modifier is Shift by default — the very key that levels a
        // channel corner — so sweeping across a drawn line blinks the aim
        // off for its grab band, in step with the move cursor the drawings
        // put up.
        if input.canvas_claimed {
            return None;
        }
        // An armed limit/stop is an intent already stated, with its own
        // hint on screen; a modifier resting under the hand must not turn
        // that click into a different order and leave the ticket armed.
        if self.armed.is_some() {
            return None;
        }
        let scale = input.scale?;
        let (pointer, side, forced) = match self.cmd_preview_force {
            // The harness has no hand; park the pointer mid-chart, or at
            // the x the hook stated — which is the whole point of a run
            // capturing where the label rides, so it wins over a stray
            // real pointer that in such a run is nobody's aim.
            Some(force) => {
                let pointer = match force.x_fraction {
                    Some(fraction) => egui::pos2(
                        input.chart.left() + input.chart.width() * fraction,
                        input
                            .pointer
                            .map_or_else(|| input.chart.center().y, |pointer| pointer.y),
                    ),
                    None => input.pointer.unwrap_or(input.chart.center()),
                };
                (pointer, force.side, true)
            }
            None => {
                let pointer = input.pointer?;
                let buy = self.cmd_trading.buy.is_down(input.modifiers);
                let sell = self.cmd_trading.sell.is_down(input.modifiers);
                let side = match (buy, sell) {
                    (true, false) => Side::Buy,
                    (false, true) => Side::Sell,
                    _ => return None,
                };
                (pointer, side, false)
            }
        };
        if !input.chart.contains(pointer) {
            return None;
        }
        // This module's own furniture outranks the aim, the same way an
        // annotation does: an ✕ or a bracket handle under the pointer, and
        // any line a press would grab. Otherwise holding the modifier
        // while reaching for a stop would rest a new order on top of it,
        // with the hand cursor promising exactly that.
        if self.control_at(pointer, input.chart, scale).is_some()
            || self.line_at(pointer, scale).is_some()
        {
            return None;
        }
        let mark = self.sim.mark_price()?;
        let raw_price = scale.price_at(pointer.y);
        let price = self.snap(raw_price);
        // The context menu's own validity table: above the mark a buy
        // stops in, below it a buy waits at a limit; a sell mirrors. On
        // the mark exactly nothing can rest.
        let kind = match (price > mark, price < mark, side) {
            (true, _, Side::Buy) | (_, true, Side::Sell) => EntryKind::Stop,
            (true, _, Side::Sell) | (_, true, Side::Buy) => EntryKind::Limit,
            _ => return None,
        };
        Some(CmdPreview {
            side,
            kind,
            price,
            raw_price,
            pointer,
            forced,
        })
    }

    /// Which resting orders state themselves in full this frame, and which
    /// of those offer their ✕. At rest a tag is a compact pill, so the
    /// candles behind the most recent price stay readable; it opens under
    /// the pointer, while its dock row is hovered, and for as long as it is
    /// being dragged — a trader repricing an order needs every field of it,
    /// though a moving order offers no cancel.
    ///
    /// Computed once, from the pointer and the rect the *press* uses, and
    /// then read by both sides (see [`OpenTag`]).
    ///
    /// Per-frame path: the buffer is taken and refilled rather than rebuilt,
    /// so hovering an order costs no allocation once its capacity is up.
    fn refresh_open_tags(&mut self, input: &ChartInput<'_>) {
        let mut open = std::mem::take(&mut self.open_tags);
        open.clear();
        self.fill_open_tags(&mut open, input);
        self.open_tags = open;
    }

    /// See [`Self::refresh_open_tags`] — split out so the order list and
    /// the buffer are never borrowed from `self` at the same time.
    fn fill_open_tags(&self, open: &mut Vec<OpenTag>, input: &ChartInput<'_>) {
        if !input.layer_visible {
            return;
        }
        let Some(scale) = input.scale else {
            return;
        };
        for order in self.sim.orders() {
            let Some(level) = order.price else { continue };
            let dragged = self.drag == PaperDrag::Order(order.id);
            let price = if dragged {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let expanded = dragged
                || self.hovered_order == Some(order.id)
                || self.order_hover_force
                || input
                    .pointer
                    .is_some_and(|pointer| tag_row_hit(pointer, scale.y(price), input.chart));
            if expanded {
                open.push(OpenTag {
                    id: order.id,
                    cancel: !dragged,
                });
            }
        }
    }

    /// This frame's answer for one order's tag, or `None` while it rests
    /// as a pill. See [`OpenTag`].
    fn open_tag(&self, id: OrderId) -> Option<OpenTag> {
        self.open_tags.iter().copied().find(|tag| tag.id == id)
    }

    /// Turn a pending entry-line press into the leg the pull chose, once it
    /// travelled far enough to mean it.
    fn decide_pending_leg(&mut self, pointer_y: f32, scale: &PriceScale) {
        let Some(position) = self.sim.position() else {
            self.drag = PaperDrag::Blocked;
            return;
        };
        let entry_y = scale.y(position.avg_price.to_f64().unwrap_or_default());
        let delta = pointer_y - entry_y;
        if delta.abs() < CREATE_DECIDE_THRESHOLD_PX {
            return;
        }
        // On screen, up is negative y; up is the profit side for a long.
        let profit_side = match position.side {
            Side::Buy => delta < 0.0,
            Side::Sell => delta > 0.0,
        };
        self.drag = if profit_side {
            if position.take_profit.is_none() {
                PaperDrag::CreateTakeProfit
            } else {
                PaperDrag::Blocked
            }
        } else if position.stop_loss.is_none() {
            PaperDrag::CreateStopLoss
        } else {
            PaperDrag::Blocked
        };
    }

    /// The cursor that announces what is under the pointer (audit M3/M4):
    /// a pointing hand over a painted control, vertical-resize over a
    /// draggable order/stop/target line and over an entry line with a
    /// missing bracket leg (dragging away from it creates that leg),
    /// not-allowed over a fully bracketed entry — the average entry itself
    /// is history and never moves. `None` away from everything, so the
    /// caller falls through to the drawings' own cursors.
    #[must_use]
    pub fn hover_cursor(
        &self,
        pointer: egui::Pos2,
        chart: egui::Rect,
        scale: &PriceScale,
    ) -> Option<egui::CursorIcon> {
        if !self.layer_visible {
            return None;
        }
        if self.control_at(pointer, chart, scale).is_some() {
            return Some(egui::CursorIcon::PointingHand);
        }
        // While a *real* aim is painted, the whole plot is the click, so
        // the hand says so everywhere — and only there: the aim already
        // stood down over every line and control this function would
        // otherwise announce, so the two can no longer contradict.
        if self.cmd_preview.is_some_and(|preview| !preview.forced) {
            return Some(egui::CursorIcon::PointingHand);
        }
        match self.line_at(pointer, scale)? {
            PaperDrag::Blocked => Some(egui::CursorIcon::NotAllowed),
            PaperDrag::StopLoss
            | PaperDrag::TakeProfit
            | PaperDrag::Order(_)
            | PaperDrag::CreatePending => Some(egui::CursorIcon::ResizeVertical),
            PaperDrag::None | PaperDrag::CreateStopLoss | PaperDrag::CreateTakeProfit => None,
        }
    }

    /// Which line sits under the pointer, in draw-stack priority: pending
    /// orders first (they draw on top), then take profit, stop loss, and
    /// the entry line — which starts a bracket-creating drag while a leg is
    /// missing, and blocks the gesture once both exist.
    fn line_at(&self, pointer: egui::Pos2, scale: &PriceScale) -> Option<PaperDrag> {
        let near = |price: Decimal| {
            let y = scale.y(price.to_f64().unwrap_or_default());
            (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        };
        for order in self.sim.orders().iter().rev() {
            if let Some(level) = order.price
                && near(level)
            {
                return Some(PaperDrag::Order(order.id));
            }
        }
        let position = self.sim.position()?;
        if let Some(target) = position.take_profit
            && near(target)
        {
            return Some(PaperDrag::TakeProfit);
        }
        if let Some(stop) = position.stop_loss
            && near(stop)
        {
            return Some(PaperDrag::StopLoss);
        }
        if near(position.avg_price) {
            let creatable = position.stop_loss.is_none() || position.take_profit.is_none();
            return Some(if creatable {
                PaperDrag::CreatePending
            } else {
                PaperDrag::Blocked
            });
        }
        None
    }

    /// The armed click: place at the clicked price and disarm on success.
    /// Stays armed on a rejection — the toast explains where the order may
    /// sit, and the user clicks again.
    fn place_armed(&mut self, armed: ArmedPlacement, raw_price: f64) {
        if self.place_resting(armed.side, armed.kind, raw_price) {
            self.armed = None;
        }
    }

    /// Rest a limit/stop entry at `raw_price` with the ticket's quantity
    /// and offsets; returns whether the simulator accepted it.
    fn place_resting(&mut self, side: Side, kind: EntryKind, raw_price: f64) -> bool {
        let Some(quantity) = self.parse_quantity() else {
            return false;
        };
        let price = self.snap(raw_price);
        let Some(bracket) = self.parse_bracket(side, price) else {
            return false;
        };
        let command = match kind {
            EntryKind::Limit => Command::PlaceLimit {
                side,
                quantity,
                price,
                bracket,
            },
            EntryKind::Stop => Command::PlaceStop {
                side,
                quantity,
                trigger: price,
                bracket,
            },
            // Market never rests; the buttons fire it directly.
            EntryKind::Market => return false,
        };
        let events = self.sim.apply(command);
        let placed = events
            .iter()
            .any(|event| matches!(event, SimEvent::Placed(_)));
        self.handle_events(events);
        placed
    }

    /// The chart context menu's trade section, anchored at the clicked
    /// price: market both ways, then the resting types that are valid on
    /// that side of the market. The invalid ones stay visible but
    /// disabled, wearing the sim core's own rejection text — the same
    /// curriculum the toasts teach.
    pub fn context_trade_actions(&mut self, ui: &mut egui::Ui, raw_price: f64) {
        ui.label(
            egui::RichText::new("trade")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        let Some(mark) = self.sim.mark_price() else {
            ui.label(
                egui::RichText::new("no print yet - there is no market to trade against")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            return;
        };
        let quantity = self
            .quantity_preview()
            .map_or_else(|| "?".to_owned(), fmt_decimal);
        for side in [Side::Buy, Side::Sell] {
            if ui
                .button(format!("{} {quantity} market", side_word_upper(side)))
                .on_hover_text("fills at the next print, with the ticket's offsets")
                .clicked()
            {
                self.market(side);
                ui.close_menu();
            }
        }
        ui.separator();
        let price = self.snap(raw_price);
        let entries = [
            (Side::Buy, EntryKind::Limit, price < mark),
            (Side::Buy, EntryKind::Stop, price > mark),
            (Side::Sell, EntryKind::Limit, price > mark),
            (Side::Sell, EntryKind::Stop, price < mark),
        ];
        for (side, kind, valid) in entries {
            let label = format!(
                "{} {quantity} {} @ {}",
                side_word_upper(side),
                kind_word(kind),
                fmt_decimal(price),
            );
            let reason = match kind {
                EntryKind::Limit => quantick_sim::RejectReason::LimitOnWrongSide(side),
                _ => quantick_sim::RejectReason::StopOnWrongSide(side),
            };
            let response = ui
                .add_enabled(valid, egui::Button::new(label))
                .on_disabled_hover_text(reason.to_string());
            if response.clicked() {
                self.place_resting(side, kind, raw_price);
                ui.close_menu();
            }
        }
    }

    /// Round a pointer price to the precision the tape itself uses (the
    /// mark's decimal places), so a dragged line lands on a price the
    /// instrument can actually print.
    fn snap(&self, price: f64) -> Decimal {
        let places = self
            .sim
            .mark_price()
            .map_or(SNAP_FALLBACK_DECIMALS, |mark| mark.scale());
        Decimal::from_f64_retain(price)
            .unwrap_or_default()
            .round_dp(places)
            .normalize()
    }

    // ------------------------------------------------------------------
    // Dock tab
    // ------------------------------------------------------------------

    /// The Trading dock tab: position, ticket, working orders, session
    /// strip. See `docs/ux/paper-trading.md` §3.
    pub fn draw_trading_tab(&mut self, ui: &mut egui::Ui) -> Option<TradingTabAction> {
        self.hovered_order = None;
        ui.label(
            egui::RichText::new("Simulated fills from the tape - no broker, points not currency.")
                .color(theme::TEXT_MUTED)
                .small(),
        )
        .on_hover_text(
            "A market order fills at the next print; a limit at its own price when \
             the tape trades at or through it; a stop at the print that triggers it. \
             Nothing here touches a real account.",
        );
        ui.add_space(6.0);

        self.draw_position_card(ui);
        ui.separator();
        let cmd_changed = self.draw_order_entry(ui);
        ui.separator();
        self.draw_pending_orders(ui);
        ui.separator();
        let action = self.draw_session_summary(ui);
        if action.is_none() && cmd_changed {
            // Same-frame collision with another action is a picker click;
            // the settings change persists on its next touch.
            return Some(TradingTabAction::CmdTradingChanged);
        }
        action
    }

    /// A quiet action button — the HUD's control grammar, sized to share a
    /// row evenly.
    fn quiet_action(label: &str, width: f32) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(label)
                .color(theme::TEXT_PRIMARY)
                .small(),
        )
        .fill(theme::CONTROL)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .rounding(egui::Rounding::same(3.0))
        .min_size(egui::vec2(width, 22.0))
    }

    /// The position block: a one-row FLAT card while flat (with the
    /// session's realized points), the full card while a position is open —
    /// identity, brackets with their P&L, the R:R read, and the actions.
    fn draw_position_card(&mut self, ui: &mut egui::Ui) {
        let Some(position) = self.sim.position().cloned() else {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("FLAT")
                        .monospace()
                        .color(theme::TEXT_MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let realized = self.sim.realized_points();
                    ui.label(
                        egui::RichText::new(format!("{} pts", fmt_signed_points(realized)))
                            .monospace()
                            .strong()
                            .color(points_color(realized)),
                    )
                    .on_hover_text("this session's realized points");
                });
            });
            if !self.sim.orders().is_empty()
                && ui
                    .button("Cancel all orders")
                    .on_hover_text("remove every working order without trading (Shift+X)")
                    .clicked()
            {
                self.cancel_all_orders();
            }
            return;
        };
        let color = side_color(position.side);
        let open = self.sim.mark_price().map(|mark| position.open_points(mark));

        // Identity: the HUD's own chip, so the two surfaces read as one.
        ui.horizontal(|ui| {
            egui::Frame::none()
                .fill(color)
                .rounding(egui::Rounding::same(2.0))
                .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "SIM {} {}",
                            position_word(position.side),
                            fmt_decimal(position.quantity)
                        ))
                        .color(theme::CHIP_INK)
                        .strong()
                        .small(),
                    );
                });
            ui.label(
                egui::RichText::new(format!("@ {}", fmt_decimal(position.avg_price)))
                    .monospace()
                    .color(theme::TEXT_PRIMARY),
            );
            if let Some(open) = open {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} pts", fmt_signed_points(open)))
                            .monospace()
                            .strong()
                            .color(points_color(open)),
                    )
                    .on_hover_text(
                        "open profit at the last print, in points (price units × quantity)",
                    );
                });
            }
        });

        // Brackets: the level, what it pays, ✕ to clear — or the way to set
        // the missing leg right here, from the ticket's offset.
        let mut bracket_change = None;
        egui::Grid::new("paper_position_brackets")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                let legs = [
                    (
                        "SL",
                        position.stop_loss,
                        theme::SELL,
                        parse_offset(&self.stop_offset_text).ok().flatten(),
                        "remove the protective stop",
                    ),
                    (
                        "TP",
                        position.take_profit,
                        theme::BUY,
                        parse_offset(&self.profit_offset_text).ok().flatten(),
                        "remove the profit target",
                    ),
                ];
                for (word, level, leg_color, offset, clear_hover) in legs {
                    ui.label(egui::RichText::new(word).color(theme::TEXT_MUTED).small());
                    match level {
                        Some(level) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {} pts",
                                    fmt_decimal(level),
                                    fmt_signed_points(position.open_points(level)),
                                ))
                                .monospace()
                                .color(leg_color),
                            );
                            if ui.small_button("×").on_hover_text(clear_hover).clicked() {
                                bracket_change = Some(match word {
                                    "SL" => Command::SetBracket {
                                        stop_loss: None,
                                        take_profit: position.take_profit,
                                    },
                                    _ => Command::SetBracket {
                                        stop_loss: position.stop_loss,
                                        take_profit: None,
                                    },
                                });
                            }
                        }
                        None => match offset {
                            Some(offset) => {
                                ui.label(
                                    egui::RichText::new("—")
                                        .monospace()
                                        .color(theme::TEXT_FAINT),
                                );
                                if ui
                                    .small_button(format!("Set {} pts", fmt_decimal(offset)))
                                    .on_hover_text(
                                        "place this leg the ticket's offset away from the \
                                         average entry",
                                    )
                                    .clicked()
                                {
                                    let (stop_loss, take_profit) = if word == "SL" {
                                        (
                                            Some(offset_price(&position, offset, true)),
                                            position.take_profit,
                                        )
                                    } else {
                                        (
                                            position.stop_loss,
                                            Some(offset_price(&position, offset, false)),
                                        )
                                    };
                                    bracket_change = Some(Command::SetBracket {
                                        stop_loss,
                                        take_profit,
                                    });
                                }
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new(
                                        "drag from the entry line, or type an offset below",
                                    )
                                    .color(theme::TEXT_SUPPORT)
                                    .small(),
                                );
                                ui.label("");
                            }
                        },
                    }
                    ui.end_row();
                }
            });
        if let (Some(stop), Some(target)) = (position.stop_loss, position.take_profit) {
            let risk = position.avg_price.saturating_sub(stop).abs();
            let reward = target.saturating_sub(position.avg_price).abs();
            if risk > Decimal::ZERO {
                ui.label(
                    egui::RichText::new(format!("R:R {}", fmt_points(reward / risk)))
                        .monospace()
                        .color(theme::TEXT_MUTED),
                )
                .on_hover_text("reward divided by risk, in points, at the current levels");
            }
        }
        if let Some(command) = bracket_change {
            let events = self.sim.apply(command);
            self.handle_events(events);
        }

        // Actions, two per row at equal width; consequential ones stay
        // text-first, never a bare glyph.
        ui.add_space(4.0);
        let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        let word = position_word(position.side);
        let qty = fmt_decimal(position.quantity);
        ui.horizontal(|ui| {
            if ui
                .add(Self::quiet_action("× Close", half))
                .on_hover_text(format!("exit the {word} {qty} at the next print (market)"))
                .clicked()
            {
                self.close_position();
            }
            if ui
                .add(Self::quiet_action(
                    &format!("{} Reverse", icons::ARROWS_LEFT_RIGHT),
                    half,
                ))
                .on_hover_text(format!(
                    "close the {word} {qty} and open the opposite side at the same size \
                     (Shift+R)"
                ))
                .clicked()
            {
                self.reverse_position();
            }
        });
        ui.horizontal(|ui| {
            let in_profit = open.is_some_and(|open| open > Decimal::ZERO);
            if ui
                .add_enabled(in_profit, Self::quiet_action("Breakeven", half))
                .on_hover_text(
                    "move the stop to the average entry - with no fees simulated, \
                     break-even is the entry exactly",
                )
                .on_disabled_hover_text(
                    "the stop can only move to entry while the position is in profit - \
                     below it, this would widen your risk",
                )
                .clicked()
            {
                let events = self.sim.apply(Command::SetBracket {
                    stop_loss: Some(position.avg_price),
                    take_profit: position.take_profit,
                });
                self.handle_events(events);
            }
            if ui
                .add(Self::quiet_action("Close 50%", half))
                .on_hover_text(
                    "close half the open quantity at the next print; the rest keeps \
                     its average entry and brackets",
                )
                .clicked()
            {
                let events = self.sim.apply(Command::ClosePartial {
                    quantity: (position.quantity / Decimal::TWO).normalize(),
                });
                self.handle_events(events);
            }
        });
        let full = ui.available_width();
        if ui
            .add(Self::quiet_action("Flatten all", full))
            .on_hover_text("close the position and cancel every working order (Shift+F)")
            .clicked()
        {
            self.flatten();
        }
    }

    fn draw_order_entry(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label(caption("ORDER"));
        // Qty: free decimal text (empty must keep meaning "fix me"), with
        // steppers beside it; Shift steps by ten.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Qty").color(theme::TEXT_MUTED).small());
            let step = if ui.input(|input| input.modifiers.shift) {
                Decimal::TEN
            } else {
                Decimal::ONE
            };
            if ui
                .small_button("−")
                .on_hover_text("one less (Shift: ten)")
                .clicked()
            {
                self.step_quantity(-step);
            }
            ui.add(egui::TextEdit::singleline(&mut self.qty_text).desired_width(56.0));
            if ui
                .small_button("+")
                .on_hover_text("one more (Shift: ten)")
                .clicked()
            {
                self.step_quantity(step);
            }
        });
        // Type: three pills. Picking Limit or Stop is a promise of an
        // accent line on the chart, so the selected pill wears the accent.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Type").color(theme::TEXT_MUTED).small());
            for kind in [EntryKind::Market, EntryKind::Limit, EntryKind::Stop] {
                let on = self.order_type == kind;
                if pill_toggle(ui, kind_word(kind), on, "how the entry meets the market").clicked()
                    && !on
                {
                    self.order_type = kind;
                    self.armed = None;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Stop").color(theme::TEXT_MUTED).small())
                .on_hover_text(
                    "optional protective stop, this many points on the losing side of the \
                     entry; empty places no stop",
                );
            ui.add(egui::TextEdit::singleline(&mut self.stop_offset_text).desired_width(52.0));
            ui.label(
                egui::RichText::new("Target")
                    .color(theme::TEXT_MUTED)
                    .small(),
            )
            .on_hover_text(
                "optional profit target, this many points on the winning side of the \
                 entry; empty places no target",
            );
            ui.add(egui::TextEdit::singleline(&mut self.profit_offset_text).desired_width(52.0));
            ui.label(egui::RichText::new("pts").color(theme::TEXT_FAINT).small());
        });
        ui.add_space(4.0);

        // The entry pair: the surface where you commit, taller than the
        // toolbar's buttons. An armed side inverts — a mode you are in must
        // be visible on the control that put you there.
        let half = (ui.available_width() - 6.0) / 2.0;
        let ready = self.ready();
        let mut fire: Option<Side> = None;
        let mut arm: Option<Side> = None;
        let mut disarm = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for side in [Side::Buy, Side::Sell] {
                let color = side_color(side);
                let armed_here = self.armed.is_some_and(|armed| armed.side == side);
                let armed_other = self.armed.is_some_and(|armed| armed.side != side);
                let label = match (self.order_type, armed_here) {
                    (_, true) => "Click a price…".to_owned(),
                    (EntryKind::Market, _) => self.entry_label(side),
                    (kind, _) => format!(
                        "{} {} {}",
                        side_word_upper(side),
                        kind_word(kind).to_uppercase(),
                        self.quantity_preview()
                            .map_or_else(String::new, fmt_decimal),
                    ),
                };
                let button = if armed_here {
                    egui::Button::new(egui::RichText::new(label).color(color).strong())
                        .fill(theme::CONTROL)
                        .stroke(egui::Stroke::new(1.5_f32, color))
                } else {
                    egui::Button::new(egui::RichText::new(label).color(theme::CHIP_INK).strong())
                        .fill(color)
                        .stroke(egui::Stroke::NONE)
                }
                .rounding(egui::Rounding::same(3.0))
                .min_size(egui::vec2(half, 34.0));
                let response = ui
                    .add_enabled(ready && !armed_other, button)
                    .on_hover_text(self.entry_hover(side))
                    .on_disabled_hover_text(if armed_other {
                        "cancel the armed order first (Esc)"
                    } else {
                        "waiting for the first print - there is no market yet"
                    });
                if response.clicked() {
                    match (self.order_type, armed_here) {
                        (_, true) => disarm = true,
                        (EntryKind::Market, _) => fire = Some(side),
                        (EntryKind::Limit | EntryKind::Stop, _) => arm = Some(side),
                    }
                }
            }
        });
        if disarm {
            self.armed = None;
        }
        if let Some(side) = fire {
            self.market(side);
        }
        if let Some(side) = arm {
            self.armed = Some(ArmedPlacement {
                side,
                kind: self.order_type,
            });
        }
        ui.label(
            egui::RichText::new(if self.armed.is_some() {
                "Click the chart at your price. Esc cancels."
            } else if self.order_type == EntryKind::Market {
                "Market orders fill at the next print."
            } else {
                "The button arms a click; the next chart click rests the order there."
            })
            .color(theme::TEXT_SUPPORT)
            .small(),
        );
        self.draw_cmd_trading_settings(ui)
    }

    /// The cmd-trading block of the ticket: the enable pill and the two
    /// key bindings. Returns whether anything changed, so the host can
    /// persist and fan out.
    fn draw_cmd_trading_settings(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.add_space(4.0);
        ui.label(caption("CMD TRADING"));
        ui.horizontal(|ui| {
            if pill_toggle(
                ui,
                "Enabled",
                self.cmd_trading.enabled,
                "hold a key over the chart: a dashed line shows exactly where the order \
                 will rest, and the click places it",
            )
            .clicked()
            {
                self.cmd_trading.enabled = !self.cmd_trading.enabled;
                changed = true;
            }
            for (word, slot) in [("Buy", true), ("Sell", false)] {
                ui.label(egui::RichText::new(word).color(theme::TEXT_MUTED).small());
                let current = if slot {
                    self.cmd_trading.buy
                } else {
                    self.cmd_trading.sell
                };
                egui::ComboBox::from_id_salt(("cmd_trading_modifier", word))
                    .width(64.0)
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        for modifier in CmdModifier::ALL {
                            if ui
                                .selectable_label(current == modifier, modifier.label())
                                .clicked()
                                && current != modifier
                            {
                                if slot {
                                    self.cmd_trading.buy = modifier;
                                } else {
                                    self.cmd_trading.sell = modifier;
                                }
                                changed = true;
                            }
                        }
                    });
            }
        });
        if self.cmd_trading.enabled && self.cmd_trading.buy == self.cmd_trading.sell {
            // A shared key is ambiguous, so the gesture shows nothing —
            // said here rather than discovered over the chart.
            ui.label(
                egui::RichText::new(
                    "buy and sell share a key - the gesture stays hidden until they differ",
                )
                .color(theme::AMBER)
                .small(),
            );
        } else if self.cmd_trading.enabled {
            // The gesture is invisible until a key is held; its one line
            // of instructions lives where the toggle does, not in a
            // tooltip a newcomer never hovers.
            ui.label(
                egui::RichText::new(format!(
                    "hold {} over the chart to buy, {} to sell - the dashed line shows \
                     where, and the click places it",
                    self.cmd_trading.buy.label(),
                    self.cmd_trading.sell.label(),
                ))
                .color(theme::TEXT_SUPPORT)
                .small(),
            );
        }
        changed
    }

    /// Step the quantity field by `delta`, never below a positive value;
    /// an unparseable field steps from one.
    fn step_quantity(&mut self, delta: Decimal) {
        let current = self
            .qty_text
            .trim()
            .parse::<Decimal>()
            .ok()
            .filter(|quantity| *quantity > Decimal::ZERO)
            .unwrap_or(Decimal::ONE);
        let next = current.saturating_add(delta);
        if next > Decimal::ZERO {
            self.qty_text = fmt_decimal(next);
        }
    }

    /// Cancel every working order (resting and queued), trading nothing.
    pub fn cancel_all_orders(&mut self) {
        let mut ids: Vec<OrderId> = self.sim.orders().iter().map(|order| order.id).collect();
        ids.extend(self.sim.queued().iter().filter_map(|action| match action {
            QueuedAction::Entry(order) => Some(order.id),
            QueuedAction::Close | QueuedAction::ClosePartial { .. } => None,
        }));
        for id in ids {
            let events = self.sim.apply(Command::CancelOrder { id });
            self.handle_events(events);
        }
    }

    fn draw_pending_orders(&mut self, ui: &mut egui::Ui) {
        let queued_entries = self
            .sim
            .queued()
            .iter()
            .filter(|action| matches!(action, QueuedAction::Entry(_)))
            .count();
        let queued_closes = self.sim.queued().len() - queued_entries;
        let orders: Vec<_> = self.working_orders().to_vec();
        ui.label(caption(&format!("WORKING ORDERS · {}", orders.len())));
        if queued_entries > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{queued_entries} market order(s) await the next print"
                ))
                .color(theme::TEXT_MUTED)
                .small(),
            );
        }
        if queued_closes > 0 {
            ui.label(
                egui::RichText::new("closing at the next print…")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
        }
        if orders.is_empty() && queued_entries == 0 {
            ui.label(egui::RichText::new("No working orders.").color(theme::TEXT_MUTED));
            ui.label(
                egui::RichText::new("Pick Limit or Stop, then click a price on the chart.")
                    .color(theme::TEXT_SUPPORT)
                    .small(),
            );
            return;
        }
        for order in orders {
            let response = ui.horizontal(|ui| {
                // A short accent dash, echoing the dashed chart line.
                let (dash, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
                ui.painter().line_segment(
                    [
                        egui::pos2(dash.left(), dash.center().y),
                        egui::pos2(dash.right(), dash.center().y),
                    ],
                    egui::Stroke::new(2.0_f32, theme::ACCENT),
                );
                ui.label(
                    egui::RichText::new(format!("#{}", order.id.0))
                        .color(theme::TEXT_FAINT)
                        .small(),
                );
                ui.label(
                    egui::RichText::new(side_word_upper(order.side))
                        .monospace()
                        .color(side_color(order.side)),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} {} @ {}",
                        kind_short(order.kind),
                        fmt_decimal(order.quantity),
                        order.price.map_or_else(String::new, fmt_decimal),
                    ))
                    .monospace()
                    .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("cancel this order")
                        .clicked()
                    {
                        let events = self.sim.apply(Command::CancelOrder { id: order.id });
                        self.handle_events(events);
                    }
                });
            });
            // One hover, two surfaces: the row lifts its chart line.
            if response.response.hovered() {
                self.hovered_order = Some(order.id);
            }
        }
    }

    fn draw_session_summary(&mut self, ui: &mut egui::Ui) -> Option<TradingTabAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            let realized = self.sim.realized_points();
            ui.label(
                egui::RichText::new(format!("{} pts", fmt_signed_points(realized)))
                    .monospace()
                    .strong()
                    .color(points_color(realized)),
            );
            ui.label(
                egui::RichText::new(format!(
                    "realized · {} trades",
                    self.sim.closed_trades().len()
                ))
                .color(theme::TEXT_MUTED)
                .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Report…")
                    .on_hover_text("performance metrics computed from the saved history")
                    .clicked()
                {
                    self.open_report();
                }
            });
        });
        ui.horizontal(|ui| {
            if ui
                .small_button(icons::FOLDER_OPEN)
                .on_hover_text(
                    "choose where trades are saved — applies to every tab and is \
                     remembered across restarts",
                )
                .clicked()
            {
                action = Some(TradingTabAction::PickTradesDir);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("trades saved to: {}", self.dir.display()))
                            .color(theme::TEXT_MUTED)
                            .small(),
                    )
                    .fill(theme::CONTROL)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(egui::Rounding::same(3.0))
                    .min_size(egui::vec2(ui.available_width(), 20.0)),
                )
                .on_hover_text(format!(
                    "click to open the folder — {}\nthe folder button beside this picks a \
                     new one; [paper] trades_dir in quantick.toml sets the base and \
                     QUANTICK_TRADES_DIR overrides it for one run. Anything writing the \
                     quantick-trades format here (a future bot included) shows up in the \
                     ledger, the report and the export.",
                    std::path::absolute(&self.dir)
                        .unwrap_or_else(|_| self.dir.clone())
                        .display()
                ))
                .clicked()
            {
                reveal_folder(&self.dir);
            }
        });
        action
    }

    // ------------------------------------------------------------------
    // Trades ledger tab
    // ------------------------------------------------------------------

    /// Load the earlier sessions' rows for the ledger, scoped to the
    /// current symbol or the whole folder. The live session's own file is
    /// excluded — its trades are already in the simulator.
    fn reload_ledger(&mut self) {
        let symbol = match self.ledger_scope {
            ReportScope::Symbol => Some(self.symbol.as_str()),
            ReportScope::All => None,
        };
        self.history_cache = Some(load_history(&self.dir, symbol, &self.session_journal_paths));
    }

    /// The Trades dock tab: the ledger of closed simulated trades — the
    /// open position pinned on top, this session under it, the saved
    /// history under that, and a totals strip that never scrolls away.
    pub fn draw_trades_tab(&mut self, ui: &mut egui::Ui, tz: TzOffset) -> Option<LedgerAction> {
        if self.history_cache.is_none() {
            self.reload_ledger();
        }
        let mut action = None;

        // Scope row: which symbols the saved history contributes.
        let mut reload = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if self.symbol.is_empty() {
                    "—"
                } else {
                    &self.symbol
                })
                .monospace()
                .color(theme::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icons::ARROWS_CLOCKWISE)
                    .on_hover_text("re-read the history folder")
                    .clicked()
                {
                    reload = true;
                }
                let all = self.ledger_scope == ReportScope::All;
                if pill_toggle(
                    ui,
                    "All symbols",
                    all,
                    "include every symbol's saved history, not just this chart's",
                )
                .clicked()
                {
                    self.ledger_scope = if all {
                        ReportScope::Symbol
                    } else {
                        ReportScope::All
                    };
                    reload = true;
                }
            });
        });
        if reload {
            self.reload_ledger();
        }

        ui.horizontal(|ui| {
            ui.label(caption("TRADE"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(caption("PTS"));
            });
        });
        ui.separator();

        // The open position rides above the scroll — panning through
        // history must never hide the trade you are in.
        if let Some(summary) = self.position_summary() {
            ui.label(caption("OPEN"));
            let held_ms = self
                .sim
                .mark_timestamp_ms()
                .zip(self.sim.position().map(|position| position.opened_ms))
                .map(|(mark, opened)| mark.saturating_sub(opened));
            draw_open_row(ui, &summary, self.sim.mark_price(), held_ms);
        }

        let all_scope = self.ledger_scope == ReportScope::All;
        let session: Vec<(usize, &ClosedTrade)> =
            self.sim.closed_trades().iter().enumerate().rev().collect();
        let earlier: Vec<(&str, &ClosedTrade)> = self
            .history_cache
            .as_ref()
            .map(|cache| {
                cache
                    .rows
                    .iter()
                    .rev()
                    .map(|row| (row.symbol.as_str(), &row.trade))
                    .collect()
            })
            .unwrap_or_default();

        if session.is_empty() && earlier.is_empty() {
            if self.position_summary().is_none() {
                ui.add_space(12.0);
                let headline = match self.ledger_scope {
                    ReportScope::Symbol if !self.symbol.is_empty() => {
                        format!("No trades for {}.", self.symbol)
                    }
                    _ => "No simulated trades yet.".to_owned(),
                };
                ui.label(egui::RichText::new(headline).color(theme::TEXT_PRIMARY));
                ui.label(
                    egui::RichText::new(
                        "Close a position and it lands here - this session and every saved one.",
                    )
                    .color(theme::TEXT_SUPPORT)
                    .small(),
                );
                ui.add_space(4.0);
                if ui
                    .button("Open the ticket")
                    .on_hover_text("switch to the Trading tab and place an order")
                    .clicked()
                {
                    action = Some(LedgerAction::OpenTicket);
                }
            }
            self.draw_ledger_disclosure(ui);
            return action;
        }

        let mut rows = Vec::new();
        if !session.is_empty() {
            rows.push(LedgerRow::Header("THIS SESSION", session.len()));
            rows.extend(
                session
                    .iter()
                    .map(|(index, trade)| LedgerRow::Session(*index, trade)),
            );
        }
        if !earlier.is_empty() {
            rows.push(LedgerRow::Header("EARLIER SESSIONS", earlier.len()));
            rows.extend(
                earlier
                    .iter()
                    .map(|(symbol, trade)| LedgerRow::Earlier(symbol, trade)),
            );
        }

        // Totals over everything listed, computed before the scroll so the
        // strip below can never disagree with the rows above it.
        let mut total_trades = 0usize;
        let mut total_wins = 0usize;
        let mut net = Decimal::ZERO;
        for trade in session
            .iter()
            .map(|(_, trade)| *trade)
            .chain(earlier.iter().map(|(_, trade)| *trade))
        {
            total_trades += 1;
            if trade.pnl_points > Decimal::ZERO {
                total_wins += 1;
            }
            net = net.saturating_add(trade.pnl_points);
        }

        let list_height = (ui.available_height() - TOTALS_STRIP_PX).max(LEDGER_ROW_HEIGHT_PX);
        let selected = self.selected_trade;
        let mut clicked: Option<Option<usize>> = None;
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt("paper_trades_ledger")
            .auto_shrink([false, false])
            .max_height(list_height)
            .show_rows(ui, LEDGER_ROW_HEIGHT_PX, rows.len(), |ui, range| {
                for index in range {
                    match &rows[index] {
                        LedgerRow::Header(label, count) => draw_group_header(ui, label, *count),
                        LedgerRow::Session(trade_index, trade) => {
                            let is_selected = selected == Some(*trade_index);
                            let response = draw_ledger_row(ui, trade, None, is_selected, true, tz);
                            if response.navigate {
                                navigate =
                                    Some(LedgerAction::Navigate(trade.opened_ms, trade.closed_ms));
                            } else if response.clicked {
                                clicked = Some((!is_selected).then_some(*trade_index));
                            }
                        }
                        LedgerRow::Earlier(symbol, trade) => {
                            let symbol = all_scope.then_some(*symbol);
                            draw_ledger_row(ui, trade, symbol, false, false, tz);
                        }
                    }
                }
            });
        if let Some(selection) = clicked {
            self.selected_trade = selection;
        }
        if navigate.is_some() {
            action = navigate;
        }

        ui.separator();
        ui.horizontal(|ui| {
            let win_rate = (total_wins * 100)
                .checked_div(total_trades)
                .map_or_else(String::new, |rate| format!(" · {rate}% win"));
            ui.label(
                egui::RichText::new(format!("{total_trades} trades{win_rate}"))
                    .monospace()
                    .color(theme::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} pts", fmt_signed_points(net)))
                        .monospace()
                        .strong()
                        .color(points_color(net)),
                );
            });
        });
        self.draw_ledger_disclosure(ui);
        action
    }

    /// The honesty line under the ledger: unreadable files and skipped rows
    /// are counted, never silently dropped.
    fn draw_ledger_disclosure(&self, ui: &mut egui::Ui) {
        let Some(cache) = &self.history_cache else {
            return;
        };
        if cache.unreadable_files == 0 && cache.problem_rows == 0 {
            return;
        }
        ui.label(
            egui::RichText::new(format!(
                "{} file(s) unreadable, {} row(s) skipped - counted, never silently dropped.",
                cache.unreadable_files, cache.problem_rows,
            ))
            .color(theme::WARN)
            .small(),
        );
    }

    // ------------------------------------------------------------------
    // Report window
    // ------------------------------------------------------------------

    /// The `QUANTICK_PAPER_REPORT_AUTOSTART` hook: the report, scoped to
    /// every symbol — an autostart runs before the first feed settles, so
    /// "the current symbol" would be the wrong one anyway.
    pub(crate) fn autostart_report(&mut self) {
        self.report_symbol = None;
        self.open_report();
    }

    /// Open the report window — the `Report…` button's path.
    fn open_report(&mut self) {
        self.report_open = true;
        if self.report.is_none() && !self.symbol.is_empty() {
            // First open lands on the chart's own symbol; later opens keep
            // whatever the user last chose.
            self.report_symbol = Some(self.symbol.clone());
        }
        self.reload_report();
    }

    fn reload_report(&mut self) {
        self.report = Some(load_history(&self.dir, self.report_symbol.as_deref(), &[]));
        self.report_view = None;
        self.report_symbols = list_symbol_folders(&self.dir);
    }

    /// Rebuild the filtered view when the period, source, timezone or the
    /// loaded history changed. The anchor is the newest trade in scope —
    /// after the Source filter, never a clock.
    fn ensure_report_view(&mut self, tz: TzOffset) {
        let fresh = self.report_view.as_ref().is_some_and(|view| {
            view.period == self.report_period && view.source == self.report_source && view.tz == tz
        });
        if fresh {
            return;
        }
        let Some(history) = &self.report else {
            self.report_view = None;
            return;
        };
        let in_scope: Vec<&HistoryRow> = history
            .rows
            .iter()
            .filter(|row| self.report_source.admits(row.source))
            .collect();
        let hidden_by_source = history.rows.len().saturating_sub(in_scope.len());
        let anchor_ms = in_scope.last().map(|row| row.trade.closed_ms);
        let cutoff = anchor_ms.and_then(|anchor| self.report_period.cutoff_ms(anchor, tz));
        let trades: Vec<ClosedTrade> = in_scope
            .iter()
            .map(|row| &row.trade)
            .filter(|trade| cutoff.is_none_or(|cutoff| trade.closed_ms >= cutoff))
            .cloned()
            .collect();
        let hidden_before = in_scope.len().saturating_sub(trades.len());
        let report = PerformanceReport::from_trades(&trades);
        self.report_view = Some(ReportView {
            period: self.report_period,
            source: self.report_source,
            tz,
            anchor_ms,
            hidden_before,
            hidden_by_source,
            trades,
            report,
        });
    }

    /// The performance report, computed from what is actually on disk.
    /// Non-modal by the app's contract — dimming the chart while a
    /// simulated position is open would be dangerous.
    pub fn draw_report_window(&mut self, ctx: &egui::Context, tz: TzOffset) {
        if !self.report_open {
            return;
        }
        self.ensure_report_view(tz);
        let mut open = true;
        let mut reload = false;
        egui::Window::new("Simulated performance")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(600.0, 520.0))
            .min_width(REPORT_MIN_WIDTH_PX)
            .min_height(REPORT_MIN_HEIGHT_PX)
            .show(ctx, |ui| {
                reload = self.draw_report_filters(ui);
                ui.separator();
                match &self.report_view {
                    Some(view) if !view.trades.is_empty() => {
                        draw_report_tiles(ui, &view.report);
                        ui.add_space(8.0);
                        draw_equity_curve(ui, view);
                        ui.add_space(4.0);
                        // The grids fill whatever height the user gave the
                        // window and scroll inside it. Letting them take
                        // their content height instead (auto-shrink) made
                        // the window itself grow to fit and refuse to
                        // resize down — the "stuck huge" report.
                        egui::ScrollArea::vertical()
                            .id_salt("paper_report_grids")
                            .auto_shrink([false, false])
                            .max_height(
                                (ui.available_height() - REPORT_FOOTER_RESERVE_PX)
                                    .max(REPORT_GRID_MIN_H_PX),
                            )
                            .show(ui, |ui| {
                                draw_report_grid(ui, &view.report);
                                draw_side_grid(ui, &view.report);
                                draw_exit_reason_grid(ui, &view.report);
                            });
                    }
                    _ => {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("No saved trades for this filter.")
                                .color(theme::TEXT_PRIMARY),
                        );
                        let scope = self
                            .report_symbol
                            .as_deref()
                            .unwrap_or("any symbol")
                            .to_owned();
                        let hidden_by_source = self
                            .report_view
                            .as_ref()
                            .map_or(0, |view| view.hidden_by_source);
                        let text = if hidden_by_source > 0 {
                            // The trades exist; the one control that would
                            // reveal them must be named, not implied.
                            format!(
                                "{scope} has {hidden_by_source} trade(s) behind the Source \
                                 filter. Try Source \"Both\".",
                            )
                        } else {
                            format!(
                                "{scope} has no trades in {}. Try \"All\", or another symbol.",
                                self.report_period.phrase(),
                            )
                        };
                        ui.label(egui::RichText::new(text).color(theme::TEXT_SUPPORT).small());
                        let default_symbol = (!self.symbol.is_empty()).then(|| self.symbol.clone());
                        if (self.report_period != ReportPeriod::All
                            || self.report_symbol != default_symbol)
                            && ui
                                .button("Clear filters")
                                .on_hover_text("back to this symbol, all time")
                                .clicked()
                        {
                            self.report_symbol = default_symbol;
                            self.report_period = ReportPeriod::All;
                            self.report_source = SourceFilter::Real;
                            reload = true;
                        }
                    }
                }
                ui.add_space(6.0);
                if let Some(history) = &self.report {
                    if history.unreadable_files > 0 || history.problem_rows > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} file(s) unreadable, {} row(s) skipped - counted, never \
                                 silently dropped.",
                                history.unreadable_files, history.problem_rows,
                            ))
                            .color(theme::WARN)
                            .small(),
                        );
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} trade(s) across {} file(s) in scope",
                            history.rows.len(),
                            history.files
                        ))
                        .color(theme::TEXT_MUTED)
                        .small(),
                    );
                }
                ui.label(
                    egui::RichText::new(
                        "All figures are simulated, in points (price units × quantity) - \
                         the workspace knows no per-instrument currency value.",
                    )
                    .color(theme::TEXT_MUTED)
                    .small(),
                );
            });
        if reload {
            self.reload_report();
        }
        if !open {
            self.report_open = false;
        }
    }

    /// The filter row (symbol combo + period pills + refresh) and the
    /// support line stating the anchor out loud. Returns whether the
    /// history must be re-read.
    fn draw_report_filters(&mut self, ui: &mut egui::Ui) -> bool {
        let mut reload = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Symbol")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            let selected = self
                .report_symbol
                .clone()
                .unwrap_or_else(|| "All symbols".to_owned());
            let symbols = self.report_symbols.clone();
            egui::ComboBox::from_id_salt("paper_report_symbol")
                .width(140.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.report_symbol.is_none(), "All symbols")
                        .clicked()
                    {
                        self.report_symbol = None;
                        reload = true;
                    }
                    for symbol in symbols {
                        let on = self.report_symbol.as_deref() == Some(symbol.as_str());
                        if ui.selectable_label(on, &symbol).clicked() {
                            self.report_symbol = Some(symbol);
                            reload = true;
                        }
                    }
                });
            ui.separator();
            ui.label(
                egui::RichText::new("Source")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            for source in SourceFilter::PILLS {
                let on = self.report_source == source;
                if pill_toggle(ui, source.label(), on, source.hover()).clicked() && !on {
                    self.report_source = source;
                    self.report_view = None;
                }
            }
            ui.separator();
            ui.label(
                egui::RichText::new("Period")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            for period in ReportPeriod::PILLS {
                let on = self.report_period == period;
                if pill_toggle(
                    ui,
                    period.label(),
                    on,
                    "measured back from the newest saved trade in scope, not the wall clock",
                )
                .clicked()
                    && !on
                {
                    self.report_period = period;
                    self.report_view = None;
                }
            }
            let response = ui
                .add(
                    egui::TextEdit::singleline(&mut self.report_custom_text)
                        .desired_width(CUSTOM_PERIOD_FIELD_PX)
                        .hint_text("2d"),
                )
                .on_hover_text("type a period - 45m, 12h, 2d or 1w - and press Enter");
            if response.lost_focus() {
                match parse_period(&self.report_custom_text) {
                    // A valid entry applies on blur as well as on Enter —
                    // a typed "2d" must never do nothing quietly.
                    Some(period_ms) => {
                        self.report_period = ReportPeriod::Custom(period_ms);
                        self.report_view = None;
                    }
                    // Only Enter earns the refusal toast: clicking away
                    // from an abandoned half-entry is not a submission.
                    None if ui.input(|input| input.key_pressed(egui::Key::Enter)) => self
                        .show_toast(format!(
                            "SIM: could not read `{}` as a period - use 45m, 12h, 2d or 1w",
                            self.report_custom_text.trim(),
                        )),
                    None => {}
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icons::ARROWS_CLOCKWISE)
                    .on_hover_text("re-read the history folder")
                    .clicked()
                {
                    reload = true;
                }
                if ui
                    .small_button(icons::DOWNLOAD_SIMPLE)
                    .on_hover_text(
                        "import trades from another folder - copies, the folder keeps its files",
                    )
                    .clicked()
                {
                    self.start_import();
                }
            });
        });
        if let Some(view) = &self.report_view {
            let text = match view.anchor_ms {
                Some(anchor) => {
                    let mut text = format!(
                        "{} up to {} (newest saved trade, not the wall clock)",
                        view.period.phrase(),
                        fmt_offset_date(anchor, view.tz),
                    );
                    if view.hidden_before > 0 {
                        // "Where did my old trades go" gets a literal
                        // answer: they are saved, before this window.
                        text.push_str(&format!(
                            " - {} older saved trade(s) before this window",
                            view.hidden_before,
                        ));
                    }
                    if view.hidden_by_source > 0 {
                        text.push_str(&format!(
                            " - {} trade(s) behind the Source filter",
                            view.hidden_by_source,
                        ));
                    }
                    text
                }
                None if view.hidden_by_source > 0 => format!(
                    "no saved trades in this scope - {} trade(s) sit behind the Source \
                     filter (try Both)",
                    view.hidden_by_source,
                ),
                None => "no saved trades in this scope".to_owned(),
            };
            ui.label(egui::RichText::new(text).color(theme::TEXT_SUPPORT).small());
        }
        reload
    }

    // ------------------------------------------------------------------
    // Toast
    // ------------------------------------------------------------------

    /// The transient message slot; newest wins, expires on its own. Runs
    /// last in the frame, so it also settles the per-frame handshakes: the
    /// dock-hover link is cleared (the chart already read it) and a
    /// finished export lands its toast.
    pub fn draw_toast(&mut self, ctx: &egui::Context, now: Instant) {
        self.hovered_order = None;
        self.poll_export();
        self.poll_import();
        let Some(toast) = &self.toast else {
            return;
        };
        if now.saturating_duration_since(toast.shown_at) >= Duration::from_millis(TOAST_MS) {
            self.toast = None;
            return;
        }
        let message = toast.message.clone();
        egui::Area::new(egui::Id::new("paper_toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -TOAST_LIFT_PX))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::TAG_BG)
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(message).color(theme::TEXT_PRIMARY));
                    });
            });
    }

    pub(crate) fn show_toast(&mut self, message: String) {
        self.toast = Some(Toast {
            message,
            shown_at: Instant::now(),
        });
    }

    // ------------------------------------------------------------------
    // Import
    // ------------------------------------------------------------------

    /// Ask for a folder whose trades should be copied into the journal
    /// home, off the UI thread — the manual way in for legacy folders the
    /// startup consolidation cannot reach (an old working directory, a
    /// backup). One dialog at a time.
    fn start_import(&mut self) {
        if self.import_rx.is_some() {
            self.show_toast("SIM: an import is already running.".to_owned());
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let start = self.dir.clone();
        std::thread::Builder::new()
            .name("quantick-trades-import-picker".into())
            .spawn(move || {
                let mut dialog =
                    rfd::FileDialog::new().set_title("Import trades from a folder (copies)");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_folder());
            })
            .expect("spawn trades-import picker thread");
        self.import_rx = Some(receiver);
    }

    /// Land the picked folder: copy its history into the journal home and
    /// re-read, so the report answers with the merged truth.
    fn poll_import(&mut self) {
        let Some(receiver) = &self.import_rx else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.import_rx = None;
        let Some(source) = choice else { return };
        let summary = crate::paper_home::consolidate_into(&self.dir, &[source]);
        self.show_toast(crate::paper_home::import_toast(&summary));
        self.history_cache = None;
        if self.report_open {
            self.reload_report();
        }
    }

    // ------------------------------------------------------------------
    // Export
    // ------------------------------------------------------------------

    /// Write everything the ledger lists (this session plus the saved
    /// history, in the ledger's scope) to one CSV, off the UI thread. The
    /// toast answers with the path or the failure.
    pub fn start_export(&mut self) {
        if self.export_rx.is_some() {
            self.show_toast("SIM: an export is already running.".to_owned());
            return;
        }
        if self.history_cache.is_none() {
            self.reload_ledger();
        }
        let mut rows: Vec<HistoryRow> = Vec::new();
        if let Some(cache) = &self.history_cache {
            rows.extend(cache.rows.iter().cloned());
        }
        rows.extend(
            self.sim
                .closed_trades()
                .iter()
                .enumerate()
                .map(|(index, trade)| HistoryRow {
                    symbol: self.symbol.clone(),
                    // The source the trade actually closed under — the
                    // session may have flipped live/replay since.
                    source: Some(
                        self.session_trade_sources
                            .get(index)
                            .copied()
                            .unwrap_or(self.session_source),
                    ),
                    trade: trade.clone(),
                }),
        );
        rows.sort_by_key(|row| (row.trade.closed_ms, row.trade.opened_ms));
        if rows.is_empty() {
            self.show_toast("SIM: nothing to export yet - close a trade first.".to_owned());
            return;
        }
        let text = export_csv(&rows);
        let count = rows.len();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(0))
            .unwrap_or(0);
        let dir = self.dir.clone();
        let path = dir.join(format!("export-{}.csv", utc_compact(stamp)));
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(&path, text))
                .map(|()| (path, count))
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.export_rx = Some(receiver);
    }

    /// Land the export's result, if it arrived.
    fn poll_export(&mut self) {
        let Some(receiver) = &self.export_rx else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.export_rx = None;
        match result {
            Ok((path, count)) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_TRADES_EXPORTED",
                    path = %path.display(),
                    trades = count,
                    "exported the simulated trade history"
                );
                self.show_toast(format!("Exported {count} trades to {}", elide_path(&path)));
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_TRADES_EXPORT_FAILED",
                    %error,
                    action = "export_not_saved",
                    "could not write the trade export"
                );
                self.show_toast(
                    "SIM: could not write the export - see the log for the path.".to_owned(),
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Events, journal, parsing
    // ------------------------------------------------------------------

    /// One funnel for everything the simulator reports: closures are
    /// journaled, fills and closures toast, rejections teach.
    fn handle_events(&mut self, events: Vec<SimEvent>) {
        for event in events {
            match event {
                SimEvent::Rejected(reason) => self.show_toast(format!("SIM: {reason}")),
                SimEvent::BracketDropped { reason } => {
                    self.show_toast(format!("SIM: dropped at the fill - {reason}"));
                }
                SimEvent::Filled(fill) => {
                    if matches!(fill.role, quantick_sim::FillRole::Entry(_)) {
                        self.show_toast(format!(
                            "SIM fill: {} {} @ {}",
                            side_word(fill.side),
                            fmt_decimal(fill.quantity),
                            fmt_decimal(fill.price),
                        ));
                    }
                }
                SimEvent::Closed(trade) => {
                    let saved = self.journal(&trade);
                    if self.report_open {
                        // The report reads from disk and the close just
                        // wrote to disk; re-read now or the window shows
                        // yesterday until the manual refresh — the "my
                        // trade is missing" report.
                        self.reload_report();
                    }
                    // The toast slot holds one message: a healthy "closed"
                    // must not paint over the could-not-save warning.
                    if saved {
                        self.show_toast(format!(
                            "SIM closed: {} {} → {} pts ({})",
                            position_word(trade.side),
                            fmt_decimal(trade.quantity),
                            fmt_signed_points(trade.pnl_points),
                            trade.exit_reason.as_str().replace('_', " "),
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    /// Append one closed trade to the session's history file, creating the
    /// file (with its header) on the first close. Also records the source
    /// the trade closed under, for the export. Returns whether the write
    /// landed — a failed write warns once and never crashes a trading
    /// session, but its caller must not paint a healthy toast over the
    /// warning.
    fn journal(&mut self, trade: &ClosedTrade) -> bool {
        self.session_trade_sources.push(self.session_source);
        if self.symbol.is_empty() {
            return false;
        }
        let folder = self.dir.join(sanitize_symbol(&self.symbol));
        let path = self
            .journal_path
            .get_or_insert_with(|| free_session_path(&folder, &utc_compact(trade.closed_ms)));
        if self.session_journal_paths.last() != Some(path) {
            self.session_journal_paths.push(path.clone());
        }
        let mut text = String::new();
        if !path.exists() {
            text.push_str(&history::write_header(&self.symbol, self.session_source));
        }
        text.push_str(&history::write_trade(trade));
        let written = std::fs::create_dir_all(&folder).and_then(|()| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&*path)
                .and_then(|mut file| file.write_all(text.as_bytes()))
        });
        if let Err(error) = &written
            && !self.journal_warned
        {
            self.journal_warned = true;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_TRADE_JOURNAL_FAILED",
                path = %path.display(),
                %error,
                action = "trade_not_saved",
                "could not append to the paper-trading history"
            );
            self.show_toast(
                "SIM: could not save the trade history - see the log for the path.".to_owned(),
            );
        }
        written.is_ok()
    }

    fn parse_quantity(&mut self) -> Option<Decimal> {
        match self.qty_text.trim().parse::<Decimal>() {
            Ok(quantity) if quantity > Decimal::ZERO => Some(quantity),
            _ => {
                self.show_toast(format!(
                    "SIM: quantity must be a positive number - got `{}`",
                    self.qty_text.trim(),
                ));
                None
            }
        }
    }

    /// Turn the optional offset fields into absolute protective prices
    /// around `reference` (the entry's own price, or the mark for market
    /// orders). `None` means a field failed to parse and was toasted.
    fn parse_bracket(&mut self, side: Side, reference: Decimal) -> Option<Bracket> {
        let stop_offset = match parse_offset(&self.stop_offset_text) {
            Ok(value) => value,
            Err(got) => {
                self.show_toast(format!(
                    "SIM: the stop offset must be a positive number of points - got `{got}`",
                ));
                return None;
            }
        };
        let profit_offset = match parse_offset(&self.profit_offset_text) {
            Ok(value) => value,
            Err(got) => {
                self.show_toast(format!(
                    "SIM: the profit offset must be a positive number of points - got `{got}`",
                ));
                return None;
            }
        };
        let (stop_loss, take_profit) = match side {
            Side::Buy => (
                stop_offset.map(|offset| reference.saturating_sub(offset)),
                profit_offset.map(|offset| reference.saturating_add(offset)),
            ),
            Side::Sell => (
                stop_offset.map(|offset| reference.saturating_add(offset)),
                profit_offset.map(|offset| reference.saturating_sub(offset)),
            ),
        };
        Some(Bracket {
            stop_loss,
            take_profit,
        })
    }
}

/// The three headline tiles: NET (the one coloured number in the window),
/// WIN RATE, PROFIT FACTOR — each with its denominator under it, so every
/// tile is a self-explaining fact rather than a floating statistic.
fn draw_report_tiles(ui: &mut egui::Ui, report: &PerformanceReport) {
    let width = ((ui.available_width() - 2.0 * TILE_GUTTER_PX) / 3.0).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TILE_GUTTER_PX;
        draw_tile(
            ui,
            width,
            "NET",
            fmt_signed_points(report.net_points),
            "pts",
            points_color(report.net_points),
            format!("{} trades", report.trades),
        );
        draw_tile(
            ui,
            width,
            "WIN RATE",
            report
                .win_rate_pct
                .map_or_else(|| "—".to_owned(), |rate| fmt_decimal(rate.round_dp(0))),
            "%",
            theme::TEXT_PRIMARY,
            format!(
                "{} W · {} L · {} scratch",
                report.wins, report.losses, report.scratches
            ),
        );
        draw_tile(
            ui,
            width,
            "PROFIT FACTOR",
            report
                .profit_factor
                .map_or_else(|| "—".to_owned(), fmt_points),
            "",
            theme::TEXT_PRIMARY,
            format!(
                "+{} / -{}",
                fmt_points(report.gross_profit),
                fmt_points(report.gross_loss)
            ),
        );
    });
}

/// One headline tile: caption, hero value with its unit, denominator line.
fn draw_tile(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    value: String,
    unit: &str,
    value_color: egui::Color32,
    subline: String,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, TILE_HEIGHT_PX), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::same(4.0), theme::INSET);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(4.0),
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.text(
        rect.min + egui::vec2(12.0, 6.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    let value_galley = painter.layout_no_wrap(
        value,
        egui::FontId::monospace(HEADLINE_FONT_PX),
        value_color,
    );
    let value_w = value_galley.size().x;
    let baseline = rect.top() + 38.0;
    painter.galley(
        egui::pos2(rect.left() + 12.0, baseline - value_galley.size().y),
        value_galley,
        value_color,
    );
    if !unit.is_empty() {
        painter.text(
            egui::pos2(rect.left() + 12.0 + value_w + 4.0, baseline - 2.0),
            egui::Align2::LEFT_BOTTOM,
            unit,
            egui::FontId::monospace(11.0),
            theme::TEXT_MUTED,
        );
    }
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        subline,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
}

/// The realized equity curve, `E_k` by trade index — the closing order
/// that defines the drawdown, so calling the axis "time" would misstate
/// what is plotted. One quiet line; a diverging fill against the zero
/// baseline answers "was I ever under water?" at a glance; the deepest
/// drawdown is annotated so the number in the grid below is locatable.
fn draw_equity_curve(ui: &mut egui::Ui, view: &ReportView) {
    let n = view.trades.len();
    if n == 0 {
        return;
    }
    ui.label(caption("REALIZED EQUITY"));
    let height =
        (ui.available_height() - CURVE_GRID_RESERVE_PX).clamp(CURVE_MIN_H_PX, CURVE_MAX_H_PX);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);

    // The walk E_0..E_n, with E_0 = 0 before the first trade.
    let mut equity = Vec::with_capacity(n + 1);
    equity.push(0.0_f32);
    let mut sum = Decimal::ZERO;
    for trade in &view.trades {
        sum = sum.saturating_add(trade.pnl_points);
        equity.push(sum.to_f64().unwrap_or_default() as f32);
    }
    let (mut low, mut high) = (0.0_f32, 0.0_f32);
    for value in &equity {
        low = low.min(*value);
        high = high.max(*value);
    }
    let span = (high - low).max(f32::EPSILON);
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + CURVE_GUTTER_PX, rect.top() + 4.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 14.0),
    );
    let x_at = |k: usize| plot.left() + plot.width() * (k as f32) / (n as f32);
    let y_at = |value: f32| plot.bottom() - (value - low) / span * plot.height();
    let zero_y = y_at(0.0);

    // Gridlines + y ticks at the floor, zero and the ceiling.
    let mut ticks = vec![low, 0.0, high];
    ticks.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
    for tick in ticks {
        let y = y_at(tick);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(
                    theme::CONTROL.r(),
                    theme::CONTROL.g(),
                    theme::CONTROL.b(),
                    CURVE_GRID_LINE_ALPHA,
                ),
            ),
        );
        painter.text(
            egui::pos2(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            fmt_points(Decimal::from_f64_retain(f64::from(tick)).unwrap_or_default()),
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }

    // Diverging fill to the zero baseline: gains ground in BUY, losses in
    // SELL, split exactly at each crossing.
    let stride = n.div_ceil(CURVE_MAX_POINTS).max(1);
    let fill_of = |value: f32| {
        let color = if value >= 0.0 {
            theme::BUY
        } else {
            theme::SELL
        };
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), CURVE_FILL_ALPHA)
    };
    let mut drawn_points = vec![egui::pos2(x_at(0), y_at(equity[0]))];
    let mut previous = 0usize;
    let mut next = stride;
    while previous < n {
        let k = next.min(n);
        let (a, b) = (equity[previous], equity[k]);
        let (x0, x1) = (x_at(previous), x_at(k));
        if a == 0.0 && b == 0.0 {
            // Nothing to fill on the baseline itself.
        } else if (a >= 0.0) == (b >= 0.0) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, y_at(a)),
                    egui::pos2(x1, y_at(b)),
                    egui::pos2(x1, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(if a == 0.0 { b } else { a }),
                egui::Stroke::NONE,
            ));
        } else {
            // The step crosses zero: split it at the exact crossing.
            let t = a / (a - b);
            let xc = x0 + (x1 - x0) * t;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, y_at(a)),
                    egui::pos2(xc, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(a),
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(xc, zero_y),
                    egui::pos2(x1, y_at(b)),
                    egui::pos2(x1, zero_y),
                ],
                fill_of(b),
                egui::Stroke::NONE,
            ));
        }
        drawn_points.push(egui::pos2(x1, y_at(b)));
        previous = k;
        next += stride;
    }

    // The zero baseline over the fill, then the line over everything.
    painter.line_segment(
        [
            egui::pos2(plot.left(), zero_y),
            egui::pos2(plot.right(), zero_y),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.add(egui::Shape::line(
        drawn_points,
        egui::Stroke::new(1.5_f32, theme::TEXT_PRIMARY),
    ));
    if n == 1 {
        painter.circle_filled(
            egui::pos2(x_at(1), y_at(equity[1])),
            2.0,
            theme::TEXT_PRIMARY,
        );
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "one trade — no curve yet",
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }

    // The deepest drawdown, locatable: its peak level dotted, the drop
    // solid, the number on a chip at the drop's midpoint.
    if let Some((peak, trough)) = view.report.max_drawdown_span {
        let (peak, trough) = (peak as usize, trough as usize);
        if trough <= n {
            let peak_y = y_at(equity[peak]);
            let trough_x = x_at(trough);
            let trough_y = y_at(equity[trough]);
            painter.extend(egui::Shape::dashed_line(
                &[egui::pos2(x_at(peak), peak_y), egui::pos2(trough_x, peak_y)],
                egui::Stroke::new(1.0_f32, theme::SELL),
                2.0,
                3.0,
            ));
            painter.line_segment(
                [egui::pos2(trough_x, peak_y), egui::pos2(trough_x, trough_y)],
                egui::Stroke::new(1.0_f32, theme::SELL),
            );
            let label = format!("-{} pts", fmt_points(view.report.max_drawdown_points));
            let galley =
                painter.layout_no_wrap(label, egui::FontId::monospace(10.0), theme::CHIP_INK);
            let center = egui::pos2((x_at(peak) + trough_x) / 2.0, (peak_y + trough_y) / 2.0);
            let bg = egui::Rect::from_center_size(center, galley.size() + egui::vec2(8.0, 4.0));
            painter.rect_filled(bg, egui::Rounding::same(2.0), theme::SELL);
            painter.galley(bg.min + egui::vec2(4.0, 2.0), galley, theme::CHIP_INK);
        }
    }

    // Axis words: first and last trade index, with the unit between them.
    painter.text(
        egui::pos2(plot.left(), rect.bottom()),
        egui::Align2::LEFT_BOTTOM,
        "1",
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(plot.right(), rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        n.to_string(),
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(plot.center().x, rect.bottom()),
        egui::Align2::CENTER_BOTTOM,
        "trades",
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    if stride > 1 {
        painter.text(
            egui::pos2(plot.left() + 4.0, plot.top() + 2.0),
            egui::Align2::LEFT_TOP,
            format!(
                "curve downsampled to ~{CURVE_MAX_POINTS} points; every metric uses every trade"
            ),
            egui::FontId::proportional(10.0),
            theme::TEXT_SUPPORT,
        );
    }

    // Hover: snap to the nearest trade index and answer in the ledger's
    // vocabulary, so the two surfaces never disagree.
    if let Some(pointer) = response.hover_pos()
        && plot.contains(pointer)
    {
        let k = (((pointer.x - plot.left()) / plot.width()) * (n as f32))
            .round()
            .clamp(0.0, n as f32) as usize;
        let x = x_at(k);
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, theme::TEXT_FAINT),
        );
        let head = if k == 0 {
            "start".to_owned()
        } else {
            let trade = &view.trades[k - 1];
            format!(
                "#{k} · {} {} · {} pts",
                position_word(trade.side),
                fmt_decimal(trade.quantity),
                fmt_signed_points(trade.pnl_points),
            )
        };
        let lines = [
            head,
            format!(
                "equity {} pts",
                fmt_signed_points(
                    Decimal::from_f64_retain(f64::from(equity[k])).unwrap_or_default()
                )
            ),
            if k == 0 {
                String::new()
            } else {
                fmt_utc_minute(view.trades[k - 1].closed_ms)
            },
        ];
        draw_hover_card(&painter, rect, pointer, &lines);
    }
}

/// A small TAG_BG hover card with up to three lines, clamped into `rect`.
fn draw_hover_card(
    painter: &egui::Painter,
    rect: egui::Rect,
    pointer: egui::Pos2,
    lines: &[String],
) {
    let font = egui::FontId::monospace(10.0);
    let galleys: Vec<_> = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| painter.layout_no_wrap(line.clone(), font.clone(), theme::TEXT_PRIMARY))
        .collect();
    if galleys.is_empty() {
        return;
    }
    let width = galleys
        .iter()
        .map(|galley| galley.size().x)
        .fold(0.0_f32, f32::max)
        + 12.0;
    let height: f32 = galleys.iter().map(|galley| galley.size().y).sum::<f32>() + 10.0;
    let mut origin = pointer + egui::vec2(10.0, -height - 6.0);
    origin.x = origin.x.min(rect.right() - width).max(rect.left());
    origin.y = origin.y.max(rect.top());
    let card = egui::Rect::from_min_size(origin, egui::vec2(width, height));
    painter.rect_filled(card, egui::Rounding::same(4.0), theme::TAG_BG);
    let mut y = card.top() + 5.0;
    for galley in galleys {
        let advance = galley.size().y;
        painter.galley(
            egui::pos2(card.left() + 6.0, y),
            galley,
            theme::TEXT_PRIMARY,
        );
        y += advance;
    }
}

/// The metric grid: one metric per row, the explanation on hover, honest
/// blanks (`—`) where a ratio has no denominator. Every value here has a
/// structurally fixed sign, so none of them is coloured.
fn draw_report_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    let blank = || "—".to_owned();
    let pts = |value: Decimal| format!("{} pts", fmt_points(value));
    let opt_pts = |value: Option<Decimal>| {
        value.map_or_else(blank, |value| format!("{} pts", fmt_points(value)))
    };
    let opt_plain = |value: Option<Decimal>| value.map_or_else(blank, fmt_points);
    let duration = |value: Option<i64>| value.map_or_else(blank, fmt_duration_ms);
    egui::Grid::new("paper_report_grid")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String, explain: &str| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED))
                    .on_hover_text(explain.to_owned());
                ui.label(value);
                ui.end_row();
            };
            row(
                "trades",
                format!(
                    "{} ({} long / {} short)",
                    report.trades, report.long_trades, report.short_trades
                ),
                "closed round trips under the current filter",
            );
            row(
                "max drawdown",
                pts(report.max_drawdown_points),
                "deepest drop of realized equity below its running peak, in closing order",
            );
            row(
                "max run-up",
                pts(report.max_runup_points),
                "highest rise of realized equity above its running trough — the drawdown's mirror",
            );
            row(
                "recovery factor",
                opt_plain(report.recovery_factor),
                "net profit divided by the max drawdown; blank while nothing drew down",
            );
            row(
                "gross profit / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.gross_profit),
                    fmt_points(report.gross_loss)
                ),
                "winners' points and losers' magnitude, before netting",
            );
            row(
                "avg win / loss",
                format!(
                    "{} / {} pts",
                    opt_plain(report.avg_win),
                    opt_plain(report.avg_loss),
                ),
                "mean winner and mean loser magnitude",
            );
            row(
                "payoff ratio",
                opt_plain(report.payoff_ratio),
                "average win divided by average loss — how much a winner pays for a loser",
            );
            row(
                "expectancy",
                report
                    .expectancy_points
                    .map_or_else(blank, |value| format!("{} pts/trade", fmt_points(value))),
                "net profit per trade: what one average trade pays",
            );
            row(
                "std deviation",
                opt_pts(report.stddev_points),
                "sample standard deviation of trade points (N−1); blank below two trades",
            );
            row(
                "longest streaks",
                format!(
                    "{} wins / {} losses",
                    report.max_consecutive_wins, report.max_consecutive_losses
                ),
                "longest consecutive runs, in closing order; a scratch breaks both",
            );
            row(
                "avg / median duration",
                format!(
                    "{} / {}",
                    duration(report.avg_duration_ms),
                    duration(report.median_duration_ms),
                ),
                "trade lifetimes in venue time",
            );
            row(
                "winner vs loser duration",
                format!(
                    "{} / {}",
                    duration(report.avg_win_duration_ms),
                    duration(report.avg_loss_duration_ms),
                ),
                "how long winners run against how long losers are held",
            );
            row(
                "largest win / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.largest_win),
                    fmt_points(report.largest_loss)
                ),
                "best single trade and worst single trade magnitude",
            );
            row(
                "avg winner MAE",
                report.avg_winner_mae_points.map_or_else(blank, |value| {
                    format!(
                        "{} pts (over {} of {})",
                        fmt_points(value),
                        report.winners_with_mae,
                        report.wins
                    )
                }),
                "how far the average winner first ran against you; only trades that \
                 recorded an excursion count, and the denominator says how many did",
            );
            row(
                "avg loser MFE",
                report.avg_loser_mfe_points.map_or_else(blank, |value| {
                    format!(
                        "{} pts (over {} of {})",
                        fmt_points(value),
                        report.losers_with_mfe,
                        report.losses
                    )
                }),
                "how far the average loser was in profit before it lost; same disclosure",
            );
        });
}

/// Long and short, side by side, with the core metrics in each column.
fn draw_side_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    ui.add_space(8.0);
    ui.label(caption("LONG VS SHORT"));
    fn opt_plain(value: Option<Decimal>) -> String {
        value.map_or_else(|| "—".to_owned(), fmt_points)
    }
    /// One side-by-side row's value, computed per column.
    type SideValue = Box<dyn Fn(&SideReport) -> String>;
    let side_rows: [(&str, SideValue); 7] = [
        ("trades", Box::new(|side| side.trades.to_string())),
        (
            "net",
            Box::new(|side| format!("{} pts", fmt_signed_points(side.net_points))),
        ),
        (
            "win rate",
            Box::new(|side| {
                side.win_rate_pct
                    .map_or_else(|| "—".to_owned(), |rate| format!("{}%", fmt_points(rate)))
            }),
        ),
        (
            "profit factor",
            Box::new(|side| opt_plain(side.profit_factor)),
        ),
        ("avg win", Box::new(|side| opt_plain(side.avg_win))),
        ("avg loss", Box::new(|side| opt_plain(side.avg_loss))),
        (
            "expectancy",
            Box::new(|side| opt_plain(side.expectancy_points)),
        ),
    ];
    egui::Grid::new("paper_report_sides")
        .num_columns(3)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(egui::RichText::new("LONG").color(theme::BUY).small());
            ui.label(egui::RichText::new("SHORT").color(theme::SELL).small());
            ui.end_row();
            for (label, value_of) in &side_rows {
                ui.label(egui::RichText::new(*label).color(theme::TEXT_MUTED));
                ui.label(value_of(&report.long));
                ui.label(value_of(&report.short));
                ui.end_row();
            }
        });
}

/// How trades that left one way performed — count and net per exit reason.
fn draw_exit_reason_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    if report.by_exit_reason.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(caption("BY EXIT REASON"));
    egui::Grid::new("paper_report_reasons")
        .num_columns(3)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for reason in &report.by_exit_reason {
                ui.label(
                    egui::RichText::new(reason.reason.as_str().replace('_', " "))
                        .color(theme::TEXT_MUTED),
                );
                ui.label(format!("{} trade(s)", reason.trades));
                ui.label(format!("{} pts", fmt_signed_points(reason.net_points)));
                ui.end_row();
            }
        });
}

/// Read every history file under `dir` (one symbol's folder, or all of
/// them), remembering each row's symbol and skipping every path in
/// `exclude` (the live session's own files — their trades are already in
/// the simulator). Missing folders are simply empty, not an error.
fn load_history(dir: &Path, symbol: Option<&str>, exclude: &[PathBuf]) -> LoadedHistory {
    let mut folders = Vec::new();
    match symbol {
        Some(symbol) => folders.push(dir.join(sanitize_symbol(symbol))),
        None => {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        folders.push(path);
                    }
                }
                folders.sort();
            }
        }
    }
    let mut rows = Vec::new();
    let mut files = 0usize;
    let mut unreadable_files = 0usize;
    let mut problem_rows = 0usize;
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let folder_symbol = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == history::FILE_EXTENSION)
            })
            .collect();
        paths.sort();
        for path in paths {
            if exclude.contains(&path) {
                continue;
            }
            files += 1;
            match std::fs::read_to_string(&path).map_err(|error| error.to_string()) {
                Ok(text) => match history::parse(&text) {
                    Ok(parsed) => {
                        problem_rows += parsed.problems.len();
                        let symbol = parsed.symbol.unwrap_or_else(|| folder_symbol.clone());
                        let source = parsed.source;
                        rows.extend(parsed.trades.into_iter().map(|trade| HistoryRow {
                            symbol: symbol.clone(),
                            source,
                            trade,
                        }));
                    }
                    Err(_) => unreadable_files += 1,
                },
                Err(_) => unreadable_files += 1,
            }
        }
    }
    // Files are per-session; merge into one closing-order timeline so the
    // drawdown walk is honest across sessions.
    rows.sort_by_key(|row| (row.trade.closed_ms, row.trade.opened_ms));
    LoadedHistory {
        rows,
        files,
        unreadable_files,
        problem_rows,
    }
}

/// Aggregate loaded history rows — the tests' shortcut from a journal on
/// disk to a report.
#[cfg(test)]
fn report_from_history(history: &LoadedHistory) -> PerformanceReport {
    let trades: Vec<ClosedTrade> = history.rows.iter().map(|row| row.trade.clone()).collect();
    PerformanceReport::from_trades(&trades)
}

/// The frame geometry every paper paint helper reads: one struct so a tag,
/// a chip and a line can never disagree about where the plot ends.
struct PaintCtx<'a> {
    painter: &'a egui::Painter,
    chart_rect: egui::Rect,
    /// Right edge of the interactive plot (the lane divider when a live
    /// lane is up, the chart's edge otherwise) — tags anchor inside it.
    tag_right: f32,
    /// Left edge of the price axis — lines run to it, gutter chips sit past
    /// it.
    axis_x: f32,
    scale: &'a PriceScale,
    /// The last-price chip's row, which every gutter chip dodges.
    reserved_chip_y: Option<f32>,
    /// The pointer, `Some` only on the pane that owns paper input.
    pointer: Option<egui::Pos2>,
}

/// One protective leg's paint inputs (see `draw_bracket_leg`).
struct LegPaint<'a> {
    position: &'a Position,
    level: Option<Decimal>,
    /// The other leg's level, for the R:R read while dragging.
    other_level: Option<Decimal>,
    word: &'static str,
    color: egui::Color32,
    amend: PaperDrag,
    create: PaperDrag,
}

impl PaintCtx<'_> {
    fn in_range(&self, y: f32) -> bool {
        y >= self.chart_rect.top() && y <= self.chart_rect.bottom()
    }

    /// Whether the pointer is within grab range of the line at `y`.
    fn hovers_line(&self, y: f32) -> bool {
        self.pointer.is_some_and(|pointer| {
            self.chart_rect.contains(pointer) && (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        })
    }

    /// The line itself: hover thickens it, a drag thickens it further and
    /// paints the drawings' halo treatment beneath, so a grabbed stop feels
    /// identical to a grabbed drawing.
    fn level_line(
        &self,
        y: f32,
        color: egui::Color32,
        dashed: bool,
        base_width: f32,
        hovered: bool,
        dragged: bool,
    ) {
        let width = if dragged {
            LINE_DRAG_WIDTH_PX
        } else if hovered {
            LINE_HOVER_WIDTH_PX.max(base_width)
        } else {
            base_width
        };
        let points = [
            egui::pos2(self.chart_rect.left(), y),
            egui::pos2(self.axis_x, y),
        ];
        if dragged {
            self.painter.line_segment(
                points,
                egui::Stroke::new(width + DRAG_HALO_EXTRA_WIDTH_PX, DRAG_HALO_COLOR),
            );
        }
        let stroke = egui::Stroke::new(width, color);
        if dashed {
            self.painter.extend(egui::Shape::dashed_line(
                &points,
                stroke,
                ORDER_DASH_PX,
                ORDER_GAP_PX,
            ));
        } else {
            self.painter.line_segment(points, stroke);
        }
    }

    /// The gutter chip: the price and nothing else, on the last-price
    /// chip's geometry. The line stays at its true price; only the chip
    /// dodges the reserved last-price row.
    fn gutter_chip(&self, y: f32, color: egui::Color32, text: &str) {
        let chip_y = dodged_chip_y(
            y,
            self.reserved_chip_y,
            self.chart_rect.top(),
            self.chart_rect.bottom(),
        );
        let galley = self.painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::monospace(11.0),
            theme::CHIP_INK,
        );
        let text_pos = egui::pos2(self.axis_x + 6.0, chip_y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        self.painter
            .rect_filled(bg, egui::Rounding::same(2.0), color);
        self.painter.galley(text_pos, galley, theme::CHIP_INK);
    }

    /// A solid tag on a line, right-anchored inside the plot. When it is
    /// closable (`with_close` — everything but a mid-drag preview) its ✕
    /// occupies the tag's right edge, painted on `close_button_rect`'s own
    /// geometry — the exact rect the press-time hit-test computes, so the
    /// two can never disagree. Overlay ✕s carry no tooltip of their own —
    /// each has a full-size, fully labelled twin in the chrome.
    fn chip_tag(&self, y: f32, fill: egui::Color32, text: &str, with_close: bool) {
        let galley = self.painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::monospace(11.0),
            theme::CHIP_INK,
        );
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = clamp_tag_center(y, self.chart_rect.top(), self.chart_rect.bottom());
        let right = self.tag_right - TAG_GAP_PX;
        let button_w = if with_close { TAG_BUTTON_PX } else { 0.0 };
        let content_w = galley.size().x + 2.0 * TAG_PAD_X + button_w;
        let full = egui::Rect::from_min_max(
            egui::pos2(right - content_w, center_y - half),
            egui::pos2(right, center_y + half),
        );
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), fill);
        self.painter.galley(
            egui::pos2(full.left() + TAG_PAD_X, center_y - galley.size().y / 2.0),
            galley,
            theme::CHIP_INK,
        );
        if !with_close {
            return;
        }
        let button = close_button_rect(self.tag_right, center_y);
        self.painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::monospace(11.0),
            theme::CHIP_INK,
        );
        // A hairline of ink between the words and the ✕, so the zone reads
        // as a button rather than a longer label.
        self.painter.line_segment(
            [
                egui::pos2(button.left(), full.top() + 4.0),
                egui::pos2(button.left(), full.bottom() - 4.0),
            ],
            egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(
                    theme::CHIP_INK.r(),
                    theme::CHIP_INK.g(),
                    theme::CHIP_INK.b(),
                    CLOSE_DIVIDER_ALPHA,
                ),
            ),
        );
    }

    /// The position's tag wears the card grammar, not a chip: a position is
    /// a fact about the account, not an order that will fire. Its ✕ sits at
    /// the right edge on `close_button_rect`'s own geometry, like every
    /// chart close. Returns the tag's rect (the handle-reveal hover zone).
    fn position_tag(
        &self,
        y: f32,
        side_color: egui::Color32,
        side_text: &str,
        points: Option<(String, egui::Color32)>,
    ) -> egui::Rect {
        let font = egui::FontId::monospace(11.0);
        let side_galley =
            self.painter
                .layout_no_wrap(side_text.to_owned(), font.clone(), side_color);
        let points_galley = points.map(|(text, color)| {
            (
                self.painter.layout_no_wrap(text, font.clone(), color),
                color,
            )
        });
        let rail = 3.0;
        let mut content_w = rail + TAG_PAD_X + side_galley.size().x + TAG_PAD_X + TAG_BUTTON_PX;
        if let Some((galley, _)) = &points_galley {
            content_w += galley.size().x + TAG_PAD_X;
        }
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = clamp_tag_center(y, self.chart_rect.top(), self.chart_rect.bottom());
        let right = self.tag_right - TAG_GAP_PX;
        let full = egui::Rect::from_min_max(
            egui::pos2(right - content_w, center_y - half),
            egui::pos2(right, center_y + half),
        );
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), theme::INSET);
        self.painter.rect_stroke(
            full,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        // The side rail rides the card's left edge.
        self.painter.rect_filled(
            egui::Rect::from_min_max(
                full.min + egui::vec2(1.0, 1.0),
                egui::pos2(full.min.x + 1.0 + rail, full.max.y - 1.0),
            ),
            egui::Rounding {
                nw: 2.0,
                sw: 2.0,
                ne: 0.0,
                se: 0.0,
            },
            side_color,
        );
        let mut x = full.left() + rail + TAG_PAD_X;
        let side_size = side_galley.size();
        self.painter.galley(
            egui::pos2(x, center_y - side_size.y / 2.0),
            side_galley,
            side_color,
        );
        x += side_size.x + TAG_PAD_X;
        if let Some((galley, color)) = points_galley {
            let size = galley.size();
            self.painter
                .galley(egui::pos2(x, center_y - size.y / 2.0), galley, color);
        }
        let button = close_button_rect(self.tag_right, center_y);
        let over_button = self.pointer.is_some_and(|pointer| button.contains(pointer));
        self.painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::monospace(11.0),
            if over_button {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
        // A hairline between the words and the ✕ zone.
        self.painter.line_segment(
            [
                egui::pos2(button.left(), full.top() + 3.0),
                egui::pos2(button.left(), full.bottom() - 3.0),
            ],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        full
    }

    /// A labelled `SL`/`TP` handle on the entry line: quiet control fill,
    /// the leg's own colour on hover.
    fn bracket_handle(&self, rect: egui::Rect, label: &str, leg_color: egui::Color32) {
        let hovered = self.pointer.is_some_and(|pointer| rect.contains(pointer));
        self.painter
            .rect_filled(rect, egui::Rounding::same(3.0), theme::CONTROL);
        self.painter.rect_stroke(
            rect,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, if hovered { leg_color } else { theme::BORDER }),
        );
        self.painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(9.0),
            if hovered {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
    }
}

/// A tag's vertical center: the line's row, kept fully inside the plot.
pub(crate) fn clamp_tag_center(y: f32, top: f32, bottom: f32) -> f32 {
    let half = TAG_HEIGHT_PX / 2.0;
    y.clamp(top + half, bottom - half)
}

/// The ✕ zone every closable tag reserves at its right edge — a fixed
/// position derivable without measuring text, which is what lets the
/// paint and the press-time geometric hit-test share one truth.
pub(crate) fn close_button_rect(tag_right: f32, center_y: f32) -> egui::Rect {
    let right = tag_right - TAG_GAP_PX;
    egui::Rect::from_min_max(
        egui::pos2(right - TAG_BUTTON_PX, center_y - TAG_HEIGHT_PX / 2.0),
        egui::pos2(right, center_y + TAG_HEIGHT_PX / 2.0),
    )
}

/// A bracket handle's rect: the ✕ column, one clear step above or below
/// the entry line so it never overlaps the position tag between them.
fn bracket_handle_rect(tag_right: f32, entry_y: f32, above: bool) -> egui::Rect {
    let right = tag_right - TAG_GAP_PX;
    let y = if above {
        entry_y - HANDLE_CLEAR_PX - HANDLE_SIZE.y
    } else {
        entry_y + HANDLE_CLEAR_PX
    };
    egui::Rect::from_min_size(egui::pos2(right - HANDLE_SIZE.x, y), HANDLE_SIZE)
}

/// Reward over risk at the dragged level, against the other leg — the read
/// that turns a drag into a decision. `None` until both legs are known or
/// while the risk is zero.
fn rr_ratio(leg: &LegPaint<'_>, dragged: Decimal) -> Option<String> {
    let other = leg.other_level?;
    let entry = leg.position.avg_price;
    let (stop, target) = if leg.word == "SL" {
        (dragged, other)
    } else {
        (other, dragged)
    };
    let risk = entry.saturating_sub(stop).abs();
    let reward = target.saturating_sub(entry).abs();
    (risk > Decimal::ZERO).then(|| fmt_points(reward / risk))
}

/// The side's chrome colour — one mapping for every paper surface.
fn side_color(side: Side) -> egui::Color32 {
    match side {
        Side::Buy => theme::BUY,
        Side::Sell => theme::SELL,
    }
}

/// Three-letter order kind for the compact chart tags (`LMT`, `STP`, `MKT`).
fn kind_short(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "MKT",
        EntryKind::Limit => "LMT",
        EntryKind::Stop => "STP",
    }
}

/// Uppercase section caption — the ledger's group labels and column header.
fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(10.0)
        .color(theme::TEXT_FAINT)
}

/// A pill-shaped toggle chip: accent fill while on, quiet control while off.
fn pill_toggle(ui: &mut egui::Ui, label: &str, on: bool, hover: &str) -> egui::Response {
    let (fill, ink, stroke) = if on {
        (theme::ACCENT, theme::CHIP_INK, egui::Stroke::NONE)
    } else {
        (
            theme::CONTROL,
            theme::TEXT_MUTED,
            egui::Stroke::new(1.0_f32, theme::BORDER),
        )
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(ink).small())
            .fill(fill)
            .stroke(stroke)
            .rounding(egui::Rounding::same(9.0)),
    )
    .on_hover_text(hover)
}

/// A group caption inside the virtualised list, sharing the fixed row
/// height so `show_rows` stays honest about where every row is.
fn draw_group_header(ui: &mut egui::Ui, label: &str, count: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{label} · {count}"),
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
}

/// The pinned open-position row: sunken, live open points on the right,
/// the current mark standing in for the exit.
fn draw_open_row(
    ui: &mut egui::Ui,
    summary: &PositionSummary,
    mark: Option<Decimal>,
    held_ms: Option<i64>,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::ZERO, theme::INSET);
    for y in [rect.top(), rect.bottom()] {
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
    }
    painter.rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        side_color(summary.side),
    );
    let exit = mark.map_or_else(|| "…".to_owned(), fmt_decimal);
    let held = held_ms.map_or_else(String::new, |ms| format!("open {}", fmt_duration_ms(ms)));
    draw_row_lines(
        painter,
        rect,
        RowLines {
            side: summary.side,
            head: format!(
                "{} {}",
                position_word(summary.side),
                fmt_decimal(summary.quantity)
            ),
            route: format!("{} → {}", fmt_decimal(summary.avg_price), exit),
            points: summary.open_points,
            detail: format!("{held} · at the last print"),
        },
    );
}

/// The two text lines every ledger row shares.
struct RowLines {
    side: Side,
    head: String,
    route: String,
    points: Option<Decimal>,
    detail: String,
}

fn draw_row_lines(painter: &egui::Painter, rect: egui::Rect, lines: RowLines) {
    let font = egui::FontId::monospace(11.0);
    let x = rect.left() + SIDE_RAIL_WIDTH_PX + 6.0;
    let y1 = rect.top() + 9.0;
    let y2 = rect.top() + 24.0;
    let color = side_color(lines.side);
    let head = painter.layout_no_wrap(lines.head, font.clone(), color);
    let head_w = head.size().x;
    painter.galley(egui::pos2(x, y1 - head.size().y / 2.0), head, color);
    painter.text(
        egui::pos2(x + head_w + 8.0, y1),
        egui::Align2::LEFT_CENTER,
        lines.route,
        font.clone(),
        theme::TEXT_MUTED,
    );
    if let Some(points) = lines.points {
        painter.text(
            egui::pos2(rect.right() - 6.0, y1),
            egui::Align2::RIGHT_CENTER,
            fmt_signed_points(points),
            font,
            points_color(points),
        );
    }
    painter.text(
        egui::pos2(x, y2),
        egui::Align2::LEFT_CENTER,
        lines.detail,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
}

/// One closed trade in the ledger. Session rows select on click and reveal
/// a jump-to-chart control on hover; rows from earlier sessions are
/// display-only — their tape is not the one on screen, so the chart has
/// nothing honest to point at.
fn draw_ledger_row(
    ui: &mut egui::Ui,
    trade: &ClosedTrade,
    symbol: Option<&str>,
    selected: bool,
    session: bool,
    tz: TzOffset,
) -> LedgerRowResponse {
    let width = ui.available_width();
    let sense = if session {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, LEDGER_ROW_HEIGHT_PX), sense);
    if !ui.is_rect_visible(rect) {
        return LedgerRowResponse::default();
    }
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::ZERO,
            theme::active_tint(theme::ACCENT),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::ZERO, theme::BORDER);
    }
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        side_color(trade.side),
    );
    let mut detail = format!(
        "{} · {} · {}",
        crate::app::fmt_time(trade.closed_ms, tz),
        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
        trade.exit_reason.as_str().replace('_', " "),
    );
    if let Some(symbol) = symbol {
        detail.push_str(" · ");
        detail.push_str(symbol);
    }
    draw_row_lines(
        ui.painter(),
        rect,
        RowLines {
            side: trade.side,
            head: format!(
                "{} {}",
                position_word(trade.side),
                fmt_decimal(trade.quantity)
            ),
            route: format!(
                "{} → {}",
                fmt_decimal(trade.entry_price),
                fmt_decimal(trade.exit_price)
            ),
            points: Some(trade.pnl_points),
            detail,
        },
    );
    let mut navigate = false;
    if session && response.hovered() {
        let nav_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 14.0, rect.top() + 24.0),
            egui::vec2(16.0, 14.0),
        );
        let nav = ui
            .interact(
                nav_rect,
                ui.id()
                    .with(("ledger_nav", trade.opened_ms, trade.closed_ms)),
                egui::Sense::click(),
            )
            .on_hover_text("center the chart on this trade");
        ui.painter().text(
            nav_rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::ARROW_UP_RIGHT,
            egui::FontId::proportional(11.0),
            if nav.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
        navigate = nav.clicked();
    }
    LedgerRowResponse {
        clicked: response.clicked() && !navigate,
        navigate,
    }
}

/// `38s`, `4m 18s`, `1h 02m` — a trade's age in venue time.
pub(crate) fn fmt_duration_ms(ms: i64) -> String {
    let seconds = (ms / 1000).max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// Keep a gutter chip legible when it would land on the last-price chip:
/// push it just clear of the reserved row, towards its own side of the
/// price, clamped into the pane. At the exact fill price (no distance at
/// all) the chip steps down, below the last-price chip. When the reserved
/// row itself hugs a pane edge the clamp can land the chip back inside the
/// band — accepted: a chip pinned at the edge beats one pushed out of the
/// pane, and the next print separates them.
fn dodged_chip_y(y: f32, reserved: Option<f32>, top: f32, bottom: f32) -> f32 {
    let Some(reserved) = reserved else {
        return y;
    };
    let delta = y - reserved;
    if delta.abs() >= CHIP_CLEAR_PX {
        return y;
    }
    let dodged = if delta >= 0.0 {
        reserved + CHIP_CLEAR_PX
    } else {
        reserved - CHIP_CLEAR_PX
    };
    dodged.clamp(top, bottom)
}

/// Empty means "none"; otherwise a strictly positive decimal.
fn parse_offset(text: &str) -> Result<Option<Decimal>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<Decimal>() {
        Ok(value) if value > Decimal::ZERO => Ok(Some(value)),
        _ => Err(trimmed.to_owned()),
    }
}

fn side_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn side_word_upper(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

/// `LONG`/`SHORT` — shared with the HUD so every surface uses one register.
pub(crate) fn position_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "LONG",
        Side::Sell => "SHORT",
    }
}

fn kind_word(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "market",
        EntryKind::Limit => "limit",
        EntryKind::Stop => "stop",
    }
}

/// Green gains, red losses, muted zero — shared with the HUD.
pub(crate) fn points_color(points: Decimal) -> egui::Color32 {
    match points.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => theme::BUY,
        std::cmp::Ordering::Less => theme::SELL,
        std::cmp::Ordering::Equal => theme::TEXT_MUTED,
    }
}

/// Exact value, trailing zeros stripped — prices and quantities.
pub(crate) fn fmt_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Points rounded to two places for display (the stored value stays exact).
fn fmt_points(value: Decimal) -> String {
    value.round_dp(2).normalize().to_string()
}

/// Signed points: an explicit `+` on gains so a green `12` can never be
/// misread as a count.
pub(crate) fn fmt_signed_points(value: Decimal) -> String {
    if value > Decimal::ZERO {
        format!("+{}", fmt_points(value))
    } else {
        fmt_points(value)
    }
}

/// Keep the characters real venue symbols use (`WDO$`, `WIN@N`… stay
/// recognizable); anything else becomes `_` so a symbol can never traverse
/// paths.
pub(crate) fn sanitize_symbol(symbol: &str) -> String {
    let cleaned: String = symbol
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "-_.$#".contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".to_owned()
    } else {
        cleaned
    }
}

/// Civil UTC date-time from epoch milliseconds: `(year, month, day, hour,
/// minute, second)`. Civil-from-days per Howard Hinnant's algorithm; no
/// clock, no chrono.
fn civil_utc(timestamp_ms: i64) -> (i64, i64, i64, i64, i64, i64) {
    let seconds = timestamp_ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

/// `YYYYMMDD-HHMMSS` in UTC from epoch milliseconds — session file names
/// derive from venue time, so the same replay run names the same file.
fn utc_compact(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(timestamp_ms);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// `YYYY-MM-DD` in UTC — the report's anchor date.
#[cfg(test)]
fn fmt_utc_date(timestamp_ms: i64) -> String {
    let (year, month, day, ..) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `YYYY-MM-DD` in the display timezone — the anchor date on the same
/// calendar every other trade surface shows; a UTC date here named a day
/// no displayed trade carried for anyone west of Greenwich after 21:00.
fn fmt_offset_date(timestamp_ms: i64, tz: TzOffset) -> String {
    let (year, month, day, ..) = civil_utc(timestamp_ms.saturating_add(tz.offset_ms()));
    format!("{year:04}-{month:02}-{day:02}")
}

/// `YYYY-MM-DD HH:MM` in UTC — the equity curve's hover stamp.
fn fmt_utc_minute(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, _) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// A protective price the ticket's offset away from the average entry:
/// the losing side for a stop, the winning side for a target. A long's
/// stop and a short's target sit below the entry; the other two above.
fn offset_price(position: &Position, offset: Decimal, stop: bool) -> Decimal {
    let below = (position.side == Side::Buy) == stop;
    if below {
        position.avg_price.saturating_sub(offset)
    } else {
        position.avg_price.saturating_add(offset)
    }
}

/// Open `path` in the platform's file manager — created first, so the
/// reveal never points at nothing on a fresh install.
pub(crate) fn reveal_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let launcher = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(error) = std::process::Command::new(launcher).arg(path).spawn() {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FOLDER_REVEAL_FAILED",
            path = %path.display(),
            %error,
            "could not open a folder in the file manager"
        );
    }
}

/// The export CSV: one merged, Excel-facing artifact — the journal stays
/// the machine-readable source of truth. Human-readable UTC stamps ride
/// beside the venue epoch, decimals always use `.`, and the running
/// equity is a column so a spreadsheet shows it without a formula.
fn export_csv(rows: &[HistoryRow]) -> String {
    let mut text = String::from(
        "symbol,side,quantity,opened_ms,opened_utc,entry_price,closed_ms,closed_utc,\
         exit_price,pnl_points,cum_pnl_points,duration_ms,exit_reason,entry_agg_id,\
         exit_agg_id,mae_points,mfe_points,source\n",
    );
    let mut cumulative = Decimal::ZERO;
    let opt_u64 = |value: Option<u64>| value.map(|value| value.to_string()).unwrap_or_default();
    let opt_points = |value: Option<Decimal>| value.map(fmt_decimal).unwrap_or_default();
    for row in rows {
        let trade = &row.trade;
        cumulative = cumulative.saturating_add(trade.pnl_points);
        let side = match trade.side {
            Side::Buy => "long",
            Side::Sell => "short",
        };
        text.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.symbol.replace(',', "_"),
            side,
            fmt_decimal(trade.quantity),
            trade.opened_ms,
            fmt_utc_iso(trade.opened_ms),
            fmt_decimal(trade.entry_price),
            trade.closed_ms,
            fmt_utc_iso(trade.closed_ms),
            fmt_decimal(trade.exit_price),
            fmt_decimal(trade.pnl_points),
            fmt_decimal(cumulative),
            trade.closed_ms.saturating_sub(trade.opened_ms).max(0),
            trade.exit_reason.as_str(),
            opt_u64(trade.entry_agg_id),
            opt_u64(trade.exit_agg_id),
            opt_points(trade.mae_points),
            opt_points(trade.mfe_points),
            // Empty when the file never recorded one — unknown, not live.
            row.source.map(history::SessionSource::as_str).unwrap_or(""),
        ));
    }
    text
}

/// `YYYY-MM-DDTHH:MM:SSZ` in UTC — the export's human-readable stamp.
fn fmt_utc_iso(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// A toast-sized path: printed whole when short, elided to `…/file.csv`
/// past the limit — the file name always stays whole.
fn elide_path(path: &Path) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= EXPORT_PATH_ELIDE_CHARS {
        return text;
    }
    path.file_name().map_or(text, |name| {
        format!("…{}{}", std::path::MAIN_SEPARATOR, name.to_string_lossy())
    })
}

/// The cmd preview's geometry from the interactive band and the pointer:
/// the dashed line under the cursor running out to the right edge, and the
/// clickable label riding beside the cursor. One function for paint and
/// press alike, so a painted label and its hit-test can never disagree
/// (the overlay-controls rule).
///
/// The label follows the pointer rather than parking against the right
/// edge: aiming at a price on the left of the plot used to mean crossing
/// the whole chart to click the thing you were already pointing at. It
/// sits *beside* the cursor, never under it, so the crosshair and the
/// candle it rests on stay readable. Left is the preferred side (the line
/// and the price chip run off to the right, so the label completes the
/// sentence from its start); it flips right when the left edge leaves no
/// room, and in a band too narrow for either — well under any window this
/// app opens — it parks against the closer edge.
fn cmd_preview_layout(
    band: egui::Rect,
    pointer: egui::Pos2,
) -> (egui::Pos2, egui::Pos2, egui::Rect) {
    let need = CMD_LABEL_CURSOR_GAP_PX + CMD_LABEL_WIDTH_PX;
    let left = if pointer.x - band.left() >= need {
        pointer.x - need
    } else if band.right() - pointer.x >= need {
        pointer.x + CMD_LABEL_CURSOR_GAP_PX
    } else {
        // Narrower than the label and its gap on either side: the two
        // cannot both hold, so it parks at the left edge and the cursor
        // may cross it. Reaching this needs a band under 260 px — no
        // window this app opens is that small.
        band.left()
    };
    let center_y = clamp_tag_center(pointer.y, band.top(), band.bottom());
    let half = TAG_HEIGHT_PX / 2.0;
    let label = egui::Rect::from_min_max(
        egui::pos2(left, center_y - half),
        egui::pos2(left + CMD_LABEL_WIDTH_PX, center_y + half),
    );
    // The line starts under the cursor and reaches the axis, which is what
    // ties the label beside the hand to the price on the gutter. Close to
    // that edge it starts further left instead, so there is always a line
    // to read.
    let start = egui::pos2(
        pointer
            .x
            .min(band.right() - CMD_LINE_MIN_PX)
            .max(band.left()),
        pointer.y,
    );
    (start, egui::pos2(band.right(), pointer.y), label)
}

/// The row around a line where its in-plot tag counts as hovered: the
/// line's own grab band, plus the row a tag was clamped into near a chart
/// edge (where the two part company). Text-free geometry on purpose — a
/// press-time hit-test has no painter to measure a galley with, and the
/// paint must be able to ask the same question.
fn tag_row_hit(pointer: egui::Pos2, y: f32, chart: egui::Rect) -> bool {
    if !chart.contains(pointer) {
        return false;
    }
    let center = clamp_tag_center(y, chart.top(), chart.bottom());
    (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        || (pointer.y - center).abs() <= TAG_HEIGHT_PX / 2.0 + TAG_HOVER_SLACK_PX
}

/// The session file for `stamp` under `folder`: the plain name when free,
/// else `stamp.rerun-N` — file names derive from venue time, so replaying
/// the same recording twice reproduces the same stamp, and the second run
/// must land beside the first instead of appending duplicate trades into
/// it.
fn free_session_path(folder: &Path, stamp: &str) -> PathBuf {
    let plain = folder.join(format!("{stamp}.{}", history::FILE_EXTENSION));
    if !plain.exists() {
        return plain;
    }
    for rerun in 1..=MAX_SESSION_RERUNS {
        let candidate = folder.join(format!("{stamp}.rerun-{rerun}.{}", history::FILE_EXTENSION));
        if !candidate.exists() {
            return candidate;
        }
    }
    // A folder with a thousand same-stamp sessions is not a real journal;
    // appending to the plain file keeps the trades at the cost of
    // duplicates, which beats losing them.
    plain
}

/// A journal folder of its own per host under test — tests must never
/// touch a real documents folder, nor see one another's files.
fn test_scratch_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "quantick-paper-host-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The symbol folders under the history dir, for the report's combo box.
fn list_symbol_folders(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    folders.sort();
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(agg_id: u64, price: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    fn row(symbol: &str, source: Option<history::SessionSource>, trade: ClosedTrade) -> HistoryRow {
        HistoryRow {
            symbol: symbol.to_owned(),
            source,
            trade,
        }
    }

    fn trade_at(closed_ms: i64, pnl: i64) -> ClosedTrade {
        ClosedTrade {
            side: Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100 + pnl),
            opened_ms: closed_ms - 1000,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: quantick_sim::ExitReason::Manual,
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: None,
            mfe_points: None,
        }
    }

    #[test]
    fn utc_compact_matches_known_timestamps() {
        // 2026-03-16 13:01:08 UTC.
        assert_eq!(utc_compact(1_773_666_068_000), "20260316-130108");
        // The epoch itself.
        assert_eq!(utc_compact(0), "19700101-000000");
    }

    #[test]
    fn symbols_sanitize_without_losing_venue_spellings() {
        assert_eq!(sanitize_symbol("WDO$"), "WDO$");
        assert_eq!(sanitize_symbol("BTCUSDT"), "BTCUSDT");
        assert_eq!(sanitize_symbol("../evil"), ".._evil");
        assert_eq!(sanitize_symbol(""), "_");
    }

    #[test]
    fn signed_points_always_carry_their_sign() {
        assert_eq!(fmt_signed_points(Decimal::from(12)), "+12");
        assert_eq!(fmt_signed_points(Decimal::from(-3)), "-3");
        assert_eq!(fmt_signed_points(Decimal::ZERO), "0");
    }

    #[test]
    fn closed_trades_journal_to_one_session_file_and_reload() {
        let dir = std::env::temp_dir().join(format!(
            "quantick-paper-journal-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut paper = PaperTrading::new();
        paper.dir.clone_from(&dir);
        paper.set_symbol("TESTUSDT");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 105));
        // A second round trip appends to the same session file.
        paper.market(Side::Sell);
        paper.on_trade(&print(3, 105));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(4, 103));

        let folder = dir.join("TESTUSDT");
        let files: Vec<_> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one session, one file");
        let text = std::fs::read_to_string(files[0].path()).expect("readable");
        let parsed = history::parse(&text).expect("valid history");
        assert_eq!(parsed.symbol.as_deref(), Some("TESTUSDT"));
        assert_eq!(parsed.trades.len(), 2);
        assert!(parsed.problems.is_empty());
        assert_eq!(parsed.trades[0].pnl_points, Decimal::from(5));
        assert_eq!(parsed.trades[1].pnl_points, Decimal::from(2));

        let history = load_history(&dir, Some("TESTUSDT"), &[]);
        assert_eq!(history.rows.len(), 2);
        assert_eq!(report_from_history(&history).net_points, Decimal::from(7));
        assert_eq!(history.files, 1);
        assert_eq!(history.unreadable_files, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_session_adds_a_file_and_never_touches_the_first() {
        let dir = std::env::temp_dir().join(format!(
            "quantick-paper-accumulate-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        // Session one: a round trip closing at t=2s.
        let mut first = PaperTrading::new();
        first.dir.clone_from(&dir);
        first.set_symbol("ACCUM");
        first.seed(&print(0, 100));
        first.market(Side::Buy);
        first.on_trade(&print(1, 100));
        let events = first.sim.apply(Command::ClosePosition);
        first.handle_events(events);
        first.on_trade(&print(2, 103));
        let folder = dir.join("ACCUM");
        let first_file = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .next()
            .expect("one session file")
            .path();
        let first_bytes = std::fs::read(&first_file).expect("readable");

        // Session two: a fresh host — a restart — closing hours later.
        let mut second = PaperTrading::new();
        second.dir.clone_from(&dir);
        second.set_symbol("ACCUM");
        second.seed(&print(10_000, 200));
        second.market(Side::Sell);
        second.on_trade(&print(10_001, 200));
        let events = second.sim.apply(Command::ClosePosition);
        second.handle_events(events);
        second.on_trade(&print(10_002, 190));

        let files: Vec<_> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .collect();
        assert_eq!(files.len(), 2, "each session opens its own file");
        assert_eq!(
            std::fs::read(&first_file).expect("still readable"),
            first_bytes,
            "the earlier session's file is byte-for-byte untouched"
        );
        let history = load_history(&dir, Some("ACCUM"), &[]);
        assert_eq!(history.files, 2);
        assert_eq!(history.rows.len(), 2, "both sessions' trades load");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_timeline_reset_journals_the_flatten_and_clears_the_form_state() {
        let dir =
            std::env::temp_dir().join(format!("quantick-paper-reset-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut paper = PaperTrading::new();
        paper.dir.clone_from(&dir);
        paper.set_symbol("RESETX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        paper.on_timeline_reset();
        assert!(paper.sim.position().is_none());
        assert!(
            paper.armed.is_none(),
            "an armed click dies with the timeline"
        );
        assert!(paper.toast.is_some(), "the flatten is never silent");
        let history = load_history(&dir, Some("RESETX"), &[]);
        assert_eq!(history.rows.len(), 1);
        assert_eq!(
            report_from_history(&history).trades,
            1,
            "the reset exit is a real, journaled trade"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The stored-pick-vs-configured-base precedence now lives in
    // `paper_home::chosen`, tested there beside the documents default.

    /// The panel's folder picker retargets everything downstream: the next
    /// close opens a new session file under the new home, and the ledger
    /// and report re-read from it. Files already written stay put.
    #[test]
    fn switching_the_trades_dir_retargets_journal_ledger_and_report() {
        let dir_a =
            std::env::temp_dir().join(format!("quantick-paper-dir-a-{}", std::process::id()));
        let dir_b =
            std::env::temp_dir().join(format!("quantick-paper-dir-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
        let mut paper = PaperTrading::new();
        paper.dir.clone_from(&dir_a);
        paper.set_symbol("SWITCHX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 103));
        assert!(paper.journal_path.is_some(), "the close journaled under A");
        paper.reload_ledger();
        assert!(paper.history_cache.is_some());

        paper.set_trades_dir(dir_b.clone());
        assert_eq!(paper.trades_dir(), dir_b.as_path());
        assert!(
            paper.journal_path.is_none(),
            "the next close opens a new session file under B"
        );
        assert!(
            paper.history_cache.is_none(),
            "the ledger re-reads from the new home"
        );
        assert!(paper.toast.is_some(), "the switch is never silent");
        assert!(
            dir_a.join("SWITCHX").exists(),
            "files already written stay where they are"
        );
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn durations_format_by_magnitude() {
        assert_eq!(fmt_duration_ms(38_000), "38s");
        assert_eq!(fmt_duration_ms(258_000), "4m 18s");
        assert_eq!(fmt_duration_ms(3_720_000), "1h 02m");
        assert_eq!(fmt_duration_ms(-5), "0s", "clock skew never explodes");
    }

    /// The ledger's cache reads every saved file except the live session's
    /// own (its trades are already in the simulator), and remembers which
    /// symbol each row came from.
    #[test]
    fn the_ledger_cache_excludes_the_live_session_file() {
        let dir =
            std::env::temp_dir().join(format!("quantick-paper-ledger-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut paper = PaperTrading::new();
        paper.dir.clone_from(&dir);
        paper.set_symbol("LEDGX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 105));
        assert!(paper.journal_path.is_some(), "the close journaled");

        // An earlier session's file, written by hand beside the live one.
        let trade = ClosedTrade {
            side: Side::Sell,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(200),
            exit_price: Decimal::from(195),
            opened_ms: 10,
            closed_ms: 20,
            pnl_points: Decimal::from(5),
            exit_reason: quantick_sim::ExitReason::Manual,
            entry_agg_id: Some(1),
            exit_agg_id: Some(2),
            mae_points: Some(Decimal::ZERO),
            mfe_points: Some(Decimal::from(5)),
        };
        let mut text = history::write_header("LEDGX", history::SessionSource::Live);
        text.push_str(&history::write_trade(&trade));
        std::fs::write(dir.join("LEDGX").join("20200101-000000.csv"), text)
            .expect("the earlier session file writes");

        paper.reload_ledger();
        let cache = paper.history_cache.as_ref().expect("loaded");
        assert_eq!(
            cache.rows.len(),
            1,
            "the live session's file is excluded, the earlier one loads"
        );
        assert_eq!(cache.rows[0].symbol, "LEDGX");
        assert_eq!(cache.rows[0].trade, trade);
        assert_eq!(cache.rows[0].source, Some(history::SessionSource::Live));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_replay_rerun_lands_beside_its_first_run_never_inside_it() {
        let dir =
            std::env::temp_dir().join(format!("quantick-paper-rerun-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The same recording replayed twice: identical prints, identical
        // venue times, so both sessions derive the same file stamp.
        for _ in 0..2 {
            let mut paper = PaperTrading::new();
            paper.dir.clone_from(&dir);
            paper.set_symbol("RERUN");
            paper.set_session_source(history::SessionSource::Replay);
            paper.seed(&print(0, 100));
            paper.market(Side::Buy);
            paper.on_trade(&print(1, 100));
            let events = paper.sim.apply(Command::ClosePosition);
            paper.handle_events(events);
            paper.on_trade(&print(2, 103));
        }

        let folder = dir.join("RERUN");
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .collect();
        names.sort();
        assert_eq!(
            names.len(),
            2,
            "the second run opened its own file instead of appending duplicates"
        );
        assert!(names[1].contains(".rerun-1."), "{names:?}");
        let history = load_history(&dir, Some("RERUN"), &[]);
        assert_eq!(history.rows.len(), 2);
        assert!(
            history
                .rows
                .iter()
                .all(|row| row.source == Some(history::SessionSource::Replay)),
            "both files carry the replay source"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ledger_never_lists_this_sessions_trades_twice_after_a_retarget() {
        // Hunt-confirmed: close live, flip to replay (what a same-symbol
        // replay open does), reload the ledger — the live session's file
        // must stay excluded, or every trade counts twice in the totals
        // and the export.
        let mut paper = PaperTrading::new();
        paper.set_symbol("DUPX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 105));
        assert_eq!(paper.sim.closed_trades().len(), 1);

        paper.set_session_source(history::SessionSource::Replay);
        paper.on_timeline_reset();
        paper.reload_ledger();
        let cache = paper.history_cache.as_ref().expect("loaded");
        assert_eq!(
            cache.rows.len(),
            0,
            "the session's own files stay excluded across the retarget"
        );
    }

    #[test]
    fn an_in_session_rerun_opens_its_own_file() {
        // Seek-to-start / reopen-same-recording is a timeline reset with
        // the source unchanged: run 2 must not append into run 1's file.
        let mut paper = PaperTrading::new();
        paper.set_symbol("RESEEK");
        paper.set_session_source(history::SessionSource::Replay);
        for _ in 0..2 {
            paper.seed(&print(0, 100));
            paper.market(Side::Buy);
            paper.on_trade(&print(1, 100));
            let events = paper.sim.apply(Command::ClosePosition);
            paper.handle_events(events);
            paper.on_trade(&print(2, 103));
            paper.on_timeline_reset();
        }
        let folder = paper.dir.join("RESEEK");
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "each run has its own file: {names:?}");
        assert!(names[1].contains(".rerun-1."), "{names:?}");
    }

    #[test]
    fn export_rows_remember_the_source_each_trade_closed_under() {
        let mut paper = PaperTrading::new();
        paper.set_symbol("SRCX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 105));
        paper.set_session_source(history::SessionSource::Replay);
        assert_eq!(
            paper.session_trade_sources,
            vec![history::SessionSource::Live],
            "a trade keeps the source it closed under, not the current one"
        );
    }

    #[test]
    fn the_report_opens_on_real_and_keeps_replay_out_until_asked() {
        let utc = TzOffset::new(0);
        let day = 86_400_000_i64;
        let mut paper = PaperTrading::new();
        assert_eq!(
            paper.report_source,
            SourceFilter::Real,
            "practice runs never inflate the real track record unasked"
        );
        paper.report = Some(LoadedHistory {
            rows: vec![
                row("X", Some(history::SessionSource::Live), trade_at(day, 5)),
                row("X", None, trade_at(2 * day, 3)),
                row(
                    "X",
                    Some(history::SessionSource::Replay),
                    trade_at(3 * day, 100),
                ),
            ],
            files: 3,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("view built");
        assert_eq!(
            view.trades.len(),
            2,
            "live + unrecorded-legacy count as real"
        );
        assert_eq!(
            view.hidden_by_source, 1,
            "the replay trade sits behind the filter"
        );
        assert_eq!(
            view.anchor_ms,
            Some(2 * day),
            "the anchor comes from the filtered scope, not the replay trade"
        );
        assert_eq!(view.report.net_points, Decimal::from(8));

        paper.report_source = SourceFilter::Replay;
        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("rebuilt");
        assert_eq!(view.trades.len(), 1, "the practice run, alone");
        assert_eq!(view.report.net_points, Decimal::from(100));
        assert_eq!(view.hidden_by_source, 2);

        paper.report_source = SourceFilter::All;
        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("rebuilt");
        assert_eq!(view.trades.len(), 3, "All mixes on purpose");
        assert_eq!(view.hidden_by_source, 0);
    }

    #[test]
    fn the_export_csv_carries_readable_stamps_and_running_equity() {
        let trade = |closed_ms: i64, pnl: i64, mae: Option<i64>| ClosedTrade {
            side: Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100 + pnl),
            opened_ms: closed_ms - 60_000,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: quantick_sim::ExitReason::Manual,
            entry_agg_id: mae.map(|_| 1),
            exit_agg_id: mae.map(|_| 2),
            mae_points: mae.map(Decimal::from),
            mfe_points: mae.map(Decimal::from),
        };
        let rows = vec![
            row(
                "BTCUSDT",
                Some(history::SessionSource::Live),
                trade(1_773_666_068_000, 5, Some(2)),
            ),
            row("WINQ26", None, trade(1_773_666_368_000, -2, None)),
        ];
        let text = export_csv(&rows);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "header plus two rows");
        assert!(lines[0].starts_with("symbol,side,quantity,opened_ms,opened_utc"));
        assert!(lines[0].ends_with(",source"), "{}", lines[0]);
        assert!(
            lines[1].contains("2026-03-16T13:01:08Z"),
            "human-readable UTC beside the epoch: {}",
            lines[1]
        );
        assert!(lines[1].ends_with(",manual,1,2,2,2,live"), "{}", lines[1]);
        assert!(
            lines[2].contains(",3,"),
            "running equity 5 + (-2): {}",
            lines[2]
        );
        assert!(
            lines[2].ends_with(",manual,,,,,"),
            "unknown v1 fields and an unrecorded source stay empty: {}",
            lines[2]
        );
    }

    #[test]
    fn long_export_paths_elide_to_the_file_name() {
        let short = Path::new("paper-trades/export-1.csv");
        assert_eq!(elide_path(short), short.display().to_string());
        let long = Path::new(
            "C:/some/extremely/long/path/that/never/ends/and/keeps/going/paper-trades/export-20260805-141233.csv",
        );
        let elided = elide_path(long);
        assert!(elided.starts_with('…'), "{elided}");
        assert!(elided.ends_with("export-20260805-141233.csv"), "{elided}");
    }

    #[test]
    fn report_periods_anchor_to_the_given_trade_never_a_clock() {
        let utc = TzOffset::new(0);
        // 2026-03-16 13:01:08 UTC.
        let anchor = 1_773_666_068_000_i64;
        assert_eq!(ReportPeriod::All.cutoff_ms(anchor, utc), None);
        assert_eq!(
            ReportPeriod::Today.cutoff_ms(anchor, utc),
            Some(1_773_619_200_000),
            "the anchor's own UTC midnight"
        );
        assert_eq!(
            ReportPeriod::Week.cutoff_ms(anchor, utc),
            Some(anchor - 7 * 86_400_000)
        );
        assert_eq!(
            ReportPeriod::Custom(2 * 86_400_000).cutoff_ms(anchor, utc),
            Some(anchor - 2 * 86_400_000),
            "a typed 2d reaches back exactly two days"
        );
    }

    #[test]
    fn today_breaks_at_the_displayed_midnight_not_utc() {
        let day = 86_400_000_i64;
        // 03 Jan 01:00 UTC is 02 Jan 22:00 in UTC-03:00.
        let anchor = 2 * day + 3_600_000;
        let sao_paulo = TzOffset::new(-180);
        assert_eq!(
            ReportPeriod::Today.cutoff_ms(anchor, sao_paulo),
            Some(day + 10_800_000),
            "midnight of the anchor's displayed day, expressed in UTC"
        );
        assert_eq!(
            ReportPeriod::Today.cutoff_ms(anchor, TzOffset::new(0)),
            Some(2 * day),
            "UTC viewers keep the UTC midnight"
        );
    }

    #[test]
    fn typed_periods_parse_strictly_and_format_back() {
        assert_eq!(parse_period("2d"), Some(2 * 86_400_000));
        assert_eq!(parse_period(" 3D "), Some(3 * 86_400_000));
        assert_eq!(parse_period("12h"), Some(12 * 3_600_000));
        assert_eq!(parse_period("45m"), Some(45 * 60_000));
        assert_eq!(parse_period("1w"), Some(7 * 86_400_000));
        for refused in ["", "d", "2", "2x", "0d", "-2d", "2.5d", "d2"] {
            assert_eq!(parse_period(refused), None, "{refused:?} must be refused");
        }
        assert_eq!(fmt_period_ms(2 * 86_400_000), "2d");
        assert_eq!(fmt_period_ms(36 * 3_600_000), "36h");
        assert_eq!(fmt_period_ms(45 * 60_000), "45m");
        assert_eq!(fmt_period_ms(14 * 86_400_000), "2w");
    }

    #[test]
    fn utc_dates_format_from_the_same_civil_math() {
        assert_eq!(fmt_utc_date(1_773_666_068_000), "2026-03-16");
        assert_eq!(fmt_utc_minute(1_773_666_068_000), "2026-03-16 13:01");
    }

    #[test]
    fn the_report_view_filters_by_period_from_the_newest_trade() {
        let utc = TzOffset::new(0);
        let day = 86_400_000_i64;
        let mut paper = PaperTrading::new();
        paper.report = Some(LoadedHistory {
            rows: vec![
                row("X", None, trade_at(10 * day, 5)),
                row("X", None, trade_at(18 * day, -2)),
                row("X", None, trade_at(20 * day + 3_600_000, 7)),
            ],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_period = ReportPeriod::Week;
        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("view built");
        assert_eq!(
            view.anchor_ms,
            Some(20 * day + 3_600_000),
            "anchored to the newest saved trade, not a clock"
        );
        assert_eq!(view.trades.len(), 2, "the 10-day-old trade is outside 7d");
        assert_eq!(view.hidden_before, 1, "and the view counts what it hides");
        assert_eq!(view.report.net_points, Decimal::from(5));

        paper.report_period = ReportPeriod::All;
        paper.ensure_report_view(utc);
        assert_eq!(
            paper.report_view.as_ref().expect("rebuilt").trades.len(),
            3,
            "All sees everything again"
        );
    }

    #[test]
    fn all_symbols_hides_older_markets_but_says_so_and_scope_restores_them() {
        let utc = TzOffset::new(0);
        let day = 86_400_000_i64;
        let mut paper = PaperTrading::new();
        paper.report = Some(LoadedHistory {
            rows: vec![
                row("OLDSYM", None, trade_at(day, 5)),
                row("NEWSYM", None, trade_at(60 * day, 7)),
            ],
            files: 2,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_period = ReportPeriod::Month;
        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("view built");
        assert_eq!(
            view.trades.len(),
            1,
            "30d back from the newest trade hides the older market entirely"
        );
        assert_eq!(view.hidden_before, 1, "the support line can say so");

        // Narrowing the combo re-anchors on the old market's own newest
        // trade — the "my trades came back" behaviour, now spelled out.
        paper.report = Some(LoadedHistory {
            rows: vec![row("OLDSYM", None, trade_at(day, 5))],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_view = None;
        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("rebuilt");
        assert_eq!(view.trades.len(), 1, "its own scope shows the old market");
        assert_eq!(view.hidden_before, 0);
    }

    #[test]
    fn the_report_scopes_by_symbol_folder_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "quantick-paper-symbols-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (symbol, id0, price) in [("AAAUSDT", 0, 100), ("BBBUSDT", 100, 200)] {
            let mut paper = PaperTrading::new();
            paper.dir.clone_from(&dir);
            paper.set_symbol(symbol);
            paper.seed(&print(id0, price));
            paper.market(Side::Buy);
            paper.on_trade(&print(id0 + 1, price));
            let events = paper.sim.apply(Command::ClosePosition);
            paper.handle_events(events);
            paper.on_trade(&print(id0 + 2, price + 5));
        }

        assert_eq!(
            list_symbol_folders(&dir),
            vec!["AAAUSDT".to_owned(), "BBBUSDT".to_owned()],
            "the combo lists every traded asset"
        );
        let all = load_history(&dir, None, &[]);
        assert_eq!(all.rows.len(), 2, "All symbols reads both journals");
        let symbols: Vec<&str> = all.rows.iter().map(|row| row.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["AAAUSDT", "BBBUSDT"]);
        let one = load_history(&dir, Some("BBBUSDT"), &[]);
        assert_eq!(one.rows.len(), 1, "a symbol scope reads only its folder");
        assert_eq!(one.rows[0].symbol, "BBBUSDT");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_close_refreshes_an_open_report_by_itself() {
        let utc = TzOffset::new(0);
        let mut paper = PaperTrading::new();
        paper.set_symbol("FRESH");
        paper.report_open = true;
        paper.reload_report();
        paper.ensure_report_view(utc);
        assert!(
            paper.report_view.as_ref().expect("view").trades.is_empty(),
            "nothing saved yet"
        );

        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 105));

        paper.ensure_report_view(utc);
        let view = paper.report_view.as_ref().expect("refreshed");
        assert_eq!(
            view.trades.len(),
            1,
            "the close re-read the journal without a manual refresh"
        );
    }

    #[test]
    fn market_offsets_become_a_bracket_around_the_reference() {
        let mut paper = PaperTrading::new();
        paper.stop_offset_text = "5".to_owned();
        paper.profit_offset_text = "10".to_owned();
        let bracket = paper
            .parse_bracket(Side::Buy, Decimal::from(100))
            .expect("both parse");
        assert_eq!(bracket.stop_loss, Some(Decimal::from(95)));
        assert_eq!(bracket.take_profit, Some(Decimal::from(110)));
        let bracket = paper
            .parse_bracket(Side::Sell, Decimal::from(100))
            .expect("both parse");
        assert_eq!(bracket.stop_loss, Some(Decimal::from(105)));
        assert_eq!(bracket.take_profit, Some(Decimal::from(90)));
    }

    #[test]
    fn a_bad_offset_toasts_and_blocks_the_order() {
        let mut paper = PaperTrading::new();
        paper.stop_offset_text = "abc".to_owned();
        assert!(paper.parse_bracket(Side::Buy, Decimal::from(100)).is_none());
        assert!(paper.toast.is_some(), "the refusal teaches, never silent");
    }

    /// A 800×400 chart over the given price range, plus the input for one
    /// pointer frame at `(x, y)`.
    fn chart_and_scale(lo: f64, hi: f64) -> (egui::Rect, PriceScale) {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        (chart, PriceScale::from_range(lo, hi, 0.0, 400.0))
    }

    fn frame<'a>(
        chart: egui::Rect,
        scale: &'a PriceScale,
        y: f32,
        pressed: bool,
        down: bool,
        released: bool,
    ) -> ChartInput<'a> {
        ChartInput {
            chart,
            scale: Some(scale),
            pointer: Some(egui::pos2(400.0, y)),
            primary_pressed: pressed,
            primary_down: down,
            primary_released: released,
            modifiers: egui::Modifiers::default(),
            canvas_claimed: false,
            layer_visible: true,
        }
    }

    /// A `frame` with held modifiers and a free pointer — the cmd-trading
    /// gesture's shape of input.
    fn cmd_frame<'a>(
        chart: egui::Rect,
        scale: &'a PriceScale,
        pointer: egui::Pos2,
        modifiers: egui::Modifiers,
        pressed: bool,
    ) -> ChartInput<'a> {
        ChartInput {
            chart,
            scale: Some(scale),
            pointer: Some(pointer),
            primary_pressed: pressed,
            primary_down: pressed,
            primary_released: false,
            modifiers,
            canvas_claimed: false,
            layer_visible: true,
        }
    }

    /// Escape (routed through the app's escape stack) cancels exactly one
    /// paper interaction per press, and a cancelled drag submits nothing.
    #[test]
    fn escape_cancels_the_armed_placement_then_the_grabbed_line() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        assert!(paper.cancel_interaction(), "the armed placement dies first");
        assert!(paper.armed.is_none());
        assert!(!paper.cancel_interaction(), "nothing left to cancel");

        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // Grab the stop at 90 (y = 300), cancel, then let go: no submit.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
        assert!(paper.cancel_interaction(), "the grabbed line is released");
        assert!(!paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("still long").stop_loss,
            Some(Decimal::from(90)),
            "a cancelled drag never moves the stop"
        );
    }

    #[test]
    fn an_armed_click_places_the_order_at_the_clicked_price_and_disarms() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        let (chart, scale) = chart_and_scale(90.0, 110.0);
        // y = 300 sits at price 95 on this scale.
        let consumed = paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false));
        assert!(consumed, "the armed click never reaches the chart pan");
        assert!(paper.armed.is_none(), "a successful placement disarms");
        assert_eq!(paper.sim.orders().len(), 1);
        assert_eq!(paper.sim.orders()[0].price, Some(Decimal::from(95)));
    }

    #[test]
    fn a_rejected_armed_click_stays_armed_and_teaches() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        let (chart, scale) = chart_and_scale(90.0, 110.0);
        // y = 100 sits at price 105 — a buy limit above the market.
        let consumed = paper.handle_chart_input(&frame(chart, &scale, 100.0, true, true, false));
        assert!(consumed);
        assert!(
            paper.armed.is_some(),
            "the user clicks again after the toast"
        );
        assert!(paper.sim.orders().is_empty());
        assert!(paper.toast.is_some(), "the refusal explains itself");
    }

    /// The lines say what a press would do before it happens (audit M3/M4):
    /// draggable levels wear the resize cursor, an entry line with a
    /// missing leg offers the create-drag, and empty tape asks nothing.
    #[test]
    fn hover_cursors_announce_draggable_and_creatable_lines() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 300.0), chart, &scale),
            Some(egui::CursorIcon::ResizeVertical),
            "the stop at 90 sits at y 300 and drags"
        );
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 200.0), chart, &scale),
            Some(egui::CursorIcon::ResizeVertical),
            "the entry at 100 offers the missing take profit by drag"
        );
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 40.0), chart, &scale),
            None,
            "empty tape belongs to the chart"
        );
    }

    #[test]
    fn the_cmd_gesture_previews_and_a_label_click_places_the_order() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let ctrl = egui::Modifiers {
            command: true,
            ..Default::default()
        };
        let both = egui::Modifiers {
            shift: true,
            command: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);

        // Hold buy above the mark: a stop. Below: a limit.
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 100.0),
            shift,
            false,
        ));
        let preview = paper.cmd_preview.expect("preview above the mark");
        assert_eq!((preview.side, preview.kind), (Side::Buy, EntryKind::Stop));
        assert_eq!(preview.price, Decimal::from(110));
        assert_eq!(
            preview.pointer,
            egui::pos2(400.0, 100.0),
            "the aim is the hand"
        );
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 300.0),
            shift,
            false,
        ));
        let preview = paper.cmd_preview.expect("preview below the mark");
        assert_eq!((preview.side, preview.kind), (Side::Buy, EntryKind::Limit));

        // The sell key mirrors the table.
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 100.0),
            ctrl,
            false,
        ));
        let preview = paper.cmd_preview.expect("sell above the mark");
        assert_eq!((preview.side, preview.kind), (Side::Sell, EntryKind::Limit));

        // Both keys is ambiguous, no key is no gesture.
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 100.0),
            both,
            false,
        ));
        assert!(paper.cmd_preview.is_none(), "ambiguity shows nothing");
        paper.handle_chart_input(&frame(chart, &scale, 100.0, false, false, false));
        assert!(paper.cmd_preview.is_none(), "no key, no line");

        // The click places exactly what the preview said, wherever in the
        // plot it lands, through the same path as the right-click menu —
        // a label that rides the pointer can never be landed on, so the
        // held modifier is the deliberate act.
        let aim = egui::pos2(120.0, 300.0);
        assert!(
            paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, true)),
            "the click is the gesture's"
        );
        let orders = paper.working_orders();
        assert_eq!(orders.len(), 1, "the click rested the order");
        assert_eq!(orders[0].side, Side::Buy);
        assert_eq!(orders[0].price, Some(Decimal::from(90)));

        // No modifier, no preview, no order: an unmodified click on empty
        // canvas belongs to the chart.
        assert!(
            !paper.handle_chart_input(&frame(chart, &scale, 100.0, true, true, false)),
            "a bare click is nobody's order"
        );
        assert_eq!(paper.working_orders().len(), 1, "still just the one");

        // Disabled means invisible.
        paper.set_cmd_trading(CmdTradingSettings {
            enabled: false,
            ..Default::default()
        });
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 100.0),
            shift,
            false,
        ));
        assert!(paper.cmd_preview.is_none(), "the toggle hides the gesture");
    }

    /// Off-screen render proof (for environments where no window can
    /// present): the preview paints a dashed line, a label carrying
    /// side+kind+qty, and the gutter chip with the snapped price.
    #[test]
    fn the_cmd_preview_paints_line_label_and_price_chip() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 300.0),
            shift,
            false,
        ));
        assert!(paper.cmd_preview.is_some(), "the held key builds a preview");

        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let paint = PaintCtx {
                painter: &painter,
                chart_rect: chart,
                tag_right: chart.right(),
                axis_x: chart.right(),
                scale: &scale,
                reserved_chip_y: None,
                pointer: Some(egui::pos2(400.0, 300.0)),
            };
            paper.draw_cmd_preview(&paint);
        });
        let shapes = format!("{:?}", output.shapes);
        assert!(shapes.contains("BUY"), "the label names the side: {shapes}");
        assert!(
            shapes.contains("90"),
            "the gutter chip carries the snapped price"
        );
        let segments = shapes.matches("LineSegment").count();
        assert!(
            segments >= 8,
            "a dashed line paints as many short segments, got {segments}"
        );
    }

    /// The aim label rides the pointer instead of parking at the right
    /// edge — the whole point of the change — while the dashed line still
    /// reaches the axis, so label and price chip stay one statement.
    #[test]
    fn the_cmd_label_follows_the_pointer_and_the_line_reaches_the_axis() {
        let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let mut previous: Option<f32> = None;
        for x in [200.0_f32, 400.0, 600.0] {
            let (start, end, label) = cmd_preview_layout(band, egui::pos2(x, 250.0));
            assert_eq!(end.x, 800.0, "the line always reaches the axis");
            assert_eq!(start.x, x, "the line starts under the cursor");
            assert_eq!(
                label.right(),
                x - CMD_LABEL_CURSOR_GAP_PX,
                "the label rides a fixed gap off the pointer"
            );
            assert_eq!(label.width(), CMD_LABEL_WIDTH_PX);
            assert_eq!(label.center().y, 250.0);
            assert!(
                !label.contains(egui::pos2(x, 250.0)),
                "never under the cursor it belongs to"
            );
            if let Some(previous) = previous {
                assert!(label.left() > previous, "moving right moves the label");
            }
            previous = Some(label.left());
        }
    }

    /// The two edges: near the left one the label flips to the pointer's
    /// right rather than leaving the band, and near the right one the line
    /// starts further left so there is still a line to read.
    #[test]
    fn the_cmd_layout_clamps_at_both_edges_of_the_band() {
        let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

        let pointer = egui::pos2(20.0, 250.0);
        let (_, _, label) = cmd_preview_layout(band, pointer);
        assert!(label.left() >= band.left(), "never off the left edge");
        assert_eq!(
            label.left(),
            pointer.x + CMD_LABEL_CURSOR_GAP_PX,
            "no room on the left, so it flips right"
        );
        assert!(!label.contains(pointer), "still clear of the cursor");

        let pointer = egui::pos2(780.0, 250.0);
        let (start, end, label) = cmd_preview_layout(band, pointer);
        assert!(label.right() <= band.right(), "never off the right edge");
        assert_eq!(
            end.x - start.x,
            CMD_LINE_MIN_PX,
            "close to the axis the line starts further left"
        );
        assert!(!label.contains(pointer), "still clear of the cursor");

        // A band narrower than the label plus its gap cannot hold both; it
        // parks at the left edge rather than running off-plot to the left.
        let sliver = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 400.0));
        let (start, _, label) = cmd_preview_layout(sliver, egui::pos2(50.0, 250.0));
        assert_eq!(label.left(), sliver.left(), "a sliver parks at its edge");
        assert_eq!(start.x, sliver.left(), "and the line spans what there is");
    }

    /// Paint and press read one geometry: the label the layout hands the
    /// painter is the label the pointer that produced it was measured
    /// against (the overlay-controls rule), and the preview carries that
    /// exact pointer rather than re-deriving it.
    #[test]
    fn the_cmd_preview_carries_the_pointer_the_paint_lays_out_from() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(180.0, 300.0);
        paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, false));
        let preview = paper.cmd_preview.expect("the held key builds a preview");
        assert_eq!(preview.pointer, aim, "the aim is the pointer, whole");
        let (_, _, label) = cmd_preview_layout(chart, preview.pointer);
        assert_eq!(
            label.right(),
            aim.x - CMD_LABEL_CURSOR_GAP_PX,
            "the paint lays out from that same pointer"
        );
    }

    /// An annotation under the pointer keeps its pixel: no aim paints and
    /// no click places there, so Shift+drag on a channel corner still
    /// levels it. One gate governs paint, cursor and press together — the
    /// label can never promise an order the press will not make.
    #[test]
    fn the_aim_yields_the_pixel_to_a_drawing_already_under_it() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 300.0);

        let over_drawing = ChartInput {
            chart,
            scale: Some(&scale),
            pointer: Some(aim),
            primary_pressed: true,
            primary_down: true,
            primary_released: false,
            modifiers: shift,
            canvas_claimed: true,
            layer_visible: true,
        };
        assert!(
            !paper.handle_chart_input(&over_drawing),
            "the press belongs to the drawing"
        );
        assert!(paper.cmd_preview.is_none(), "and nothing aims over it");
        assert!(paper.working_orders().is_empty(), "so nothing was placed");
        assert_eq!(
            paper.hover_cursor(aim, chart, &scale),
            None,
            "no hand promising a click that will not happen"
        );

        // The very same pixel, one step off the line: the aim is back.
        assert!(paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, true)));
        assert_eq!(paper.working_orders().len(), 1, "clear canvas, order rests");
    }

    /// A pointer from another pane's band paints nothing here: the label
    /// rides an x, so laying it out against a band that does not hold that
    /// x would put a click target off the end of the plot.
    #[test]
    fn the_aim_paints_only_in_the_band_it_was_aimed_in() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(700.0, 300.0),
            shift,
            false,
        ));
        assert!(paper.cmd_preview.is_some(), "aimed on this band");

        // The same simulator drawn against a narrower band — the other
        // pane of a split, whose right edge stops short of that x.
        let other = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 400.0));
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            paper.draw_cmd_preview(&PaintCtx {
                painter: &painter,
                chart_rect: other,
                tag_right: other.right(),
                axis_x: other.right(),
                scale: &scale,
                reserved_chip_y: None,
                pointer: Some(egui::pos2(700.0, 300.0)),
            });
        });
        let shapes = format!("{:?}", output.shapes);
        assert!(
            !shapes.contains("BUY"),
            "a foreign pointer paints no label: {shapes}"
        );
    }

    /// The capture hook: a side, and optionally where along the band to
    /// park the hand the run does not have.
    #[test]
    fn the_cmd_preview_hook_parses_a_side_and_an_optional_x() {
        assert_eq!(
            CmdPreviewForce::parse("buy"),
            Some(CmdPreviewForce {
                side: Side::Buy,
                x_fraction: None
            })
        );
        assert_eq!(
            CmdPreviewForce::parse("SELL@0.15"),
            Some(CmdPreviewForce {
                side: Side::Sell,
                x_fraction: Some(0.15)
            })
        );
        assert_eq!(
            CmdPreviewForce::parse("buy@9"),
            Some(CmdPreviewForce {
                side: Side::Buy,
                x_fraction: Some(1.0)
            }),
            "out of range clamps into the band"
        );
        assert_eq!(
            CmdPreviewForce::parse("buy@left"),
            Some(CmdPreviewForce {
                side: Side::Buy,
                x_fraction: None
            }),
            "a bad fraction still paints, mid-band"
        );
        assert_eq!(CmdPreviewForce::parse("hold"), None);
    }

    /// The parked x is what a capture run states, so it wins over a real
    /// pointer that in such a run is nobody's aim — and without it the
    /// hook keeps its old mid-band park.
    #[test]
    fn the_forced_preview_aims_where_the_hook_says() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.cmd_preview_force = Some(CmdPreviewForce {
            side: Side::Sell,
            x_fraction: Some(0.25),
        });
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(700.0, 100.0),
            egui::Modifiers::default(),
            false,
        ));
        let preview = paper.cmd_preview.expect("the hook forces a preview");
        assert_eq!(preview.side, Side::Sell);
        assert_eq!(preview.pointer.x, 200.0, "a quarter into an 800px band");
        assert_eq!(preview.pointer.y, 100.0, "the real hand still sets price");

        paper.cmd_preview_force = Some(CmdPreviewForce {
            side: Side::Sell,
            x_fraction: None,
        });
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(700.0, 100.0),
            egui::Modifiers::default(),
            false,
        ));
        assert_eq!(
            paper.cmd_preview.expect("still forced").pointer.x,
            700.0,
            "with no stated x the real pointer is left alone"
        );
    }

    /// Paint one frame of the paper layer and return its shape dump — the
    /// off-screen render proof the tag tests read.
    fn layer_shapes(
        paper: &PaperTrading,
        chart: egui::Rect,
        scale: &PriceScale,
        pointer: Option<egui::Pos2>,
    ) -> String {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            paper.draw_layer(
                &painter,
                chart,
                chart.right(),
                chart.right(),
                scale,
                None,
                pointer,
            );
        });
        format!("{:?}", output.shapes)
    }

    /// A resting order rests as a pill and opens under the pointer: the
    /// full tag used to sit over the candles at the live price all
    /// session. The pill still names side, kind and size — an order line
    /// is accent-coloured whatever its side, so dropping the word would
    /// leave the chart unable to say what waits there. Off-screen render
    /// proof.
    #[test]
    fn a_resting_order_tag_is_a_pill_until_the_pointer_reaches_it() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // A buy limit at 90 — y 300 on this scale.
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let id = paper.working_orders()[0].id.0;

        // The frame the pointer arrives in is the frame that decides; the
        // paint reads that decision rather than re-asking its own pointer.
        let frame_at = |paper: &mut PaperTrading, y: f32| {
            paper.handle_chart_input(&cmd_frame(
                chart,
                &scale,
                egui::pos2(400.0, y),
                egui::Modifiers::default(),
                false,
            ));
            layer_shapes(paper, chart, &scale, Some(egui::pos2(400.0, y)))
        };

        let resting = frame_at(&mut paper, 60.0);
        assert!(
            resting.contains("BUY LMT 1"),
            "the pill still names side, kind and size: {resting}"
        );
        assert!(
            !resting.contains(&format!("#{id}")),
            "the id waits until you mean to act on it: {resting}"
        );
        assert!(
            !resting.contains("@ 90"),
            "the price is the gutter chip's job: {resting}"
        );
        assert!(!resting.contains('×'), "and no ✕ over the candles");

        let opened = frame_at(&mut paper, 300.0);
        assert!(
            opened.contains(&format!("#{id} BUY LMT 1 @ 90")),
            "reaching for it states the order whole: {opened}"
        );
        assert!(opened.contains('×'), "…and offers the cancel: {opened}");
    }

    /// The ✕ and its press are one thing, checked the only way that means
    /// anything: sweep a pointer down the ✕ column, and for every stop ask
    /// **the painter** whether a ✕ came out and **`control_at`** whether a
    /// cancel is offered. Both sides run off one `handle_chart_input`, so
    /// this fails the moment they are handed different pointers, different
    /// rects, or different `dragged` terms again.
    #[test]
    fn a_tag_offers_its_cancel_exactly_while_it_paints_one() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // Two orders: one near the top edge, where `clamp_tag_center`
        // pushes the tag off its own line and the two rows part company,
        // and one mid-plot where they coincide. Above the mark a buy rests
        // as a stop, below it as a limit.
        assert!(paper.place_resting(Side::Buy, EntryKind::Stop, 119.5));
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let x = chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0;
        let mut painted_anywhere = false;
        for step in -4_i16..=84 {
            let pointer = egui::pos2(x, f32::from(step) * 5.0);
            paper.handle_chart_input(&cmd_frame(
                chart,
                &scale,
                pointer,
                egui::Modifiers::default(),
                false,
            ));
            let painted = layer_shapes(&paper, chart, &scale, Some(pointer)).contains('×');
            let pressable = matches!(
                paper.control_at(pointer, chart, &scale),
                Some(PaperControl::CancelOrder(_))
            );
            assert_eq!(painted, pressable, "at y {}", pointer.y);
            painted_anywhere |= painted;
        }
        assert!(painted_anywhere, "the sweep crossed a ✕ at all");
    }

    /// The press side is fed a pointer the paint side never sees — the
    /// pane nulls `hover_pos` over its own chrome while `latest_pos`
    /// survives — so the ✕'s offer must come from the frame's *input*, not
    /// from whatever pointer each side happens to hold.
    #[test]
    fn a_cancel_offered_this_frame_survives_a_paint_with_no_pointer() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let on_row = egui::pos2(chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0, 300.0);
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            on_row,
            egui::Modifiers::default(),
            false,
        ));
        assert!(
            layer_shapes(&paper, chart, &scale, None).contains('×'),
            "the paint follows the frame's decision, not its own pointer"
        );
        assert!(matches!(
            paper.control_at(on_row, chart, &scale),
            Some(PaperControl::CancelOrder(_))
        ));

        // Pointer off the row: neither side offers anything.
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(on_row.x, 60.0),
            egui::Modifiers::default(),
            false,
        ));
        assert!(!layer_shapes(&paper, chart, &scale, Some(on_row)).contains('×'));
        assert!(paper.control_at(on_row, chart, &scale).is_none());
    }

    /// Switched off, the layer is unpainted — so it is also untouchable:
    /// the aim's target is the whole plot, and an invisible plot-sized
    /// order button is the worst kind of hidden control.
    #[test]
    fn a_hidden_layer_paints_nothing_and_takes_no_press() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let hidden = ChartInput {
            chart,
            scale: Some(&scale),
            pointer: Some(egui::pos2(400.0, 200.0)),
            primary_pressed: true,
            primary_down: true,
            primary_released: false,
            modifiers: shift,
            canvas_claimed: false,
            layer_visible: false,
        };
        assert!(
            !paper.handle_chart_input(&hidden),
            "the press is the chart's"
        );
        assert!(paper.cmd_preview.is_none(), "and nothing is aimed");
        assert_eq!(
            paper.working_orders().len(),
            1,
            "no order rested through a hidden layer"
        );
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 300.0), chart, &scale),
            None,
            "and no cursor promises an invisible control"
        );
    }

    /// The aim stands down wherever something concrete already holds the
    /// pixel — this module's own lines and ✕s included. Otherwise holding
    /// the modifier while reaching for a stop rests a new order on top of
    /// it, and the hand cursor promises exactly that.
    #[test]
    fn the_aim_stands_down_over_paper_lines_controls_and_an_armed_ticket() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);

        // y 300 is the stop at 90 — a line a press would grab.
        let on_stop = egui::pos2(400.0, 300.0);
        paper.handle_chart_input(&cmd_frame(chart, &scale, on_stop, shift, false));
        assert!(paper.cmd_preview.is_none(), "the stop line keeps its pixel");
        assert_eq!(
            paper.hover_cursor(on_stop, chart, &scale),
            Some(egui::CursorIcon::ResizeVertical),
            "and the cursor still says so"
        );
        assert!(
            paper.handle_chart_input(&cmd_frame(chart, &scale, on_stop, shift, true)),
            "the press is paper's"
        );
        assert_eq!(
            paper.drag,
            PaperDrag::StopLoss,
            "it grabbed the stop instead of resting an order"
        );
        assert!(paper.working_orders().is_empty());
        paper.cancel_interaction();

        // An armed placement is an intent already stated.
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 100.0),
            shift,
            false,
        ));
        assert!(
            paper.cmd_preview.is_none(),
            "the armed ticket keeps the click"
        );
        assert!(paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 320.0),
            shift,
            true
        )));
        assert!(
            paper.armed.is_none(),
            "the armed placement fired and disarmed"
        );
        let orders = paper.working_orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(
            orders[0].kind,
            EntryKind::Limit,
            "the kind the ticket armed"
        );
    }

    /// A forced aim is a capture fixture: it paints so a screenshot has
    /// something to show, and it never places — a run with nobody at the
    /// keyboard is holding no modifier, and its stray clicks must not
    /// write orders into a journal.
    #[test]
    fn a_forced_aim_paints_but_never_places() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.cmd_preview_force = Some(CmdPreviewForce {
            side: Side::Buy,
            x_fraction: Some(0.5),
        });
        let aim = egui::pos2(400.0, 300.0);
        assert!(!paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            aim,
            egui::Modifiers::default(),
            true
        )));
        assert!(paper.cmd_preview.is_some(), "it still paints");
        assert!(paper.working_orders().is_empty(), "and never places");
        assert_eq!(
            paper.hover_cursor(aim, chart, &scale),
            None,
            "no hand promising a click that does nothing"
        );
    }

    /// A dragged order keeps every field: a trader repricing one is
    /// reading the number they are moving, and the pointer is on the line
    /// they grabbed, not on the tag.
    #[test]
    fn a_dragged_order_keeps_its_full_statement() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let id = paper.working_orders()[0].id;
        paper.drag = PaperDrag::Order(id);
        paper.drag_price = Some(88.0);
        // The frame decides; the paint reads. A button-free frame leaves
        // the drag exactly where it was.
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 60.0),
            egui::Modifiers::default(),
            false,
        ));
        let shapes = layer_shapes(&paper, chart, &scale, None);
        assert!(
            shapes.contains(&format!("#{} BUY LMT 1 @ 88", id.0)),
            "the drag reads the price it is moving: {shapes}"
        );
        assert!(
            !shapes.contains('×'),
            "a moving order offers no cancel, as before: {shapes}"
        );
    }

    /// The capture hook rests a rung around the mark on the first print:
    /// one order each side, so whichever way the tape moves a tag is still
    /// on screen for the shutter, and it happens at once rather than 220
    /// prints in.
    #[test]
    fn the_orders_hook_rests_a_rung_either_side_of_the_mark() {
        let mut paper = PaperTrading::new();
        paper.orders_demo = Some(2);
        paper.on_trade(&print(1, 100_000));
        assert!(paper.orders_demo.is_none(), "placed once, never again");
        let prices: Vec<_> = paper
            .working_orders()
            .iter()
            .map(|order| (order.side, order.price.expect("a limit has a price")))
            .collect();
        assert_eq!(
            prices,
            vec![
                (Side::Buy, Decimal::from(99_940)),
                (Side::Sell, Decimal::from(100_060)),
                (Side::Buy, Decimal::from(99_880)),
                (Side::Sell, Decimal::from(100_120)),
            ],
            "two rungs, each side, stepping out from the mark"
        );
        paper.on_trade(&print(2, 100_000));
        assert_eq!(paper.working_orders().len(), 4, "a second print adds none");
    }

    /// A coarsely quoted mark rounds 6 bp to nothing: both legs would
    /// price *at* the mark, the simulator would refuse every one of them,
    /// and the run would photograph an empty chart with nothing to explain
    /// it. The step floors at one unit of the instrument's own precision.
    #[test]
    fn the_orders_hook_still_rests_on_an_integer_quoted_mark() {
        let mut paper = PaperTrading::new();
        paper.orders_demo = Some(2);
        // 620 * 0.0006 = 0.372, which rounds to zero at scale 0.
        paper.on_trade(&print(1, 620));
        let prices: Vec<_> = paper
            .working_orders()
            .iter()
            .map(|order| (order.side, order.price.expect("a limit has a price")))
            .collect();
        assert_eq!(
            prices,
            vec![
                (Side::Buy, Decimal::from(619)),
                (Side::Sell, Decimal::from(621)),
                (Side::Buy, Decimal::from(618)),
                (Side::Sell, Decimal::from(622)),
            ],
            "one tick per rung, and the rungs stay apart"
        );
        assert!(paper.orders_demo.is_none(), "orders rested, hook disarmed");
    }

    /// Nothing rested means the hook stays armed: disarming before the
    /// simulator has accepted anything is exactly how a silent empty
    /// capture happens.
    #[test]
    fn the_orders_hook_stays_armed_until_something_rests() {
        let mut paper = PaperTrading::new();
        paper.orders_demo = Some(1);
        // No mark yet: nothing to place around, nothing consumed.
        paper.rest_capture_orders();
        assert_eq!(paper.orders_demo, Some(1), "no mark, still armed");
        paper.on_trade(&print(1, 100_000));
        assert_eq!(paper.working_orders().len(), 2);
        assert!(paper.orders_demo.is_none());
    }

    /// The capture hook opens every tag with nobody at the mouse — the
    /// pill's open form is otherwise unreachable from a scripted run.
    #[test]
    fn the_order_hover_hook_opens_the_tag_with_no_pointer() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let id = paper.working_orders()[0].id.0;
        assert!(
            !layer_shapes(&paper, chart, &scale, None).contains(&format!("#{id}")),
            "no hand, no open tag"
        );
        paper.order_hover_force = true;
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 60.0),
            egui::Modifiers::default(),
            false,
        ));
        assert!(
            layer_shapes(&paper, chart, &scale, None).contains(&format!("#{id} BUY LMT 1 @ 90")),
            "the hook supplies the hand"
        );
    }

    /// One hover, two surfaces: the dock row already lifted the chart
    /// line, and now it opens the tag too.
    #[test]
    fn hovering_the_dock_row_opens_the_chart_tag() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let id = paper.working_orders()[0].id;
        paper.hovered_order = Some(id);
        // The dock draws before the canvas, so the frame that carries the
        // row's hover is the frame the chart reads.
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, 60.0),
            egui::Modifiers::default(),
            false,
        ));
        assert!(
            layer_shapes(&paper, chart, &scale, None)
                .contains(&format!("#{} BUY LMT 1 @ 90", id.0)),
            "the row's hover reaches the chart"
        );
    }

    #[test]
    fn cmd_modifier_tokens_round_trip_and_state_defaults_fill_gaps() {
        for modifier in CmdModifier::ALL {
            assert_eq!(CmdModifier::parse(modifier.as_str()), Some(modifier));
        }
        assert_eq!(CmdModifier::parse("hyper"), None);
        let state = crate::paper_state::PaperState {
            cmd_trading_enabled: Some(false),
            cmd_buy_modifier: Some("alt".to_owned()),
            cmd_sell_modifier: Some("hyper".to_owned()),
            ..Default::default()
        };
        let settings = CmdTradingSettings::from_state(&state);
        assert!(!settings.enabled);
        assert_eq!(settings.buy, CmdModifier::Alt);
        assert_eq!(
            settings.sell,
            CmdModifier::Ctrl,
            "an unknown token falls back to the default"
        );
    }

    #[test]
    fn dragging_the_stop_loss_reprices_it_on_release() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        assert_eq!(
            paper.sim.position().expect("long").stop_loss,
            Some(Decimal::from(90)),
        );
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // The stop at 90 sits at y = 300; grab it, pull to 95 (y = 250), drop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("still long").stop_loss,
            Some(Decimal::from(95)),
            "the drop resubmitted the bracket at the dragged price"
        );
    }

    /// The TradingView gesture: pull away from the entry line and the
    /// missing bracket leg is born on release — the profit side makes a
    /// take profit, the losing side a stop.
    #[test]
    fn dragging_from_the_entry_line_creates_the_missing_leg() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // Grab the entry at 100 (y = 200), pull up to 105 (y = 150), drop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
        let position = paper.sim.position().expect("still long");
        assert_eq!(
            position.take_profit,
            Some(Decimal::from(105)),
            "above a long is the profit side"
        );
        assert_eq!(
            position.avg_price,
            Decimal::from(100),
            "the entry itself never moves"
        );
        // The same pull downward births the stop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
        let position = paper.sim.position().expect("still long");
        assert_eq!(position.stop_loss, Some(Decimal::from(90)));
        assert_eq!(position.take_profit, Some(Decimal::from(105)), "untouched");
    }

    #[test]
    fn a_fully_bracketed_entry_line_blocks_the_gesture_but_never_moves() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.profit_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 200.0), chart, &scale),
            Some(egui::CursorIcon::NotAllowed),
            "both legs exist, so their own lines are the handles"
        );
        // Grabbing the entry still consumes the gesture (the chart must not
        // pan under it) but repositions nothing.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
        let position = paper.sim.position().expect("long");
        assert_eq!(position.avg_price, Decimal::from(100));
        assert_eq!(position.stop_loss, Some(Decimal::from(90)), "untouched");
        assert_eq!(position.take_profit, Some(Decimal::from(110)), "untouched");
        // Empty space is not ours: the press falls through to the chart.
        assert!(
            !paper.handle_chart_input(&frame(chart, &scale, 40.0, true, true, false)),
            "a press far from every line belongs to the pan"
        );
    }

    /// The ✕ on a working order's chart tag: the hit is pure geometry from
    /// the live scale — no hover, no prior paint, no cached rect that goes
    /// stale while a live chart autoscales — it wins over the armed click
    /// (which used to eat it), and it never reads as a drag on the line.
    #[test]
    fn the_order_tags_close_is_geometric_and_beats_the_armed_click() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let events = paper.sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: Decimal::from(95),
            bracket: Bracket::none(),
        });
        paper.handle_events(events);
        // The trap that used to swallow every chart click.
        paper.armed = Some(ArmedPlacement {
            side: Side::Sell,
            kind: EntryKind::Stop,
        });
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let close = close_button_rect(
            chart.right(),
            clamp_tag_center(scale.y(95.0), chart.top(), chart.bottom()),
        );
        let press = ChartInput {
            chart,
            scale: Some(&scale),
            pointer: Some(close.center()),
            primary_pressed: true,
            primary_down: true,
            primary_released: false,
            modifiers: egui::Modifiers::default(),
            canvas_claimed: false,
            layer_visible: true,
        };
        assert!(paper.handle_chart_input(&press), "the ✕ owns the press");
        assert!(
            paper.sim.orders().is_empty(),
            "the order is gone and the armed click placed nothing"
        );
        assert!(
            paper.armed.is_some(),
            "the armed placement neither fired nor died"
        );
        assert_eq!(paper.drag, PaperDrag::None, "and nothing started dragging");
    }

    /// A bracket handle press starts the create-drag — its rect is the
    /// hit-test's own geometry beside the entry tag, above the line for
    /// the profit side and below it for the losing side.
    #[test]
    fn a_bracket_handle_press_starts_the_create_drag() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // A long's SL handle sits below the entry line (above = false).
        let entry_center = clamp_tag_center(scale.y(100.0), chart.top(), chart.bottom());
        let handle = bracket_handle_rect(chart.right(), entry_center, false);
        let press = ChartInput {
            chart,
            scale: Some(&scale),
            pointer: Some(handle.center()),
            primary_pressed: true,
            primary_down: true,
            primary_released: false,
            modifiers: egui::Modifiers::default(),
            canvas_claimed: false,
            layer_visible: true,
        };
        assert!(
            paper.handle_chart_input(&press),
            "the handle owns the press"
        );
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("long").stop_loss,
            Some(Decimal::from(90)),
            "the handle drag placed the stop"
        );
    }

    /// One long, then every label the entry buttons can wear: the button
    /// must disclose close-or-reverse, because the quantity deciding it
    /// lives in a tab the toolbar never shows.
    #[test]
    fn entry_labels_disclose_what_the_press_would_do() {
        let mut paper = PaperTrading::new();
        assert_eq!(
            paper.entry_label(Side::Buy),
            "BUY 1",
            "flat is a plain entry"
        );
        paper.qty_text = "x".to_owned();
        assert_eq!(
            paper.entry_label(Side::Sell),
            "SELL",
            "an unparseable quantity promises nothing"
        );
        paper.qty_text = "2".to_owned();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));

        paper.qty_text = "1".to_owned();
        assert_eq!(paper.entry_label(Side::Buy), "BUY 1 (adds to 3)");
        assert_eq!(paper.entry_label(Side::Sell), "SELL 1 (closes 1 of 2)");
        paper.qty_text = "2".to_owned();
        assert_eq!(paper.entry_label(Side::Sell), "SELL 2 (closes)");
        paper.qty_text = "5".to_owned();
        assert_eq!(
            paper.entry_label(Side::Sell),
            "SELL 5 (reverses to short 3)"
        );
    }

    /// The status cell answers the reported question — "am I in a trade?" —
    /// not just "how many points".
    #[test]
    fn the_status_cell_distinguishes_open_from_flat() {
        let mut paper = PaperTrading::new();
        assert!(paper.status_cell().is_none(), "untouched owes no line");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (text, _) = paper.status_cell().expect("a position is state");
        assert_eq!(text, "SIM LONG 1 · 0 pts");
        paper.on_trade(&print(2, 105));
        let (text, sign) = paper.status_cell().expect("still open");
        assert_eq!(text, "SIM LONG 1 · +5 pts");
        assert_eq!(sign, std::cmp::Ordering::Greater);

        paper.close_position();
        paper.on_trade(&print(3, 107));
        let (text, sign) = paper.status_cell().expect("history keeps the cell");
        assert_eq!(text, "SIM +7 pts · flat");
        assert_eq!(sign, std::cmp::Ordering::Greater);
        assert!(paper.close_button_label().is_none(), "flat has no close");
    }

    #[test]
    fn the_close_button_names_the_position_it_exits() {
        let mut paper = PaperTrading::new();
        paper.qty_text = "3".to_owned();
        paper.seed(&print(0, 100));
        paper.market(Side::Sell);
        paper.on_trade(&print(1, 100));
        assert_eq!(paper.close_button_label().as_deref(), Some("Close 3 SHORT"));
        let summary = paper.position_summary().expect("open");
        assert_eq!(summary.side, Side::Sell);
        assert_eq!(summary.quantity, Decimal::from(3));
        assert_eq!(summary.avg_price, Decimal::from(100));
    }

    /// Reverse flips side and size in one market order, and the form's
    /// protective offsets ride along to the new entry.
    #[test]
    fn reverse_flips_the_position_with_the_forms_bracket() {
        let mut paper = PaperTrading::new();
        paper.qty_text = "2".to_owned();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        paper.stop_offset_text = "5".to_owned();
        paper.reverse_position();
        paper.on_trade(&print(2, 100));
        let position = paper.sim.position().expect("reversed, not flat");
        assert_eq!(position.side, Side::Sell);
        assert_eq!(position.quantity, Decimal::from(2));
        assert_eq!(
            position.stop_loss,
            Some(Decimal::from(105)),
            "the new short is protected by the form's offset"
        );
    }

    /// The chip dodge: lines keep their price, chips clear the last-price
    /// row by the minimum, and the fill-moment tie steps down.
    #[test]
    fn paper_chips_dodge_the_last_price_chip_never_the_line() {
        // No reservation, or far enough away: the chip stays at its line.
        assert_eq!(dodged_chip_y(100.0, None, 0.0, 400.0), 100.0);
        assert_eq!(dodged_chip_y(100.0, Some(200.0), 0.0, 400.0), 100.0);
        // Inside the band: pushed just clear, towards its own side.
        assert_eq!(
            dodged_chip_y(210.0, Some(200.0), 0.0, 400.0),
            200.0 + CHIP_CLEAR_PX
        );
        assert_eq!(
            dodged_chip_y(190.0, Some(200.0), 0.0, 400.0),
            200.0 - CHIP_CLEAR_PX
        );
        // The fill moment: entry == last price, and the chip steps down.
        assert_eq!(
            dodged_chip_y(200.0, Some(200.0), 0.0, 400.0),
            200.0 + CHIP_CLEAR_PX
        );
        // Never dodged out of the pane.
        assert_eq!(dodged_chip_y(398.0, Some(399.0), 0.0, 400.0), 383.0);
    }

    #[test]
    fn snapping_uses_the_marks_own_precision() {
        let mut paper = PaperTrading::new();
        paper.seed(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: Decimal::new(10325, 2), // 103.25 → two decimal places
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(paper.snap(101.23456), Decimal::new(10123, 2));
        // An integer-printing instrument snaps drags to whole points.
        paper.seed(&print(2, 182_035));
        assert_eq!(paper.snap(182_036.7), Decimal::from(182_037));
    }
}
