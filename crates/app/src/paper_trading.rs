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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui;
use quantick_engine::{Side, Trade};
use quantick_sim::{
    Bracket, BracketTarget, ClosedTrade, EntryKind, OrderId, OrderIntent, VenueEvent,
};
// The journal's own format, and the command type the sim takes, are named
// only by the tests that drive one; the writing moved to `paper_account`.
#[cfg(test)]
use quantick_sim::{Command, history};
use rust_decimal::Decimal;

use crate::chart::PriceScale;
// One date law for every trade surface - see `paper_calendar`.
pub(crate) use crate::paper_account::{
    ArmedPlacement, CmdEntryKind, CmdModifier, CmdTradingSettings, Leg, PaperControl, side_word,
};
// The report's anchor date is formatted only under test.
#[cfg(test)]
use crate::paper_calendar::civil_utc;
use crate::paper_chrome::{PositionSummary, fmt_decimal};
use crate::theme;

// The report and the ledger moved to `paper_report`; these names did not.
// The control plane, the dock and the harness hooks all reach them through
// this module, and a type that changed address because its code did would
// make every one of those callers pay for a move they did not ask for.
pub(crate) use crate::paper_report::{LedgerAction, LedgerScope};

mod cmd;
mod input;
mod leg_tag;
mod paint;
mod paint_ctx;
mod ruler;
mod strategies;
mod ticket;

use cmd::{CmdPreview, CmdPreviewForce};
// The tag geometry, named by the tests that press a ✕ where one was painted.
#[cfg(test)]
pub(crate) use paint_ctx::{clamp_tag_center, close_button_rect};
use ticket::parse_offset;

/// `=<rungs>` rests entry orders around the mark as soon as the tape has
/// one, so the in-plot order tag can be photographed at all. The scripted
/// demo's own order is 220 prints away and sits 0.4 % out — far enough to
/// fall outside an autoscaled price range, and close enough that a lively
/// tape fills it before the shutter. Each rung is a **buy limit below and
/// a sell limit above**: a move in either direction can fill only one side
/// of it, so a resting tag always survives on screen.
const PAPER_ORDERS_ENV: &str = "QUANTICK_PAPER_ORDERS";
/// `=1` gives every order `QUANTICK_PAPER_ORDERS` rests a protective stop
/// and target, so the working-order bracket — its two dashed leg lines,
/// their gutter chips and their tags — can be photographed without a hand
/// to drag them into being. Pairs with `QUANTICK_PAPER_ORDER_HOVER`, which
/// opens one order's tag and with it the labelled `SL`/`TP` handles for the
/// legs it does *not* have; set both and one capture holds every state the
/// bracket has.
const PAPER_ORDER_BRACKET_ENV: &str = "QUANTICK_PAPER_ORDER_BRACKET";
/// How far a hooked bracket's legs sit from the order, as a fraction of the
/// mark. Wider than the rung step so the legs never land on a neighbouring
/// order's line, and wide enough apart that stop and target read as two
/// levels rather than one thick one.
const PAPER_ORDER_BRACKET_FRACTION: Decimal = Decimal::from_parts(15, 0, 0, false, 4);
/// How far the first rung sits from the mark, as a fraction of it. Small
/// on purpose: a line outside the chart's autoscaled price range paints no
/// tag, so an order that cannot be reached also cannot be seen.
const PAPER_ORDERS_STEP_FRACTION: Decimal = Decimal::from_parts(6, 0, 0, false, 4);
/// Rungs past this are refused — a capture wants a tag or two, not a book.
const PAPER_ORDERS_MAX_RUNGS: u8 = 4;
/// Grab distance for order lines — the drawings' select radius, so the two
/// grammars feel identical under the pointer.
const LINE_GRAB_RADIUS_PX: f32 = 10.0;
/// Dash geometry of a pending order's line (the last-price line's rhythm).
const ORDER_DASH_PX: f32 = 4.0;
/// Gap between dashes of a pending order's line.
const ORDER_GAP_PX: f32 = 4.0;

/// How far the axis notch reaches back into the plot, in pixels.
///
/// Small enough to read as a pointer rather than as another line, big
/// enough to find on a busy heat map — the levels it marks are the ones a
/// trader is about to commit size against.
const GUTTER_NOTCH_PX: f32 = 6.0;

/// The smallest wheel travel that can still count as a notch.
///
/// A floor, not the notch itself: how many pixels a mouse reports per notch
/// is the mouse's business, not ours. This build guessed 50 and met a mouse
/// that reports 40 — under which every roll computed zero ticks and the
/// ruler silently refused to move. The notch is *learned* from the smallest
/// travel actually seen (`ruler_notch_px`), and this floor only keeps a
/// trackpad's near-zero jitter from being mistaken for one.
const RULER_MIN_NOTCH_PX: f32 = 1.0;

/// The distance a freshly added rung starts at, in ticks.
///
/// A seed, not a default anyone lives with: the editor exists to change it,
/// and a row that arrived at zero would be a row the strategy refuses. Named
/// because it appears in three places - a new strategy, a new row, and a leg
/// switched back on - and three copies of a starting point drift.
const NEW_RUNG_TICKS: u32 = 20;

/// The furthest the ruler walks from the aim, counted in *notches*.
///
/// A tick count cannot be the bound once a notch is worth more than a tick:
/// at five points a notch on a one-cent instrument, the second roll would
/// hit a 999-tick ceiling and the ruler would stop dead at ten points —
/// short of every distance it exists to measure. Two hundred rolls is a
/// wrist's worth of wheel in either direction, whatever the step is worth.
const RULER_MAX_NOTCHES: u32 = 200;

/// What the strategy selector calls "no strategy" - the bare order.
const STRATEGY_NONE: &str = "<None>";

/// Opens the strategy editor on launch, so a capture run can photograph it
/// without a hand on the mouse. See `docs/ux/paper-trading.md`.
const STRATEGY_EDITOR_ENV: &str = "QUANTICK_PAPER_STRATEGY_EDITOR";
/// Stands the ruler at this many ticks on launch.
///
/// The ruler is walked with the wheel, and a scripted run has no wheel — so
/// without this the projected pair, its distance in points and ticks and the
/// `1:1` it reads are unreachable from a capture. Pair with
/// `QUANTICK_CMD_PREVIEW`, which supplies the aim the ruler measures from.
const RULER_TICKS_ENV: &str = "QUANTICK_PAPER_RULER_TICKS";
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

/// `=buy`/`=sell` forces the cmd-trading preview for a capture run, and an
/// optional `@<fraction>` parks the virtual pointer at that fraction of the
/// band's width (`buy@0.15` aims near the left edge). The held modifier and
/// the hand that moves the mouse are the two inputs a run with nobody at
/// the keyboard cannot supply (the ParkedHand rule) — and now that the
/// label rides the pointer, its x is a state of its own to capture.
const CMD_PREVIEW_ENV: &str = "QUANTICK_CMD_PREVIEW";
/// Forces every resting order's in-plot tag, and every bracket leg's, to its
/// expanded form for a capture run — the same ParkedHand problem: the
/// compact pill opens under a pointer no scripted run has.
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
/// Vertical clearance between a paper chip's centre and the last-price
/// chip's, in pixels — just over one chip height, so the two can never
/// overprint. At the instant a market order fills, the entry price *is* the
/// last price, and without this the one persistent "you are long" statement
/// is born unreadable.
const CHIP_CLEAR_PX: f32 = 16.0;

/// Which simulated line the pointer is dragging.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PaperDrag {
    #[default]
    None,
    /// Moving a protective leg that already exists.
    Leg { owner: BracketTarget, leg: Leg },
    /// Pulling a leg into existence, from its owner's line or its labelled
    /// handle; release submits it, exactly like repricing an existing one.
    CreateLeg { owner: BracketTarget, leg: Leg },
    /// Repricing a working order.
    Order(OrderId),
    /// The press landed on the position's entry line: an average entry is
    /// history, not an order, so the geometry stays put — but the gesture
    /// still belongs to the line (the chart must not pan under it). This is
    /// the state for a fully bracketed position, whose legs are their own
    /// handles.
    Blocked,
    /// The press landed on the position's entry line and at least one leg
    /// is missing: the first committed pull decides which leg the drag
    /// creates (profit side → take profit, losing side → stop loss). A
    /// working order needs no such state — its line already means
    /// "reprice", so its legs are born from their handles alone.
    CreatePending,
    /// Moving one rung of a resting entry's ladder.
    ///
    /// The rung belongs to the *order*, not to the strategy that shaped it:
    /// the strategy was the template, the order carries a copy, and hauling
    /// this line edits the copy. Nothing is written back to the named
    /// ladder, so the next order still rests with what the trader saved.
    ///
    /// A filled position needs no such state — its rungs are working orders
    /// by then, and their own lines already mean "reprice".
    Rung {
        order: OrderId,
        index: usize,
        leg: Leg,
    },
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
    /// The named exit strategies, or which one the ticket is set to,
    /// changed. App-wide for the same reason: a trader who builds a ladder
    /// in one tab means it everywhere.
    OrderStrategiesChanged,
    /// The risk per trade, the declared capital or an instrument's money
    /// changed. App-wide like the rest of the ticket's settings: a ceiling
    /// set in one tab is meant in all of them, and what a point of an
    /// instrument is worth does not change because a second tab is looking.
    RiskSettingsChanged,
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
    /// This frame's wheel travel over the chart, in pixels. The ruler
    /// spends it while the aim is up; the chart's own zoom is told to leave
    /// it alone on those frames (see [`PaperTrading::consumed_scroll`]).
    pub scroll_y: f32,
    /// The wheel was *pressed* this frame. With an aim up it puts the ruler
    /// away: the hand is already on the wheel that walked it out, so the
    /// way back costs no travel and no glance.
    pub middle_pressed: bool,
    /// Whether the paper layer is painted this frame. Switched off, its
    /// lines and tags are unpainted — so they take no press either. An
    /// invisible control is not a control, and the aim's target is now the
    /// whole plot rather than one small label.
    pub layer_visible: bool,
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
    key: TagKey,
    /// The ✕ is painted with the full statement, so a press may act on it.
    /// False while the order is being dragged: a moving order offers no
    /// cancel, and its tag is on a different row from its resting price.
    cancel: bool,
}

/// Whose tag an [`OpenTag`] opens: a working order's, or one leg of a
/// bracket (see `leg_tag`) — the same two states, one contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKey {
    Order(OrderId),
    Leg(BracketTarget, Leg),
}

/// The app-side trading host: the venue, order-entry form state,
/// chart-layer interaction, journal and report.
pub struct PaperTrading {
    /// Whether `QUANTICK_PAPER_ORDER_BRACKET` asked the capture hook's
    /// resting orders to carry protective legs.
    order_bracket_demo: bool,
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
    /// Whether the strategy editor window is up.
    strategy_editor_open: bool,
    /// An edit inside the editor that has not been saved yet.
    ///
    /// A name is typed one character at a time and a `DragValue` fires every
    /// frame it is held; persisting each of those would read, parse and
    /// rewrite the sidecar - and clone the list into every tab - dozens of
    /// times for one word, on the UI thread. The edits live in memory and
    /// the save happens when the editor closes, which is also when the
    /// trader has finished saying what they meant.
    strategy_dirty: bool,
    /// Which strategy the editor has open; `None` while the list is empty.
    strategy_editing: Option<usize>,
    /// How many *notches* the wheel has walked the projected bracket out
    /// from the aim. Sticky across aims within a session: a trader who
    /// decided their distance should not have to re-roll it for the next
    /// setup — but not across instruments, where the step itself changes.
    ruler_notches: u32,
    /// What the trader typed for this instrument's step, in points. Empty
    /// follows the instrument (see `RULER_DEFAULT_STEP_FRACTION`).
    ruler_step_text: String,
    /// The step each instrument was last given, in points, by symbol.
    ///
    /// Keyed by the bare symbol rather than by feed and symbol: the step
    /// describes the instrument's price geometry, not who streams it, and a
    /// recorded session must not make a trader relearn their wheel. The
    /// journal is already keyed this way.
    ruler_steps: BTreeMap<String, Decimal>,
    /// What the trader typed for the fixed risk per trade.
    risk_amount_text: String,
    /// What the trader typed for the percentage of capital.
    risk_percent_text: String,
    /// What the trader typed for this instrument's point value.
    point_value_text: String,
    /// What the trader typed for this instrument's size step.
    size_step_text: String,
    /// What the trader typed for this instrument's currency code.
    currency_text: String,
    /// What the trader typed for the capital in this instrument's currency.
    capital_text: String,
    /// Sub-notch wheel travel not yet worth a tick (a trackpad's scroll
    /// arrives in fractions of a notch).
    ruler_travel_px: f32,
    /// Whether the wheel has ever been rolled over an aim this session.
    ///
    /// Only the hint under the aim's label reads it: an affordance nobody
    /// can see needs saying once, and saying it forever is clutter a trader
    /// has to look past on every aim they take.
    ruler_rolled: bool,
    /// How much travel this pointing device reports for one notch, learned
    /// from the smallest roll seen rather than assumed.
    ///
    /// A mouse reports a fixed step per detent — 40 px here, 50 on the
    /// machine this was written on, something else on the next one — and a
    /// trackpad reports a continuous stream. Taking the smallest non-zero
    /// travel as the notch makes "one notch, one tick" true on all of them,
    /// and makes the first roll count instead of being swallowed.
    ruler_notch_px: f32,
    /// Whether the ruler spent this frame's wheel travel, so the chart's
    /// zoom can leave it alone.
    scroll_consumed: bool,
    // Chart-layer drag.
    drag: PaperDrag,
    drag_price: Option<f64>,
    /// The working order hovered in the dock this frame — its chart line
    /// lifts, so one hover reads on both surfaces. Cleared after the chart
    /// consumed it ([`PaperTrading::settle`] runs last in the frame).
    hovered_order: Option<OrderId>,
    /// The acknowledgement waiting to be handed to the window's one toast —
    /// The policy half: the venue, the journal, the risk, the
    /// strategies and the events. Reached by the control plane
    /// through [`Self::account`]; this module only draws it.
    account: crate::paper_account::PaperAccount,
}

impl Default for PaperTrading {
    fn default() -> Self {
        Self::new()
    }
}

fn side_word_upper(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

fn kind_word(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "market",
        EntryKind::Limit => "limit",
        EntryKind::Stop => "stop",
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

/// The leg's colour: a stop is an exit at a loss, a target an exit at a gain,
/// whatever side the trade is.
///
/// A free function here rather than a method on `Leg`, because `Leg` moved to
/// the account and a colour is a pixel. The account decides which leg a
/// gesture is about; this decides what that leg looks like.
fn leg_color(leg: Leg) -> egui::Color32 {
    match leg {
        Leg::StopLoss => theme::SELL,
        Leg::TakeProfit => theme::BUY,
    }
}

/// Whether the modifier is held. `Ctrl` reads the platform command key, so the
/// binding keeps meaning "the control-ish key" on every OS. A pixel-side read
/// for the same reason as [`leg_color`]: the key state is the window's.
fn modifier_is_down(modifier: CmdModifier, modifiers: egui::Modifiers) -> bool {
    match modifier {
        CmdModifier::Shift => modifiers.shift,
        CmdModifier::Ctrl => modifiers.command,
        CmdModifier::Alt => modifiers.alt,
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
        Self::with_trades_dir(crate::paper_account::PaperAccount::resolve_trades_dir(
            None, None,
        ))
    }

    /// A host journaling to `dir`, already resolved from config and
    /// environment.
    ///
    /// The policy half builds itself: the venue, the journal, the risk hook,
    /// the strategies and the scripted demo are the account's, and this
    /// constructor no longer knows how any of them are made.
    #[must_use]
    pub fn with_trades_dir(dir: PathBuf) -> Self {
        Self {
            account: crate::paper_account::PaperAccount::with_trades_dir(dir),
            order_bracket_demo: std::env::var(PAPER_ORDER_BRACKET_ENV)
                .is_ok_and(|value| value == "1"),
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
            strategy_editor_open: std::env::var(STRATEGY_EDITOR_ENV)
                .is_ok_and(|value| value == "1"),
            strategy_dirty: false,
            strategy_editing: None,
            ruler_notches: std::env::var(RULER_TICKS_ENV)
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .map_or(0, |notches| notches.min(RULER_MAX_NOTCHES)),
            ruler_step_text: String::new(),
            ruler_steps: BTreeMap::new(),
            risk_amount_text: String::new(),
            risk_percent_text: String::new(),
            point_value_text: String::new(),
            size_step_text: String::new(),
            currency_text: String::new(),
            capital_text: String::new(),
            ruler_travel_px: 0.0,
            ruler_rolled: false,
            ruler_notch_px: f32::INFINITY,
            scroll_consumed: false,
            drag: PaperDrag::None,
            drag_price: None,
            hovered_order: None,
        }
    }

    /// Everything the account needs from the ticket for one call.
    ///
    /// Built per call and never kept: a stored copy would answer for the form
    /// the trader used to have typed, which is `ReportEnv`'s reason too.
    /// The reading half of [`Self::parse_bracket`]: the same arithmetic with
    /// no toast, so the projection can ask what the ticket says without
    /// putting a message on screen every frame.
    fn ticket_bracket(&self, side: Side, reference: Decimal) -> Bracket {
        self.ticket_form().bracket(side, reference)
    }

    fn account_env(&self, side: Side, price: Decimal) -> crate::paper_account::AccountEnv {
        crate::paper_account::AccountEnv {
            ruler_levels: match self.ruler_levels(side, price) {
                (Some(stop), Some(target)) => Some((stop, target)),
                _ => None,
            },
            form: self.ticket_form(),
        }
    }

    /// The three typed boxes, read. The quantity carries its own complaint so
    /// that the account can raise it only if it ever reaches the box.
    fn ticket_form(&self) -> crate::paper_account::TicketForm {
        crate::paper_account::TicketForm {
            quantity: match self.qty_text.trim().parse::<Decimal>() {
                Ok(quantity) if quantity > Decimal::ZERO => Ok(quantity),
                _ => Err(format!(
                    "SIM: quantity must be a positive number - got `{}`",
                    self.qty_text.trim(),
                )),
            },
            // Both boxes or neither: one that does not parse fails the pair,
            // which is what `ticket_bracket`'s `?` did.
            offsets: match (
                parse_offset(&self.stop_offset_text),
                parse_offset(&self.profit_offset_text),
            ) {
                (Ok(stop), Ok(profit)) => Some((stop, profit)),
                _ => None,
            },
        }
    }

    /// The bracket the ticket's offsets describe, or the complaint about the
    /// text that does not parse - which is toasted here, beside the box.
    fn parse_bracket(&mut self, side: Side, reference: Decimal) -> Option<Bracket> {
        let stop_offset = match parse_offset(&self.stop_offset_text) {
            Ok(value) => value,
            Err(got) => {
                self.show_toast(format!(
                    "SIM: the stop offset must be a positive number of points - got `{got}`"
                ));
                return None;
            }
        };
        let profit_offset = match parse_offset(&self.profit_offset_text) {
            Ok(value) => value,
            Err(got) => {
                self.show_toast(format!(
                    "SIM: the profit offset must be a positive number of points - got `{got}`"
                ));
                return None;
            }
        };
        let form = crate::paper_account::TicketForm {
            quantity: Ok(Decimal::ONE),
            offsets: Some((stop_offset, profit_offset)),
        };
        Some(form.bracket(side, reference))
    }

    /// What the risk per trade says about the entry the aim is holding.
    pub(crate) fn risk_state(
        &self,
        side: Side,
        reference: Decimal,
    ) -> crate::risk_sizing::RiskState {
        self.account
            .risk_state(side, reference, &self.account_env(side, reference))
    }

    /// The risk read the control plane and the ticket's own line share.
    pub(crate) fn risk_report(&self) -> (crate::risk_sizing::RiskState, bool) {
        let reference = self.account.mark_price().unwrap_or_default();
        self.account
            .risk_report(&self.account_env(Side::Buy, reference))
    }

    /// The bracket an armed entry would carry, at this size.
    pub(crate) fn armed_bracket(
        &self,
        side: Side,
        reference: Decimal,
        quantity: Decimal,
    ) -> Bracket {
        self.account.armed_bracket(
            side,
            reference,
            quantity,
            &self.account_env(side, reference),
        )
    }

    /// The report's state, as its own surfaces read it. Test-only, like the
    /// account's own pair: the drawing code reaches the report through
    /// `report_parts`.
    #[cfg(test)]
    pub(crate) fn report_state(&self) -> &crate::paper_report::ReportState {
        self.account().report_state()
    }

    /// The report's state, mutably. Test-only, as above.
    #[cfg(test)]
    pub(crate) fn report_state_mut(&mut self) -> &mut crate::paper_report::ReportState {
        self.account_mut().report_state_mut()
    }

    /// Point the journal at a scratch folder (tests only).
    #[cfg(test)]
    pub(crate) fn redirect_history_dir(&mut self, dir: PathBuf) {
        self.account_mut().redirect_history_dir(dir);
    }

    /// Run one simulator command straight at the venue (tests only).
    #[cfg(test)]
    pub(crate) fn apply_sim_command_for_tests(&mut self, command: Command) {
        self.account_mut().apply_sim_command_for_tests(command);
    }

    /// The policy half, for readers that want it and not the pixels.
    ///
    /// The control plane goes through here: `control::{trade, session,
    /// interaction}` ask the account, so the second operator reads the money
    /// path rather than the ticket that draws it.
    pub(crate) fn account(&self) -> &crate::paper_account::PaperAccount {
        &self.account
    }

    /// The policy half, mutably.
    ///
    /// Nothing drains the outbox per call, and nothing needs to: the account
    /// holds the one toast slot the ticket used to hold, and the per-frame
    /// `settle` loop takes it the same way it always did. A caller here posts
    /// an acknowledgement by acting, not by remembering to hand one on.
    pub(crate) fn account_mut(&mut self) -> &mut crate::paper_account::PaperAccount {
        &mut self.account
    }

    // ------------------------------------------------------------------
    // The account, reached by name
    //
    // Every one of these moved to `paper_account`; the names stayed here so
    // that `app`, `tab`, `dock` and `toolbar` did not pay for a move they
    // did not ask for. The control plane does not come through these - it
    // asks `account()` directly, so a reader of `control::trade` sees the
    // money path and not the ticket that draws it.
    // ------------------------------------------------------------------

    pub(crate) fn close_button_label(&self) -> Option<String> {
        self.account().close_button_label()
    }

    /// Close everything, now. Named here as well as on the account because
    /// `Iterator::flatten` otherwise wins method resolution on a bare
    /// `.flatten()` and the shortcut stops closing the position.
    pub(crate) fn flatten(&mut self) {
        self.account_mut().flatten();
    }

    pub(crate) fn close_position(&mut self) {
        self.account_mut().close_position()
    }

    pub(crate) fn is_flat(&self) -> bool {
        self.account().is_flat()
    }

    pub(crate) fn mark_price(&self) -> Option<Decimal> {
        self.account().mark_price()
    }

    pub(crate) fn position_summary(&self) -> Option<PositionSummary> {
        self.account().position_summary()
    }

    pub(crate) fn ready(&self) -> bool {
        self.account().ready()
    }

    pub(crate) fn seed(&mut self, trade: &Trade) {
        self.account_mut().seed(trade)
    }

    pub(crate) fn session_trades(&self) -> &[ClosedTrade] {
        self.account().session_trades()
    }

    pub(crate) fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        self.account().status_cell()
    }

    pub(crate) fn working_orders(&self) -> &[quantick_sim::Order] {
        self.account().working_orders()
    }

    /// Follow the app's active symbol. A change retargets the journal; the
    /// simulator itself was already flattened by the timeline reset that
    /// every switch performs.
    pub fn set_symbol(&mut self, symbol: &str) {
        let Some(arriving) = self.account.set_symbol(symbol) else {
            return;
        };
        // The ruler goes with the instrument for the reason the tick does -
        // see `PaperAccount::set_symbol`. Arriving is not a switch.
        if !arriving {
            self.ruler_notches = 0;
        }
        self.ruler_travel_px = 0.0;
        self.ruler_step_text = self
            .ruler_steps
            .get(symbol)
            .map(|step| fmt_decimal(*step))
            .unwrap_or_default();
    }

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
        self.account.on_trade(trade);
        // `orders_demo` is a harness field, so its orders rest from here.
        if self.orders_demo.is_some() {
            self.rest_capture_orders();
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
        let Some(mark) = self.account.venue.mark_price() else {
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
                // The legs go on the correct side of the *order's own*
                // price, which is what the venue validates against — the
                // hook cannot place one the venue would refuse.
                // A ticket armed with a ladder rests a laddered order, which
                // is the only way a capture reaches a working order's rungs:
                // arranging one by hand needs a strategy selected and an
                // order placed, and a scripted run has neither click.
                // One contract per rung, so each one gets a whole contract
                // and the ladder photographs as the trader wrote it rather
                // than collapsing into a single rounded part.
                let quantity = self
                    .account()
                    .selected_order_strategy()
                    .map_or(Decimal::ONE, |strategy| {
                        Decimal::from(strategy.rows.len().max(1))
                    });
                let armed = self
                    .account()
                    .selected_order_strategy()
                    .and_then(|strategy| strategy.resolve(side, price, quantity, tick).ok())
                    .filter(quantick_sim::Bracket::is_laddered);
                let bracket = if let Some(bracket) = armed {
                    bracket
                } else if self.order_bracket_demo {
                    // The same floor the rung `step` carries, for the same
                    // reason: on a coarsely quoted market 15 bp rounds to
                    // nothing, both legs price *at* the order, and
                    // `validate_bracket` then refuses the whole
                    // `PlaceLimit` — the hook would rest no orders at all
                    // and photograph an empty chart with nothing to explain
                    // it.
                    let reach = (mark * PAPER_ORDER_BRACKET_FRACTION)
                        .round_dp(mark.scale())
                        .max(tick);
                    match side {
                        Side::Buy => Bracket::whole(
                            Some(price.saturating_sub(reach)),
                            Some(price.saturating_add(reach)),
                        ),
                        Side::Sell => Bracket::whole(
                            Some(price.saturating_add(reach)),
                            Some(price.saturating_sub(reach)),
                        ),
                    }
                } else {
                    Bracket::none()
                };
                let events = self
                    .account
                    .venue
                    .submit(OrderIntent::limit(side, quantity, price).with_bracket(bracket));
                self.account.handle_events(events);
            }
        }
        if self.account.venue.working_orders().is_empty() {
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

    /// The source rebuilt its timeline (replay seek, feed/symbol switch,
    /// restart): pending orders are swept and the position flattens at the
    /// last mark, labeled `reset` — never silently.
    pub fn on_timeline_reset(&mut self) {
        let had_position = self.account.venue.position().is_some();
        let had_orders =
            !self.account.venue.working_orders().is_empty() || self.account.venue.in_flight() > 0;
        let events = self.account.venue.reset();
        let mut all_saved = true;
        for event in &events {
            if let VenueEvent::Closed(trade) = event {
                all_saved &= self.account.journal(&trade.clone());
            }
        }
        // A reset ends the tape session, so it ends the file session too:
        // the next close opens a fresh file (same venue stamp lands as
        // `.rerun-N`). Without this, replaying the same recording again
        // without leaving replay appended run 2 into run 1's file.
        self.account.journal_path = None;
        self.account.armed = None;
        self.drag = PaperDrag::None;
        self.drag_price = None;
        // The instances disarm on the same reset; events from the torn-down
        // timeline must not leak into their next life.
        self.account.bot_events.clear();
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

    /// A toolbar/panel market order using the form's quantity and offsets.
    pub fn market(&mut self, side: Side) {
        let reference = self.account.mark_price().unwrap_or_default();
        // The offsets are the ticket's text; the rest is placement.
        let Some(ticket) = self.parse_bracket(side, reference) else {
            return;
        };
        let env = self.account_env(side, reference);
        self.account.market(side, reference, ticket, &env);
    }

    /// Flip the open position: one market order for twice its size, which
    /// closes it and opens the opposite side at the same quantity. The
    /// form's protective offsets apply to the new entry, exactly as they do
    /// to any market order.
    pub fn reverse_position(&mut self) {
        // The account says which way and at what mark; the ticket resolves
        // the protection for that side, because the offsets are its text.
        let Some((side, reference)) = self.account.reverse_aim() else {
            return;
        };
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return;
        };
        self.account.reverse_position(bracket);
    }

    // ------------------------------------------------------------------
    // Import
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Export
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Events, journal, parsing
    // ------------------------------------------------------------------
}

/// `YYYY-MM-DD` in UTC — the report's anchor date.
#[cfg(test)]
fn fmt_utc_date(timestamp_ms: i64) -> String {
    let (year, month, day, ..) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02}")
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

/// A journal folder of its own per host under test — tests must never
/// touch a real documents folder, nor see one another's files.
fn test_scratch_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Under the thread's own directory, which removes the whole tree when the
    // test's thread ends. Before this, the folder was named after the process
    // id alone and never removed: a reused pid handed a later run the earlier
    // run's journal, and three tests here failed on it.
    crate::scratch::thread_dir("paper-host").join(NEXT.fetch_add(1, Ordering::Relaxed).to_string())
}

crate::hooks::declare_hooks![
    "QUANTICK_CMD_PREVIEW",
    "QUANTICK_PAPER_ORDERS",
    "QUANTICK_PAPER_ORDER_BRACKET",
    "QUANTICK_PAPER_ORDER_HOVER",
    "QUANTICK_PAPER_RULER_TICKS",
    "QUANTICK_PAPER_STRATEGY_EDITOR"
];

#[cfg(test)]
mod tests;
