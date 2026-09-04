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

    /// The size and bracket an entry would take, or nothing with the reason
    /// already posted.
    fn entry_size(
        &mut self,
        side: Side,
        reference: Decimal,
        ticket: Bracket,
    ) -> Option<(Decimal, Bracket)> {
        let env = self.account_env(side, reference);
        self.account.entry_size(side, reference, ticket, &env)
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
        if self.account.symbol != symbol {
            // Leaving an instrument is what forgets its geometry; *arriving*
            // at the first one is not a switch. The app names the opening
            // symbol a frame after construction, so treating that as a
            // departure wiped a ruler the launch had just been asked for -
            // which is also why the harness hook below could never paint a
            // stop and target.
            let arriving = self.account.symbol.is_empty();
            self.account.symbol = symbol.to_owned();
            // Money a launch hook asked for lands on the symbol the tab
            // opens with, for the same reason the ruler does: the app names
            // that symbol a frame after construction, so there is nothing to
            // key it by until now. Spent once - a later switch is a real
            // switch, and the trader's own book answers for it.
            if let Some(money) = self.account.hook_money.take() {
                self.account
                    .instrument_money
                    .insert(symbol.to_owned(), money);
            }
            // The tick is the *instrument's*, and it only ever ratchets
            // finer: carried across a switch it would price the next
            // market's ruler and ladders in a precision that market has
            // never printed. The standing ruler goes with it — a distance
            // chosen on one instrument means nothing on the next, and it
            // would silently arm the first order placed there.
            self.account.tick_scale = 0;
            if !arriving {
                self.ruler_notches = 0;
            }
            self.ruler_travel_px = 0.0;
            self.ruler_step_text = self
                .ruler_steps
                .get(symbol)
                .map(|step| fmt_decimal(*step))
                .unwrap_or_default();
            self.account.journal_path = None;
            self.account.report.symbol_changed();
            // The revealed page is deliberately left alone. Every tab
            // syncs its journal to its symbol on every drain, so this
            // runs on the frame — resetting here retired the
            // `QUANTICK_LEDGER_PAGES` hook before the first row was
            // painted, and would retire a trader's scroll-back just as
            // silently. `LedgerPage::of` already clamps a page count the
            // new history cannot back.
        }
    }

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
        self.account.observe_precision(trade);
        let events = self.account.venue.on_trade(trade);
        self.account.handle_events(events);
        if self.orders_demo.is_some() {
            self.rest_capture_orders();
        }
        if self.account.demo.is_some() {
            self.account.run_demo_step();
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
        let reference = self.account.venue.mark_price().unwrap_or_default();
        let Some(ticket) = self.parse_bracket(side, reference) else {
            return;
        };
        // The same three sources the aim reads, in the same order. A button
        // sitting directly under the Strategy row must not place a bare
        // order while that row says a ladder is armed. The size comes from
        // the same call, so the risk per trade governs a toolbar press as
        // much as it governs the aim.
        let Some((quantity, bracket)) = self.entry_size(side, reference, ticket) else {
            return;
        };
        let events = self
            .account
            .venue
            .submit(OrderIntent::market(side, quantity).with_bracket(bracket));
        self.account.handle_events(events);
    }

    /// Flip the open position: one market order for twice its size, which
    /// closes it and opens the opposite side at the same quantity. The
    /// form's protective offsets apply to the new entry, exactly as they do
    /// to any market order.
    pub fn reverse_position(&mut self) {
        let Some(position) = self.account.venue.position().cloned() else {
            return;
        };
        let side = match position.side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let reference = self.account.venue.mark_price().unwrap_or_default();
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return;
        };
        let events = self.account.venue.submit(
            OrderIntent::market(side, position.quantity.saturating_add(position.quantity))
                .with_bracket(bracket),
        );
        self.account.handle_events(events);
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
        let Some(ticket) = self.parse_bracket(side, price) else {
            return false;
        };
        let Some((quantity, bracket)) = self.entry_size(side, price, ticket) else {
            return false;
        };
        let command = match kind {
            EntryKind::Limit => Command::PlaceLimit {
                side,
                quantity,
                price,
                bracket,
                cancel_at: None,
                flat_only: false,
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
        let events = self.account.dispatch(command);
        let placed = events
            .iter()
            .any(|event| matches!(event, VenueEvent::Placed(_)));
        self.account.handle_events(events);
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
        self.account.poll_export();
        self.account.poll_import();
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
mod risk_tests {
    use super::*;

    fn win_money() -> quantick_sim::InstrumentMoney {
        quantick_sim::InstrumentMoney {
            point_value: Decimal::new(20, 2),
            size_step: Decimal::ONE,
            min_size: Decimal::ONE,
            max_size: None,
            currency: quantick_sim::Currency::new("BRL").expect("BRL"),
            source: quantick_sim::MoneySource::Declared,
        }
    }

    fn book(money: quantick_sim::InstrumentMoney) -> crate::risk_sizing::InstrumentBook {
        [("WIN$N".to_owned(), money)].into_iter().collect()
    }

    /// A ticket on WIN$N with the money declared and a fixed risk set.
    fn armed_ticket(price: i64) -> PaperTrading {
        let mut paper = PaperTrading::new();
        paper.set_symbol("WIN$N");
        paper.seed(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        paper.account_mut().set_instrument_money(book(win_money()));
        paper
            .account_mut()
            .set_risk_settings(crate::risk_sizing::RiskSettings {
                basis: crate::risk_sizing::RiskBasis::Amount,
                amount: Decimal::from(100),
                amount_currency: None,
                percent: Decimal::ZERO,
                lock: true,
            });
        paper.set_ruler_step(Some(Decimal::ONE));
        paper
    }

    /// The wheel and a saved strategy are two ways of saying where the stop
    /// is, and the size must not depend on which the trader used - that is
    /// the whole reason both go through `aim_bracket`. Proven by sizing one
    /// entry each way and comparing, not by reading the funnel.
    #[test]
    fn the_wheel_and_a_saved_strategy_size_one_entry_identically() {
        let stop_points = 10_u32;

        let mut wheel = armed_ticket(140_000);
        wheel.set_ruler_ticks(stop_points);
        let by_wheel = wheel.risk_state(Side::Buy, Decimal::from(140_000));

        let mut ladder = armed_ticket(140_000);
        ladder.account_mut().set_order_strategies(
            vec![crate::order_strategies::OrderStrategy {
                name: "one rung".to_owned(),
                rows: vec![crate::order_strategies::StrategyRow {
                    share_percent: Decimal::ONE_HUNDRED,
                    gain_ticks: Some(stop_points),
                    loss_ticks: Some(stop_points),
                }],
            }],
            Some("one rung"),
        );
        let by_ladder = ladder.risk_state(Side::Buy, Decimal::from(140_000));

        assert_eq!(
            by_wheel.derived_quantity(),
            by_ladder.derived_quantity(),
            "wheel {by_wheel:?} vs ladder {by_ladder:?}"
        );
        assert_eq!(by_wheel.code(), by_ladder.code());
        // 10 points x 0.20 = 2.00 a contract; 100 / 2 = 50.
        assert_eq!(by_wheel.derived_quantity(), Some(Decimal::from(50)));
    }

    /// The lock, at the surface. With a risk per trade set there is no entry
    /// that exceeds it, and the refusal names the number rather than leaving
    /// a wheel that quietly stopped turning.
    #[test]
    fn a_stop_too_wide_for_the_budget_refuses_the_entry_and_says_why() {
        let mut paper = armed_ticket(140_000);
        // 4000 points x 0.20 = 800 for one contract, against a budget of 100.
        // Reached with a coarse step rather than more notches: the ruler
        // itself stops at `RULER_MAX_NOTCHES`.
        paper.set_ruler_step(Some(Decimal::from(20)));
        paper.set_ruler_ticks(200);
        let (state, blocks) = paper.risk_report();
        assert_eq!(state.code(), "clamped_at_minimum", "{state:?}");
        assert!(blocks, "the lock stands");
        let sentence = state.sentence();
        assert!(sentence.contains("800 BRL"), "{sentence}");
        assert!(sentence.contains("raise the risk"), "{sentence}");

        // And it is the *placement* that is refused, not merely the label.
        paper.market(Side::Buy);
        assert!(
            paper.is_flat(),
            "the lock refused the order rather than only colouring the ticket"
        );
    }

    /// The ceiling holds on the named path too. A lock the ticket enforces
    /// and `place_intent` does not would be a ceiling that stands only while
    /// a human is clicking - and `CLAUDE.md` makes the other operator
    /// first-class.
    #[test]
    fn the_lock_refuses_a_named_order_the_same_as_a_clicked_one() {
        let mut paper = armed_ticket(140_000);
        let intent = quantick_sim::OrderIntent::market(Side::Buy, Decimal::from(10))
            .with_bracket(Bracket::whole(Some(Decimal::from(136_000)), None));
        // 4000 points x 0.20 x 10 = 8,000 BRL against a budget of 100.
        let refusal = paper
            .account
            .risk_refusal_for(&intent)
            .expect("the ceiling names it");
        assert!(refusal.contains("8000 BRL"), "{refusal}");
        assert!(refusal.contains("turn the lock off"), "{refusal}");
        assert!(
            paper.account.place_intent(intent).is_empty(),
            "a named call must not slip past the ceiling the ticket enforces"
        );
        assert!(paper.is_flat(), "nothing was placed");
    }

    /// With the lock down the same named order goes out - the refusal is the
    /// trader's setting, not a rule the platform invented.
    #[test]
    fn a_named_order_over_budget_goes_out_once_the_lock_is_down() {
        let mut paper = armed_ticket(140_000);
        let mut risk = paper.account().risk_settings().clone();
        risk.lock = false;
        paper.account_mut().set_risk_settings(risk);
        let intent = quantick_sim::OrderIntent::market(Side::Buy, Decimal::from(10))
            .with_bracket(Bracket::whole(Some(Decimal::from(136_000)), None));
        assert!(paper.account.risk_refusal_for(&intent).is_none());
    }

    /// Taking the lock down is how a trader takes that entry anyway - a
    /// deliberate act, never a silent one.
    #[test]
    fn taking_the_lock_down_lets_the_over_budget_entry_through() {
        let mut paper = armed_ticket(140_000);
        paper.set_ruler_step(Some(Decimal::from(20)));
        paper.set_ruler_ticks(200);
        let mut risk = paper.account().risk_settings().clone();
        risk.lock = false;
        paper.account_mut().set_risk_settings(risk);
        let (state, blocks) = paper.risk_report();
        assert_eq!(state.code(), "clamped_at_minimum", "still over budget");
        assert!(!blocks, "but no longer refused");
    }

    /// An instrument with no declared money never guesses a size. In
    /// particular it must not borrow this file's `tick_size`, which is
    /// derived from the decimal places the tape has printed and says 1 for
    /// WIN$N where the real step is 5.
    #[test]
    fn an_undeclared_instrument_sizes_nothing_and_says_so() {
        let mut paper = armed_ticket(140_000);
        paper
            .account_mut()
            .set_instrument_money(crate::risk_sizing::InstrumentBook::new());
        paper.set_ruler_ticks(10);
        let (state, blocks) = paper.risk_report();
        assert_eq!(state.code(), "instrument_unknown");
        assert_eq!(state.derived_quantity(), None);
        assert!(!blocks, "an unknown instrument is not an over-budget one");
        assert!(state.sentence().contains("WIN$N"), "{}", state.sentence());
    }

    /// The steppers walk the instrument's own size step. A hard-coded 1 is
    /// already wrong on any fractional lot - it moved a crypto size by a
    /// hundred thousand steps.
    #[test]
    fn the_quantity_steppers_walk_the_instruments_own_size_step() {
        let mut paper = armed_ticket(140_000);
        paper
            .account_mut()
            .set_instrument_money(book(quantick_sim::InstrumentMoney {
                size_step: Decimal::new(1, 5),
                min_size: Decimal::new(1, 5),
                ..win_money()
            }));
        paper.qty_text = "0.00002".to_owned();
        paper.step_quantity(Decimal::ONE);
        assert_eq!(paper.qty_text, "0.00003");
        paper.step_quantity(-Decimal::ONE);
        assert_eq!(paper.qty_text, "0.00002");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper_account::{elide_path, export_csv, utc_compact};
    use crate::paper_report::HistoryRow;
    // Journalling tests read back the folders the writer created; the
    // helper that lists them lives with the rest of the shared chrome.
    use crate::paper_chrome::list_symbol_folders;
    use crate::paper_report::{load_history, report_from_history};

    /// One journal row: the trade, the folder it came from, the source its
    /// file recorded.
    fn row(symbol: &str, source: Option<history::SessionSource>, trade: ClosedTrade) -> HistoryRow {
        HistoryRow {
            symbol: symbol.to_owned(),
            source,
            trade,
        }
    }

    fn print(agg_id: u64, price: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
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
    fn an_in_session_rerun_opens_its_own_file() {
        // Seek-to-start / reopen-same-recording is a timeline reset with
        // the source unchanged: run 2 must not append into run 1's file.
        let mut paper = PaperTrading::new();
        paper.set_symbol("RESEEK");
        paper
            .account_mut()
            .set_session_source(history::SessionSource::Replay);
        for _ in 0..2 {
            paper.seed(&print(0, 100));
            paper.market(Side::Buy);
            paper.on_trade(&print(1, 100));
            let events = paper.account.dispatch(Command::ClosePosition);
            paper.account.handle_events(events);
            paper.on_trade(&print(2, 103));
            paper.on_timeline_reset();
        }
        let folder = paper.account.dir.join("RESEEK");
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
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 105));
        paper
            .account_mut()
            .set_session_source(history::SessionSource::Replay);
        assert_eq!(
            paper.account.session_trade_sources,
            vec![history::SessionSource::Live],
            "a trade keeps the source it closed under, not the current one"
        );
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
    fn utc_dates_format_from_the_same_civil_math() {
        assert_eq!(fmt_utc_date(1_773_666_068_000), "2026-03-16");
    }

    #[test]
    fn market_offsets_become_a_bracket_around_the_reference() {
        let mut paper = PaperTrading::new();
        paper.stop_offset_text = "5".to_owned();
        paper.profit_offset_text = "10".to_owned();
        let bracket = paper
            .parse_bracket(Side::Buy, Decimal::from(100))
            .expect("both parse");
        assert_eq!(bracket.stop_loss(), Some(Decimal::from(95)));
        assert_eq!(bracket.take_profit(), Some(Decimal::from(110)));
        let bracket = paper
            .parse_bracket(Side::Sell, Decimal::from(100))
            .expect("both parse");
        assert_eq!(bracket.stop_loss(), Some(Decimal::from(105)));
        assert_eq!(bracket.take_profit(), Some(Decimal::from(90)));
    }

    #[test]
    fn a_bad_offset_toasts_and_blocks_the_order() {
        let mut paper = PaperTrading::new();
        paper.stop_offset_text = "abc".to_owned();
        assert!(paper.parse_bracket(Side::Buy, Decimal::from(100)).is_none());
        assert!(
            paper.account.has_toast(),
            "the refusal teaches, never silent"
        );
    }

    /// The headline gesture: a working order, hovered, offers labelled
    /// SL/TP handles; pressing one and dragging sets that leg; the tape
    /// then fills the order and the position opens already protected.
    ///
    /// This is the whole promise in one test — the handle the trader
    /// presses, the venue call it produces, and the fill that arms it.
    #[test]
    fn dragging_a_working_orders_handle_arms_the_position_it_opens() {
        let mut paper = PaperTrading::new();
        paper.account.venue.seed(&print(0, 100));
        // A buy limit at 95, below the market: the kind the tape can fill.
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
        let id = paper.working_orders()[0].id;

        // 80..120 over 400 px, price falling with y: 95 is y 250, 90 is y 300.
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let order_center = clamp_tag_center(250.0, chart.top(), chart.bottom());
        // The SL handle sits on the losing side of a buy — below it.
        let handle = bracket_handle_rect(chart.right(), order_center, false);
        assert_eq!(
            paper.control_at(handle.center(), chart, &scale),
            Some(PaperControl::Handle {
                owner: BracketTarget::Order(id),
                leg: Leg::StopLoss,
            }),
            "a working order offers the same handles the position does"
        );

        // Press the handle, drag down to 90, release.
        paper.handle_chart_input(&frame_at(chart, &scale, handle.center(), true, true, false));
        assert_eq!(
            paper.drag,
            PaperDrag::CreateLeg {
                owner: BracketTarget::Order(id),
                leg: Leg::StopLoss,
            },
            "the press started a create-drag on the order, not the position"
        );
        paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false));
        paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true));

        assert_eq!(
            paper.working_orders()[0].bracket.stop_loss(),
            Some(Decimal::from(90)),
            "the drag set the order's own stop, before it ever filled"
        );
        assert!(
            paper.account.venue.position().is_none(),
            "and opened no position doing it"
        );

        // The tape reaches the limit: the position arrives protected.
        paper.on_trade(&print(1, 95));
        let position = paper.account.venue.position().expect("the limit filled");
        assert_eq!(
            position.stop_loss,
            Some(Decimal::from(90)),
            "the leg armed itself on the fill - no window without a stop"
        );
    }

    /// A pane that is not feeding paper input paints the order and its
    /// lines — an order is a fact about the account, true on whichever
    /// chart you are looking at — but **not** its bracket handles.
    ///
    /// The tag opens on every pane at once by design (one hover, two
    /// surfaces), so `reveal` is true over there too. Without the pointer
    /// gate the other pane drew a pressable-looking `SL`/`TP` beside an
    /// order whose presses it does not take.
    #[test]
    fn a_pane_without_the_pointer_paints_no_bracket_handles() {
        let mut paper = PaperTrading::new();
        paper.account.venue.seed(&print(0, 100));
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));

        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let order_center = clamp_tag_center(250.0, chart.top(), chart.bottom());
        let handle = bracket_handle_rect(chart.right(), order_center, false);

        // The pane with the pointer offers the handle...
        assert!(
            paper
                .control_at(handle.center(), chart, &scale)
                .is_some_and(|control| matches!(control, PaperControl::Handle { .. })),
            "the input pane can be pressed there"
        );

        // ...and the paint asks one predicate whether to draw them, so the
        // rule is readable in one place rather than inferred from a paint
        // this test would have to reproduce to check.
        assert!(
            !handles_visible(None, true, false),
            "revealed, but no hand on this pane: nothing is drawn"
        );
        assert!(
            handles_visible(Some(handle.center()), true, false),
            "with the hand here, a revealed owner shows its handles"
        );
        assert!(
            handles_visible(Some(handle.center()), false, true),
            "and reaching straight for a handle keeps it up"
        );
    }

    /// A leg that exists is its own handle: its line is grabbable, and its
    /// tag cross clears it without touching the other leg.
    #[test]
    fn a_working_orders_legs_are_draggable_and_clearable() {
        let mut paper = PaperTrading::new();
        paper.account.venue.seed(&print(0, 100));
        paper.stop_offset_text = "5".to_owned();
        paper.profit_offset_text = "15".to_owned();
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
        let id = paper.working_orders()[0].id;
        assert_eq!(
            paper.working_orders()[0].bracket,
            Bracket::whole(Some(Decimal::from(90)), Some(Decimal::from(110)),),
            "the ticket offsets rode along on the resting order"
        );

        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // Price 90 is y 300: the order's stop line.
        assert_eq!(
            paper.line_at(egui::pos2(400.0, 300.0), &scale),
            Some(PaperDrag::Leg {
                owner: BracketTarget::Order(id),
                leg: Leg::StopLoss,
            }),
            "the order's stop line is grabbable like the position's"
        );

        // Its tag cross clears that leg and leaves the target alone.
        let stop_center = clamp_tag_center(300.0, chart.top(), chart.bottom());
        let cross = close_button_rect(chart.right(), stop_center);
        assert_eq!(
            paper.control_at(cross.center(), chart, &scale),
            Some(PaperControl::ClearLeg {
                owner: BracketTarget::Order(id),
                leg: Leg::StopLoss,
            })
        );
        paper
            .account
            .amend_leg(BracketTarget::Order(id), Leg::StopLoss, None);
        assert_eq!(
            paper.working_orders()[0].bracket,
            Bracket::whole(None, Some(Decimal::from(110)),),
            "clearing one leg never drops the other"
        );
    }

    /// A leg the venue refuses snaps back and says why — the order's own
    /// price is the reference, so a buy limit at 95 cannot take a stop at
    /// 96 even though 96 is below the market at 100.
    #[test]
    fn a_working_orders_leg_is_judged_against_the_order_not_the_market() {
        let mut paper = PaperTrading::new();
        paper.account.venue.seed(&print(0, 100));
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
        let id = paper.working_orders()[0].id;

        paper.account.amend_leg(
            BracketTarget::Order(id),
            Leg::StopLoss,
            Some(Decimal::from(96)),
        );
        assert_eq!(
            paper.working_orders()[0].bracket.stop_loss(),
            None,
            "the refusal left the order as it was"
        );
        let toast = paper.account.peek_toast().expect("the refusal teaches");
        assert!(
            toast.contains("stop loss"),
            "and says which leg was wrong: {}",
            toast
        );
    }

    /// The stated kind wins where both are conceivable to a trader but only
    /// one can rest — which is every price except the mark.
    ///
    /// The pairing to read here is the second and third assertion: at 95,
    /// with the market at 100, `Auto` yields a limit. Ask for a stop at that
    /// same price and the aim stands down instead of handing you the limit.
    /// That is the whole feature: the click that lands is the order you came
    /// to place, or no click at all.
    #[test]
    fn a_stated_entry_kind_is_honoured_or_the_aim_stands_down() {
        let mark = Decimal::from(100);
        let below = Decimal::from(95);
        let above = Decimal::from(105);

        // Auto reads the market, exactly as it always has.
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Auto, Side::Buy, below, mark),
            Some(EntryKind::Limit),
            "a buy below the market waits at a limit"
        );
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Auto, Side::Buy, above, mark),
            Some(EntryKind::Stop),
            "and above it stops in"
        );

        // A stated kind takes the price where it is valid...
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Limit, Side::Buy, below, mark),
            Some(EntryKind::Limit)
        );
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Stop, Side::Buy, above, mark),
            Some(EntryKind::Stop)
        );

        // ...and stands the aim down where it is not, rather than silently
        // placing the other kind. A trader who came to buy a pullback must
        // never be handed a breakout stop.
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Stop, Side::Buy, below, mark),
            None,
            "a buy stop cannot arm below the market, so nothing is offered"
        );
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Limit, Side::Buy, above, mark),
            None,
            "and a buy limit above it would fill at once"
        );

        // A sell mirrors, on every choice.
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Limit, Side::Sell, above, mark),
            Some(EntryKind::Limit)
        );
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Stop, Side::Sell, below, mark),
            Some(EntryKind::Stop)
        );
        assert_eq!(
            resolve_cmd_kind(CmdEntryKind::Limit, Side::Sell, below, mark),
            None
        );

        // On the mark nothing rests, whatever was asked for: a resting order
        // there fills on the next print, which is a market order wearing the
        // wrong name.
        for choice in CmdEntryKind::ALL {
            assert_eq!(
                resolve_cmd_kind(choice, Side::Buy, mark, mark),
                None,
                "{choice:?} rests nothing on the mark"
            );
        }
    }

    /// The choice survives a restart, and an unknown token in a
    /// hand-edited sidecar falls back rather than refusing to open.
    #[test]
    fn the_entry_kind_choice_is_remembered_and_unknown_tokens_fall_back() {
        let state = crate::paper_state::PaperState {
            cmd_entry_kind: Some("stop".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            CmdTradingSettings::from_state(&state).kind,
            CmdEntryKind::Stop
        );

        let state = crate::paper_state::PaperState {
            cmd_entry_kind: Some("teleport".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            CmdTradingSettings::from_state(&state).kind,
            CmdEntryKind::Auto,
            "a token this build does not know is the default, not a crash"
        );
    }

    /// The aim itself obeys the choice: same pointer, same market, one
    /// preview and one silence.
    #[test]
    fn the_aim_obeys_the_stated_kind() {
        let mut paper = PaperTrading::new();
        paper.account.venue.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        // y 250 is price 95 — below the market, where a buy limit rests.
        let aim = egui::pos2(400.0, 250.0);

        paper.account.cmd_trading.kind = CmdEntryKind::Auto;
        paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, false));
        assert_eq!(
            paper.cmd_preview.map(|preview| preview.kind),
            Some(EntryKind::Limit),
            "auto offers the kind that can rest there"
        );

        paper.account.cmd_trading.kind = CmdEntryKind::Stop;
        paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, false));
        assert!(
            paper.cmd_preview.is_none(),
            "a trader who asked for a stop is shown no limit"
        );

        // And the press places nothing where the aim shows nothing.
        assert!(!paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, true)));
        assert!(paper.working_orders().is_empty());
    }

    /// A 800×400 chart over the given price range, plus the input for one
    /// pointer frame at `(x, y)`.
    /// The strategy from the trader's own editor, halved.
    fn halves() -> crate::order_strategies::OrderStrategy {
        use crate::order_strategies::{OrderStrategy, StrategyRow};
        OrderStrategy {
            name: "halves".to_owned(),
            rows: vec![
                StrategyRow {
                    share_percent: Decimal::from(50),
                    gain_ticks: Some(8),
                    loss_ticks: Some(4),
                },
                StrategyRow {
                    share_percent: Decimal::from(50),
                    gain_ticks: Some(2),
                    loss_ticks: Some(5),
                },
            ],
        }
    }

    /// The projection and the placement go through one function, so what the
    /// aim showed is exactly what rested. Proven by comparing the two
    /// brackets rather than by reading the code that builds them.
    #[test]
    fn the_strategys_ladder_is_both_projected_and_placed() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        paper.qty_text = "2".to_owned();
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // y = 250 is 95: below the mark, so a buy rests as a limit there.
        let aim = egui::pos2(400.0, 250.0);

        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        let projected = paper.cmd_preview.expect("the aim is up").bracket;
        let parts: Vec<_> = projected.parts().copied().collect();
        assert_eq!(parts.len(), 2, "both rungs are projected: {parts:?}");
        assert_eq!(parts[0].quantity, Some(Decimal::ONE));
        assert_eq!(parts[0].take_profit, Some(Decimal::from(103)));
        assert_eq!(parts[0].stop_loss, Some(Decimal::from(91)));
        assert_eq!(parts[1].take_profit, Some(Decimal::from(97)));
        assert_eq!(parts[1].stop_loss, Some(Decimal::from(90)));

        let mut press = ruler_frame(chart, &scale, aim, 0.0);
        press.primary_pressed = true;
        press.primary_down = true;
        assert!(paper.handle_chart_input(&press), "the aim placed");

        assert_eq!(
            paper.working_orders()[0].bracket,
            projected,
            "the order carries the very bracket the aim projected"
        );
    }

    /// The ruler is a compass, and a trader wants it most when they already
    /// have a ladder in mind: rolling the wheel works with a strategy armed,
    /// and rolling back to zero hands the ladder its projection again.
    ///
    /// This module used to stand the ruler down under a strategy. That was a
    /// rule it invented and the trader never asked for, and it made the
    /// wheel look broken in the configuration they actually use.
    #[test]
    fn the_ruler_works_with_a_strategy_armed_and_yields_when_it_is_put_away() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        paper.qty_text = "2".to_owned();
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);

        // With the ruler at zero the armed ladder is what the aim projects.
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        assert_eq!(
            paper.cmd_preview.expect("aim up").bracket.parts().count(),
            2,
            "the ladder projects while the ruler is put away"
        );

        // Three notches: the wheel is the ruler's, strategy or no strategy.
        for _ in 0..3 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }
        assert!(paper.consumed_scroll(), "the wheel belonged to the ruler");
        assert_eq!(paper.ruler_notches, 3);
        let preview = paper.cmd_preview.expect("aim up");
        assert_eq!(
            preview.bracket.stop_loss(),
            Some(Decimal::from(92)),
            "and the ruler's symmetric pair is what it now shows"
        );
        assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(98)));

        // Roll it back to zero and the ladder returns; neither gesture costs
        // the other.
        for _ in 0..3 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -40.0));
        }
        assert_eq!(paper.ruler_notches, 0);
        assert_eq!(
            paper.cmd_preview.expect("aim up").bracket.parts().count(),
            2,
            "the armed ladder is back"
        );
    }

    /// A name the strategies no longer carry selects nothing rather than
    /// quietly arming a different ladder.
    #[test]
    fn a_selection_naming_a_missing_strategy_selects_nothing() {
        let mut paper = PaperTrading::new();
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("a strategy that was deleted"));
        assert!(paper.account().selected_order_strategy().is_none());
        assert_eq!(
            paper.account().order_strategies().len(),
            1,
            "the list is intact"
        );
    }

    /// The named call and the wheel leave the ruler in the same place, and
    /// neither can put it somewhere the other cannot reach.
    #[test]
    fn setting_the_ruler_by_name_lands_where_the_wheel_would() {
        let mut by_name = PaperTrading::new();
        by_name.seed(&print(0, 100));
        assert_eq!(by_name.set_ruler_ticks(3), 3);

        let mut by_wheel = PaperTrading::new();
        by_wheel.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);
        for _ in 0..3 {
            by_wheel.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }

        assert_eq!(by_name.ruler_notches, by_wheel.ruler_notches);
        // And the bound is the same bound: a caller cannot reach past what
        // the wheel itself clamps to.
        assert_eq!(by_name.set_ruler_ticks(u32::MAX), RULER_MAX_NOTCHES);
    }

    /// The ticket's own buttons honour the ticket's own strategy.
    ///
    /// The Strategy row sits directly above BUY/SELL and prints what is
    /// armed; a button under it that placed a bare order would be two
    /// surfaces disagreeing about the very next order.
    #[test]
    fn the_market_buttons_honour_the_selected_strategy() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        paper.qty_text = "2".to_owned();

        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));

        let legs: Vec<_> = paper
            .working_orders()
            .iter()
            .filter(|order| order.is_protective())
            .collect();
        assert_eq!(
            legs.len(),
            4,
            "both rungs armed from the button, not a bare order: {legs:?}"
        );
    }

    /// The tick is the finest move the tape has shown, not the last print's
    /// own decoration.
    ///
    /// A venue that quotes `78112.57000000` has a raw scale of eight and a
    /// real step of two; the next print at `78100` normalizes to zero. Both
    /// readings would make the ruler step by a different amount under the
    /// trader's hand.
    #[test]
    fn the_tick_is_the_finest_step_the_tape_has_shown() {
        let mut paper = PaperTrading::new();
        // Trailing zeros are decoration, not precision.
        paper.seed(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::new(7_811_257_000_000, 8),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(
            paper.account.tick(),
            Decimal::new(1, 2),
            "two places, not eight"
        );

        // A rounder print does not coarsen an instrument already seen finer.
        paper.on_trade(&print(1, 78_100));
        assert_eq!(
            paper.account.tick(),
            Decimal::new(1, 2),
            "the tick never grows back under a round print"
        );

        // A genuinely finer print does refine it.
        paper.on_trade(&Trade {
            agg_id: 2,
            timestamp_ms: 2_000,
            price: Decimal::new(78_100_123, 3),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(paper.account.tick(), Decimal::new(1, 3));
    }

    /// A laddered position shows its rungs and offers no create-handle.
    ///
    /// Its own `stop_loss`/`take_profit` are `None` under a ladder, and a
    /// surface reading only those drew the position as unprotected *and*
    /// offered the handles of an unprotected one - where a single drag
    /// replaces every rung with one level.
    #[test]
    fn a_laddered_position_reads_as_protected_and_offers_no_handle() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        paper.qty_text = "2".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));

        let position = paper.account.venue.position().expect("long").clone();
        let bracket = paper.account.position_bracket(&position);
        assert!(
            bracket.is_laddered(),
            "the legs fold back into the ladder that armed them: {bracket:?}"
        );
        assert_eq!(bracket.parts().count(), 2, "one rung per OCO pair");
        assert_eq!(
            bracket.stop_loss(),
            None,
            "and it still refuses to name one stop for two"
        );

        // The handles are what a drag would destroy the ladder through.
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let entry_y = 200.0;
        for above in [true, false] {
            let handle = bracket_handle_rect(chart.right(), entry_y, above).center();
            assert_eq!(
                paper.control_at(handle, chart, &scale),
                None,
                "a laddered position offers no create-handle (above: {above})"
            );
        }
    }

    /// A rung of a resting order can be hauled, and hauling it edits *that
    /// order* - never the named ladder that shaped it.
    ///
    /// The strategy is a template. Once an order is on the chart it is the
    /// trader's, and a stop they cannot move is not a stop.
    #[test]
    fn a_rung_of_a_resting_order_moves_and_leaves_the_strategy_alone() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        paper.qty_text = "2".to_owned();
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // y = 250 is 95: below the mark, so a buy rests as a limit there.
        let aim = egui::pos2(400.0, 250.0);
        let mut press = ruler_frame(chart, &scale, aim, 0.0);
        press.primary_pressed = true;
        press.primary_down = true;
        assert!(paper.handle_chart_input(&press), "the aim placed");

        let id = paper.working_orders()[0].id;
        let before: Vec<_> = paper.working_orders()[0].bracket.parts().copied().collect();
        assert_eq!(before.len(), 2, "a two-rung ladder rests: {before:?}");
        // The first rung's stop is 91 (95 - 4 ticks), at y = 290.
        let stop = before[0].stop_loss.expect("the first rung is stopped");
        assert_eq!(stop, Decimal::from(91));

        let stop_y = scale.y(91.0);
        assert_eq!(
            paper.line_at(egui::pos2(400.0, stop_y), &scale),
            Some(PaperDrag::Rung {
                order: id,
                index: 0,
                leg: Leg::StopLoss
            }),
            "the rung is a line the trader can grab"
        );
        assert!(paper.handle_chart_input(&frame(chart, &scale, stop_y, true, true, false)));
        // Pull it down to 88 (y = 320) and let go.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 320.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 320.0, false, false, true)));

        let after: Vec<_> = paper.working_orders()[0].bracket.parts().copied().collect();
        assert_eq!(
            after[0].stop_loss,
            Some(Decimal::from(88)),
            "the rung moved where it was dropped"
        );
        assert_eq!(
            after[0].take_profit, before[0].take_profit,
            "its own target is untouched"
        );
        assert_eq!(after[1], before[1], "and the other rung never moved");

        // The template is exactly as the trader saved it.
        let strategy = paper
            .account()
            .selected_order_strategy()
            .expect("still armed");
        assert_eq!(strategy.rows[0].loss_ticks, Some(4));
        assert_eq!(strategy.rows[0].gain_ticks, Some(8));
    }

    /// The state the editor's own `+ Row` used to create: a third rung at
    /// 0%, which fails `ShareNotPositive` and silently stopped the chart
    /// projecting a ladder the ticket still said was armed.
    ///
    /// The trader hit `+ Row`, held the modifier and saw nothing, with no
    /// word anywhere saying why. A row that arrives broken is the bug.
    #[test]
    fn adding_a_row_never_breaks_the_strategy_that_was_working() {
        use crate::order_strategies::StrategyRow;

        let mut strategy = halves();
        assert!(strategy.validate().is_ok(), "50/50 is usable");

        // What `+ Row` does, as the button does it.
        let assigned: Decimal = strategy.rows.iter().map(|row| row.share_percent).sum();
        assert_eq!(assigned, Decimal::ONE_HUNDRED, "nothing is left over");
        let last = strategy.rows.last_mut().expect("has rows");
        let half = (last.share_percent / Decimal::TWO).round_dp(2);
        last.share_percent -= half;
        strategy.rows.push(StrategyRow {
            share_percent: half,
            gain_ticks: Some(NEW_RUNG_TICKS),
            loss_ticks: Some(NEW_RUNG_TICKS),
        });

        assert_eq!(strategy.rows.len(), 3);
        assert!(
            strategy.validate().is_ok(),
            "the third rung splits the last one instead of arriving at zero: {:?}",
            strategy.rows
        );
        let shares: Decimal = strategy.rows.iter().map(|row| row.share_percent).sum();
        assert_eq!(shares, Decimal::ONE_HUNDRED, "and they still add up");
    }

    /// An armed ladder that cannot resolve must say so where it is armed.
    #[test]
    fn an_unusable_strategy_is_named_in_the_ticket_not_left_silent() {
        use crate::order_strategies::StrategyRow;

        let mut broken = halves();
        broken.rows.push(StrategyRow {
            share_percent: Decimal::ZERO,
            gain_ticks: Some(20),
            loss_ticks: Some(20),
        });
        let error = broken.validate().expect_err("a zero share is not a share");
        assert!(
            !error.advice().is_empty(),
            "and the reason is a sentence the ticket can print"
        );

        // The chart is silent by design in this state - that is exactly why
        // the ticket has to speak.
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![broken], Some("halves"));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 0.0));
        let preview = paper.cmd_preview.expect("the aim is still up");
        assert!(
            preview.bracket.is_empty(),
            "an unusable ladder projects nothing - which is why it must be named"
        );
    }

    /// Reproduction: the ticket at its default quantity of one, with a
    /// two-rung 50/50 ladder armed. This is what the trader actually had.
    #[test]
    fn repro_the_aim_projects_a_ladder_at_quantity_one() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        // qty_text is left at its default.
        assert_eq!(paper.qty_text, "1");
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));

        let preview = paper.cmd_preview.expect("the aim is up");
        let parts: Vec<_> = preview.bracket.parts().copied().collect();
        println!("quantity one -> parts: {parts:?}");
        assert!(
            !preview.bracket.is_empty(),
            "the aim must project the armed ladder, not nothing"
        );
    }

    /// A wheel that reports 40 px a notch must move the ruler, and so must
    /// one that reports 50, or 120, or 13.
    ///
    /// This build assumed 50 and met a mouse that reports 40: every roll
    /// computed zero ticks and the ruler silently refused to move, which is
    /// indistinguishable from the feature not existing. The notch is the
    /// device's to declare, not ours to assume.
    #[test]
    fn the_first_roll_moves_the_ruler_whatever_the_wheel_reports() {
        for notch in [13.0_f32, 40.0, 50.0, 120.0] {
            let mut paper = PaperTrading::new();
            paper.seed(&print(0, 100));
            let (chart, scale) = chart_and_scale(80.0, 120.0);
            let aim = egui::pos2(400.0, 250.0);

            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, notch));
            assert_eq!(
                paper.ruler_notches, 1,
                "one notch of {notch} px is one tick"
            );

            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, notch * 3.0));
            assert_eq!(paper.ruler_notches, 4, "three more notches at {notch} px");

            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -notch * 2.0));
            assert_eq!(paper.ruler_notches, 2, "and it walks back");
        }
    }

    /// Esc puts the ruler away - but only once the gestures it was already
    /// for have nothing to cancel. A standing distance must not shadow
    /// disarming an order.
    #[test]
    fn escape_clears_the_ruler_but_only_after_an_armed_placement() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 40.0));
        assert_eq!(paper.ruler_notches, 1, "the ruler stands");
        paper.account.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });

        assert!(paper.cancel_interaction(), "the first press has work to do");
        assert!(
            paper.account.armed.is_none(),
            "and it disarmed the placement"
        );
        assert_eq!(
            paper.ruler_notches, 1,
            "the distance survives - it was not what the trader was cancelling"
        );

        assert!(
            paper.cancel_interaction(),
            "the second press reaches the ruler"
        );
        assert_eq!(paper.ruler_notches, 0);
        assert!(
            !paper.cancel_interaction(),
            "and then there is nothing left"
        );
    }

    /// The old, narrower guarantee, kept: with nothing else in flight the
    /// first press is the ruler's.
    #[test]
    fn escape_clears_the_ruler() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 200.0));
        assert!(paper.ruler_notches > 0, "the ruler is standing");
        assert!(paper.cancel_interaction(), "escape had something to cancel");
        assert_eq!(paper.ruler_notches, 0, "and it put the ruler away");
    }

    /// The gesture the ruler is made of is the one Windows reports sideways.
    ///
    /// Holding a modifier turns a vertical wheel into horizontal scroll, so
    /// `raw_scroll_delta` arrives as `x = 40, y = 0` for exactly the roll the
    /// ruler exists to serve. Reading only `y` meant the ruler saw nothing
    /// whenever the trader was actually holding the key - the one case that
    /// matters. The pane hands over whichever axis carried it; this proves
    /// the ruler steps on what it is handed.
    #[test]
    fn the_ruler_steps_on_the_travel_the_pane_hands_it() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);
        // 40 px is what this machine's wheel reports, on whichever axis.
        for _ in 0..3 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }
        assert_eq!(paper.ruler_notches, 3);
        let preview = paper.cmd_preview.expect("the aim is up");
        assert_eq!(preview.bracket.stop_loss(), Some(Decimal::from(92)));
        assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(98)));
    }

    /// Rolling back to zero takes the stop and the target off the chart.
    ///
    /// Zero is not "a very small bracket": it is the bare order the trader
    /// started with, and the aim has to look like one. Anything still drawn
    /// there is a level they did not ask for and would carry into the click.
    #[test]
    fn rolling_back_to_zero_leaves_no_stop_and_no_target() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);

        for _ in 0..4 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }
        assert_eq!(paper.ruler_notches, 4);
        assert!(
            !paper.cmd_preview.expect("aim up").bracket.is_empty(),
            "the ruler is drawing a pair"
        );

        // All the way back down, and one notch past it for good measure.
        for _ in 0..5 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -40.0));
        }
        assert_eq!(paper.ruler_notches, 0, "it reaches zero and stops there");
        assert!(
            paper.cmd_preview.expect("aim up").bracket.is_empty(),
            "and nothing is left on the chart to place"
        );

        // Esc is the other way home, and lands in the same place.
        for _ in 0..3 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }
        assert!(paper.cancel_interaction());
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        assert!(
            paper.cmd_preview.expect("aim up").bracket.is_empty(),
            "escape leaves the aim as bare as rolling back does"
        );
    }

    /// Selecting a strategy must change what the aim draws, and choosing
    /// `<None>` must take it away again. The trader reported "selecting a
    /// strategy changes nothing"; this is the contract that claim is about.
    #[test]
    fn the_strategy_combo_changes_what_the_aim_draws() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.qty_text = "2".to_owned();
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);

        // `<None>`: the modifier alone draws nothing.
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], None);
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        assert!(
            paper.cmd_preview.expect("aim up").bracket.is_empty(),
            "with no strategy the aim is bare until the wheel is rolled"
        );

        // Selected: the ladder is there on the modifier alone.
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], Some("halves"));
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        assert_eq!(
            paper.cmd_preview.expect("aim up").bracket.parts().count(),
            2,
            "selecting a strategy puts its rungs on the aim"
        );

        // Back to `<None>`: gone again.
        paper
            .account_mut()
            .set_order_strategies(vec![halves()], None);
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        assert!(
            paper.cmd_preview.expect("aim up").bracket.is_empty(),
            "and choosing none takes it away"
        );
    }

    /// A notch is worth what the instrument's step says, and the default is
    /// derived so the very first roll feels right without configuration.
    #[test]
    fn the_default_step_scales_with_the_instrument() {
        // A one-cent instrument near 78,000: half a basis point is 3.9,
        // which the 1-2-5 ladder rounds up to 5 points.
        let mut btc = PaperTrading::new();
        btc.seed(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::new(7_800_057, 2),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(btc.account.tick(), Decimal::new(1, 2));
        assert_eq!(
            btc.ruler_step(),
            Decimal::from(5),
            "twenty to forty points is four to eight rolls away"
        );

        // The mini index near 138,000 prints in whole five-point steps.
        let mut win = PaperTrading::new();
        win.seed(&print(0, 138_000));
        win.on_trade(&print(1, 138_005));
        assert_eq!(win.ruler_step(), Decimal::from(10), "two ticks a notch");
    }

    /// A typed step is the trader's, saved per instrument, and switching
    /// away and back keeps it.
    #[test]
    fn a_typed_step_is_kept_per_symbol() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 78_000));
        paper.set_symbol("BTCUSDT");
        paper.set_ruler_step(Some(Decimal::from(25)));
        assert_eq!(paper.ruler_step(), Decimal::from(25));

        paper.set_symbol("WIN$N");
        assert_ne!(
            paper.ruler_step(),
            Decimal::from(25),
            "another instrument does not inherit it"
        );

        paper.set_symbol("BTCUSDT");
        assert_eq!(paper.ruler_step(), Decimal::from(25), "and it comes back");

        // Clearing puts the instrument back on its derived default.
        paper.set_ruler_step(None);
        assert_eq!(paper.ruler_step(), paper.account.derived_ruler_step());
    }

    /// Switching instrument drops the standing ruler and the tick it was
    /// measured in. A distance chosen on one market means nothing on the
    /// next, and would otherwise arm the first order placed there.
    #[test]
    fn a_symbol_switch_forgets_the_ruler_and_the_tick() {
        let mut paper = PaperTrading::new();
        // The symbol first: switching to it is what clears the tick, and a
        // tape seeded before the switch would be cleared with it.
        paper.set_symbol("BTCUSDT");
        paper.seed(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::new(7_800_057, 2),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        let (chart, scale) = chart_and_scale(77_000.0, 79_000.0);
        paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 40.0));
        assert!(paper.ruler_notches > 0);
        assert_eq!(paper.account.tick(), Decimal::new(1, 2));

        paper.set_symbol("WIN$N");
        assert_eq!(paper.ruler_notches, 0, "the distance does not travel");
        // The tick falls back to the coarsest until the new tape prints:
        // erring coarse means a wider step, never a phantom precision the
        // new market has not shown.
        assert_eq!(
            paper.account.tick(),
            Decimal::ONE,
            "the old market's precision does not travel either"
        );
        // And the first print of the new one refines it honestly.
        paper.on_trade(&Trade {
            agg_id: 9,
            timestamp_ms: 9_000,
            price: Decimal::new(1_380_055, 1),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(paper.account.tick(), Decimal::new(1, 1));
    }

    /// The opening instrument is arrival, not departure. The app names the
    /// symbol a frame after construction, so a ruler standing before that
    /// first call — the launch hook's, or a session restored into a fresh
    /// simulator — must survive it. Only leaving an instrument forgets.
    #[test]
    fn the_first_symbol_of_a_session_keeps_a_standing_ruler() {
        let mut paper = PaperTrading::new();
        paper.ruler_notches = 6;
        paper.set_symbol("BTCUSDT");
        assert_eq!(
            paper.ruler_notches, 6,
            "arriving at the opening instrument is not a switch"
        );
        paper.set_symbol("WIN$N");
        assert_eq!(paper.ruler_notches, 0, "leaving one still forgets");
    }

    /// Pressing the wheel puts the ruler away, and only while an aim is up.
    #[test]
    fn pressing_the_wheel_puts_the_ruler_away() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);
        for _ in 0..3 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }
        assert_eq!(paper.ruler_notches, 3);

        let mut press = ruler_frame(chart, &scale, aim, 0.0);
        press.middle_pressed = true;
        paper.handle_chart_input(&press);
        assert_eq!(
            paper.ruler_notches, 0,
            "the wheel that walked it out puts it away"
        );
        assert!(
            paper.cmd_preview.expect("aim up").bracket.is_empty(),
            "and the aim is bare again"
        );

        // With no aim up the press is nobody's business here.
        for _ in 0..2 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
        }
        let mut bare = ruler_frame(chart, &scale, aim, 0.0);
        bare.modifiers = egui::Modifiers::default();
        bare.middle_pressed = true;
        paper.handle_chart_input(&bare);
        assert_eq!(paper.ruler_notches, 2, "no aim, no claim on the press");
    }

    /// A frame with the aim's modifier held and wheel travel to spend.
    fn ruler_frame<'a>(
        chart: egui::Rect,
        scale: &'a PriceScale,
        pointer: egui::Pos2,
        scroll_y: f32,
    ) -> ChartInput<'a> {
        ChartInput {
            chart,
            scale: Some(scale),
            pointer: Some(pointer),
            primary_pressed: false,
            primary_down: false,
            primary_released: false,
            modifiers: egui::Modifiers {
                shift: true,
                ..Default::default()
            },
            canvas_claimed: false,
            scroll_y,
            middle_pressed: false,
            layer_visible: true,
        }
    }

    /// The ruler walks both legs out together, one tick per notch, and says
    /// how far in the units the trader reads.
    #[test]
    fn the_wheel_walks_the_projected_bracket_out_symmetrically() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // y = 250 is 95: below the mark, so a buy rests as a limit there.
        let aim = egui::pos2(400.0, 250.0);

        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
        let preview = paper.cmd_preview.expect("the aim is up");
        assert_eq!(preview.kind, EntryKind::Limit);
        assert_eq!(preview.ruler_ticks, 0, "the ruler starts off");
        assert_eq!(preview.bracket.stop_loss(), None, "and projects nothing");

        // Three notches up, one roll each - which is what a wheel does.
        for _ in 0..3 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 50.0));
        }
        assert!(paper.consumed_scroll(), "the wheel belonged to the ruler");
        assert_eq!(paper.ruler_notches, 3);
        let preview = paper.cmd_preview.expect("the aim is still up");
        assert_eq!(
            preview.bracket.stop_loss(),
            Some(Decimal::from(92)),
            "three ticks below the aim"
        );
        assert_eq!(
            preview.bracket.take_profit(),
            Some(Decimal::from(98)),
            "and three ticks above it - the same distance, which is the 1:1"
        );

        // One notch back down.
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -50.0));
        assert_eq!(paper.ruler_notches, 2);
        let preview = paper.cmd_preview.expect("still aiming");
        assert_eq!(preview.bracket.stop_loss(), Some(Decimal::from(93)));
        assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(97)));
    }

    /// One unreadable offset box stands the whole bracket down, rather than
    /// projecting the other one on its own.
    ///
    /// The two boxes are a pair. A ticket whose stop says `abc` and whose
    /// target says `5` must project nothing: showing a target-only bracket
    /// there would put protection on the chart that the trader never typed,
    /// and it is the trade they would take it for. `ticket_bracket` read both
    /// with `?` and one bad box failed the call; the seam carries that as
    /// `TicketForm::offsets` being all-or-nothing, and this is what says so.
    #[test]
    fn one_unreadable_offset_stands_the_whole_bracket_down() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let reference = Decimal::from(100);

        paper.stop_offset_text = "2".to_owned();
        paper.profit_offset_text = "5".to_owned();
        let both = paper.armed_bracket(Side::Buy, reference, Decimal::ONE);
        assert_eq!(both.stop_loss(), Some(Decimal::from(98)));
        assert_eq!(both.take_profit(), Some(Decimal::from(105)));

        paper.stop_offset_text = "abc".to_owned();
        let spoiled = paper.armed_bracket(Side::Buy, reference, Decimal::ONE);
        assert_eq!(
            spoiled.stop_loss(),
            None,
            "an unreadable stop projects no stop"
        );
        assert_eq!(
            spoiled.take_profit(),
            None,
            "and takes the readable target down with it"
        );
    }

    /// A short's ruler mirrors: the stop goes above the aim, the target
    /// below, and both stay the same distance from it.
    #[test]
    fn the_ruler_mirrors_for_a_sell() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // y = 250 is 95: below the mark, so a sell arms as a stop there.
        let aim = egui::pos2(400.0, 250.0);
        let mut frame = ruler_frame(chart, &scale, aim, 50.0);
        frame.modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        // Four notches, one roll each.
        for _ in 0..4 {
            paper.handle_chart_input(&frame);
        }

        let preview = paper.cmd_preview.expect("the sell aim is up");
        assert_eq!(preview.side, Side::Sell);
        assert_eq!(
            preview.bracket.stop_loss(),
            Some(Decimal::from(99)),
            "above a short"
        );
        assert_eq!(
            preview.bracket.take_profit(),
            Some(Decimal::from(91)),
            "below it"
        );
    }

    /// The wheel with no aim up is the chart's, not the ruler's.
    #[test]
    fn without_an_aim_the_wheel_is_left_to_the_chart() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let mut frame = ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 150.0);
        frame.modifiers = egui::Modifiers::default();
        paper.handle_chart_input(&frame);
        assert!(paper.cmd_preview.is_none(), "no modifier, no aim");
        assert!(!paper.consumed_scroll(), "so the wheel is not the ruler's");
        assert_eq!(paper.ruler_notches, 0);
    }

    /// What the ruler shows is what the click places.
    #[test]
    fn the_order_the_click_places_carries_the_rulers_bracket() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);
        for _ in 0..5 {
            paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 50.0));
        }
        assert_eq!(paper.ruler_notches, 5);

        let mut press = ruler_frame(chart, &scale, aim, 0.0);
        press.primary_pressed = true;
        press.primary_down = true;
        assert!(paper.handle_chart_input(&press), "the aim placed");

        let order = &paper.working_orders()[0];
        assert_eq!(order.price, Some(Decimal::from(95)));
        assert_eq!(
            order.bracket.stop_loss(),
            Some(Decimal::from(90)),
            "the stop the ruler was showing"
        );
        assert_eq!(
            order.bracket.take_profit(),
            Some(Decimal::from(100)),
            "and the target beside it"
        );
    }

    fn chart_and_scale(lo: f64, hi: f64) -> (egui::Rect, PriceScale) {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        (chart, PriceScale::from_range(lo, hi, 0.0, 400.0))
    }

    /// One pointer frame mid-chart, where nothing right-anchored lives.
    fn frame<'a>(
        chart: egui::Rect,
        scale: &'a PriceScale,
        y: f32,
        pressed: bool,
        down: bool,
        released: bool,
    ) -> ChartInput<'a> {
        frame_at(chart, scale, egui::pos2(400.0, y), pressed, down, released)
    }

    /// One pointer frame at an exact position — for the controls that live
    /// against the plot's right edge, which `frame`'s mid-chart x misses.
    fn frame_at<'a>(
        chart: egui::Rect,
        scale: &'a PriceScale,
        pointer: egui::Pos2,
        pressed: bool,
        down: bool,
        released: bool,
    ) -> ChartInput<'a> {
        ChartInput {
            chart,
            scale: Some(scale),
            pointer: Some(pointer),
            primary_pressed: pressed,
            primary_down: down,
            primary_released: released,
            modifiers: egui::Modifiers::default(),
            canvas_claimed: false,
            scroll_y: 0.0,
            middle_pressed: false,
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
            scroll_y: 0.0,
            middle_pressed: false,
            layer_visible: true,
        }
    }

    /// Escape (routed through the app's escape stack) cancels exactly one
    /// paper interaction per press, and a cancelled drag submits nothing.
    #[test]
    fn escape_cancels_the_armed_placement_then_the_grabbed_line() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.account.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        assert!(paper.cancel_interaction(), "the armed placement dies first");
        assert!(paper.account.armed.is_none());
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
            paper
                .account
                .venue
                .position()
                .expect("still long")
                .stop_loss,
            Some(Decimal::from(90)),
            "a cancelled drag never moves the stop"
        );
    }

    #[test]
    fn an_armed_click_places_the_order_at_the_clicked_price_and_disarms() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.account.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        let (chart, scale) = chart_and_scale(90.0, 110.0);
        // y = 300 sits at price 95 on this scale.
        let consumed = paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false));
        assert!(consumed, "the armed click never reaches the chart pan");
        assert!(
            paper.account.armed.is_none(),
            "a successful placement disarms"
        );
        assert_eq!(paper.account.venue.working_orders().len(), 1);
        assert_eq!(
            paper.account.venue.working_orders()[0].price,
            Some(Decimal::from(95))
        );
    }

    #[test]
    fn a_rejected_armed_click_stays_armed_and_teaches() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.account.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        let (chart, scale) = chart_and_scale(90.0, 110.0);
        // y = 100 sits at price 105 — a buy limit above the market.
        let consumed = paper.handle_chart_input(&frame(chart, &scale, 100.0, true, true, false));
        assert!(consumed);
        assert!(
            paper.account.armed.is_some(),
            "the user clicks again after the toast"
        );
        assert!(paper.account.venue.working_orders().is_empty());
        assert!(paper.account.has_toast(), "the refusal explains itself");
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
            let (start, end, label) = cmd_preview_layout(band, band.right(), egui::pos2(x, 250.0));
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

    /// The tape lane is not a wall. Its divider ends the *band* — where a
    /// press can still land — and the label stops there with it, but the
    /// line carries on to the axis, because a gap across the widest lane on
    /// the chart is exactly where a trader loses the order.
    #[test]
    fn the_aim_line_crosses_the_live_lane_to_the_axis() {
        // A chart 1000 wide whose live tape lane opens at 700: the band the
        // aim lays out against stops at the divider, the gutter does not.
        let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(700.0, 400.0));
        let axis_x = 1000.0;
        let (start, end, label) = cmd_preview_layout(band, axis_x, egui::pos2(400.0, 250.0));
        assert_eq!(
            end.x, axis_x,
            "the line spans the lane instead of stopping at its divider"
        );
        assert!(
            end.x > band.right(),
            "and it is the lane it crosses, not the plot it started in"
        );
        assert_eq!(start.x, 400.0, "it still starts under the cursor");
        assert!(
            label.right() <= band.right(),
            "the label stays inside the band a press can reach: {label:?}"
        );
    }

    /// A pane with no live lane hands the same x twice, and the line must
    /// not double back on itself.
    #[test]
    fn the_aim_line_ends_at_the_axis_with_no_lane_open() {
        let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let (_, end, _) = cmd_preview_layout(band, 800.0, egui::pos2(400.0, 250.0));
        assert_eq!(end.x, 800.0, "band right and axis coincide");
        // A gutter reported left of the plot (a pane mid-resize) must never
        // shorten the line to a stub pointing the wrong way.
        let (start, end, _) = cmd_preview_layout(band, 10.0, egui::pos2(400.0, 250.0));
        assert!(end.x >= start.x, "never a line running backwards");
    }

    /// The two edges: near the left one the label flips to the pointer's
    /// right rather than leaving the band, and near the right one the line
    /// starts further left so there is still a line to read.
    #[test]
    fn the_cmd_layout_clamps_at_both_edges_of_the_band() {
        let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

        let pointer = egui::pos2(20.0, 250.0);
        let (_, _, label) = cmd_preview_layout(band, band.right(), pointer);
        assert!(label.left() >= band.left(), "never off the left edge");
        assert_eq!(
            label.left(),
            pointer.x + CMD_LABEL_CURSOR_GAP_PX,
            "no room on the left, so it flips right"
        );
        assert!(!label.contains(pointer), "still clear of the cursor");

        let pointer = egui::pos2(780.0, 250.0);
        let (start, end, label) = cmd_preview_layout(band, band.right(), pointer);
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
        let (start, _, label) = cmd_preview_layout(sliver, sliver.right(), egui::pos2(50.0, 250.0));
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
        let (_, _, label) = cmd_preview_layout(chart, chart.right(), preview.pointer);
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
            scroll_y: 0.0,
            middle_pressed: false,
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
        for bad in [
            "buy@left", "buy@nan", "buy@NaN", "buy@inf", "buy@", "buy@0,15",
        ] {
            assert_eq!(
                CmdPreviewForce::parse(bad),
                Some(CmdPreviewForce {
                    side: Side::Buy,
                    x_fraction: None
                }),
                "a bad fraction still paints, mid-band: {bad}"
            );
        }
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
            scroll_y: 0.0,
            middle_pressed: false,
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
            PaperDrag::Leg {
                owner: BracketTarget::Position,
                leg: Leg::StopLoss,
            },
            "it grabbed the stop instead of resting an order"
        );
        assert!(paper.working_orders().is_empty());
        paper.cancel_interaction();

        // An armed placement is an intent already stated.
        paper.account.armed = Some(ArmedPlacement {
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
            paper.account.armed.is_none(),
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

    /// The bracket hook dresses those same rungs, so a working order's two
    /// dashed legs are photographable without a hand to drag them into
    /// being. Each leg lands on the correct side of the *order's own*
    /// price, which is what the venue validates against — a hook that
    /// placed one the venue refuses would photograph an empty chart.
    #[test]
    fn the_bracket_hook_dresses_every_rung_it_rests() {
        let mut paper = PaperTrading::new();
        paper.orders_demo = Some(1);
        paper.order_bracket_demo = true;
        paper.on_trade(&print(1, 100_000));

        let orders = paper.working_orders();
        assert_eq!(orders.len(), 2, "one rung, both sides");
        for order in orders {
            let price = order.price.expect("a limit has a price");
            let stop = order.bracket.stop_loss().expect("a stop rides along");
            let target = order.bracket.take_profit().expect("and a target");
            match order.side {
                Side::Buy => {
                    assert!(stop < price, "a long's stop sits below its entry");
                    assert!(target > price, "and its target above");
                }
                Side::Sell => {
                    assert!(stop > price, "a short's stop sits above its entry");
                    assert!(target < price, "and its target below");
                }
            }
        }
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

    /// A band too short to hold a tag is reachable — `split_panes` carves
    /// the indicator strips out of the plot with no floor of its own — and
    /// `f32::clamp` panics rather than saturating once its bounds cross.
    /// Every tag, every ✕ hit-test and the aim's own layout run through
    /// this, so the panic would take a live session down.
    #[test]
    fn a_band_too_short_for_a_tag_centres_it_instead_of_panicking() {
        // Shorter than a tag, and flat: the two cases that cross the bounds.
        assert_eq!(clamp_tag_center(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp_tag_center(99.0, 40.0, 40.0), 40.0);
        assert_eq!(clamp_tag_center(-99.0, 0.0, TAG_HEIGHT_PX), 10.0);
        // And with room, it still clamps exactly as before.
        assert_eq!(clamp_tag_center(0.0, 0.0, 400.0), 10.0);
        assert_eq!(clamp_tag_center(400.0, 0.0, 400.0), 390.0);
        assert_eq!(clamp_tag_center(200.0, 0.0, 400.0), 200.0);

        // The paint and the press both survive it end to end.
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let sliver = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 12.0));
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 12.0);
        assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
        let pointer = egui::pos2(400.0, 6.0);
        paper.handle_chart_input(&cmd_frame(
            sliver,
            &scale,
            pointer,
            egui::Modifiers::default(),
            false,
        ));
        let _ = layer_shapes(&paper, sliver, &scale, Some(pointer));
        let _ = paper.control_at(pointer, sliver, &scale);
        let _ = cmd_preview_layout(sliver, sliver.right(), pointer);
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
            paper.account.venue.position().expect("long").stop_loss,
            Some(Decimal::from(90)),
        );
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // The stop at 90 sits at y = 300; grab it, pull to 95 (y = 250), drop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
        assert_eq!(
            paper
                .account
                .venue
                .position()
                .expect("still long")
                .stop_loss,
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
        let position = paper.account.venue.position().expect("still long");
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
        let position = paper.account.venue.position().expect("still long");
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
        let position = paper.account.venue.position().expect("long");
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
        let events = paper.account.dispatch(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: Decimal::from(95),
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        });
        paper.account.handle_events(events);
        // The trap that used to swallow every chart click.
        paper.account.armed = Some(ArmedPlacement {
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
            scroll_y: 0.0,
            middle_pressed: false,
            layer_visible: true,
        };
        assert!(paper.handle_chart_input(&press), "the ✕ owns the press");
        assert!(
            paper.account.venue.working_orders().is_empty(),
            "the order is gone and the armed click placed nothing"
        );
        assert!(
            paper.account.armed.is_some(),
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
            scroll_y: 0.0,
            middle_pressed: false,
            layer_visible: true,
        };
        assert!(
            paper.handle_chart_input(&press),
            "the handle owns the press"
        );
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
        assert_eq!(
            paper.account.venue.position().expect("long").stop_loss,
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
        let position = paper.account.venue.position().expect("reversed, not flat");
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
    fn snapping_uses_the_instruments_learned_precision() {
        let mut paper = PaperTrading::new();
        paper.seed(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: Decimal::new(10325, 2), // 103.25 → two decimal places
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(paper.account.snap(101.23456), Decimal::new(10123, 2));
        // A cent-printing instrument keeps cents, whatever round numbers
        // come after: precision the tape has shown does not go away because
        // the next print happened to land on a whole one.
        paper.on_trade(&print(2, 103));
        assert_eq!(paper.account.snap(101.23456), Decimal::new(10123, 2));

        // An instrument that only ever prints whole points snaps to them -
        // its own instance, because a tick belongs to one market.
        let mut whole = PaperTrading::new();
        whole.seed(&print(1, 182_035));
        assert_eq!(whole.account.snap(182_036.7), Decimal::from(182_037));
    }

    /// The session file [`the_journal_bytes_are_fixed`] must open, named
    /// from the first close's own timestamp.
    const JOURNAL_GOLDEN_FILE: &str = "19700101-000002.csv";

    /// Every byte [`the_journal_bytes_are_fixed`] must write. Recorded from
    /// a run against this file *before* the policy half moved out, and not
    /// touched since.
    ///
    /// SHA-256 of these bytes:
    /// `ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6`.
    /// The hash is written down so that the "before" and the "after" of the
    /// extraction can be compared by someone who is reading neither this
    /// file's history nor the diff — a reviewer, or the trader.
    const JOURNAL_GOLDEN: &str = concat!(
        "# quantick-trades 2\n",
        "# symbol=GOLDEN\n",
        "# source=live\n",
        "opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,",
        "exit_reason,entry_agg_id,exit_agg_id,mae_points,mfe_points\n",
        // A long taken at the market and closed by hand: 100 to 105.
        "1000,2000,long,1,100,105,5,manual,1,2,0,5\n",
        // A short, the same way: 105 down to 103.
        "3000,4000,short,1,105,103,2,manual,3,4,0,2\n",
        // A long stopped out. The entry is 103 and the ticket's stop offset
        // is 2, so the stop sits at 101 and the tape reaches it.
        "5000,6000,long,1,103,101,-2,stop_loss,5,6,2,0\n",
        // A long taken at its target: entry 101, offset 6, filled at 107.
        "7000,8000,long,1,101,107,6,take_profit,7,8,0,6\n",
    );

    /// One fixed tape, one journal, asserted byte for byte.
    ///
    /// This is the money path's golden. It is written *before* the policy
    /// half of this file moves into `paper_account.rs`, and its expected
    /// bytes do not change when it does — that is the whole point. An
    /// extraction that alters a fill rule, a bracket price, the risk lock's
    /// arithmetic, a rounding or the journal's own format fails here rather
    /// than in front of the trader, and it fails naming the byte.
    ///
    /// The tape is fixed in every respect the writer reads: prices and
    /// quantities are exact decimals, every timestamp is derived from the
    /// print's own `agg_id` rather than a clock, and the session file's name
    /// comes from the first close's `closed_ms`. So the file name is
    /// asserted too — a session that opened a differently named file would
    /// still hold the right rows, and the trader would still have lost the
    /// trade in a folder nobody reads.
    ///
    /// Four round trips, chosen to cover the four ways a position ends:
    /// a long closed by hand, a short closed by hand, a long stopped out,
    /// and a long taken at its target. The last two go through the ticket's
    /// offset text, so the bracket arithmetic is under the golden and not
    /// only the flat manual close.
    #[test]
    fn the_journal_bytes_are_fixed() {
        // Its own scratch folder, carrying a run token and removed with the
        // value: a reused process id would otherwise hand this run the last
        // one's journal, and the golden would fail on a file it never wrote.
        let dir = crate::scratch::ScratchDir::new("paper-journal-golden");
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol("GOLDEN");
        paper.seed(&print(0, 100));

        // 1. A long, entered at the market and closed by hand: +5.
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 105));

        // 2. A short, the same way: 105 down to 103 is +2.
        paper.market(Side::Sell);
        paper.on_trade(&print(3, 105));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(4, 103));

        // 3. A long with protection, stopped out. The offsets are the
        //    ticket's own text, so `ticket_bracket` and the rounding that
        //    follows it are under the golden with everything else.
        paper.stop_offset_text = "2".to_owned();
        paper.profit_offset_text = "6".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(5, 103));
        paper.on_trade(&print(6, 101));

        // 4. A long with the same protection, taken at its target.
        paper.market(Side::Buy);
        paper.on_trade(&print(7, 101));
        paper.on_trade(&print(8, 107));

        let folder = dir.path().join("GOLDEN");
        let mut files: Vec<_> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .map(|entry| entry.path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 1, "one session, one file: {files:?}");
        assert_eq!(
            files[0].file_name().and_then(|name| name.to_str()),
            Some(JOURNAL_GOLDEN_FILE),
            "the session file is named from the first close, not from a clock"
        );

        let text = std::fs::read_to_string(&files[0]).expect("readable");
        assert_eq!(
            text, JOURNAL_GOLDEN,
            "the journal's bytes moved; the money path is not what it was"
        );
    }

    #[test]
    fn closed_trades_journal_to_one_session_file_and_reload() {
        let dir = crate::scratch::ScratchDir::new("paper-journal-test");
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol("TESTUSDT");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 105));
        // A second round trip appends to the same session file.
        paper.market(Side::Sell);
        paper.on_trade(&print(3, 105));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
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
    }

    #[test]
    fn a_second_session_adds_a_file_and_never_touches_the_first() {
        let dir = crate::scratch::ScratchDir::new("paper-accumulate-test");
        // Session one: a round trip closing at t=2s.
        let mut first = PaperTrading::new();
        first.account.dir = dir.path().to_path_buf();
        first.set_symbol("ACCUM");
        first.seed(&print(0, 100));
        first.market(Side::Buy);
        first.on_trade(&print(1, 100));
        let events = first.account.dispatch(Command::ClosePosition);
        first.account.handle_events(events);
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
        second.account.dir = dir.path().to_path_buf();
        second.set_symbol("ACCUM");
        second.seed(&print(10_000, 200));
        second.market(Side::Sell);
        second.on_trade(&print(10_001, 200));
        let events = second.account.dispatch(Command::ClosePosition);
        second.account.handle_events(events);
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
    }

    #[test]
    fn a_timeline_reset_journals_the_flatten_and_clears_the_form_state() {
        let dir = crate::scratch::ScratchDir::new("paper-reset-test");
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol("RESETX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        paper.account.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        paper.on_timeline_reset();
        assert!(paper.account.venue.position().is_none());
        assert!(
            paper.account.armed.is_none(),
            "an armed click dies with the timeline"
        );
        assert!(paper.account.has_toast(), "the flatten is never silent");
        let history = load_history(&dir, Some("RESETX"), &[]);
        assert_eq!(history.rows.len(), 1);
        assert_eq!(
            report_from_history(&history).trades,
            1,
            "the reset exit is a real, journaled trade"
        );
    }

    // The stored-pick-vs-configured-base precedence now lives in
    // `paper_home::chosen`, tested there beside the documents default.

    /// The panel's folder picker retargets everything downstream: the next
    /// close opens a new session file under the new home, and the ledger
    /// and report re-read from it. Files already written stay put.
    #[test]
    fn switching_the_trades_dir_retargets_journal_ledger_and_report() {
        let dir_a = crate::scratch::ScratchDir::new("paper-dir-a");
        let dir_b = crate::scratch::ScratchDir::new("paper-dir-b");
        let mut paper = PaperTrading::new();
        paper.account.dir = dir_a.path().to_path_buf();
        paper.set_symbol("SWITCHX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 103));
        assert!(
            paper.account.journal_path.is_some(),
            "the close journaled under A"
        );
        {
            let (state, env) = paper.report_parts();
            state.reload_ledger(&env)
        };
        assert!(paper.account.report_state().saved_rows_loaded().is_some());

        paper.account.set_trades_dir(dir_b.path().to_path_buf());
        assert_eq!(paper.account.trades_dir(), dir_b.path());
        assert!(
            paper.account.journal_path.is_none(),
            "the next close opens a new session file under B"
        );
        assert!(
            paper.account.report_state().saved_rows_loaded().is_none(),
            "the ledger re-reads from the new home"
        );
        assert!(paper.account.has_toast(), "the switch is never silent");
        assert!(
            dir_a.join("SWITCHX").exists(),
            "files already written stay where they are"
        );
    }

    /// The ledger's cache reads every saved file except the live session's
    /// own (its trades are already in the simulator), and remembers which
    /// symbol each row came from.
    #[test]
    fn the_ledger_cache_excludes_the_live_session_file() {
        let dir = crate::scratch::ScratchDir::new("paper-ledger-test");
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol("LEDGX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 105));
        assert!(paper.account.journal_path.is_some(), "the close journaled");

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

        {
            let (state, env) = paper.report_parts();
            state.reload_ledger(&env)
        };
        let cache = paper
            .account
            .report_state()
            .saved_rows_loaded()
            .expect("loaded");
        assert_eq!(
            cache.len(),
            1,
            "the live session's file is excluded, the earlier one loads"
        );
        assert_eq!(cache[0].symbol, "LEDGX");
        assert_eq!(cache[0].trade, trade);
        assert_eq!(cache[0].source, Some(history::SessionSource::Live));
    }

    #[test]
    fn a_replay_rerun_lands_beside_its_first_run_never_inside_it() {
        let dir = crate::scratch::ScratchDir::new("paper-rerun-test");
        // The same recording replayed twice: identical prints, identical
        // venue times, so both sessions derive the same file stamp.
        for _ in 0..2 {
            let mut paper = PaperTrading::new();
            paper.account.dir = dir.path().to_path_buf();
            paper.set_symbol("RERUN");
            paper
                .account_mut()
                .set_session_source(history::SessionSource::Replay);
            paper.seed(&print(0, 100));
            paper.market(Side::Buy);
            paper.on_trade(&print(1, 100));
            let events = paper.account.dispatch(Command::ClosePosition);
            paper.account.handle_events(events);
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
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 105));
        assert_eq!(paper.account.venue.closed_trades().len(), 1);

        paper
            .account_mut()
            .set_session_source(history::SessionSource::Replay);
        paper.on_timeline_reset();
        {
            let (state, env) = paper.report_parts();
            state.reload_ledger(&env)
        };
        let cache = paper
            .account
            .report_state()
            .saved_rows_loaded()
            .expect("loaded");
        assert_eq!(
            cache.len(),
            0,
            "the session's own files stay excluded across the retarget"
        );
    }

    /// A revealed page must survive the ledger's own lazy first load.
    /// `QUANTICK_LEDGER_PAGES` sets the page count during construction,
    /// long before the Trades tab is first drawn; the load that tab
    /// triggers used to reset it, so the hook reached page one and the
    /// state it exists to photograph was unreachable.
    #[test]
    fn a_revealed_page_survives_the_ledgers_lazy_first_load() {
        let dir = crate::scratch::ScratchDir::new("paper-pages-test");
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol("PAGEX");

        paper.account.report_state_mut().autostart_ledger_pages(3);
        assert_eq!(paper.account.report_state().revealed_pages(), 3);
        // What the first `draw_trades_tab` does before painting a row.
        assert!(paper.account.report_state().saved_rows_loaded().is_none());
        {
            let (state, env) = paper.report_parts();
            state.reload_ledger(&env)
        };
        assert_eq!(
            paper.account.report_state().revealed_pages(),
            3,
            "the lazy load must not retire the hook's page count"
        );
        // And what every tab does on every drain: sync the journal to its
        // symbol. This runs on the frame, so it must not retire the page
        // either — the hook set it before the feed had a symbol at all.
        paper.set_symbol("PAGEY");
        assert_eq!(
            paper.account.report_state().revealed_pages(),
            3,
            "the per-frame symbol sync must not retire the page"
        );

        // A *scope* change is the one thing that does reset it: a deep
        // page cannot survive a list it was never counted against.
        {
            let (state, env) = paper.report_parts();
            state.rescope_ledger(&env)
        };
        assert_eq!(paper.account.report_state().revealed_pages(), 1);
    }

    #[test]
    fn the_report_scopes_by_symbol_folder_on_disk() {
        let dir = crate::scratch::ScratchDir::new("paper-symbols-test");
        for (symbol, id0, price) in [("AAAUSDT", 0, 100), ("BBBUSDT", 100, 200)] {
            let mut paper = PaperTrading::new();
            paper.account.dir = dir.path().to_path_buf();
            paper.set_symbol(symbol);
            paper.seed(&print(id0, price));
            paper.market(Side::Buy);
            paper.on_trade(&print(id0 + 1, price));
            let events = paper.account.dispatch(Command::ClosePosition);
            paper.account.handle_events(events);
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
    }

    #[test]
    fn a_close_refreshes_an_open_report_by_itself() {
        let utc = TzOffset::new(0);
        let mut paper = PaperTrading::new();
        paper.set_symbol("FRESH");
        {
            let (state, env) = paper.report_parts();
            state.open(&env)
        };
        paper.account.report_state_mut().ensure_report_view(utc);
        assert!(
            paper
                .account
                .report_state()
                .view_rows()
                .expect("view")
                .is_empty(),
            "nothing saved yet"
        );

        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 105));

        paper.account.report_state_mut().ensure_report_view(utc);
        let view = paper.account.report_state().view_rows().expect("refreshed");
        assert_eq!(
            view.len(),
            1,
            "the close re-read the journal without a manual refresh"
        );
    }
}
