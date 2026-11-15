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
//!
//! What this host decides is nothing. The ticket's text, the ruler, the cmd
//! aim and which line or ✕ a press lands on are `quantick_paper::desk`'s
//! functional core, and the money path is the account's; this module paints
//! them, reads the window's input into plain values for the desk, and
//! carries out the command a press comes back with.

use std::path::{Path, PathBuf};

use eframe::egui;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_engine::Side;
use quantick_engine::Trade;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_paper::desk::CmdPreviewForce;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_paper::desk::ruler::RULER_MAX_NOTCHES;
use quantick_session::paper::TabPaper;
// The words every order surface speaks, decided beside the account.
use quantick_paper::format::{kind_short, kind_word, side_word_upper};
#[cfg(any(feature = "scenario-harness", test))]
use quantick_sim::{Bracket, OrderIntent};
// The journal's own format, and the command type the sim takes, are named
// only by the tests that drive one; the writing moved to `paper_account`.
#[cfg(test)]
use quantick_sim::{ClosedTrade, Command, history};
#[cfg(any(feature = "scenario-harness", test))]
use rust_decimal::Decimal;

use crate::chart::PriceScale;
// One date law for every trade surface - see `paper_calendar`.
pub(crate) use crate::paper_account::{Leg, side_word};
// The desk's values, under the names the app's callers already use.
pub(crate) use quantick_paper::desk::{
    ArmedPlacement, CmdEntryKind, CmdModifier, CmdTradingSettings,
};
// The press-side answer, named by the tests that ask what a pixel holds.
#[cfg(test)]
pub(crate) use quantick_paper::desk::PaperControl;
// The report's anchor date is formatted only under test.
#[cfg(test)]
use quantick_civil::civil_utc;

// The report and the ledger moved to `paper_report`; these names did not.
// The control plane, the dock and the harness hooks all reach them through
// this module, and a type that changed address because its code did would
// make every one of those callers pay for a move they did not ask for.
pub(crate) use crate::paper_report::LedgerAction;
#[cfg(any(feature = "scenario-harness", test))]
pub(crate) use crate::paper_report::LedgerScope;

mod cmd;
mod input;
mod paint;
mod paint_ctx;
mod strategies;
mod ticket;

pub(crate) use paint_ctx::tag_band;
// The tag geometry, named by the tests that press a ✕ where one was painted.
#[cfg(test)]
pub(crate) use paint_ctx::{clamp_tag_center, close_button_rect};

/// `=<rungs>` rests entry orders around the mark as soon as the tape has
/// one, so the in-plot order tag can be photographed at all. The scripted
/// demo's own order is 220 prints away and sits 0.4 % out — far enough to
/// fall outside an autoscaled price range, and close enough that a lively
/// tape fills it before the shutter. Each rung is a **buy limit below and
/// a sell limit above**: a move in either direction can fill only one side
/// of it, so a resting tag always survives on screen.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_ORDERS_ENV: &str = "QUANTICK_PAPER_ORDERS";
/// `=1` gives every order `QUANTICK_PAPER_ORDERS` rests a protective stop
/// and target, so the working-order bracket — its two dashed leg lines,
/// their gutter chips and their tags — can be photographed without a hand
/// to drag them into being. Pairs with `QUANTICK_PAPER_ORDER_HOVER`, which
/// opens one order's tag and with it the labelled `SL`/`TP` handles for the
/// legs it does *not* have; set both and one capture holds every state the
/// bracket has.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_ORDER_BRACKET_ENV: &str = "QUANTICK_PAPER_ORDER_BRACKET";
/// How far a hooked bracket's legs sit from the order, as a fraction of the
/// mark. Wider than the rung step so the legs never land on a neighbouring
/// order's line, and wide enough apart that stop and target read as two
/// levels rather than one thick one.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_ORDER_BRACKET_FRACTION: Decimal = Decimal::from_parts(15, 0, 0, false, 4);
/// How far the first rung sits from the mark, as a fraction of it. Small
/// on purpose: a line outside the chart's autoscaled price range paints no
/// tag, so an order that cannot be reached also cannot be seen.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_ORDERS_STEP_FRACTION: Decimal = Decimal::from_parts(6, 0, 0, false, 4);
/// Rungs past this are refused — a capture wants a tag or two, not a book.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_ORDERS_MAX_RUNGS: u8 = 4;
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

/// What the strategy selector calls "no strategy" - the bare order.
const STRATEGY_NONE: &str = "<None>";

/// Opens the strategy editor on launch, so a capture run can photograph it
/// without a hand on the mouse. See `docs/ux/paper-trading.md`.
#[cfg(any(feature = "scenario-harness", test))]
const STRATEGY_EDITOR_ENV: &str = "QUANTICK_PAPER_STRATEGY_EDITOR";
/// Stands the ruler at this many ticks on launch.
///
/// The ruler is walked with the wheel, and a scripted run has no wheel — so
/// without this the projected pair, its distance in points and ticks and the
/// `1:1` it reads are unreachable from a capture. Pair with
/// `QUANTICK_CMD_PREVIEW`, which supplies the aim the ruler measures from.
#[cfg(any(feature = "scenario-harness", test))]
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
/// How much wider than the line the halo pass paints.
const DRAG_HALO_EXTRA_WIDTH_PX: f32 = 3.5;
/// Horizontal padding inside a tag.
const TAG_PAD_X: f32 = 6.0;
/// Alpha of the ink hairline between a chip tag's ✕ zone and its words.
const CLOSE_DIVIDER_ALPHA: u8 = 90;

/// `=buy`/`=sell` forces the cmd-trading preview for a capture run, and an
/// optional `@<fraction>` parks the virtual pointer at that fraction of the
/// band's width (`buy@0.15` aims near the left edge). The held modifier and
/// the hand that moves the mouse are the two inputs a run with nobody at
/// the keyboard cannot supply (the ParkedHand rule) — and now that the
/// label rides the pointer, its x is a state of its own to capture.
#[cfg(any(feature = "scenario-harness", test))]
const CMD_PREVIEW_ENV: &str = "QUANTICK_CMD_PREVIEW";
/// Forces every resting order's in-plot tag, and every bracket leg's, to its
/// expanded form for a capture run — the same ParkedHand problem: the
/// compact pill opens under a pointer no scripted run has.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_ORDER_HOVER_ENV: &str = "QUANTICK_PAPER_ORDER_HOVER";
/// Most dash segments the aim line is allowed to paint. It now runs from
/// the pointer all the way to the axis, which ties the label beside the
/// hand to the price on the gutter — but on a maximised chart that is
/// thousands of pixels, and `Shape::dashed_line` allocates one segment per
/// dash *every frame the modifier is held*. Past this the dash period
/// stretches instead, so the cost is bounded and the rhythm still reads.
const CMD_LINE_MAX_DASHES: f32 = 96.0;
/// The aim's echo in the panes it was not made in: a side-coloured line as
/// wide as a hovered order line and nearly opaque, so the level reads across
/// a busy chart. Its long 12/4 dash, against the 4/4 of the aim and of every
/// working order and bracket leg (the position's entry line is solid), and
/// its hollow chip say it is a level, not an order.
pub(crate) const AIM_ECHO_ALPHA: f32 = 0.85;
const AIM_ECHO_WIDTH_PX: f32 = LINE_HOVER_WIDTH_PX;
const AIM_ECHO_DASH_PX: f32 = 12.0;
const AIM_ECHO_GAP_PX: f32 = 4.0;

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
/// caller so this module never reads raw input state itself: the desk's
/// `ChartFrame` in the window's types, whose fields document each value,
/// plus the price scale the desk reads through.
pub struct ChartInput<'a> {
    pub chart: egui::Rect,
    pub aim_area: egui::Rect,
    pub scale: Option<&'a PriceScale>,
    pub pointer: Option<egui::Pos2>,
    pub primary_pressed: bool,
    pub primary_down: bool,
    pub primary_released: bool,
    pub modifiers: egui::Modifiers,
    pub canvas_claimed: bool,
    pub on_chrome: bool,
    pub scroll_y: f32,
    pub middle_pressed: bool,
    pub layer_visible: bool,
}

/// The app-side trading host: the tab's paper (desk and account), the
/// capture hooks, and the paint and widgets around both.
pub struct PaperTrading {
    /// Whether `QUANTICK_PAPER_ORDER_BRACKET` asked the capture hook's
    /// resting orders to carry protective legs.
    #[cfg_attr(not(any(feature = "scenario-harness", test)), allow(dead_code))]
    order_bracket_demo: bool,
    /// Harness override: how many rungs of resting orders to place on the
    /// first mark (`QUANTICK_PAPER_ORDERS`); `None` once they are placed.
    #[cfg_attr(not(any(feature = "scenario-harness", test)), allow(dead_code))]
    orders_demo: Option<u8>,
    /// The tab's desk and account, and the decisions that join them
    /// (`quantick_session::paper`). The account is held with the report and
    /// the export threads beside it; everything the ticket, the dock and the
    /// control plane call by name — `market`, `flatten`, the ruler, the risk
    /// read — is the tab's, reached through `Deref`.
    paper: TabPaper<crate::paper_account::PaperAccount>,
}

impl std::ops::Deref for PaperTrading {
    type Target = TabPaper<crate::paper_account::PaperAccount>;

    fn deref(&self) -> &Self::Target {
        &self.paper
    }
}

impl std::ops::DerefMut for PaperTrading {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.paper
    }
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
fn leg_color(palette: &crate::theme::Palette, leg: Leg) -> egui::Color32 {
    match leg {
        Leg::StopLoss => palette.sell(),
        Leg::TakeProfit => palette.buy(),
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
        #[cfg_attr(
            not(any(feature = "scenario-harness", test)),
            allow(clippy::let_and_return)
        )]
        let host = Self {
            paper: TabPaper::new(crate::paper_account::PaperAccount::with_trades_dir(dir)),
            order_bracket_demo: false,
            orders_demo: None,
        };
        #[cfg(any(feature = "scenario-harness", test))]
        let host = host.with_launch_hooks();
        host
    }

    /// The ticket's capture hooks, applied over the defaults: the staged
    /// bracket, the forced previews, the order ladder demo, the strategy
    /// editor and the ruler notches. Compiled only with the scenario harness
    /// (or under test).
    #[cfg(any(feature = "scenario-harness", test))]
    fn with_launch_hooks(mut self) -> Self {
        self.order_bracket_demo =
            crate::hooks::captured::var(PAPER_ORDER_BRACKET_ENV).is_some_and(|value| value == "1");
        self.paper.desk.gesture.cmd_preview_force = crate::hooks::captured::var(CMD_PREVIEW_ENV)
            .and_then(|value| {
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
            });
        self.paper.desk.gesture.order_hover_force =
            crate::hooks::captured::var(PAPER_ORDER_HOVER_ENV).is_some_and(|value| value == "1");
        self.orders_demo = crate::hooks::captured::var(PAPER_ORDERS_ENV).and_then(|value| {
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
        });
        self.paper.desk.strategy_editor.open =
            crate::hooks::captured::var(STRATEGY_EDITOR_ENV).is_some_and(|value| value == "1");
        self.paper.desk.ruler.notches = crate::hooks::captured::var(RULER_TICKS_ENV)
            .and_then(|value| value.trim().parse::<u32>().ok())
            .map_or(0, |notches| notches.min(RULER_MAX_NOTCHES));
        self
    }

    /// Follow the app's active symbol: the tab's paper retargets the journal
    /// and the ruler, and an open report starts over on the new instrument.
    pub fn set_symbol(&mut self, symbol: &str) {
        // The revealed ledger page is left alone: `LedgerPage::of` clamps a
        // page the new history cannot back, and resetting it retired the
        // `QUANTICK_LEDGER_PAGES` hook before its first row was painted.
        if self.paper.set_symbol(symbol) {
            self.paper.account.report.symbol_changed();
        }
    }

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
        self.paper.on_trade(trade);
        self.paper.account.follow_journal();
        // `orders_demo` is a harness field, so its orders rest from here.
        #[cfg(any(feature = "scenario-harness", test))]
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
    #[cfg(any(feature = "scenario-harness", test))]
    fn rest_capture_orders(&mut self) {
        let Some(rungs) = self.orders_demo else {
            return;
        };
        let Some(mark) = self.paper.account.venue().mark_price() else {
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
                    .paper
                    .account
                    .venue_mut()
                    .submit(OrderIntent::limit(side, quantity, price).with_bracket(bracket));
                self.paper.account.handle_events(events);
            }
        }
        if self.paper.account.venue().working_orders().is_empty() {
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
}

#[cfg(test)]
impl PaperTrading {
    /// The report's state, as its own surfaces read it. Test-only, like the
    /// account's own pair: the drawing code reaches the report through
    /// `report_parts`.
    pub(crate) fn report_state(&self) -> &crate::paper_report::ReportState {
        self.account().report_state()
    }

    /// The report's state, mutably. Test-only, as above.
    pub(crate) fn report_state_mut(&mut self) -> &mut crate::paper_report::ReportState {
        self.account_mut().report_state_mut()
    }

    /// Point the journal at a scratch folder (tests only).
    pub(crate) fn redirect_history_dir(&mut self, dir: PathBuf) {
        self.account_mut().redirect_history_dir(dir);
    }

    /// Run one simulator command straight at the venue (tests only).
    pub(crate) fn apply_sim_command_for_tests(&mut self, command: Command) {
        self.account_mut().apply_sim_command_for_tests(command);
    }

    pub(crate) fn close_button_label(&self) -> Option<String> {
        self.account().close_button_label()
    }

    pub(crate) fn ready(&self) -> bool {
        self.account().ready()
    }
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

#[cfg(any(feature = "scenario-harness", test))]
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
