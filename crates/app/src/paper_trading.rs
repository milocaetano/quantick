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
use quantick_engine::Trade;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_paper::desk::CmdPreviewForce;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_paper::desk::ruler::RULER_MAX_NOTCHES;
use quantick_session::paper::TabPaper;
// The words every order surface speaks, decided beside the account.
use quantick_paper::format::{kind_short, kind_word, side_word_upper};

use crate::chart::PriceScale;
// One date law for every trade surface - see `paper_calendar`.
pub(crate) use crate::paper_account::{Leg, side_word};
// The desk's values, under the names the app's callers already use.
pub(crate) use quantick_paper::desk::{
    ArmedPlacement, CmdEntryKind, CmdModifier, CmdTradingSettings,
};

// The report and the ledger moved to `paper_report`; these names did not.
// The control plane, the dock and the harness hooks all reach them through
// this module, and a type that changed address because its code did would
// make every one of those callers pay for a move they did not ask for.
pub(crate) use crate::paper_report::LedgerAction;
#[cfg(any(feature = "scenario-harness", test))]
pub(crate) use crate::paper_report::LedgerScope;

pub(crate) mod account_route;
mod cmd;
mod input;
mod paint;
pub(crate) mod paint_ctx;
mod strategies;
mod ticket;

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
    /// Harness override: the resting orders to place on the first mark
    /// (`QUANTICK_PAPER_ORDERS`, `QUANTICK_PAPER_ORDER_BRACKET`); `None`
    /// once they rest.
    #[cfg_attr(not(any(feature = "scenario-harness", test)), allow(dead_code))]
    capture_orders: Option<quantick_paper::account::CaptureOrders>,
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
            capture_orders: None,
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
        let bracket =
            crate::hooks::captured::var(PAPER_ORDER_BRACKET_ENV).is_some_and(|v| v == "1");
        self.capture_orders = crate::hooks::captured::var(PAPER_ORDERS_ENV)
            .and_then(|value| quantick_paper::account::CaptureOrders::parse(&value, bracket));
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
        // `capture_orders` is a harness field, so its orders rest from here.
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(orders) = self.capture_orders
            && self.paper.account.rest_capture_orders(orders)
        {
            self.capture_orders = None;
        }
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
