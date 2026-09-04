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
use egui_phosphor::regular as icons;
use quantick_engine::{Side, Trade};
use quantick_sim::{
    Bracket, BracketTarget, ClosedTrade, Command, EntryKind, OrderId, OrderIntent, OrderRole,
    Position, VenueEvent, signed_points,
};
// The journal's own format is named only by the tests that read one back;
// the writing moved to `paper_account`.
#[cfg(test)]
use quantick_sim::history;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::chart::PriceScale;
// One date law for every trade surface - see `paper_calendar`.
pub(crate) use crate::paper_account::{
    ArmedPlacement, CmdEntryKind, CmdModifier, CmdTradingSettings, Leg, PaperControl, side_word,
};
// The report's anchor date is formatted only under test.
#[cfg(test)]
use crate::paper_calendar::civil_utc;
use crate::paper_chrome::{
    PositionSummary, caption, fmt_decimal, fmt_points, fmt_signed_points, pill_toggle,
    points_color, position_word,
};
use crate::theme;
use crate::timezone::TzOffset;

// The report and the ledger moved to `paper_report`; these names did not.
// The control plane, the dock and the harness hooks all reach them through
// this module, and a type that changed address because its code did would
// make every one of those callers pay for a move they did not ask for.
pub(crate) use crate::paper_report::{LedgerAction, LedgerScope};

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

/// The kind the aim places at `price`, or `None` where nothing may rest
/// there.
///
/// [`CmdEntryKind::Auto`] reads the market: above the mark a buy stops in,
/// below it a buy waits at a limit; a sell mirrors. On the mark exactly,
/// nothing can rest — a resting order there would fill on the next print,
/// which is a market order wearing the wrong name.
///
/// A stated kind is honoured only where it is valid. Returning `None`
/// instead of the other kind is the point: the aim's promise is that the
/// label can never advertise an order the press will not make, and a
/// silent substitution would break it in the most expensive way — placing
/// a breakout stop for a trader who came to buy a pullback.
#[must_use]
fn resolve_cmd_kind(
    choice: CmdEntryKind,
    side: Side,
    price: Decimal,
    mark: Decimal,
) -> Option<EntryKind> {
    let available = match (price > mark, price < mark, side) {
        (true, _, Side::Buy) | (_, true, Side::Sell) => EntryKind::Stop,
        (true, _, Side::Sell) | (_, true, Side::Buy) => EntryKind::Limit,
        _ => return None,
    };
    match choice {
        CmdEntryKind::Auto => Some(available),
        CmdEntryKind::Limit => (available == EntryKind::Limit).then_some(EntryKind::Limit),
        CmdEntryKind::Stop => (available == EntryKind::Stop).then_some(EntryKind::Stop),
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
    /// The protection this order would carry: a strategy's ladder, the
    /// ruler's symmetric pair, or the ticket's typed offsets. Empty when the
    /// order would rest bare.
    ///
    /// One value, computed once, painted by the projection and placed by the
    /// click - a preview that promised one bracket while the order took
    /// another is the worst bug this surface can have.
    bracket: Bracket,
    /// How many ticks the ruler stands at; zero means it is not in use.
    ruler_ticks: u32,
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

    /// Install cmd-trading settings — the app's fan-out on boot and on a
    /// change made in any tab (one gesture, one meaning, everywhere).
    pub fn set_cmd_trading(&mut self, settings: CmdTradingSettings) {
        self.account.cmd_trading = settings;
        if !settings.enabled {
            self.cmd_preview = None;
        }
    }

    /// Drop the frame's preview — the pane calls this when a drawing tool
    /// owns the hand, so a stale line never keeps painting.
    pub fn clear_cmd_preview(&mut self) {
        self.cmd_preview = None;
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
        // The size the press would actually send. The risk-derived quantity
        // is written into the field by the ticket, so a toolbar reading the
        // field while the dock is closed promised a size the click did not
        // send - the button naming one number and the order carrying
        // another is the plainest kind of lie this surface can tell.
        let derived = self.risk_report().0.derived_quantity();
        let Some(qty) = derived.or_else(|| self.quantity_preview()) else {
            return word.to_owned();
        };
        let qty_text = fmt_decimal(qty);
        let Some(position) = self.account.venue.position() else {
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

        for order in self.account.venue.working_orders() {
            let Some(level) = order.price else { continue };
            let dragged = self.drag == PaperDrag::Order(order.id);
            let price = if dragged {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let y = ctx.scale.y(price);
            // The order's own line may be scrolled off while a leg of its
            // bracket is still on screen, so the legs are painted before
            // this gate rather than after it. `control_at` offers a leg's
            // cross whenever that leg is visible; skipping the paint here
            // left a cross that cleared a take profit on what looked like
            // empty chart — the inversion of "an invisible control is not a
            // control", and the more dangerous half of it.
            let entry_visible = ctx.in_range(y);
            if !entry_visible {
                self.draw_bracket_of(&ctx, BracketTarget::Order(order.id), true, false);
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
            let shown = if dragged {
                self.account.snap(price)
            } else {
                level
            };
            // A protective leg is a working order like any other, but it
            // is not an entry and must never read as one: it takes its
            // role's colour and says what it does rather than which way it
            // trades. Four accent-dashed `SELL LMT 1` rows over a long the
            // trader is already holding is the misreading this surface
            // cannot afford.
            let color = order_line_color(order.role);
            ctx.level_line(y, color, true, LINE_WIDTH_PX, hovered, dragged);
            ctx.gutter_chip(y, color, &fmt_decimal(shown));
            // Resting, the pill states what the gutter chip cannot — which
            // side and what kind of order waits there. The price is the
            // chip's job, and the id only matters once you mean to act on
            // it, so both wait for the open form.
            let name = order_line_name(order);
            let mut text = if expanded {
                format!(
                    "#{} {} {} @ {}",
                    order.id.0,
                    name,
                    fmt_decimal(order.quantity),
                    fmt_decimal(shown),
                )
            } else {
                format!("{name} {}", fmt_decimal(order.quantity))
            };
            // An order that can remove itself says so: without this, a
            // retest limit vanishing at its target reads as a glitch.
            if expanded && let Some(cancel) = order.cancel_at {
                text.push_str(&format!(" · cancels @ {}", fmt_decimal(cancel)));
            }
            // The ✕ is painted exactly when the press-side offers it —
            // one value, read twice, never two formulas.
            ctx.chip_tag(y, color, &text, open.is_some_and(|tag| tag.cancel));

            // The order's own protective legs, and the handles for the ones
            // it does not have yet. Dashed, because they are a promise: they
            // arm on the fill and not before. They paint here, beside the
            // order, rather than after every order — an order's stop belongs
            // in that order's z-order, not above a later order's line.
            //
            // Revealed by the same two things that open the tag: the line
            // under the pointer, or the order's row hovered in the dock. An
            // always-visible pair of buttons beside every resting order would
            // bury the candles they sit on.
            self.draw_bracket_of(
                &ctx,
                BracketTarget::Order(order.id),
                true,
                hovered || expanded,
            );
        }

        // Whether the pointer is on the position's entry line or its tag —
        // written inside the block below, read by the bracket paint after
        // it, because the tag's rect is only known once it is drawn.
        let mut position_reveal = false;
        if let Some(position) = self.account.venue.position().cloned() {
            let color = theme::side_color(position.side);
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
                    .account
                    .venue
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
                // The handles are revealed by the entry line or its tag —
                // the affordance behind "drag from the position line to
                // create".
                let over_tag = ctx
                    .pointer
                    .is_some_and(|pointer| tag_rect.expand(TAG_HOVER_SLACK_PX).contains(pointer));
                position_reveal = line_hovered || over_tag;
            }

            // Live legs on an open position: solid, because they are exits
            // that can fire on the next print.
            self.draw_bracket_of(&ctx, BracketTarget::Position, false, position_reveal);
        }

        if let Some(armed) = self.account.armed {
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
        let color = theme::side_color(preview.side);
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
        let (start, end, label) = cmd_preview_layout(band, ctx.axis_x, preview.pointer);
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
        self.draw_aim_bracket(ctx, &preview);
        // The wheel is an invisible affordance, and with no strategy armed
        // it is the only path to a stop. One faint line under the aim's own
        // label says so - at the pointer, at the moment it matters - and it
        // erases itself the first time the wheel is rolled, so a trader who
        // knows never reads it twice.
        if preview.bracket.is_empty() && !self.ruler_rolled {
            let (_, _, label) = cmd_preview_layout(
                egui::Rect::from_min_max(
                    ctx.chart_rect.min,
                    egui::pos2(
                        ctx.tag_right.min(ctx.chart_rect.right()),
                        ctx.chart_rect.max.y,
                    ),
                ),
                ctx.axis_x,
                preview.pointer,
            );
            ctx.painter.text(
                egui::pos2(label.center().x, label.max.y + 3.0),
                egui::Align2::CENTER_TOP,
                "roll the wheel for a stop and target",
                egui::FontId::proportional(10.0),
                theme::TEXT_SUPPORT,
            );
        }
    }

    /// The protection the aim is carrying, drawn beside it.
    ///
    /// Under the ruler both levels sit the same distance from the pointer,
    /// so the pair *is* the 1:1 read, and one chip states that distance in
    /// points and in ticks - a trader deciding whether a setup is worth
    /// taking is asking how far, not where. Under a strategy the whole
    /// ladder is drawn instead, every rung of it, before the click.
    fn draw_aim_bracket(&self, ctx: &PaintCtx<'_>, preview: &CmdPreview) {
        if preview.bracket.is_empty() {
            return;
        }
        let laddered = preview.bracket.is_laddered();
        // The same arithmetic the levels came from. Measuring the chip
        // against the tick while the lines were walked in points printed
        // `0.02 pts` beside a line twenty points away - a number a trader
        // would have sized a position from.
        let distance = self
            .ruler_step()
            .saturating_mul(Decimal::from(preview.ruler_ticks));
        for part in preview.bracket.parts() {
            for (level, word, color) in [
                (part.stop_loss, "SL", theme::SELL),
                (part.take_profit, "TP", theme::BUY),
            ] {
                let Some(level) = level else { continue };
                let y = ctx.scale.y(level.to_f64().unwrap_or_default());
                if !ctx.in_range(y) {
                    continue;
                }
                ctx.level_line(y, color, true, LINE_WIDTH_PX, false, false);
                ctx.gutter_chip(y, color, &fmt_decimal(level));
                let text = if laddered {
                    // A ladder labels each rung with the slice it closes:
                    // "TP" on three lines at three prices says nothing about
                    // which part each one belongs to.
                    format!(
                        "{word} {} · {}",
                        fmt_decimal(level),
                        part.quantity.map_or_else(|| "all".to_owned(), fmt_decimal),
                    )
                } else if preview.ruler_ticks > 0 {
                    // No tick count: it was only ever meaningful while one
                    // notch *was* one tick, and a notch is now worth what
                    // the instrument's step says.
                    format!(
                        "{word} {} · {} pts · 1:1",
                        fmt_decimal(level),
                        fmt_decimal(distance),
                    )
                } else {
                    format!("{word} {}", fmt_decimal(level))
                };
                ctx.chip_tag(y, color, &text, false);
            }
        }
    }

    /// One protective leg: its resting line and tag, the drag that reprices
    /// it, or — while a create-drag runs and the leg does not exist yet —
    /// the dashed preview of where release would put it. The tag gains the
    /// live R:R read once both legs are known, which is what turns the drag
    /// into a decision.
    fn draw_bracket_leg(&self, ctx: &PaintCtx<'_>, paint: &LegPaint) {
        let identity = (paint.owner, paint.leg);
        let amending =
            matches!(self.drag, PaperDrag::Leg { owner, leg } if (owner, leg) == identity);
        let creating = matches!(self.drag, PaperDrag::CreateLeg { owner, leg } if (owner, leg) == identity)
            && paint.level.is_none();
        let resting = paint.level.map(|level| level.to_f64().unwrap_or_default());
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
            self.account.snap(price)
        } else {
            paint.level.unwrap_or_else(|| self.account.snap(price))
        };
        let color = leg_color(paint.leg);
        // Dashed while it is being created (it is not placed yet) and while
        // it rides an unfilled order (it is a promise that arms on the
        // fill). Solid only once it is a live exit on an open position.
        ctx.level_line(
            y,
            color,
            creating || paint.pending,
            LINE_WIDTH_PX,
            hovered,
            dragging,
        );
        ctx.gutter_chip(y, color, &fmt_decimal(shown));
        // A leg riding an unfilled order names the order it belongs to and
        // says it is not protecting anything yet.
        //
        // Both halves are honesty, not decoration. Without the id, two
        // resting orders put two identical `SL` tags on the chart and no
        // way to tell which is whose. Without `on fill`, `SL 90.0 -5.0
        // pts` reads exactly like a live position's stop — a trader would
        // read protection they do not have, which is the same class of
        // mistake as reading a simulated fill as a real one. Dashing the
        // line says it too, but a dash is not a sentence, and this is the
        // number the eye lands on.
        let owner = match paint.owner {
            BracketTarget::Order(id) => format!("#{} ", id.0),
            BracketTarget::Position => String::new(),
        };
        let mut text = format!(
            "{}{} {} {} pts",
            owner,
            paint.leg.word(),
            fmt_decimal(shown),
            fmt_signed_points(signed_points(
                paint.side,
                paint.reference,
                shown,
                paint.quantity
            )),
        );
        if paint.pending {
            text.push_str(" · on fill");
        }
        if dragging && let Some(ratio) = rr_ratio(paint, shown) {
            text.push_str(&format!(" · R:R {ratio}"));
        }
        ctx.chip_tag(y, color, &text, !dragging);
    }

    /// Every rung of a laddered bracket, labelled with the slice it closes.
    ///
    /// Dashed like any protection that has not armed yet, and carrying the
    /// quantity because "TP" on three lines at three prices says nothing
    /// about which part each one belongs to. No handles and no `×`: a rung
    /// is the strategy's, not the pointer's.
    fn draw_ladder_rungs(
        &self,
        ctx: &PaintCtx<'_>,
        owner: BracketTarget,
        side: Side,
        reference: Decimal,
        bracket: Bracket,
    ) {
        let order = match owner {
            BracketTarget::Order(id) => Some(id),
            BracketTarget::Position => None,
        };
        for (index, part) in bracket.parts().enumerate() {
            for (level, leg) in [
                (part.stop_loss, Leg::StopLoss),
                (part.take_profit, Leg::TakeProfit),
            ] {
                let Some(level) = level else { continue };
                let color = leg_color(leg);
                // The rung being hauled follows the pointer, exactly as a
                // whole bracket's leg does: what the trader sees moving is
                // what the release will submit.
                let dragging = order.is_some_and(|id| {
                    self.drag
                        == PaperDrag::Rung {
                            order: id,
                            index,
                            leg,
                        }
                });
                let shown = if dragging {
                    self.drag_price
                        .map_or(level, |price| self.account.snap(price))
                } else {
                    level
                };
                let y = ctx.scale.y(shown.to_f64().unwrap_or_default());
                if !ctx.in_range(y) {
                    continue;
                }
                let hovered = ctx.hovers_line(y);
                ctx.level_line(y, color, true, LINE_WIDTH_PX, hovered, dragging);
                ctx.gutter_chip(y, color, &fmt_decimal(shown));
                let share = part.quantity.unwrap_or(Decimal::ONE);
                let points = signed_points(side, reference, shown, share);
                // The cross only where a press would take it: a rung of a
                // resting entry. A position's rungs are working orders by
                // then and carry their own.
                ctx.chip_tag(
                    y,
                    color,
                    &format!(
                        "{} {} · {} · {} pts",
                        leg.word(),
                        fmt_decimal(shown),
                        fmt_decimal(share),
                        fmt_signed_points(points),
                    ),
                    order.is_some() && !dragging,
                );
            }
        }
    }

    /// Both legs of one bracket owner, plus the labelled handles for the
    /// legs it does not have yet.
    ///
    /// One function for the position and for every working order: the
    /// grammar a trader learns on a position is the grammar that then works
    /// on an order, because it is the same code. `reveal` is whether the
    /// owner's own line or tag is under the pointer — the handles appear
    /// with it, since an always-visible pair of buttons beside every
    /// resting order would bury the candles they sit on.
    fn draw_bracket_of(
        &self,
        ctx: &PaintCtx<'_>,
        owner: BracketTarget,
        pending: bool,
        reveal: bool,
    ) {
        let Some((side, reference, bracket, quantity)) = self.account.bracket_owner(owner) else {
            return;
        };
        // A ladder's rungs are several prices and none of them is amendable
        // by a drag: the numbers belong to the strategy that shaped them.
        // They are drawn — an order the trader protected must never look
        // naked — and then this function stops, so no handle is offered that
        // would replace the whole ladder with one level.
        if bracket.is_laddered() {
            self.draw_ladder_rungs(ctx, owner, side, reference, bracket);
            return;
        }
        for leg in [Leg::StopLoss, Leg::TakeProfit] {
            self.draw_bracket_leg(
                ctx,
                &LegPaint {
                    owner,
                    leg,
                    side,
                    reference,
                    quantity,
                    level: leg.level(bracket),
                    other_level: leg.other(bracket),
                    pending,
                },
            );
        }
        if self.drag != PaperDrag::None {
            return;
        }
        let reference_y = ctx.scale.y(reference.to_f64().unwrap_or_default());
        if !ctx.in_range(reference_y) {
            return;
        }
        let center = clamp_tag_center(reference_y, ctx.chart_rect.top(), ctx.chart_rect.bottom());
        // Each handle sits on its leg's *price* side of the reference,
        // mapped through the chart's orientation: upside down the TP handle
        // keeps pointing at take-profit prices instead of trading places
        // with the stop's — the same price-not-pixels rule
        // `decide_pending_leg` follows.
        let flip = ctx.scale.is_inverted();
        let missing = [Leg::StopLoss, Leg::TakeProfit]
            .into_iter()
            .filter(|leg| leg.level(bracket).is_none());
        let over_handle = ctx.pointer.is_some_and(|pointer| {
            missing.clone().any(|leg| {
                bracket_handle_rect(ctx.tag_right, center, leg.sits_above_entry(side) != flip)
                    .contains(pointer)
            })
        });
        if !handles_visible(ctx.pointer, reveal, over_handle) {
            return;
        }
        for leg in missing {
            let rect =
                bracket_handle_rect(ctx.tag_right, center, leg.sits_above_entry(side) != flip);
            ctx.bracket_handle(rect, leg.word(), leg_color(leg));
        }
    }

    /// Route pointer input to the simulated lines. Returns true when paper
    /// trading owns the gesture this frame — the chart must not pan and the
    /// drawings must not select under it.
    /// Where `QUANTICK_PAPER_ORDER_HOVER` parks the hand a capture run does
    /// not have — on the first working order's line, in the tag column.
    ///
    /// The hook forces the *tag* open, but the bracket handles are drawn
    /// only where a pointer actually is, so that a pane the hand is not on
    /// never offers a press it will not take. Without a parked pointer the
    /// handles would be unreachable from a scripted run — the ParkedHand
    /// problem the aim's own `CmdPreviewForce` already solves this way.
    ///
    /// Only the pane feeding paper input asks, so parking one hand cannot
    /// put handles on two charts. It paints and never places: this is read
    /// by the draw, and the press side reads the real pointer alone.
    #[must_use]
    pub fn forced_hover_pointer(
        &self,
        chart: egui::Rect,
        tag_right: f32,
        scale: &PriceScale,
    ) -> Option<egui::Pos2> {
        if !self.order_hover_force {
            return None;
        }
        // The first order whose line this pane can actually show. Not
        // simply the first order: panes hold different price ranges, and a
        // hand parked on a line that is off-range here would be a hand on
        // nothing — the handles would stay unreachable on exactly the pane
        // the capture was pointed at.
        self.account
            .venue
            .working_orders()
            .iter()
            .filter_map(|order| order.price)
            .map(|level| scale.y(level.to_f64().unwrap_or_default()))
            .find(|y| *y >= chart.top() && *y <= chart.bottom())
            .map(|y| egui::pos2(tag_right - TAG_GAP_PX - TAG_BUTTON_PX / 2.0, y))
    }

    /// Whether a chart gesture this module owns is in flight — a line being
    /// dragged, a bracket leg being pulled into existence.
    ///
    /// The tab asks so it can keep the gesture with the pane that started
    /// it: the price under a grabbed line is read against one pane's scale,
    /// and letting the pointer wander into a neighbour mid-drag would
    /// reprice the order to whatever that pane's scale says.
    #[must_use]
    pub fn gesture_active(&self) -> bool {
        self.drag != PaperDrag::None
    }

    /// Cancel the transient chart interaction — an armed placement or a
    /// grabbed line (dropped without submitting). Called from the app's
    /// escape stack; returns true when there was something to cancel, so
    /// the stack spends exactly one layer on it.
    pub fn cancel_interaction(&mut self) -> bool {
        if self.account.armed.take().is_some() {
            return true;
        }
        if self.drag != PaperDrag::None {
            self.drag = PaperDrag::None;
            self.drag_price = None;
            return true;
        }
        if self.account.report.clear_selected_trade() {
            return true;
        }
        // The ruler is the *last* layer, not the first. It is a standing
        // preference rather than a gesture left half-finished, and putting
        // it in front shadowed the two things Escape was already for: a
        // trader with an order armed presses Escape to disarm it, and must
        // not lose their distance instead.
        self.clear_ruler()
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

        // The ruler. With the aim up, the wheel walks the projected stop and
        // target out from the pointer, one tick per notch, the same distance
        // on both sides - so what is on screen before the click is the trade
        // at 1:1, and the trader can see whether that distance is worth
        // taking. A forced aim is a capture fixture with no hand behind it
        // and never spends the wheel; a selected strategy owns the distances
        // and hands the wheel back to the chart.
        // Pressing the wheel puts the ruler away. Only while an aim is up,
        // so a middle-click anywhere else stays whatever it already was.
        if input.middle_pressed
            && self.cmd_preview.is_some_and(|preview| !preview.forced)
            && self.clear_ruler()
        {
            self.cmd_preview = self.compute_cmd_preview(input);
        }
        self.scroll_consumed = false;
        if input.scroll_y.abs() > f32::EPSILON
            && self.cmd_preview.is_some_and(|preview| !preview.forced)
        {
            self.step_ruler(input.scroll_y);
            self.ruler_rolled = true;
            self.scroll_consumed = true;
            // The aim now carries its bracket; recompute so the paint and
            // the press read one value rather than two.
            self.cmd_preview = self.compute_cmd_preview(input);
        }

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
                PaperControl::ClosePosition => self.account.close_position(),
                PaperControl::ClearLeg { owner, leg } => self.account.amend_leg(owner, leg, None),
                PaperControl::ClearRung { order, index, leg } => {
                    self.account.amend_rung(order, index, leg, None);
                }
                PaperControl::CancelOrder(id) => {
                    let events = self.account.venue.cancel(id);
                    self.account.handle_events(events);
                }
                PaperControl::Handle { owner, leg } => {
                    self.drag = PaperDrag::CreateLeg { owner, leg };
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
        if let Some(armed) = self.account.armed
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
                let price = self.account.snap(price);
                match drag {
                    PaperDrag::Leg { owner, leg } | PaperDrag::CreateLeg { owner, leg } => {
                        self.account.amend_leg(owner, leg, Some(price));
                    }
                    PaperDrag::Order(id) => {
                        let events = self.account.venue.amend_price(id, price);
                        self.account.handle_events(events);
                    }
                    PaperDrag::Rung { order, index, leg } => {
                        self.account.amend_rung(order, index, leg, Some(price));
                    }
                    PaperDrag::None | PaperDrag::Blocked | PaperDrag::CreatePending => {}
                }
            }
            return true;
        }

        self.drag != PaperDrag::None
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
        for order in self.account.venue.working_orders().iter().rev() {
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
        // Legs and handles, for every owner that has them. Working orders
        // first and newest-first, matching the draw stack above; the
        // position last, because its legs are the ones a trader reaches for
        // least often while an entry is still resting on top of them.
        for owner in self.account.bracket_owners() {
            let Some((side, reference, bracket, _)) = self.account.bracket_owner(owner) else {
                continue;
            };
            // A ladder offers no *whole-bracket* handle: one drag there
            // would replace every rung with a single level. Its rungs are
            // reachable one at a time instead, each with its own cross,
            // tested below - the trader's order is the trader's to move.
            if bracket.is_laddered() {
                if let BracketTarget::Order(id) = owner
                    && let Some(control) =
                        self.rung_control_at(id, bracket, pointer, tag_right, &visible_center)
                {
                    return Some(control);
                }
                continue;
            }
            let reference_center = visible_center(reference);
            for leg in [Leg::TakeProfit, Leg::StopLoss] {
                match leg.level(bracket) {
                    // A leg that exists offers its cross.
                    Some(level) => {
                        if let Some(center_y) = visible_center(level)
                            && close_button_rect(tag_right, center_y).contains(pointer)
                        {
                            return Some(PaperControl::ClearLeg { owner, leg });
                        }
                    }
                    // A leg that does not offers its handle. Same
                    // orientation mapping as the paint: the hit-test and the
                    // pixels must name the same handle, or a press acts on
                    // the leg the trader was not looking at.
                    None => {
                        if let Some(center_y) = reference_center
                            && bracket_handle_rect(
                                tag_right,
                                center_y,
                                leg.sits_above_entry(side) != scale.is_inverted(),
                            )
                            .contains(pointer)
                        {
                            return Some(PaperControl::Handle { owner, leg });
                        }
                    }
                }
            }
        }
        let position = self.account.venue.position()?;
        let entry_center = visible_center(position.avg_price)?;
        if close_button_rect(tag_right, entry_center).contains(pointer) {
            return Some(PaperControl::ClosePosition);
        }
        None
    }

    /// Spend this frame's wheel travel on the ruler, one tick per notch.
    ///
    /// Rolling up widens the bracket, rolling down narrows it; at zero the
    /// ruler is off and the order rests as bare as it always did.
    fn step_ruler(&mut self, scroll_y: f32) {
        // Learn this device's notch from the smallest roll it has produced,
        // so the very first roll moves a tick instead of vanishing into an
        // assumption about how a wheel is built.
        let travel = scroll_y.abs();
        if travel >= RULER_MIN_NOTCH_PX && travel < self.ruler_notch_px {
            self.ruler_notch_px = travel;
        }
        let notch = if self.ruler_notch_px.is_finite() {
            self.ruler_notch_px
        } else {
            return;
        };
        self.ruler_travel_px += scroll_y;
        let notches = (self.ruler_travel_px / notch).trunc();
        if notches == 0.0 {
            return;
        }
        self.ruler_travel_px -= notches * notch;
        let stepped = i64::from(self.ruler_notches) + notches as i64;
        self.ruler_notches = stepped.clamp(0, i64::from(RULER_MAX_NOTCHES)) as u32;
    }

    /// Put the ruler at `ticks`, clamped to what the wheel itself can reach,
    /// and answer with where it landed.
    ///
    /// The named form of rolling the wheel: same field, same bound, so a
    /// hand and a second operator can never leave the ruler somewhere the
    /// other cannot.
    pub(crate) fn set_ruler_ticks(&mut self, notches: u32) -> u32 {
        self.ruler_notches = notches.min(RULER_MAX_NOTCHES);
        self.ruler_travel_px = 0.0;
        self.ruler_notches
    }

    /// How far one notch walks this instrument's ruler, in points.
    ///
    /// What the trader typed for this symbol, else the derived default. A
    /// typed value is never silently corrected: it is theirs, and the field
    /// beside it is where a bad one is refused.
    #[must_use]
    pub(crate) fn ruler_step(&self) -> Decimal {
        if let Some(step) = self.ruler_steps.get(&self.account.symbol)
            && *step > Decimal::ZERO
        {
            return *step;
        }
        self.account.derived_ruler_step()
    }

    /// Name this instrument's step, in points. A value that is not positive
    /// clears it, which puts the instrument back on the derived default.
    pub(crate) fn set_ruler_step(&mut self, step: Option<Decimal>) {
        match step.filter(|value| *value > Decimal::ZERO) {
            Some(value) => {
                self.ruler_steps.insert(self.account.symbol.clone(), value);
            }
            None => {
                self.ruler_steps.remove(&self.account.symbol);
            }
        }
    }

    /// Every step the trader has named, by symbol, for the sidecar.
    pub(crate) fn ruler_steps(&self) -> &BTreeMap<String, Decimal> {
        &self.ruler_steps
    }

    /// Replace the remembered steps wholesale, from the sidecar.
    pub(crate) fn set_ruler_steps(&mut self, steps: BTreeMap<String, Decimal>) {
        self.ruler_steps = steps;
        self.ruler_step_text = self
            .ruler_steps
            .get(&self.account.symbol)
            .map(|step| fmt_decimal(*step))
            .unwrap_or_default();
    }

    /// Clear the ruler, so the next aim starts from the entry again.
    ///
    /// A distance chosen for one setup silently arming the next order was
    /// the review's own finding; `Esc` is where a trader already asks for
    /// "never mind".
    pub(crate) fn clear_ruler(&mut self) -> bool {
        let stood = self.ruler_notches > 0;
        self.ruler_notches = 0;
        self.ruler_travel_px = 0.0;
        stood
    }

    /// How far the ruler stands from the aim, in ticks; zero when it is off.
    #[must_use]
    pub(crate) fn ruler_ticks(&self) -> u32 {
        self.ruler_notches
    }

    /// Whether an aim is on screen this frame.
    ///
    /// The pointer compass asks before writing its own price on the axis:
    /// the aim already puts one there, on the same pixel, and two chips
    /// stacked on one pixel is not two facts. Same rule the crosshair tool
    /// already earns.
    #[must_use]
    pub fn aiming(&self) -> bool {
        self.cmd_preview.is_some()
    }

    /// Whether the ruler spent this frame's wheel travel. The chart asks
    /// before zooming: one wheel, one meaning at a time.
    #[must_use]
    pub fn consumed_scroll(&self) -> bool {
        self.scroll_consumed
    }

    /// The ruler's projected protection around an aimed entry.
    ///
    /// The same distance either side, always: what the trader reads is the
    /// trade at 1:1, and the question it answers is "is that distance worth
    /// it?" - asked before the order exists rather than after.
    ///
    /// It answers whatever the ticket is armed with, a strategy included:
    /// the ruler is a *compass*, and a trader deciding whether a setup is
    /// worth taking needs it most when they already have a ladder in mind.
    /// Standing it down under a strategy was a rule this module invented
    /// and the trader never asked for. `None` only while the ruler is at
    /// zero, and for a stop that would fall through zero on a cheap
    /// instrument.
    fn ruler_levels(&self, side: Side, price: Decimal) -> (Option<Decimal>, Option<Decimal>) {
        if self.ruler_notches == 0 {
            return (None, None);
        }
        let distance = self
            .ruler_step()
            .saturating_mul(Decimal::from(self.ruler_notches));
        let (stop, target) = match side {
            Side::Buy => (
                price.saturating_sub(distance),
                price.saturating_add(distance),
            ),
            Side::Sell => (
                price.saturating_add(distance),
                price.saturating_sub(distance),
            ),
        };
        if stop <= Decimal::ZERO || target <= Decimal::ZERO {
            return (None, None);
        }
        (Some(stop), Some(target))
    }

    /// The ✕ of whichever rung the pointer is over, if any.
    ///
    /// Separate from the whole-bracket controls because a rung is addressed
    /// by index: clearing one must leave the others exactly where they are,
    /// which a leg-shaped control cannot say.
    fn rung_control_at(
        &self,
        order: OrderId,
        bracket: Bracket,
        pointer: egui::Pos2,
        tag_right: f32,
        visible_center: &impl Fn(Decimal) -> Option<f32>,
    ) -> Option<PaperControl> {
        for (index, part) in bracket.parts().enumerate() {
            for (level, leg) in [
                (part.stop_loss, Leg::StopLoss),
                (part.take_profit, Leg::TakeProfit),
            ] {
                if let Some(level) = level
                    && let Some(center_y) = visible_center(level)
                    && close_button_rect(tag_right, center_y).contains(pointer)
                {
                    return Some(PaperControl::ClearRung { order, index, leg });
                }
            }
        }
        None
    }

    fn compute_cmd_preview(&self, input: &ChartInput<'_>) -> Option<CmdPreview> {
        if !self.account.cmd_trading.enabled || !input.layer_visible {
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
        if self.account.armed.is_some() {
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
                let buy = modifier_is_down(self.account.cmd_trading.buy, input.modifiers);
                let sell = modifier_is_down(self.account.cmd_trading.sell, input.modifiers);
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
        let mark = self.account.venue.mark_price()?;
        let raw_price = scale.price_at(pointer.y);
        let price = self.account.snap(raw_price);
        // The context menu's own validity table, plus the trader's stated
        // kind. `None` stands the aim down rather than substituting the
        // other kind — see `resolve_cmd_kind`.
        let kind = resolve_cmd_kind(self.account.cmd_trading.kind, side, price, mark)?;
        let quantity = self.quantity_preview().unwrap_or(Decimal::ONE);
        let ticket = self.ticket_bracket(side, price);
        Some(CmdPreview {
            side,
            kind,
            price,
            raw_price,
            pointer,
            forced,
            bracket: self.account.aim_bracket(
                side,
                price,
                quantity,
                ticket,
                &self.account_env(side, price),
            ),
            ruler_ticks: self.ruler_notches,
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
        for order in self.account.venue.working_orders() {
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
        let Some(position) = self.account.venue.position() else {
            self.drag = PaperDrag::Blocked;
            return;
        };
        let entry_y = scale.y(position.avg_price.to_f64().unwrap_or_default());
        let delta = pointer_y - entry_y;
        if delta.abs() < CREATE_DECIDE_THRESHOLD_PX {
            return;
        }
        // The leg is chosen by *price*, not by screen direction: on an
        // inverted chart up is the loss side for a long, and reading pixels
        // would hand the pull the wrong leg.
        let above_entry =
            scale.price_at(pointer_y) > position.avg_price.to_f64().unwrap_or_default();
        let profit_side = match position.side {
            Side::Buy => above_entry,
            Side::Sell => !above_entry,
        };
        let leg = if profit_side {
            Leg::TakeProfit
        } else {
            Leg::StopLoss
        };
        let bracket = Bracket::whole(position.stop_loss, position.take_profit);
        // A side whose leg already exists stays blocked: that leg's own
        // line is its handle.
        self.drag = if leg.level(bracket).is_none() {
            PaperDrag::CreateLeg {
                owner: BracketTarget::Position,
                leg,
            }
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
            PaperDrag::Leg { .. }
            | PaperDrag::Rung { .. }
            | PaperDrag::Order(_)
            | PaperDrag::CreatePending => Some(egui::CursorIcon::ResizeVertical),
            PaperDrag::None | PaperDrag::CreateLeg { .. } => None,
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
        for order in self.account.venue.working_orders().iter().rev() {
            if let Some(level) = order.price
                && near(level)
            {
                return Some(PaperDrag::Order(order.id));
            }
        }
        // A resting entry's rungs, newest order first, matching the draw
        // stack. They are the trader's to move: the strategy was the
        // template and this order carries a copy of it.
        for entry in self
            .account
            .venue
            .working_orders()
            .iter()
            .rev()
            // Only a ladder has rungs. A whole bracket's two levels stay
            // `Leg`s, which is the grammar every other surface already
            // speaks for them.
            .filter(|order| !order.is_protective() && order.bracket.is_laddered())
        {
            for (index, part) in entry.bracket.parts().enumerate() {
                for (level, leg) in [
                    (part.take_profit, Leg::TakeProfit),
                    (part.stop_loss, Leg::StopLoss),
                ] {
                    if level.is_some_and(near) {
                        return Some(PaperDrag::Rung {
                            order: entry.id,
                            index,
                            leg,
                        });
                    }
                }
            }
        }
        for owner in self.account.bracket_owners() {
            let Some((.., bracket, _)) = self.account.bracket_owner(owner) else {
                continue;
            };
            for leg in [Leg::TakeProfit, Leg::StopLoss] {
                if let Some(level) = leg.level(bracket)
                    && near(level)
                {
                    return Some(PaperDrag::Leg { owner, leg });
                }
            }
        }
        let position = self.account.venue.position()?;
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
            self.account.armed = None;
        }
    }

    /// Rest a limit/stop entry at `raw_price` with the ticket's quantity
    /// and offsets; returns whether the simulator accepted it.
    fn place_resting(&mut self, side: Side, kind: EntryKind, raw_price: f64) -> bool {
        let price = self.account.snap(raw_price);
        // The offsets are read here because an unreadable one is a message
        // beside the box the trader typed in. Everything after is placement.
        let Some(ticket) = self.parse_bracket(side, price) else {
            return false;
        };
        let env = self.account_env(side, price);
        self.account.place_resting(side, kind, price, ticket, &env)
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
        let Some(mark) = self.account.venue.mark_price() else {
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
        let price = self.account.snap(raw_price);
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

    // ------------------------------------------------------------------
    // Dock tab
    // ------------------------------------------------------------------

    /// The Trading dock tab: position, ticket, working orders, session
    /// strip. See `docs/ux/paper-trading.md` §3.
    pub fn draw_trading_tab(&mut self, ui: &mut egui::Ui) -> Option<TradingTabAction> {
        self.hovered_order = None;
        ui.label(
            egui::RichText::new(
                "Simulated fills from the tape - no broker. Results are in points; a currency here is the point value you declared.",
            )
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
        let changed = self.draw_order_entry(ui);
        ui.separator();
        self.draw_pending_orders(ui);
        ui.separator();
        let action = self.draw_session_summary(ui);
        if action.is_none() {
            // Same-frame collision with another action is a picker click;
            // the settings change persists on its next touch.
            if changed.strategies {
                return Some(TradingTabAction::OrderStrategiesChanged);
            }
            if changed.cmd_trading {
                return Some(TradingTabAction::CmdTradingChanged);
            }
            if changed.risk {
                return Some(TradingTabAction::RiskSettingsChanged);
            }
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
        let Some(position) = self.account.venue.position().cloned() else {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("FLAT")
                        .monospace()
                        .color(theme::TEXT_MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let realized = self.account.venue.realized_points();
                    ui.label(
                        egui::RichText::new(format!("{} pts", fmt_signed_points(realized)))
                            .monospace()
                            .strong()
                            .color(points_color(realized)),
                    )
                    .on_hover_text("this session's realized points");
                });
            });
            if !self.account.venue.working_orders().is_empty()
                && ui
                    .button("Cancel all orders")
                    .on_hover_text("remove every working order without trading (Shift+X)")
                    .clicked()
            {
                self.account.cancel_all_orders();
            }
            return;
        };
        let color = theme::side_color(position.side);
        let open = self
            .account
            .venue
            .mark_price()
            .map(|mark| position.open_points(mark));

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
            let events = self.account.dispatch(command);
            self.account.handle_events(events);
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
                self.account.close_position();
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
                let events = self.account.dispatch(Command::SetBracket {
                    stop_loss: Some(position.avg_price),
                    take_profit: position.take_profit,
                });
                self.account.handle_events(events);
            }
            if ui
                .add(Self::quiet_action("Close 50%", half))
                .on_hover_text(
                    "close half the open quantity at the next print; the rest keeps \
                     its average entry and brackets",
                )
                .clicked()
            {
                let events = self.account.dispatch(Command::ClosePartial {
                    quantity: (position.quantity / Decimal::TWO).normalize(),
                });
                self.account.handle_events(events);
            }
        });
        let full = ui.available_width();
        if ui
            .add(Self::quiet_action("Flatten all", full))
            .on_hover_text("close the position and cancel every working order (Shift+F)")
            .clicked()
        {
            self.account.flatten();
        }
    }

    fn draw_order_entry(&mut self, ui: &mut egui::Ui) -> OrderEntryChanges {
        ui.label(caption("ORDER"));
        // What the risk per trade makes of the entry the ticket is holding.
        // Read once for the whole form: the quantity field, the support line
        // and the entry pair must all be talking about the same aim.
        //
        // Side::Buy stands for both. Every stop this reads is symmetric in
        // distance - the ruler walks both legs the same number of points,
        // and a typed offset is a distance rather than a price - so the risk
        // is the side-independent half of the answer.
        let risk_reference = self.account.venue.mark_price().unwrap_or_default();
        let risk_state = self.risk_state(Side::Buy, risk_reference);
        let derived_quantity = risk_state.derived_quantity();
        let risk_blocks = risk_state.blocks_entry(self.account.risk.lock);
        if let Some(quantity) = derived_quantity {
            // The mode writes into the field the whole form already reads,
            // rather than adding a second number beside it: one quantity on
            // screen is the quantity that will be sent.
            let derived = fmt_decimal(quantity);
            if self.qty_text != derived {
                self.qty_text = derived;
            }
        }
        // Qty: free decimal text (empty must keep meaning "fix me"), with
        // steppers beside it; Shift steps by ten. Derived and read-only
        // while the risk per trade is deciding it.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Qty").color(theme::TEXT_MUTED).small());
            let step = if ui.input(|input| input.modifiers.shift) {
                Decimal::TEN
            } else {
                Decimal::ONE
            };
            let typed = derived_quantity.is_none();
            let hint = self.account.quantity_step_hint(step);
            ui.add_enabled_ui(typed, |ui| {
                if ui
                    .small_button("−")
                    .on_hover_text(format!("{hint} less (Shift: ten steps)"))
                    .clicked()
                {
                    self.step_quantity(-step);
                }
                ui.add(egui::TextEdit::singleline(&mut self.qty_text).desired_width(56.0));
                if ui
                    .small_button("+")
                    .on_hover_text(format!("{hint} more (Shift: ten steps)"))
                    .clicked()
                {
                    self.step_quantity(step);
                }
            })
            .response
            .on_disabled_hover_text(
                "the size is derived from your risk per trade - switch the mode off to type one",
            );
        });
        // The discreet line: what the number means, or why there is none.
        // Small and quiet on purpose - it explains the size without taking
        // the screen away from the chart.
        let sentence = risk_state.sentence();
        if !sentence.is_empty() {
            let colour = if risk_blocks {
                theme::WARN
            } else {
                theme::TEXT_FAINT
            };
            ui.label(egui::RichText::new(sentence).color(colour).small());
        }
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
                    self.account.armed = None;
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
        let strategies_changed = self.draw_strategy_row(ui);
        // The whole risk surface, in its own module: this file already
        // carries the order form, and a second feature inside it is how the
        // trunk grew the first time.
        let risk_changed = crate::risk_sizing::draw_risk_block(
            ui,
            crate::risk_sizing::RiskBlock {
                symbol: &self.account.symbol,
                settings: &mut self.account.risk,
                capital: &mut self.account.capital,
                book: &mut self.account.instrument_money,
                amount_text: &mut self.risk_amount_text,
                percent_text: &mut self.risk_percent_text,
                capital_text: &mut self.capital_text,
                point_value_text: &mut self.point_value_text,
                size_step_text: &mut self.size_step_text,
                currency_text: &mut self.currency_text,
            },
        );
        ui.add_space(4.0);

        // The entry pair: the surface where you commit, taller than the
        // toolbar's buttons. An armed side inverts — a mode you are in must
        // be visible on the control that put you there.
        let half = (ui.available_width() - 6.0) / 2.0;
        // The lock, enforced on the surface as well as at the click: a
        // ceiling the trader can still press through is one they will press
        // through by accident on a fast tape.
        let ready = self.account.ready() && !risk_blocks;
        let mut fire: Option<Side> = None;
        let mut arm: Option<Side> = None;
        let mut disarm = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for side in [Side::Buy, Side::Sell] {
                let color = theme::side_color(side);
                let armed_here = self.account.armed.is_some_and(|armed| armed.side == side);
                let armed_other = self.account.armed.is_some_and(|armed| armed.side != side);
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
            self.account.armed = None;
        }
        if let Some(side) = fire {
            self.market(side);
        }
        if let Some(side) = arm {
            self.account.armed = Some(ArmedPlacement {
                side,
                kind: self.order_type,
            });
        }
        ui.label(
            egui::RichText::new(if self.account.armed.is_some() {
                "Click the chart at your price. Esc cancels."
            } else if self.order_type == EntryKind::Market {
                "Market orders fill at the next print."
            } else {
                "The button arms a click; the next chart click rests the order there."
            })
            .color(theme::TEXT_SUPPORT)
            .small(),
        );
        OrderEntryChanges {
            cmd_trading: self.draw_cmd_trading_settings(ui),
            strategies: strategies_changed,
            risk: risk_changed,
        }
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
                self.account.cmd_trading.enabled,
                "hold a key over the chart: a dashed line shows exactly where the order \
                 will rest, and the click places it",
            )
            .clicked()
            {
                self.account.cmd_trading.enabled = !self.account.cmd_trading.enabled;
                changed = true;
            }
            for (word, slot) in [("Buy", true), ("Sell", false)] {
                ui.label(egui::RichText::new(word).color(theme::TEXT_MUTED).small());
                let current = if slot {
                    self.account.cmd_trading.buy
                } else {
                    self.account.cmd_trading.sell
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
                                    self.account.cmd_trading.buy = modifier;
                                } else {
                                    self.account.cmd_trading.sell = modifier;
                                }
                                changed = true;
                            }
                        }
                    });
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Place")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            let current = self.account.cmd_trading.kind;
            egui::ComboBox::from_id_salt("cmd_trading_entry_kind")
                .width(64.0)
                .selected_text(current.label())
                .show_ui(ui, |ui| {
                    for kind in CmdEntryKind::ALL {
                        if ui.selectable_label(current == kind, kind.label()).clicked()
                            && current != kind
                        {
                            self.account.cmd_trading.kind = kind;
                            changed = true;
                        }
                    }
                })
                .response
                .on_hover_text(
                    "which order the aim places - auto takes whichever kind can rest at \
                     the price, limit and stop place only that one and show nothing where \
                     it cannot rest",
                );
            ui.label(egui::RichText::new("Step").color(theme::TEXT_MUTED).small());
            // The empty field shows this instrument's own default as a hint,
            // so blank reads as "follows the instrument" rather than as
            // nothing. A tick is what the ladder speaks, and saying what one
            // is worth here is the only place the ticket ever does.
            let derived = self.account.derived_ruler_step();
            let tick = self.account.tick();
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.ruler_step_text)
                    .desired_width(52.0)
                    .hint_text(fmt_decimal(derived)),
            );
            if response.changed() {
                let typed = self.ruler_step_text.trim();
                let step = if typed.is_empty() {
                    None
                } else {
                    typed.parse::<Decimal>().ok()
                };
                self.set_ruler_step(step);
                changed = true;
            }
            response.on_hover_text(format!(
                "how far one wheel notch walks the aim's stop and target, in points of \
                 this instrument. One tick here is {}, so this instrument defaults to \
                 {} a notch. Empty follows that default; saved per symbol.",
                fmt_decimal(tick),
                fmt_decimal(derived),
            ));
            ui.label(egui::RichText::new("pts").color(theme::TEXT_FAINT).small());
        });
        if self.account.cmd_trading.enabled && self.account.cmd_trading.kind != CmdEntryKind::Auto {
            // A stated kind is valid on one side of the market only, so the
            // aim is silent on the other half of the chart. Said here, or a
            // trader spends a minute wondering why the gesture died.
            ui.label(
                egui::RichText::new(format!(
                    "the aim shows only where a {} can rest: {} the market",
                    self.account.cmd_trading.kind.label(),
                    if self.account.cmd_trading.kind == CmdEntryKind::Limit {
                        "below it to buy, above it to sell"
                    } else {
                        "above it to buy, below it to sell"
                    },
                ))
                .color(theme::TEXT_SUPPORT)
                .small(),
            );
        }
        if self.account.cmd_trading.enabled
            && self.account.cmd_trading.buy == self.account.cmd_trading.sell
        {
            // A shared key is ambiguous, so the gesture shows nothing —
            // said here rather than discovered over the chart.
            ui.label(
                egui::RichText::new(
                    "buy and sell share a key - the gesture stays hidden until they differ",
                )
                .color(theme::AMBER)
                .small(),
            );
        } else if self.account.cmd_trading.enabled {
            // The gesture is invisible until a key is held; its one line
            // of instructions lives where the toggle does, not in a
            // tooltip a newcomer never hovers.
            ui.label(
                egui::RichText::new(format!(
                    "hold {} over the chart to buy, {} to sell - the dashed line shows \
                     where, and the click places it. Roll the wheel while holding to walk \
                     a stop and target out from the aim; roll back to zero, or press the \
                     wheel, to leave the aim as it was",
                    self.account.cmd_trading.buy.label(),
                    self.account.cmd_trading.sell.label(),
                ))
                .color(theme::TEXT_SUPPORT)
                .small(),
            );
        }
        changed
    }

    /// The ticket's strategy row: which named ladder the next order rests
    /// with, and the way into the editor that shapes them.
    ///
    /// Returns true when the selection changed, so the app can remember it.
    fn draw_strategy_row(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Strategy")
                    .color(theme::TEXT_MUTED)
                    .small(),
            )
            .on_hover_text(
                "a named exit ladder: the order rests with its parts already drawn on \
                 the chart. <None> rests a bare order you bracket by hand",
            );
            let selected = self.account.selected_order_strategy().map_or_else(
                || STRATEGY_NONE.to_owned(),
                |strategy| strategy.name.clone(),
            );
            let mut choice = self.account.selected_strategy;
            egui::ComboBox::from_id_salt("paper_order_strategy")
                .width(140.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, STRATEGY_NONE);
                    for (index, strategy) in self.account.strategies.iter().enumerate() {
                        ui.selectable_value(&mut choice, Some(index), &strategy.name);
                    }
                });
            if choice != self.account.selected_strategy {
                self.account.selected_strategy = choice;
                changed = true;
            }
            if ui
                .small_button("Edit…")
                .on_hover_text("build and change the named exit ladders")
                .clicked()
            {
                self.strategy_editor_open = true;
                self.strategy_editing =
                    self.account
                        .selected_strategy
                        .or(if self.account.strategies.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
            }
        });
        // The selected ladder, said in words: a trader must be able to read
        // what the next click will place without aiming it first.
        if let Some(strategy) = self.account.selected_order_strategy() {
            ui.label(
                egui::RichText::new(summarise_strategy(strategy))
                    .color(theme::TEXT_SUPPORT)
                    .small(),
            );
            // An armed ladder that cannot resolve draws nothing on the
            // chart, and a trader holding the modifier over a silent chart
            // has no way to know why. The reason belongs here, beside the
            // thing that is armed, in the colour the rest of this surface
            // uses for "this will not do what you think".
            if let Err(error) = strategy.validate() {
                ui.label(
                    egui::RichText::new(format!("not armed - {}", error.advice()))
                        .color(theme::AMBER)
                        .small(),
                );
            }
        }
        // The editor itself is drawn from the app's own frame, not from
        // here: it is a window, and a window that lives inside a dock tab
        // disappears the moment the trader looks at another panel.
        changed
    }

    /// The strategy editor: the list on the left, the open one's rows on the
    /// right, and one line saying why it cannot be used when it cannot.
    ///
    /// A window rather than a panel, because building a ladder is a job the
    /// trader finishes and closes - it is not part of reading the chart.
    ///
    /// Returns true when anything changed, so the app can persist it.
    pub(crate) fn draw_strategy_editor(&mut self, ctx: &egui::Context) -> bool {
        if !self.strategy_editor_open {
            return false;
        }
        // Opening onto a blank right pane while the list holds strategies is
        // an editor that looks broken. Whatever route opened it - the
        // ticket's button, or the launch hook, which has no click to carry a
        // choice - it opens on the one the ticket is armed with, else the
        // first.
        if self.strategy_editing.is_none() && !self.account.strategies.is_empty() {
            self.strategy_editing = Some(self.account.selected_strategy.unwrap_or(0));
        }
        let mut changed = false;
        let mut open = true;
        egui::Window::new("Exit strategies")
            .open(&mut open)
            .resizable(true)
            // Clear of the plot's top-left corner, where the indicator
            // legend lives: a window that opens under the legend reads as a
            // broken one, and the legend is an overlay the trader cannot
            // move out of the way.
            .default_pos(egui::pos2(360.0, 140.0))
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_min_width(150.0);
                        ui.label(caption("STRATEGIES"));
                        for index in 0..self.account.strategies.len() {
                            let selected = self.strategy_editing == Some(index);
                            let name = self.account.strategies[index].name.clone();
                            if ui.selectable_label(selected, name).clicked() {
                                self.strategy_editing = Some(index);
                            }
                        }
                        ui.add_space(4.0);
                        if ui
                            .small_button("+ New")
                            .on_hover_text("start a ladder from one whole-position rung")
                            .clicked()
                        {
                            self.account
                                .strategies
                                .push(new_strategy(self.account.strategies.len()));
                            self.strategy_editing = Some(self.account.strategies.len() - 1);
                            self.strategy_dirty = true;
                        }
                        if let Some(index) = self.strategy_editing
                            && ui
                                .small_button("Delete")
                                .on_hover_text("remove this strategy")
                                .clicked()
                        {
                            // Read the selection's *name* before the list
                            // shifts: resolving the index afterwards answers
                            // with whichever strategy slid into that slot, and
                            // the ticket would silently arm the neighbour of
                            // the one that was deleted.
                            let selected = self
                                .account()
                                .selected_order_strategy()
                                .map(|strategy| strategy.name.clone());
                            let removed = self.account.strategies.remove(index).name;
                            self.account.selected_strategy =
                                selected.filter(|name| *name != removed).and_then(|name| {
                                    self.account
                                        .strategies
                                        .iter()
                                        .position(|strategy| strategy.name == name)
                                });
                            self.strategy_editing = if self.account.strategies.is_empty() {
                                None
                            } else {
                                Some(index.min(self.account.strategies.len() - 1))
                            };
                            self.strategy_dirty = true;
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        // Structural edits and typed ones alike: held until
                        // the window closes, so one word is one save.
                        self.strategy_dirty |= self.draw_strategy_rows(ui);
                    });
                });
            });
        if !open {
            self.strategy_editor_open = false;
            // Closing is the save point: whatever was typed is what the
            // trader meant, and it reaches disk once.
            changed |= std::mem::take(&mut self.strategy_dirty);
        }
        changed
    }

    /// The open strategy's name and rungs.
    fn draw_strategy_rows(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(index) = self.strategy_editing else {
            ui.label(
                egui::RichText::new("No strategy yet - New starts one.")
                    .color(theme::TEXT_SUPPORT)
                    .small(),
            );
            return false;
        };
        let Some(strategy) = self.account.strategies.get_mut(index) else {
            return false;
        };
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Name").color(theme::TEXT_MUTED).small());
            if ui
                .add(egui::TextEdit::singleline(&mut strategy.name).desired_width(200.0))
                .changed()
            {
                changed = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Qty %")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            ui.add_space(38.0);
            ui.label(egui::RichText::new("Gain").color(theme::TEXT_MUTED).small());
            ui.add_space(34.0);
            ui.label(egui::RichText::new("Loss").color(theme::TEXT_MUTED).small());
            ui.label(
                egui::RichText::new("ticks")
                    .color(theme::TEXT_FAINT)
                    .small(),
            );
        });
        let mut remove: Option<usize> = None;
        for (row_index, row) in strategy.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let mut share = row.share_percent.to_f64().unwrap_or_default();
                if ui
                    .add(
                        egui::DragValue::new(&mut share)
                            .speed(1.0)
                            .range(0.0..=100.0)
                            .suffix("%"),
                    )
                    .changed()
                {
                    row.share_percent = Decimal::from_f64_retain(share)
                        .unwrap_or_default()
                        .round_dp(2);
                    changed = true;
                }
                changed |= ticks_field(ui, &mut row.gain_ticks, "how far the target sits");
                changed |= ticks_field(ui, &mut row.loss_ticks, "how far the stop sits");
                if ui
                    .small_button("−")
                    .on_hover_text("remove this row")
                    .clicked()
                {
                    remove = Some(row_index);
                }
            });
        }
        if let Some(row_index) = remove {
            strategy.rows.remove(row_index);
            changed = true;
        }
        if strategy.rows.len() < crate::order_strategies::MAX_ROWS
            && ui
                .small_button("+ Row")
                .on_hover_text("split the exit one more time")
                .clicked()
        {
            // A row added at zero makes the whole strategy invalid the
            // instant it appears - the shares no longer describe a whole
            // position - and the chart then silently stops projecting a
            // ladder the ticket still says is armed. The new row takes what
            // is left of 100%, and when nothing is left it halves the last
            // row rather than arriving broken.
            let assigned: Decimal = strategy.rows.iter().map(|row| row.share_percent).sum();
            let share = if assigned < Decimal::ONE_HUNDRED {
                Decimal::ONE_HUNDRED - assigned
            } else {
                let last = strategy
                    .rows
                    .last_mut()
                    .expect("a strategy always has a row to split");
                let half = (last.share_percent / Decimal::TWO).round_dp(2);
                last.share_percent -= half;
                half
            };
            strategy.rows.push(crate::order_strategies::StrategyRow {
                share_percent: share,
                gain_ticks: Some(NEW_RUNG_TICKS),
                loss_ticks: Some(NEW_RUNG_TICKS),
            });
            changed = true;
        }
        // The verdict, in one line, beside the fields that caused it.
        match strategy.validate() {
            Ok(()) => {
                ui.label(
                    egui::RichText::new("ready - the shares add up")
                        .color(theme::TEXT_SUPPORT)
                        .small(),
                );
            }
            Err(error) => {
                ui.label(
                    egui::RichText::new(error.advice())
                        .color(theme::AMBER)
                        .small(),
                );
            }
        }
        changed
    }

    /// Walk the typed quantity by `notches` of the instrument's own size
    /// step.
    ///
    /// The step is the instrument's, not one. A hard-coded 1 is already
    /// wrong on any instrument whose lot is fractional — a press moved a
    /// crypto size by a hundred thousand steps — and the floor is the
    /// instrument's minimum rather than "anything above zero", so the
    /// steppers can only ever land on a size the venue would take.
    fn step_quantity(&mut self, notches: Decimal) {
        let (unit, floor) = self
            .account
            .instrument_money
            .get(&self.account.symbol)
            .map_or((Decimal::ONE, Decimal::ONE), |money| {
                (money.size_step, money.min_size)
            });
        let current = self
            .qty_text
            .trim()
            .parse::<Decimal>()
            .ok()
            .filter(|quantity| *quantity > Decimal::ZERO)
            .unwrap_or(floor);
        let next = current.saturating_add(notches.saturating_mul(unit));
        if next >= floor {
            self.qty_text = fmt_decimal(next);
        }
    }

    fn draw_pending_orders(&mut self, ui: &mut egui::Ui) {
        let mut in_flight = Vec::new();
        self.account.venue.in_flight_entries(&mut in_flight);
        let queued_entries = in_flight.len();
        // Saturating, because the two reads cross a trait boundary and are
        // documented independently: a venue whose `in_flight_entries` says
        // more than its `in_flight` counts must not panic the render loop.
        let queued_closes = self
            .account
            .venue
            .in_flight()
            .saturating_sub(queued_entries);
        let orders: Vec<_> = self.account.working_orders().to_vec();
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
                        .color(theme::side_color(order.side)),
                );
                let mut line = format!(
                    "{} {} @ {}",
                    kind_short(order.kind),
                    fmt_decimal(order.quantity),
                    order.price.map_or_else(String::new, fmt_decimal),
                );
                // The self-cancel level rides the row — an order that can
                // vanish on its own never does so unannounced.
                if let Some(cancel) = order.cancel_at {
                    line.push_str(&format!(" · cancels @ {}", fmt_decimal(cancel)));
                }
                ui.label(
                    egui::RichText::new(line)
                        .monospace()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("cancel this order")
                        .clicked()
                    {
                        let events = self.account.dispatch(Command::CancelOrder { id: order.id });
                        self.account.handle_events(events);
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
            let realized = self.account.venue.realized_points();
            ui.label(
                egui::RichText::new(format!("{} pts", fmt_signed_points(realized)))
                    .monospace()
                    .strong()
                    .color(points_color(realized)),
            );
            ui.label(
                egui::RichText::new(format!(
                    "realized · {} trades",
                    self.account.venue.closed_trades().len()
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
                    let env = crate::paper_account::account_env!(self.account);
                    self.account.report.open(&env);
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
                        egui::RichText::new(format!(
                            "trades saved to: {}",
                            self.account.dir.display()
                        ))
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
                    std::path::absolute(&self.account.dir)
                        .unwrap_or_else(|_| self.account.dir.clone())
                        .display()
                ))
                .clicked()
            {
                reveal_folder(&self.account.dir);
            }
        });
        action
    }

    // ------------------------------------------------------------------
    // Report and ledger
    //
    // The window, the calendar and the trades tab live in `paper_report`.
    // What stays here is the seam: this host owns the journal folder, the
    // symbol and the venue, so it gathers those into a `ReportEnv` and
    // hands them over. Every wrapper below is one line for that reason and
    // not because a layer was added for its own sake - the control plane
    // and the harness hooks call these names, and a name the operator
    // already knows must not move because the code behind it did.
    // ------------------------------------------------------------------

    /// The report's state and its environment, together.
    ///
    /// Test-only, and a delegation: the parts are the account's now.
    #[cfg(test)]
    pub(crate) fn report_parts(
        &mut self,
    ) -> (
        &mut crate::paper_report::ReportState,
        crate::paper_report::ReportEnv<'_>,
    ) {
        self.account.report_parts()
    }

    /// The trades ledger tab. Returns what the ledger asked of the host.
    pub fn draw_trades_tab(&mut self, ui: &mut egui::Ui, tz: TzOffset) -> Option<LedgerAction> {
        let env = crate::paper_account::account_env!(self.account);
        self.account.report.draw_trades_tab(ui, tz, &env)
    }

    /// The performance report window, computed from what is on disk.
    pub fn draw_report_window(&mut self, ctx: &egui::Context, tz: TzOffset) {
        let env = crate::paper_account::account_env!(self.account);
        let asked = self.account.report.draw_window(ctx, tz, &env);
        // The report can decide a folder picker should open; opening one is
        // this host's job, because the import copies into *its* journal.
        if asked.start_import {
            self.account.start_import();
        }
        // And it can refuse a typed period. That message goes to the one
        // outbox every paper acknowledgement uses - dropping it here would
        // swallow a refusal the trader earned, which is what "a typed 2x
        // must never do nothing quietly" was written against.
        if let Some(message) = asked.toast {
            self.show_toast(message);
        }
    }

    // ------------------------------------------------------------------
    // End of frame
    // ------------------------------------------------------------------

    /// Settle this panel's per-frame handshakes. Runs last in the frame, and
    /// for **every** tab rather than only the one on screen.
    ///
    /// Three things happen here: the dock-hover link is cleared (the chart
    /// has already read it), and the export and import pickers are polled for
    /// a background job that finished. Both of those jobs belong to the tab
    /// that started them, and a trader who starts an export and then looks at
    /// another chart must not have to come back for it to land — which is
    /// what running this only for the active tab used to mean.
    ///
    /// It no longer draws anything. The message it produces goes to the
    /// window's one toast, through [`Self::take_toast`].
    pub fn settle(&mut self) {
        self.hovered_order = None;
        self.account.settle();
    }

    /// Take the acknowledgement waiting to be shown, if there is one.
    ///
    /// This panel used to draw its own: the same `CENTER_BOTTOM` anchor as
    /// the window's `ToastSurface`, 96px up instead of 44, on a 4-second
    /// clock instead of 8 — so two acknowledgements could sit in one lane, at
    /// two heights, disagreeing about how long an acknowledgement lasts. It
    /// posts to an outbox now and the host drains it into the one surface
    /// that owns the clock and the position.
    ///
    /// Newest wins, as the toast it replaces did: a slot, not a queue, since
    /// a trader reading a stale acknowledgement while the current one waits
    /// behind it is worse than missing the stale one. Taking rather than
    /// reading, so one message is handed over once however many frames pass.
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        self.account.take_toast()
    }

    /// Post an acknowledgement for the window to show. Newest wins.
    ///
    /// No clock is read here, unlike the toast this replaces: the surface is
    /// told the frame's `Instant` by the host, which is what makes an
    /// acknowledgement's lifetime as testable as the engine's arithmetic.
    pub(crate) fn show_toast(&mut self, message: String) {
        self.account.set_toast(message);
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

/// What the order form changed this frame; both are app-wide settings the
/// host persists and fans out to every tab.
#[derive(Debug, Clone, Copy, Default)]
struct OrderEntryChanges {
    cmd_trading: bool,
    strategies: bool,
    /// The risk per trade, the capital or an instrument's money moved, so
    /// the sidecar wants writing and the other tabs want telling.
    risk: bool,
}

/// A ladder to start from: one rung covering the whole position, which is
/// the plain bracket a trader already knows, ready to be split.
fn new_strategy(existing: usize) -> crate::order_strategies::OrderStrategy {
    crate::order_strategies::OrderStrategy {
        name: format!("Strategy {}", existing + 1),
        rows: vec![crate::order_strategies::StrategyRow {
            share_percent: Decimal::ONE_HUNDRED,
            gain_ticks: Some(NEW_RUNG_TICKS),
            loss_ticks: Some(NEW_RUNG_TICKS),
        }],
    }
}

/// One optional tick distance, with a box that empties to "no leg here".
fn ticks_field(ui: &mut egui::Ui, ticks: &mut Option<u32>, hover: &str) -> bool {
    let mut on = ticks.is_some();
    let mut changed = false;
    if ui.checkbox(&mut on, "").on_hover_text(hover).changed() {
        *ticks = if on { Some(NEW_RUNG_TICKS) } else { None };
        changed = true;
    }
    let mut value = ticks.unwrap_or(0);
    let enabled = ticks.is_some();
    if ui
        .add_enabled(
            enabled,
            egui::DragValue::new(&mut value)
                .speed(1.0)
                .range(1..=100_000),
        )
        .changed()
        && enabled
    {
        *ticks = Some(value);
        changed = true;
    }
    changed
}

/// A strategy said in one line: the rungs, in the trader's own order.
fn summarise_strategy(strategy: &crate::order_strategies::OrderStrategy) -> String {
    let rungs: Vec<String> = strategy
        .rows
        .iter()
        .map(|row| {
            let gain = row
                .gain_ticks
                .map_or_else(|| "runs".to_owned(), |ticks| format!("+{ticks}"));
            let loss = row
                .loss_ticks
                .map_or_else(|| "no stop".to_owned(), |ticks| format!("-{ticks}"));
            format!("{}% {gain}/{loss}", fmt_decimal(row.share_percent))
        })
        .collect();
    rungs.join(" · ")
}

/// A working order's line colour: accent for an entry waiting to trade, the
/// leg's own side colour for protection guarding a position.
fn order_line_color(role: OrderRole) -> egui::Color32 {
    match role {
        OrderRole::Entry => theme::ACCENT,
        OrderRole::StopLoss => theme::SELL,
        OrderRole::TakeProfit => theme::BUY,
    }
}

/// What a working order calls itself in its tag. An entry names its side and
/// kind, because those are what the accent line cannot say; a protective leg
/// names its job, because "SELL LMT" over a long is a lie about what it is
/// waiting to do.
fn order_line_name(order: &quantick_sim::Order) -> String {
    match order.role {
        OrderRole::Entry => format!("{} {}", side_word_upper(order.side), kind_short(order.kind)),
        OrderRole::StopLoss => "SL".to_owned(),
        OrderRole::TakeProfit => "TP".to_owned(),
    }
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
struct LegPaint {
    /// Whose leg this is. Also the drag identity: a leg being moved and a
    /// leg being created differ only in whether it existed a frame ago.
    owner: BracketTarget,
    leg: Leg,
    /// Side of the trade the leg protects.
    side: Side,
    /// The price the leg is measured against: the position's average entry,
    /// or the order's own resting price.
    reference: Decimal,
    /// Size, so a level can be read as points rather than as a price.
    quantity: Decimal,
    /// This leg's level today, `None` while it does not exist yet.
    level: Option<Decimal>,
    /// The other leg's level, for the R:R read while dragging.
    other_level: Option<Decimal>,
    /// Whether the leg belongs to an order that has not filled: it is a
    /// promise, not a live exit, and paints dashed to say so.
    pending: bool,
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
        // A chip dodged away from its own line is a price with no arrow back
        // to it, and near a chart edge every chip is dodged. The notch is
        // drawn at the *line's* height, not the chip's, so the two are only
        // ever read together.
        self.gutter_notch(y, color);
    }

    /// A small triangle on the axis edge, pointing into the plot at `y`.
    ///
    /// The dashed line says where across the width; this says where on the
    /// axis, which is the half a trader reads when the line is behind the
    /// tape band or the heat map. Drawn for every level the aim carries —
    /// the entry and both its legs — so one glance at the axis answers
    /// "where does this order sit" without following three lines back.
    fn gutter_notch(&self, y: f32, color: egui::Color32) {
        if !self.in_range(y) {
            return;
        }
        let x = self.axis_x;
        self.painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x - GUTTER_NOTCH_PX, y),
                egui::pos2(x, y - GUTTER_NOTCH_PX * 0.75),
                egui::pos2(x, y + GUTTER_NOTCH_PX * 0.75),
            ],
            color,
            egui::Stroke::NONE,
        ));
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
    // A band too short to hold a tag has nothing to clamp into, and
    // `f32::clamp` does not merely saturate there — it panics outright the
    // moment its bounds cross. That band is reachable: `plot_split` floors
    // the plot at 20 px, but `indicators::split_panes` then carves the
    // indicator strips out of it with no floor of its own, so a squeezed
    // window with enough panes really does leave the candles a few pixels.
    // Centre the tag in what there is rather than taking a live session
    // down.
    if bottom - top <= TAG_HEIGHT_PX {
        return f32::midpoint(top, bottom);
    }
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

/// Whether a bracket owner's `SL`/`TP` handles paint this frame.
///
/// The pointer clause is the one that is easy to miss. A pane that is not
/// feeding paper input has no pointer here, and `reveal` can still be true
/// over there — an order's tag opens on *every* pane at once, by design, so
/// one hover reads on both charts. Without the clause the other pane drew a
/// pressable-looking handle beside an order whose presses it does not take:
/// the exact inversion of the layer rule that an invisible control is not a
/// control, and no better.
fn handles_visible(pointer: Option<egui::Pos2>, reveal: bool, over_handle: bool) -> bool {
    pointer.is_some() && (reveal || over_handle)
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
fn rr_ratio(paint: &LegPaint, dragged: Decimal) -> Option<String> {
    let other = paint.other_level?;
    let entry = paint.reference;
    let (stop, target) = match paint.leg {
        Leg::StopLoss => (dragged, other),
        Leg::TakeProfit => (other, dragged),
    };
    let risk = entry.saturating_sub(stop).abs();
    let reward = target.saturating_sub(entry).abs();
    (risk > Decimal::ZERO).then(|| fmt_points(reward / risk))
}

/// Three-letter order kind for the compact chart tags (`LMT`, `STP`, `MKT`).
fn kind_short(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "MKT",
        EntryKind::Limit => "LMT",
        EntryKind::Stop => "STP",
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

/// `YYYY-MM-DD` in UTC — the report's anchor date.
#[cfg(test)]
fn fmt_utc_date(timestamp_ms: i64) -> String {
    let (year, month, day, ..) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02}")
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
    axis_x: f32,
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
    // The *band* stops at the live lane's divider, because that is where a
    // click can still be pressed; the line does not, because it is a read
    // and not a control. Stopping it there left the tape lane — the widest
    // thing on the chart — as a blank gap between the aim and its own price
    // on the axis, so the one place a trader watches the order arrive was
    // the one place the order was invisible. Every other level here already
    // spans to the axis (`level_line`); this now says the same.
    (
        start,
        egui::pos2(axis_x.max(band.right()), pointer.y),
        label,
    )
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
