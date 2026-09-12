//! The paper-trading account: the money path, with no pixels in it.
//!
//! Split out of `paper_trading`, which kept the ticket. Everything here
//! places, fills, protects, sizes and journals; nothing here draws, and the
//! module names no drawing type at all. That is the whole point: an auditor
//! asking whether a stop went on at the right price reads this file and
//! stops, rather than reading it through the code that paints the stop.
//!
//! The same shape the report extraction took: the half is *handed* what only
//! its host can know (an [`AccountEnv`]), and *answers* with what it cannot do
//! itself (an [`AccountResponse`]).

use std::path::{Path, PathBuf};

use quantick_engine::{Side, Trade};
use quantick_sim::{
    Bracket, BracketTarget, CloseAmount, ClosedTrade, Command, EntryKind, OrderId, OrderIntent,
    Simulator, TradingVenue, VenueEvent, history,
};
use rust_decimal::Decimal;

use crate::paper_calendar::civil_utc;
use crate::paper_chrome::{PositionSummary, fmt_decimal, fmt_signed_points, position_word};

mod export;
mod orders;
mod report;
mod risk;

/// `elide_path` names a path in a toast this file raises; `export_csv`
/// is reached by name from the ticket's own tests. The rest of
/// `export` is reached through [`PaperAccount`].
pub(crate) use export::elide_path;
#[cfg(test)]
pub(crate) use export::export_csv;

/// What the account is handed that only the ticket can know.
///
/// Deliberately small. The account owns the venue, the journal folder, the
/// symbol and the instrument's precision, so none of those are here - it
/// would be handed back what it already knows. What it cannot learn is what
/// the trader typed and how far the ruler has been wound, and that is the
/// whole of this struct.
#[derive(Clone, Default)]
pub(crate) struct AccountEnv {
    /// The stop and target the ruler is holding out, if it is up.
    ///
    /// Resolved by the ticket rather than handed over as a notch count: the
    /// wheel, its travel and the step it walks are pixels, and what the money
    /// path needs from all of it is two prices.
    pub ruler_levels: Option<(Decimal, Decimal)>,
    /// The ticket's typed form, already read.
    pub form: TicketForm,
}

/// The three typed boxes, resolved to values.
///
/// `None` is "empty, or does not parse", which the account reads as "no
/// protection on this side" - exactly what the ticket meant.
#[derive(Clone)]
pub(crate) struct TicketForm {
    /// The quantity box: the number, or the complaint the ticket would make
    /// about what is in it.
    ///
    /// The complaint travels with the value because only the ticket knows
    /// which box it is about and what was typed there, and only the account
    /// knows whether the number is ever reached - a risk-derived size makes
    /// the box irrelevant. Carrying the sentence lets each decide its own
    /// half, and keeps the message a trader sees exactly what it was.
    pub quantity: Result<Decimal, String>,
    /// The two protective offsets, or `None` when **either** box holds text
    /// that is not a positive number.
    ///
    /// All-or-nothing, because `ticket_bracket` was: it read both boxes with
    /// `?` and one bad box failed the whole call. Reading them independently
    /// would project a target-only bracket from a ticket whose stop says
    /// `abc`, which is a protection the trader never typed.
    pub offsets: Option<(Option<Decimal>, Option<Decimal>)>,
}

impl Default for TicketForm {
    /// An empty form: no quantity typed, and the complaint that says so.
    fn default() -> Self {
        Self {
            quantity: Err("SIM: quantity must be a positive number - got ``".to_owned()),
            offsets: Some((None, None)),
        }
    }
}

impl TicketForm {
    /// The protective bracket the typed offsets describe around `reference`.
    /// A long's stop sits below and its target above; a short's the other way.
    pub(crate) fn bracket(&self, side: Side, reference: Decimal) -> Bracket {
        let Some((stop_offset, profit_offset)) = self.offsets else {
            return Bracket::none();
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
        Bracket::whole(stop_loss, take_profit)
    }
}

/// What the account asked its host to do, and what it could not do itself.
///
/// The same shape as `ReportResponse`, for the same reason: this module can
/// decide that an acknowledgement is owed and must not be the thing that puts
/// it on screen. It owns no toast lane, no clock and no dialog.
#[derive(Default)]
pub(crate) struct AccountResponse {
    /// The acknowledgement waiting to be shown, if there is one.
    ///
    /// One slot, and deliberately: the ticket's outbox always held one, so a
    /// healthy "closed" painting over a could-not-save warning is a decision
    /// this keeps exactly as it was. A queue here would change which message
    /// a trader sees.
    pub toast: Option<String>,
}

/// The next chart click places this entry (`Limit` or `Stop` only — a
/// market order needs no price and fires straight from its button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArmedPlacement {
    pub(crate) side: Side,
    pub(crate) kind: EntryKind,
}
/// The scripted demo's only state: how many prints it has seen.
pub(crate) struct PaperDemo {
    prints: u64,
}
/// Which side of a bracket a gesture is about.
///
/// The two legs are not symmetric — one caps the loss and one takes the
/// win — but every gesture that touches either does the same thing to it,
/// so they travel as one value rather than as a `bool` nobody can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leg {
    StopLoss,
    TakeProfit,
}
impl Leg {
    /// The two-letter word on the line's tag and on its handle.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::StopLoss => "SL",
            Self::TakeProfit => "TP",
        }
    }

    /// The *other* leg's level within a bracket — the one the R:R read is
    /// measured against while this one is being dragged.
    pub(crate) fn other(self, bracket: Bracket) -> Option<Decimal> {
        match self {
            Self::StopLoss => bracket.take_profit(),
            Self::TakeProfit => bracket.stop_loss(),
        }
    }

    /// This leg's level within a bracket.
    pub(crate) fn level(self, bracket: Bracket) -> Option<Decimal> {
        match self {
            Self::StopLoss => bracket.stop_loss(),
            Self::TakeProfit => bracket.take_profit(),
        }
    }

    /// The bracket with this leg set to `level` (`None` clears it) and the
    /// other leg untouched — every amendment in this module goes through
    /// here, so "replace wholesale" can never accidentally drop the leg
    /// nobody was touching.
    pub(crate) fn applied(self, bracket: Bracket, level: Option<Decimal>) -> Bracket {
        match self {
            Self::StopLoss => Bracket::whole(level, bracket.take_profit()),
            Self::TakeProfit => Bracket::whole(bracket.stop_loss(), level),
        }
    }

    /// Whether this leg's price sits **above** the entry, for `side`.
    ///
    /// Named for the geometry and not for the meaning, because the two part
    /// company on a short: a short's stop is above its entry *and* on the
    /// losing side. The callers want the geometry — it is what decides
    /// which side of the line a handle is drawn on — so calling this
    /// "profit side" would be an invitation to fold
    /// `decide_pending_leg`'s own profit-side test into it and swap stop
    /// for target on every short.
    pub(crate) fn sits_above_entry(self, side: Side) -> bool {
        matches!(
            (self, side),
            (Self::TakeProfit, Side::Buy) | (Self::StopLoss, Side::Sell)
        )
    }
}
/// A painted overlay control from the last paint pass: a tag's ✕ or a
/// bracket handle. Hit rects are cached one frame behind the paint — the
/// input pass runs before the draw, and an immediate-mode overlay control is
/// pressed against where it was actually painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaperControl {
    /// ✕ on the position tag: exit at the next print.
    ClosePosition,
    /// ✕ on a protective leg's tag: clear that leg, keeping the other.
    ClearLeg { owner: BracketTarget, leg: Leg },
    /// ✕ on a working order's tag: cancel it.
    CancelOrder(OrderId),
    /// Labelled `SL`/`TP` handle beside a line that owns brackets: the
    /// press starts a create-drag for that leg.
    Handle { owner: BracketTarget, leg: Leg },
    /// ✕ on one rung of a resting entry's ladder: clear that rung's leg and
    /// leave every other rung alone. A rung with neither leg left is dropped
    /// — a part that protects nothing is not a part.
    ClearRung {
        order: OrderId,
        index: usize,
        leg: Leg,
    },
}
/// A modifier key the cmd-trading gesture can bind to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmdModifier {
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
}
/// Which entry kind the aim places.
///
/// The fill model leaves exactly one *resting* kind valid at any price: a
/// buy above the market can only stop in (a buy limit there would fill at
/// once), and below it can only wait at a limit. So this is not a way to
/// place a stop where a limit belongs — no venue would take it. It is a way
/// to state **which order you came to place**, so the aim shows nothing
/// rather than quietly handing you the other kind when the market is on the
/// wrong side of your level.
///
/// That case is not hypothetical: the mark moves. A level a hand's breadth
/// above the last price is a buy stop now and a buy limit after two ticks
/// up, and under [`Self::Auto`] the same click at the same level places a
/// different order depending on when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CmdEntryKind {
    /// Whichever kind can rest at the aimed price — the mark decides.
    #[default]
    Auto,
    /// Only a limit. Where a limit cannot rest, the aim stands down.
    Limit,
    /// Only a stop. Where a stop cannot arm, the aim stands down.
    Stop,
}
impl CmdEntryKind {
    /// Every choice the selector offers.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Limit, Self::Stop];

    /// Stable token for the state file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Limit => "limit",
            Self::Stop => "stop",
        }
    }

    /// The inverse of [`Self::as_str`]; unknown tokens are refused.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "auto" => Some(Self::Auto),
            "limit" => Some(Self::Limit),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    /// Display label for the selector.
    ///
    /// The same words as [`Self::as_str`] today, and delegating rather than
    /// repeating them so it stays that way by accident only where it is
    /// harmless: written out twice, renaming the selector's "stop" would
    /// silently change the on-disk token and every remembered choice would
    /// fall back to `Auto` on the next launch. Give this its own `match`
    /// the day the label and the token should differ, which is what
    /// [`CmdModifier`] already does.
    #[must_use]
    pub fn label(self) -> &'static str {
        self.as_str()
    }
}
/// Cmd trading: hold a key over the chart and a dashed line shows exactly
/// where the order will rest, with a label riding beside the cursor; the
/// click places it. Safer than the right-click menu because the price is
/// visible before anything commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CmdTradingSettings {
    pub enabled: bool,
    pub buy: CmdModifier,
    pub sell: CmdModifier,
    /// Which entry kind the aim places; see [`CmdEntryKind`].
    pub kind: CmdEntryKind,
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
            kind: state
                .cmd_entry_kind
                .as_deref()
                .and_then(CmdEntryKind::parse)
                .unwrap_or(defaults.kind),
        }
    }
}
impl Default for CmdTradingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            buy: CmdModifier::Shift,
            sell: CmdModifier::Ctrl,
            // Auto is right at almost every price, and it is what shipped;
            // the choice exists for the trader who wants to be sure.
            kind: CmdEntryKind::Auto,
        }
    }
}

/// `=1` runs a fixed sequence of ordinary sim commands driven by print
/// count, so a screenshot or demo run shows every trading surface without
/// a click. The trades are as real as any simulated trade (journaled,
/// listed, painted); point `QUANTICK_TRADES_DIR` somewhere scratch to keep
/// a demo out of your journal.
const PAPER_DEMO_ENV: &str = "QUANTICK_PAPER_DEMO";

/// The risk per trade standing up on the first frame, so the derived size,
/// the line under it and the refusal the lock produces are all reachable
/// without a hand. Declared here because the risk is the account's.
const PAPER_RISK_ENV: &str = "QUANTICK_PAPER_RISK";

crate::hooks::declare_hooks!["QUANTICK_PAPER_DEMO", "QUANTICK_PAPER_RISK"];

/// The `ReportEnv` for an account, built where it is used.
///
/// A macro and not a method, and the reason is the borrow checker rather than
/// taste: a `fn report_env(&self)` returns a value borrowing the *whole*
/// account, which then collides with the `&mut self.report` its callers need.
/// Expanded inline, each field is borrowed on its own and `report` is not
/// among them, so the two borrows are disjoint and the compiler can see it.
macro_rules! account_env {
    ($account:expr) => {
        crate::paper_report::ReportEnv {
            symbol: &$account.symbol,
            dir: &$account.dir,
            session_journal_paths: &$account.session_journal_paths,
            session_trades: $account.venue.closed_trades(),
            open: $account.open_row(),
        }
    };
}
pub(crate) use account_env;

/// The paper-trading account: everything the money path is made of,
/// and nothing the ticket draws.
pub(crate) struct PaperAccount {
    /// Where orders actually go. A [`TradingVenue`] rather than a
    /// `Simulator`, so the chart's gestures, the ticket and the
    /// control-plane actions are all written against the port a real
    /// broker will one day implement — see `quantick-trading`. Today the
    /// only venue constructed here is the deterministic paper simulator,
    /// and every surface still says `SIM`.
    pub(crate) venue: Box<dyn TradingVenue>,
    /// Symbol the journal writes under; follows the app's active symbol.
    pub(crate) symbol: String,
    pub(crate) dir: PathBuf,
    /// Current session file, named after the first closed trade.
    pub(crate) journal_path: Option<PathBuf>,
    /// Every file this host has journaled to. The ledger excludes them
    /// all — their trades are still in the simulator, which keeps closed
    /// trades across every retarget; excluding only the current file
    /// double-counted after a symbol or source switch.
    // `pub(crate)` for one reason: `account_env!` expands at the ticket's
    // own call sites, so this field is read there even though nothing else
    // outside this module names it.
    pub(crate) session_journal_paths: Vec<PathBuf>,
    /// The session source each of the simulator's closed trades closed
    /// under, index-aligned with `sim.closed_trades()` — the export must
    /// not stamp a pre-switch trade with the current source.
    pub(crate) session_trade_sources: Vec<history::SessionSource>,
    /// A failed journal write warns once, not once per trade.
    journal_warned: bool,
    /// Where this session's trades come from — the tab's feed sets it,
    /// the journal header records it.
    session_source: history::SessionSource,
    /// Cmd trading: the toggle and its two key bindings (app-wide; the
    /// app persists and fans out changes).
    pub(crate) cmd_trading: CmdTradingSettings,
    pub(crate) armed: Option<ArmedPlacement>,
    /// The named exit strategies the trader keeps, in their own order.
    pub(crate) strategies: Vec<crate::order_strategies::OrderStrategy>,
    /// Which strategy the ticket is set to; `None` is the bare order the
    /// trader brackets by hand.
    pub(crate) selected_strategy: Option<usize>,
    /// The finest precision this instrument's prints have actually shown,
    /// as a number of decimal places.
    ///
    /// One tick is `10^-tick_scale`, and it has to come from the tape rather
    /// than from any single print: a venue may quote `78112.57000000`, whose
    /// raw scale is eight and whose real step is two, and the very next print
    /// may land on `78100` and normalize to zero. Reading one print gives a
    /// tick that changes under the trader's hand — 80 of them a whole point
    /// one second and a hundred-millionth the next. Taking the finest scale
    /// the prints have *ever* shown is stable, monotonic and costs one
    /// comparison per trade.
    pub(crate) tick_scale: u32,
    /// What one trade may lose, and whether an entry over it is refused.
    ///
    /// The policy half of risk sizing; the arithmetic belongs to the kernel.
    /// See [`crate::risk_sizing`].
    pub(crate) risk: crate::risk_sizing::RiskSettings,
    /// The practice capital, one amount per currency. Never summed across
    /// currencies and never converted between them.
    pub(crate) capital: crate::risk_sizing::Capital,
    /// What one point of each instrument is worth, by bare symbol. Declared
    /// by the trader; nothing here derives it from the tape.
    pub(crate) instrument_money: crate::risk_sizing::InstrumentBook,
    /// Money a launch hook asked for, waiting for the tab to learn which
    /// symbol it opens on. Spent once, on the first symbol.
    pub(crate) hook_money: Option<quantick_sim::InstrumentMoney>,
    /// Whether a launch hook set the risk per trade for this run.
    ///
    /// An environment variable is an explicit request for one run, so it
    /// outranks the sidecar - and the sidecar fan-out that follows
    /// construction has to be told, or it silently restores the stored
    /// settings over the ones the run asked for.
    risk_from_hook: bool,
    /// The in-flight export, if any; resolved by [`PaperTrading::settle`]'s
    /// poll.
    export_rx: Option<std::sync::mpsc::Receiver<Result<(PathBuf, usize), String>>>,
    /// The in-flight history-folder import, if any; resolved by
    /// [`PaperTrading::settle`]'s poll. Imports copy — the picked folder
    /// keeps its files.
    import_rx: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    /// The performance report and the trades ledger - the reading half of
    /// paper trading, which owns its own state (`paper_report`). One field
    /// here in place of the twenty-one that used to spread across this
    /// struct, none of which an order, a bracket or a fill ever read.
    pub(crate) report: crate::paper_report::ReportState,
    /// The scripted demo (`QUANTICK_PAPER_DEMO=1`), for screenshot and
    /// validation runs; `None` in normal use.
    pub(crate) demo: Option<PaperDemo>,
    /// Whether anything is listening for per-print simulator events (armed
    /// strategy instances). Off, `on_trade` buffers nothing — the hot path
    /// pays for the bot only while a bot exists.
    bot_listening: bool,
    /// Per-print events buffered for the strategy instances since the last
    /// drain. Only ever non-empty while `bot_listening`, and prints with
    /// nothing to report push nothing.
    pub(crate) bot_events: Vec<VenueEvent>,
    /// What this module asked its host to do. Drained by the ticket
    /// after every call; see [`AccountResponse`].
    outbox: AccountResponse,
}

impl PaperAccount {
    /// Take an entry at the market, protected by `ticket`.
    pub(crate) fn market(
        &mut self,
        side: Side,
        reference: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) {
        // The same three sources the aim reads, in the same order. A button
        // sitting directly under the Strategy row must not place a bare
        // order while that row says a ladder is armed. The size comes from
        // the same call, so the risk per trade governs a toolbar press as
        // much as it governs the aim.
        let Some((quantity, bracket)) = self.entry_size(side, reference, ticket, env) else {
            return;
        };
        let events = self
            .venue
            .submit(OrderIntent::market(side, quantity).with_bracket(bracket));
        self.handle_events(events);
    }

    /// Drain whatever the export and import threads finished, once a frame.
    pub(crate) fn settle(&mut self) {
        self.poll_export();
        self.poll_import();
    }

    /// Feed one live print through the simulator and act on what it did.
    pub(crate) fn on_trade(&mut self, trade: &Trade) {
        self.observe_precision(trade);
        let events = self.venue.on_trade(trade);
        self.handle_events(events);
        if self.demo.is_some() {
            self.run_demo_step();
        }
    }

    /// Point the account at an instrument. `None` when nothing changed,
    /// `Some(arriving)` when it did.
    ///
    /// `arriving` is the caller's to act on: leaving an instrument forgets its
    /// geometry, and arriving at the first one is not a departure. The app
    /// names the opening symbol a frame after construction, so treating that
    /// as a switch wiped a ruler the launch had just been asked for.
    pub(crate) fn set_symbol(&mut self, symbol: &str) -> Option<bool> {
        if self.symbol == symbol {
            return None;
        }
        let arriving = self.symbol.is_empty();
        self.symbol = symbol.to_owned();
        // Money a launch hook asked for lands on the symbol the tab opens
        // with, for the same reason the ruler does: the app names that symbol
        // a frame after construction, so there is nothing to key it by until
        // now. Spent once - a later switch is a real switch, and the trader's
        // own book answers for it.
        if let Some(money) = self.hook_money.take() {
            self.instrument_money.insert(symbol.to_owned(), money);
        }
        // The tick is the *instrument's*, and it only ever ratchets finer:
        // carried across a switch it would price the next market's ruler and
        // ladders in a precision that market has never printed.
        self.tick_scale = 0;
        self.journal_path = None;
        self.report.symbol_changed();
        // The revealed page is deliberately left alone: resetting it here
        // retired the `QUANTICK_LEDGER_PAGES` hook before the first row was
        // painted, and would retire a trader's scroll-back as silently.
        // `LedgerPage::of` already clamps a page the new history cannot back.
        Some(arriving)
    }

    /// An account journaling to `dir`, already resolved from config and
    /// environment.
    #[must_use]
    pub(crate) fn with_trades_dir(dir: PathBuf) -> Self {
        // A spec that does not parse is reported and ignored, never
        // defaulted: a capture run that silently got a different risk than
        // it asked for photographs the wrong thing and says nothing about
        // it. The rule `QUANTICK_PAPER_ORDERS` already follows.
        let hook = std::env::var(PAPER_RISK_ENV).ok().and_then(|value| {
            crate::risk_sizing::parse_hook(&value).or_else(|| {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_RISK_HOOK_REJECTED",
                    value = %value,
                    action = "risk_left_off",
                    "QUANTICK_PAPER_RISK wants `<amount>` or `<percent>%@<capital>`, optionally \
                     `:<point value>:<size step>:<currency>` and `:unlocked`"
                );
                None
            })
        });
        Self {
            venue: Box::new(Simulator::new()),
            symbol: String::new(),
            dir,
            journal_path: None,
            session_journal_paths: Vec::new(),
            session_trade_sources: Vec::new(),
            journal_warned: false,
            session_source: history::SessionSource::Live,
            cmd_trading: CmdTradingSettings::default(),
            armed: None,
            strategies: Vec::new(),
            selected_strategy: None,
            tick_scale: 0,
            risk: hook
                .as_ref()
                .map_or_else(crate::risk_sizing::RiskSettings::default, |hook| {
                    hook.settings.clone()
                }),
            capital: hook
                .as_ref()
                .map_or_else(crate::risk_sizing::Capital::new, |hook| {
                    hook.capital.clone()
                }),
            instrument_money: crate::risk_sizing::InstrumentBook::new(),
            risk_from_hook: hook.is_some(),
            hook_money: hook.and_then(|hook| hook.money),
            export_rx: None,
            import_rx: None,
            report: crate::paper_report::ReportState::default(),
            demo: std::env::var(PAPER_DEMO_ENV)
                .is_ok_and(|value| value == "1")
                .then_some(PaperDemo { prints: 0 }),
            bot_listening: false,
            bot_events: Vec::new(),
            outbox: AccountResponse::default(),
        }
    }

    /// Post an acknowledgement. An outbox, not a toast: this module owns no
    /// lane and no clock, and the message leaves through
    /// [`AccountResponse::toast`]. The ticket's own `show_toast` - the door
    /// `QUANTICK_TOAST=paper` knocks on - comes through here too, so there is
    /// one slot and one way into it.
    pub(crate) fn set_toast(&mut self, message: String) {
        self.outbox.toast = Some(message);
    }

    /// Test-only: the acknowledgement waiting, if any. A question rather than
    /// a field, which is what keeps the outbox private now that the ticket no
    /// longer owns one.
    #[cfg(test)]
    pub(crate) fn peek_toast(&self) -> Option<&String> {
        self.outbox.toast.as_ref()
    }

    /// Take the acknowledgement, if one is waiting.
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        self.outbox.toast.take()
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

    /// Point the journal at a scratch folder (tests only).
    #[cfg(test)]
    pub(crate) fn redirect_history_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }

    /// Where trades save right now.
    #[must_use]
    pub(crate) fn trades_dir(&self) -> &Path {
        &self.dir
    }

    /// Point the journal somewhere new, in-session — the panel's folder
    /// picker. Files already written stay exactly where they are; the next
    /// close opens a new session file under the new folder, and the ledger
    /// and report re-read from the new home.
    pub(crate) fn set_trades_dir(&mut self, dir: PathBuf) {
        if self.dir == dir {
            return;
        }
        self.dir = dir;
        self.journal_path = None;
        self.journal_warned = false;
        let env = account_env!(self);
        self.report.trades_dir_changed(&env);
        self.set_toast(format!("SIM: trades now save to {}", elide_path(&self.dir)));
    }

    /// Apply a raw simulator command through the normal event funnel
    /// (tests only) — for arranging sim state the UI would need gestures
    /// to reach.
    #[cfg(test)]
    pub(crate) fn apply_sim_command_for_tests(&mut self, command: Command) {
        let events = self.dispatch(command);
        self.handle_events(events);
    }

    /// The cmd-trading settings, for the app to persist.
    #[must_use]
    pub(crate) fn cmd_trading(&self) -> CmdTradingSettings {
        self.cmd_trading
    }

    /// Follow the tab's feed: a session is wholly live or wholly a
    /// replay, and the journal's header records which. A change retargets
    /// the journal so the next close opens a file honest about its
    /// source.
    pub(crate) fn set_session_source(&mut self, source: history::SessionSource) {
        if self.session_source != source {
            self.session_source = source;
            self.journal_path = None;
        }
    }

    /// Seed the mark from backfilled history — never fills (look-ahead).
    pub(crate) fn seed(&mut self, trade: &Trade) {
        self.observe_precision(trade);
        self.venue.seed(trade);
    }

    /// Fold one print's own precision into the instrument's, so the tick the
    /// ruler and the strategies step in is the smallest move the tape has
    /// shown rather than whatever the last print happened to look like.
    ///
    /// Per-trade path: one `normalize`, one comparison, no allocation.
    pub(crate) fn observe_precision(&mut self, trade: &Trade) {
        self.tick_scale = self.tick_scale.max(trade.price.normalize().scale());
    }

    /// Turn per-print event buffering for the strategy instances on or off.
    /// The tab flips it from whether any instance exists, so an idle chart
    /// never accumulates events nobody will drain.
    pub(crate) fn set_bot_listening(&mut self, listening: bool) {
        self.bot_listening = listening;
        if !listening {
            self.bot_events.clear();
        }
    }

    /// Everything the simulator reported on prints since the last drain,
    /// for the strategy instances to attribute by order id.
    #[must_use]
    pub(crate) fn drain_bot_events(&mut self) -> Vec<VenueEvent> {
        std::mem::take(&mut self.bot_events)
    }

    /// Whether the account is *clean* — the gate an armed strategy checks
    /// before firing. Not just "no position": a queued market entry or a
    /// resting order is a position about to exist, and two instances
    /// co-triggered by one bar must not both pass this gate and stack.
    /// A bot fires only into an account with no position, no resting
    /// orders and nothing queued — the human's included.
    #[must_use]
    pub(crate) fn is_flat(&self) -> bool {
        self.venue.position().is_none()
            && self.venue.working_orders().is_empty()
            && self.venue.in_flight() == 0
    }

    /// The last price the venue was shown, or `None` before the first
    /// print — what a caller with no chart in front of it needs before it
    /// can name a price at all.
    #[must_use]
    pub(crate) fn mark_price(&self) -> Option<Decimal> {
        self.venue.mark_price()
    }

    /// One step of the scripted demo: a fixed command sequence by print
    /// count — an entry, brackets, a partial, a flatten, a resting order,
    /// a short round trip — so every surface has something honest to show.
    pub(crate) fn run_demo_step(&mut self) {
        let Some(demo) = &mut self.demo else { return };
        demo.prints += 1;
        let prints = demo.prints;
        let Some(mark) = self.venue.mark_price() else {
            return;
        };
        // Scale-free distance: 0.2% of the mark, snapped to its precision.
        let offset = (mark * Decimal::new(2, 3)).round_dp(mark.scale());
        let has_position = self.venue.position().is_some();
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
                cancel_at: None,
                flat_only: false,
            },
            260 => Command::PlaceMarket {
                side: Side::Sell,
                quantity: Decimal::ONE,
                bracket: Bracket::none(),
            },
            340 if has_position => Command::ClosePosition,
            _ => return,
        };
        let events = self.dispatch(command);
        self.handle_events(events);
    }

    /// Whether the simulator has a price to trade against — the toolbar
    /// buttons disable themselves (with the reason) until this is true.
    #[must_use]
    pub(crate) fn ready(&self) -> bool {
        self.venue.mark_price().is_some()
    }

    /// The status-bar cell, honest about open versus flat: `SIM LONG 1 ·
    /// +2 pts` while a position is open (side, size and its open profit),
    /// `SIM +7 pts · flat` otherwise (the session's realized points). `None`
    /// while the simulator has never been touched.
    #[must_use]
    pub(crate) fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        if let Some(position) = self.venue.position() {
            let open = self
                .venue
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
        let untouched = self.venue.closed_trades().is_empty()
            && self.venue.working_orders().is_empty()
            && self.venue.in_flight() == 0;
        if untouched {
            return None;
        }
        let realized = self.venue.realized_points();
        Some((
            format!("SIM {} pts · flat", fmt_signed_points(realized)),
            realized.cmp(&Decimal::ZERO),
        ))
    }

    /// The open position as the chrome reports it: side, size, entry, and
    /// the open profit at the current mark. `None` while flat.
    #[must_use]
    pub(crate) fn position_summary(&self) -> Option<PositionSummary> {
        let position = self.venue.position()?;
        Some(PositionSummary {
            side: position.side,
            quantity: position.quantity,
            avg_price: position.avg_price,
            open_points: self
                .venue
                .mark_price()
                .map(|mark| position.open_points(mark)),
        })
    }

    /// Exit the open position at the next print — the toolbar's close
    /// button, the HUD's, and the Trading tab's all funnel here.
    pub(crate) fn close_position(&mut self) {
        let events = self.venue.close(CloseAmount::All);
        self.handle_events(events);
    }

    /// Close the position and cancel every pending order.
    pub fn flatten(&mut self) {
        let events = self.venue.flatten();
        self.handle_events(events);
    }

    /// `Close 1 LONG` while a position is open — the toolbar's exit button
    /// label. `None` while flat, which is what removes the button.
    #[must_use]
    pub(crate) fn close_button_label(&self) -> Option<String> {
        let position = self.venue.position()?;
        Some(format!(
            "Close {} {}",
            fmt_decimal(position.quantity),
            position_word(position.side),
        ))
    }

    /// This session's closed round trips, oldest first — the trades whose
    /// fills the current tape can prove, and so the only ones the chart
    /// paints marks for.
    #[must_use]
    pub(crate) fn session_trades(&self) -> &[ClosedTrade] {
        self.venue.closed_trades()
    }

    /// The resting entry orders, in placement order — the simulator's own
    /// view, read-only.
    #[must_use]
    pub(crate) fn working_orders(&self) -> &[quantick_sim::Order] {
        self.venue.working_orders()
    }

    /// Index (into [`Self::session_trades`]) of the ledger's selected
    /// trade, for the chart to emphasize; `None` while nothing is selected.
    #[must_use]
    pub(crate) fn selected_trade_index(&self) -> Option<usize> {
        self.report
            .selected_trade()
            .filter(|index| *index < self.venue.closed_trades().len())
    }

    /// The instrument this ticket is aimed at. Empty before the app names
    /// the opening symbol.
    #[must_use]
    pub(crate) fn symbol(&self) -> &str {
        &self.symbol
    }

    // ------------------------------------------------------------------
    // Events, journal, parsing
    // ------------------------------------------------------------------

    /// One funnel for everything the simulator reports: closures are
    /// journaled, fills and closures toast, rejections teach — and while a
    /// bot is listening, every batch is buffered for the armed instances
    /// too. Buffering *here* is what lets a manual flatten's `Cancelled`
    /// reach the instance whose pending entry it swept: manual commands
    /// and prints flow through this one funnel alike. The strategy-issued
    /// command path (`apply_strategy_command`) also lands here, so its
    /// instance sees its own acknowledgement twice — once directly, once
    /// via the buffer — which the state machine tolerates by design (every
    /// transition consumes its trigger, so a replayed event finds no match).
    pub(crate) fn handle_events(&mut self, events: Vec<VenueEvent>) {
        if self.bot_listening && !events.is_empty() {
            self.bot_events.extend(events.iter().cloned());
        }
        for event in events {
            match event {
                VenueEvent::Rejected(reason) => self.set_toast(format!("SIM: {reason}")),
                VenueEvent::BracketDropped { reason } => {
                    self.set_toast(format!("SIM: dropped at the fill - {reason}"));
                }
                VenueEvent::Filled(fill) => {
                    if matches!(fill.role, quantick_sim::FillRole::Entry(_)) {
                        self.set_toast(format!(
                            "SIM fill: {} {} @ {}",
                            side_word(fill.side),
                            fmt_decimal(fill.quantity),
                            fmt_decimal(fill.price),
                        ));
                    }
                }
                VenueEvent::Closed(trade) => {
                    let saved = self.journal(&trade);
                    // The report reads from disk and the close just wrote
                    // to disk; re-read now or the window shows yesterday
                    // until the manual refresh - the "my trade is missing"
                    // report.
                    //
                    // Guarded rather than always gathered: this is the
                    // per-trade path, and building a `ReportEnv` for a
                    // window nobody has open is work a dense tape pays on
                    // every single close.
                    if self.report.is_open() {
                        let env = account_env!(self);
                        self.report.journal_changed(&env);
                    }
                    // The toast slot holds one message: a healthy "closed"
                    // must not paint over the could-not-save warning.
                    if saved {
                        self.set_toast(format!(
                            // Same reason as the hover card above: the
                            // toast is a proportional-font label.
                            "SIM closed: {} {} for {} pts ({})",
                            position_word(trade.side),
                            fmt_decimal(trade.quantity),
                            fmt_signed_points(trade.pnl_points),
                            trade.exit_reason.as_str().replace('_', " "),
                        ));
                    }
                }
                // The two cancels the *tape* performs, not a hand: a
                // working-order chip vanishing with no narration reads as
                // a glitch, so these toast like every other simulator act
                // the trader did not click.
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::PriceTouched,
                } => {
                    self.set_toast(format!("SIM cancelled {}: target traded first", order.id));
                }
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::AccountOccupied,
                } => {
                    self.set_toast(format!(
                        "SIM stood down {}: account busy at its price",
                        order.id
                    ));
                }
                // A ladder's own bookkeeping, for the same reason: up to
                // eight chips can vanish at once when a part closes or the
                // position ends, and a trader watching them go needs the
                // sentence more than they needed it for a single leg.
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::OcoFilled,
                } => {
                    self.set_toast(format!("SIM cancelled {}: its pair filled", order.id));
                }
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::PositionClosed,
                } => {
                    self.set_toast(format!(
                        "SIM cancelled {}: the position it protected is closed",
                        order.id
                    ));
                }
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::BracketReplaced,
                } => {
                    self.set_toast(format!("SIM cancelled {}: protection replaced", order.id));
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn side_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

/// `YYYYMMDD-HHMMSS` in UTC from epoch milliseconds — session file names
/// derive from venue time, so the same replay run names the same file.
pub(crate) fn utc_compact(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(timestamp_ms);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A form with a quantity typed and no protective offsets — the state a
    /// ticket is in when the trader has touched nothing but the size box.
    fn plain_form() -> TicketForm {
        TicketForm {
            quantity: Ok(Decimal::ONE),
            offsets: Some((None, None)),
        }
    }

    fn plain_env() -> AccountEnv {
        AccountEnv {
            ruler_levels: None,
            form: plain_form(),
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

    /// An account with nothing but a scratch folder and a symbol: no ticket,
    /// no window, no drawing type. That this compiles and trades at all is
    /// the point of the split, and this module is where it is proved.
    fn account(dir: &crate::scratch::ScratchDir, symbol: &str) -> PaperAccount {
        let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
        account.set_symbol(symbol);
        account
    }

    /// A round trip driven entirely through the account, and journaled.
    ///
    /// The ticket is not constructed anywhere in this test. That is the whole
    /// claim of the extraction: the money path runs without one.
    #[test]
    fn the_account_trades_and_journals_without_a_ticket() {
        let dir = crate::scratch::ScratchDir::new("account-round-trip");
        let mut account = account(&dir, "ACCTX");

        account.seed(&print(0, 100));
        account.market(Side::Buy, Decimal::from(100), Bracket::none(), &plain_env());
        account.on_trade(&print(1, 100));
        let events = account.dispatch(Command::ClosePosition);
        account.handle_events(events);
        account.on_trade(&print(2, 105));

        assert!(account.is_flat(), "the position closed");
        assert_eq!(account.session_trades().len(), 1, "one round trip");
        assert_eq!(
            account.session_trades()[0].pnl_points,
            Decimal::from(5),
            "100 to 105 is five points"
        );

        let folder = dir.path().join("ACCTX");
        let files: Vec<_> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one session, one file");
        let text = std::fs::read_to_string(files[0].path()).expect("readable");
        assert!(
            text.contains("# symbol=ACCTX"),
            "the journal names the instrument: {text}"
        );
    }

    /// The risk lock refuses an oversized entry, and says so through the
    /// outbox rather than by reaching for a toast lane it does not own.
    #[test]
    fn the_lock_refuses_through_the_outbox() {
        let dir = crate::scratch::ScratchDir::new("account-risk-refusal");
        let mut account = account(&dir, "WIN$N");
        account.seed(&print(0, 100));

        // The lock measures money, so the instrument's has to be declared -
        // WIN$N's, twenty centavos a point.
        let mut book = crate::risk_sizing::InstrumentBook::new();
        book.insert(
            "WIN$N".to_owned(),
            quantick_sim::InstrumentMoney {
                point_value: Decimal::new(20, 2),
                size_step: Decimal::ONE,
                min_size: Decimal::ONE,
                max_size: None,
                currency: quantick_sim::Currency::new("BRL").expect("BRL"),
                source: quantick_sim::MoneySource::Declared,
            },
        );
        account.set_instrument_money(book);
        let mut risk = account.risk_settings().clone();
        risk.lock = true;
        risk.basis = crate::risk_sizing::RiskBasis::Amount;
        risk.amount = Decimal::from(1);
        account.set_risk_settings(risk);

        // 99 points of stop x 0.20 x 1000 contracts is far past a 1 BRL budget.
        let intent = OrderIntent::market(Side::Buy, Decimal::from(1000))
            .with_bracket(Bracket::whole(Some(Decimal::from(1)), None));
        let events = account.place_intent(intent);

        assert!(events.is_empty(), "nothing reached the venue");
        assert!(
            account.peek_toast().is_some(),
            "and the refusal is waiting in the outbox, not on a lane"
        );
    }

    /// Leaving an instrument is a switch; arriving at the first one is not.
    /// The caller needs the two told apart, because one forgets the ruler
    /// and the other must not.
    #[test]
    fn set_symbol_tells_arriving_apart_from_switching() {
        let dir = crate::scratch::ScratchDir::new("account-symbol");
        let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());

        assert_eq!(account.set_symbol("FIRST"), Some(true), "arriving");
        assert_eq!(account.set_symbol("SECOND"), Some(false), "a real switch");
        assert_eq!(account.set_symbol("SECOND"), None, "no change at all");
    }
}
