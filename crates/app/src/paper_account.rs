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
    Bracket, BracketTarget, CloseAmount, ClosedTrade, Command, EntryKind, OcoId, OrderId,
    OrderIntent, OrderRole, Position, Simulator, TradingVenue, VenueEvent, history,
};
use rust_decimal::Decimal;

use crate::paper_calendar::{DaySelection, civil_utc};
use crate::paper_chrome::{
    PositionSummary, fmt_decimal, fmt_signed_points, position_word, sanitize_symbol,
};
use crate::paper_report::{HistoryRow, LedgerScope};
use crate::timezone::TzOffset;

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
    /// Test-only: the report state *and* the environment this host would
    /// hand it, together.
    ///
    /// Together deliberately. Every method on the state that reads the
    /// session takes the env, so a test needs both at once - and asking
    /// for them one at a time is exactly the borrow conflict `report_env!`
    /// exists to avoid. Destructuring is what makes the two borrows
    /// visibly disjoint.
    #[cfg(test)]
    pub(crate) fn report_parts(
        &mut self,
    ) -> (
        &mut crate::paper_report::ReportState,
        crate::paper_report::ReportEnv<'_>,
    ) {
        let open = self.open_row();
        let Self {
            report,
            symbol,
            dir,
            session_journal_paths,
            venue,
            ..
        } = self;
        (
            report,
            crate::paper_report::ReportEnv {
                symbol,
                dir,
                session_journal_paths,
                session_trades: venue.closed_trades(),
                open,
            },
        )
    }

    // ------------------------------------------------------------------
    // Risk, sizing and the bracket an entry carries
    //
    // The seam's reason for existing. Every one of these used to reach into
    // the ticket for the ruler's notch count and the three typed boxes; each
    // now takes an [`AccountEnv`] instead, and none of them can see a pixel.
    // ------------------------------------------------------------------

    /// The bracket an entry would carry at this size: the ruler first, then
    /// the armed strategy, then the ticket's own offsets.
    ///
    /// The ruler leads because rolling it is the most recent thing the trader
    /// did and it is the answer they are looking at. Rolling back to zero puts
    /// the armed ladder in front again, so neither gesture costs the other.
    pub(crate) fn aim_bracket(
        &self,
        side: Side,
        price: Decimal,
        quantity: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> Bracket {
        if let Some((stop, target)) = env.ruler_levels {
            return Bracket::whole(Some(stop), Some(target));
        }
        if let Some(strategy) = self.selected_order_strategy() {
            // A strategy edited into an invalid state falls through to the
            // plainer sources rather than blocking the trade; the ticket
            // names the reason beside the selector.
            if let Ok(bracket) = strategy.resolve(side, price, quantity, self.tick()) {
                return bracket;
            }
        }
        ticket
    }

    /// What the risk per trade makes of an entry, and the bracket it would
    /// rest with.
    pub(crate) fn risk_sized(
        &self,
        side: Side,
        reference: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> (crate::risk_sizing::RiskState, Bracket) {
        crate::risk_sizing::sized_for_aim(
            &crate::risk_sizing::RiskContext {
                settings: &self.risk,
                capital: &self.capital,
                book: &self.instrument_money,
                symbol: &self.symbol,
            },
            side,
            reference,
            &|quantity| self.aim_bracket(side, reference, quantity, ticket, env),
        )
    }

    /// What the risk per trade says about the entry the aim is holding.
    pub(crate) fn risk_state(
        &self,
        side: Side,
        reference: Decimal,
        env: &AccountEnv,
    ) -> crate::risk_sizing::RiskState {
        let ticket = env.form.bracket(side, reference);
        self.risk_sized(side, reference, ticket, env).0
    }

    /// The risk read, and whether it blocks an entry.
    pub(crate) fn risk_report(&self, env: &AccountEnv) -> (crate::risk_sizing::RiskState, bool) {
        let reference = self.mark_price().unwrap_or_default();
        let state = self.risk_state(Side::Buy, reference, env);
        let blocks = state.blocks_entry(self.risk.lock);
        (state, blocks)
    }

    /// The bracket an armed entry would carry, at this size.
    pub(crate) fn armed_bracket(
        &self,
        side: Side,
        reference: Decimal,
        quantity: Decimal,
        env: &AccountEnv,
    ) -> Bracket {
        let ticket = env.form.bracket(side, reference);
        self.aim_bracket(side, reference, quantity, ticket, env)
    }

    /// The size and bracket an entry takes, or nothing with the reason
    /// posted to the outbox.
    pub(crate) fn entry_size(
        &mut self,
        side: Side,
        reference: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> Option<(Decimal, Bracket)> {
        let (state, resting) = self.risk_sized(side, reference, ticket, env);
        if state.blocks_entry(self.risk.lock) {
            self.push_toast(format!("SIM: {}", state.sentence()));
            return None;
        }
        if let Some(quantity) = state.derived_quantity() {
            return Some((quantity, resting));
        }
        // Off, or nothing to size against: the typed quantity still rules,
        // and a quantity that does not parse complains as it always did -
        // in the ticket's own words, carried here by the form.
        match env.form.quantity.clone() {
            Ok(quantity) => Some((
                quantity,
                self.aim_bracket(side, reference, quantity, ticket, env),
            )),
            Err(complaint) => {
                self.push_toast(complaint);
                None
            }
        }
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

    /// Post an acknowledgement for the host to hand on. An outbox, not a
    /// toast: this module owns no lane and no clock, and the message leaves
    /// through [`AccountResponse::toast`].
    fn push_toast(&mut self, message: String) {
        self.outbox.toast = Some(message);
    }

    /// Post an acknowledgement on the host's behalf. The ticket's own
    /// `show_toast` - the door `QUANTICK_TOAST=paper` knocks on - comes
    /// through here, so there is one slot and not two.
    pub(crate) fn set_toast(&mut self, message: String) {
        self.outbox.toast = Some(message);
    }

    /// Test-only: whether an acknowledgement is waiting. A question rather
    /// than a field, which is what keeps the outbox private now that the
    /// ticket no longer owns one.
    #[cfg(test)]
    pub(crate) fn has_toast(&self) -> bool {
        self.outbox.toast.is_some()
    }

    /// Test-only: the acknowledgement waiting, if any.
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
        self.push_toast(format!("SIM: trades now save to {}", elide_path(&self.dir)));
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

    /// Apply a strategy-issued command through the same funnel manual
    /// orders use — journal, toasts, everything — and hand the simulator's
    /// immediate answer back for the instance to attribute.
    pub(crate) fn apply_strategy_command(&mut self, command: Command) -> Vec<VenueEvent> {
        let events = self.dispatch(command);
        self.handle_events(events.clone());
        events
    }

    /// The last price the venue was shown, or `None` before the first
    /// print — what a caller with no chart in front of it needs before it
    /// can name a price at all.
    #[must_use]
    pub(crate) fn mark_price(&self) -> Option<Decimal> {
        self.venue.mark_price()
    }

    /// Place one order, stated in full — the control plane's entry point,
    /// and the shape a hotkey or a hook uses too.
    ///
    /// Unlike the chart's aim this takes the kind rather than inferring it:
    /// a caller with no pointer has no "where I am relative to the mark" to
    /// infer from, and an action whose meaning depends on the market at the
    /// instant it lands is an action nobody can replay.
    pub(crate) fn place_intent(&mut self, intent: OrderIntent) -> Vec<VenueEvent> {
        // The risk per trade is a ceiling on the account, not on the mouse.
        // A named call is exactly the operator `CLAUDE.md` treats as
        // first-class, so a lock the ticket enforces and this path does not
        // would be a ceiling that holds only while a human is clicking.
        if let Some(refusal) = self.risk_refusal_for(&intent) {
            // Nothing is fabricated onto the venue's own event stream: the
            // lock is this application's policy, not a fact the venue
            // reported, and a `RejectReason` variant for it would put that
            // policy inside the domain crate. The trader gets the toast; a
            // named caller gets the same sentence as an error, because
            // `control::trade` asks this same function first.
            self.push_toast(format!("SIM: {refusal}"));
            return Vec::new();
        }
        let events = self.venue.submit(intent);
        self.handle_events(events.clone());
        events
    }

    /// Why the lock refuses this intent, when it does.
    ///
    /// Reads the intent's *own* protection and quantity rather than the
    /// ticket's: a named call states what it wants, and the ceiling has to
    /// be measured against what was actually asked for.
    pub(crate) fn risk_refusal_for(&self, intent: &OrderIntent) -> Option<String> {
        if !self.risk.lock || self.risk.basis == crate::risk_sizing::RiskBasis::Off {
            return None;
        }
        let reference = intent
            .price
            .or_else(|| self.venue.mark_price())
            .unwrap_or_default();
        let risk = crate::risk_sizing::risk_of(
            &self.instrument_money,
            &self.symbol,
            intent.side,
            reference,
            &intent.bracket,
            intent.quantity,
        )?;
        let budget = crate::risk_sizing::budget_for(
            &self.risk,
            &self.capital,
            &self.instrument_money.get(&self.symbol)?.currency,
        )
        .ok()?;
        (risk.amount > budget.amount).then(|| {
            format!(
                "this order risks {} {} - over your {} {} risk per trade. Raise the risk, or                  turn the lock off.",
                risk.amount.normalize(),
                risk.currency.code(),
                budget.amount.normalize(),
                budget.currency.code(),
            )
        })
    }

    /// Replace a working order's protective prices — the chart's drag, said
    /// in words.
    pub(crate) fn set_order_bracket(&mut self, id: OrderId, bracket: Bracket) -> Vec<VenueEvent> {
        let events = self.venue.amend_bracket(BracketTarget::Order(id), bracket);
        self.handle_events(events.clone());
        events
    }

    /// Remove one working order without trading.
    pub(crate) fn cancel_order(&mut self, id: OrderId) -> Vec<VenueEvent> {
        let events = self.venue.cancel(id);
        self.handle_events(events.clone());
        events
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

    /// Hand one [`Command`] to the attached venue.
    ///
    /// The chart's own gestures build [`OrderIntent`]s and call the port
    /// directly; this exists for the callers that already speak `Command`
    /// — the strategy kernel, the scripted demo, and the tests that drive
    /// this host the way the kernel does.
    pub(crate) fn dispatch(&mut self, command: Command) -> Vec<VenueEvent> {
        command.dispatch(self.venue.as_mut())
    }

    /// What a bracket owner looks like to every gesture in this module:
    /// the side it trades, the price its legs are judged against, the
    /// bracket it carries today, and the size that turns a level into
    /// points.
    ///
    /// The position's reference is its average entry; a working order's is
    /// its own resting price. That is the same reference the venue
    /// validates against, so a leg the chart lets you drop is a leg the
    /// venue accepts — the two never disagree about which side of the
    /// entry is protective.
    pub(crate) fn bracket_owner(
        &self,
        owner: BracketTarget,
    ) -> Option<(Side, Decimal, Bracket, Decimal)> {
        match owner {
            BracketTarget::Position => self.venue.position().map(|position| {
                (
                    position.side,
                    position.avg_price,
                    self.position_bracket(position),
                    position.quantity,
                )
            }),
            BracketTarget::Order(id) => self
                .venue
                .working_orders()
                .iter()
                .find(|order| order.id == id)
                .and_then(|order| {
                    order
                        .price
                        .map(|price| (order.side, price, order.bracket, order.quantity))
                }),
        }
    }

    /// Set or clear one protective leg, keeping the other — the tag cross's
    /// command and the drop of a leg drag, which are the same amendment.
    pub(crate) fn amend_leg(&mut self, owner: BracketTarget, leg: Leg, level: Option<Decimal>) {
        let Some((.., bracket, _)) = self.bracket_owner(owner) else {
            return;
        };
        let events = self.venue.amend_bracket(owner, leg.applied(bracket, level));
        self.handle_events(events);
    }

    /// Everything that can carry a bracket right now, in the order a press
    /// should consider it — which is **the reverse of the paint**, topmost
    /// first.
    ///
    /// `draw_layer` paints the working orders and then the position, so the
    /// position's lines and tag sit on top of them. A press has to resolve
    /// the same way or the two disagree wherever they overlap: a position
    /// stopped at 90 and a resting entry whose own stop is also 90 show the
    /// position's solid leg, and a hit-test that reached the order first
    /// would clear the entry's protection while the trader was looking at
    /// the position's.
    ///
    /// An iterator and not a `Vec`, because both callers run **per frame**,
    /// not per press: `hover_cursor` asks `control_at` and `line_at` what is
    /// under the pointer on every frame the hand is over the chart, and
    /// `compute_cmd_preview` asks both again. A vector here was four small
    /// allocations a frame for a list that is usually empty. Everything it
    /// borrows is borrowed immutably, so a caller can walk it while asking
    /// `self` about each entry.
    pub(crate) fn bracket_owners(&self) -> impl Iterator<Item = BracketTarget> + '_ {
        self.venue
            .position()
            .is_some()
            .then_some(BracketTarget::Position)
            .into_iter()
            .chain(
                self.venue
                    .working_orders()
                    .iter()
                    .rev()
                    // A protective leg is not an entry: it *is* protection,
                    // and offering it a bracket of its own would put SL/TP
                    // handles beside a rung the trader is already using as
                    // one. The venue refuses such an amendment anyway; the
                    // chart must not offer the gesture that earns a refusal.
                    .filter(|order| !order.is_protective())
                    .map(|order| BracketTarget::Order(order.id)),
            )
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
    /// One tick of the instrument: the smallest price move its own prints
    /// can express. Read from the mark the same way [`Self::snap`] reads it,
    /// so the ruler steps in exactly the units a drag would land on.
    #[must_use]
    pub(crate) fn tick_size(&self) -> Decimal {
        self.tick()
    }

    pub(crate) fn tick(&self) -> Decimal {
        let places = if self.venue.mark_price().is_some() {
            self.tick_scale
        } else {
            SNAP_FALLBACK_DECIMALS
        };
        Decimal::new(1, places)
    }

    /// The step an instrument gets before anyone names one.
    ///
    /// Half a basis point of the mark, rounded *up* the 1-2-5 ladder to a
    /// whole number of ticks and never below one. Volatility scales with
    /// price, so the same fraction gives a wheel that feels the same on an
    /// instrument quoted at 78,000 and one quoted at 5.
    #[must_use]
    pub(crate) fn derived_ruler_step(&self) -> Decimal {
        let tick = self.tick();
        let Some(mark) = self.venue.mark_price() else {
            return tick;
        };
        let wanted = mark.saturating_mul(RULER_DEFAULT_STEP_FRACTION);
        if wanted <= tick {
            return tick;
        }
        // Climb the 1-2-5 ladder in units of a tick until it covers `wanted`,
        // so the step is always a whole number of ticks a price can land on.
        let mut multiple = Decimal::ONE;
        loop {
            for rung in [Decimal::ONE, Decimal::TWO, Decimal::from(5)] {
                let step = tick.saturating_mul(multiple).saturating_mul(rung);
                if step >= wanted {
                    return step;
                }
            }
            multiple = multiple.saturating_mul(Decimal::TEN);
            if multiple > Decimal::from(1_000_000) {
                return wanted.round_dp(tick.scale());
            }
        }
    }

    /// What actually guards the open position, whatever shape it is in.
    ///
    /// The position's own `stop_loss`/`take_profit` answer only the plain
    /// pair; under a ladder they are `None` and the working legs carry the
    /// truth. Reading the pair alone left a laddered position drawn as if it
    /// had no protection at all *and* offering the create-handles of an
    /// unprotected one - and one drag on those replaces the whole ladder
    /// with a single level. Folding the legs back into a bracket here means
    /// every surface downstream sees the same shape for a position that it
    /// already sees for an order.
    ///
    /// Grouped by OCO id, which is what a rung *is*, and walked in placement
    /// order so the rungs read in the order the trader wrote them.
    pub(crate) fn position_bracket(&self, position: &Position) -> Bracket {
        let mut parts: Vec<(OcoId, quantick_sim::ExitPart)> = Vec::new();
        for leg in self
            .venue
            .working_orders()
            .iter()
            .filter(|order| order.is_protective())
        {
            let (Some(level), Some(oco)) = (leg.price, leg.oco) else {
                continue;
            };
            let part = match parts.iter_mut().find(|(id, _)| *id == oco) {
                Some((_, part)) => part,
                None => {
                    parts.push((
                        oco,
                        quantick_sim::ExitPart {
                            quantity: Some(leg.quantity),
                            stop_loss: None,
                            take_profit: None,
                        },
                    ));
                    &mut parts.last_mut().expect("just pushed").1
                }
            };
            match leg.role {
                OrderRole::StopLoss => part.stop_loss = Some(level),
                OrderRole::TakeProfit => part.take_profit = Some(level),
                OrderRole::Entry => {}
            }
        }
        if parts.is_empty() {
            return Bracket::whole(position.stop_loss, position.take_profit);
        }
        let rungs: Vec<quantick_sim::ExitPart> = parts.into_iter().map(|(_, part)| part).collect();
        Bracket::ladder(&rungs)
            .unwrap_or_else(|_| Bracket::whole(position.stop_loss, position.take_profit))
    }

    /// Set one rung's leg on a resting entry, or clear it with `None`.
    ///
    /// Every other rung is carried through untouched, and the named
    /// strategy is never written to: an order on the chart is the trader's,
    /// and the ladder that shaped it is a template they can still reuse.
    /// A rung left protecting nothing is dropped rather than rested empty.
    pub(crate) fn amend_rung(
        &mut self,
        order: OrderId,
        index: usize,
        leg: Leg,
        level: Option<Decimal>,
    ) {
        let Some(entry) = self
            .venue
            .working_orders()
            .iter()
            .find(|working| working.id == order)
        else {
            return;
        };
        let mut parts: Vec<quantick_sim::ExitPart> = entry.bracket.parts().copied().collect();
        let Some(part) = parts.get_mut(index) else {
            return;
        };
        match leg {
            Leg::StopLoss => part.stop_loss = level,
            Leg::TakeProfit => part.take_profit = level,
        }
        parts.retain(|part| !part.is_empty());
        let Ok(bracket) = Bracket::ladder(&parts) else {
            return;
        };
        let events = self
            .venue
            .amend_bracket(BracketTarget::Order(order), bracket);
        self.handle_events(events);
    }

    /// The instrument this ticket is aimed at. Empty before the app names
    /// the opening symbol.
    #[must_use]
    pub(crate) fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Whether a launch hook owns the risk per trade for this run, in which
    /// case the stored settings must not be fanned back over it.
    #[must_use]
    pub(crate) fn risk_from_hook(&self) -> bool {
        self.risk_from_hook
    }

    /// What one trade may lose, and whether the lock stands.
    pub(crate) fn risk_settings(&self) -> &crate::risk_sizing::RiskSettings {
        &self.risk
    }

    /// Replace the risk per trade. App-wide, like the ticket's other
    /// settings: a ceiling a trader sets in one tab is one they mean in all.
    pub(crate) fn set_risk_settings(&mut self, risk: crate::risk_sizing::RiskSettings) {
        self.risk = risk;
    }

    /// The declared practice capital, one amount per currency.
    pub(crate) fn capital(&self) -> &crate::risk_sizing::Capital {
        &self.capital
    }

    /// Replace the declared capital.
    pub(crate) fn set_capital(&mut self, capital: crate::risk_sizing::Capital) {
        self.capital = capital;
    }

    /// What one point of each instrument is worth, by bare symbol.
    pub(crate) fn instrument_money(&self) -> &crate::risk_sizing::InstrumentBook {
        &self.instrument_money
    }

    /// Replace the declared instrument money.
    pub(crate) fn set_instrument_money(&mut self, book: crate::risk_sizing::InstrumentBook) {
        self.instrument_money = book;
    }

    /// The strategies the trader keeps, in their own order.
    pub(crate) fn order_strategies(&self) -> &[crate::order_strategies::OrderStrategy] {
        &self.strategies
    }

    /// The strategy the ticket is set to, if any.
    pub(crate) fn selected_order_strategy(
        &self,
    ) -> Option<&crate::order_strategies::OrderStrategy> {
        self.strategies.get(self.selected_strategy?)
    }

    /// Replace the kept strategies and the selection, by name.
    ///
    /// A name this build no longer knows selects nothing: telling the trader
    /// their strategy is gone beats silently arming a different one.
    pub(crate) fn set_order_strategies(
        &mut self,
        strategies: Vec<crate::order_strategies::OrderStrategy>,
        selected: Option<&str>,
    ) {
        self.selected_strategy =
            selected.and_then(|name| strategies.iter().position(|item| item.name == name));
        self.strategies = strategies;
    }

    /// Round a pointer price to the precision the tape itself uses (the
    /// mark's decimal places), so a dragged line lands on a price the
    /// instrument can actually print.
    pub(crate) fn snap(&self, price: f64) -> Decimal {
        // The instrument's own precision, learned from the tape - not the
        // raw scale of the last print. A venue that quotes
        // `79172.37000000` has a raw scale of eight, and snapping to it put
        // eight decimals on every level this layer draws: `SL 79026.38465256`
        // where the instrument trades in cents. The tick is the same one the
        // ruler and the ladders step in, so every number on this surface
        // rounds the same way.
        let places = if self.venue.mark_price().is_some() {
            self.tick_scale
        } else {
            SNAP_FALLBACK_DECIMALS
        };
        Decimal::from_f64_retain(price)
            .unwrap_or_default()
            .round_dp(places)
            .normalize()
    }

    /// The size one stepper press moves, for the hover that promises it.
    pub(crate) fn quantity_step_hint(&self, notches: Decimal) -> String {
        let unit = self
            .instrument_money
            .get(&self.symbol)
            .map_or(Decimal::ONE, |money| money.size_step);
        fmt_decimal(notches.saturating_mul(unit))
    }

    /// Cancel every working order (resting and queued), trading nothing.
    pub(crate) fn cancel_all_orders(&mut self) {
        let mut ids: Vec<OrderId> = self
            .venue
            .working_orders()
            .iter()
            .map(|order| order.id)
            .collect();
        self.venue.in_flight_entries(&mut ids);
        for id in ids {
            let events = self.venue.cancel(id);
            self.handle_events(events);
        }
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

    /// Test-only: the report state this host holds.
    ///
    /// The report's own tests moved out with the report, and a handful of
    /// them still need a host that journals to a real folder, because they
    /// are about the journal rather than the arithmetic. Reading, not
    /// reaching: the state's fields stay private to its own module, so a
    /// test in `paper_report` gets at them exactly the way that module
    /// does.
    #[cfg(test)]
    pub(crate) fn report_state(&self) -> &crate::paper_report::ReportState {
        &self.report
    }

    /// Test-only: the report state, mutably.
    #[cfg(test)]
    pub(crate) fn report_state_mut(&mut self) -> &mut crate::paper_report::ReportState {
        &mut self.report
    }

    /// The open position as the ledger's top row needs it, or `None` while
    /// flat. Three venue reads gathered into one value so the reader can
    /// never be handed two of the three.
    pub(crate) fn open_row(&self) -> Option<crate::paper_report::OpenRow> {
        let summary = self.position_summary()?;
        let held_ms = self
            .venue
            .mark_timestamp_ms()
            .zip(self.venue.position().map(|position| position.opened_ms))
            .map(|(mark, opened)| mark.saturating_sub(opened));
        Some(crate::paper_report::OpenRow {
            summary,
            mark_price: self.venue.mark_price(),
            held_ms,
        })
    }

    /// Open the report window (`QUANTICK_PAPER_REPORT_AUTOSTART`).
    pub(crate) fn autostart_report(&mut self) {
        let env = account_env!(self);
        self.report.autostart_report(&env);
    }

    /// Open the report with its month grid expanded (`QUANTICK_PAPER_CALENDAR`).
    pub(crate) fn autostart_calendar(&mut self, selection: DaySelection) {
        let env = account_env!(self);
        self.report.autostart_calendar(selection, &env);
    }

    /// Point the ledger at one instrument's saved history, or all of them.
    pub(crate) fn set_ledger_scope(&mut self, scope: LedgerScope) {
        self.report.set_ledger_scope(scope);
    }

    /// Fold every day in the ledger shut (`QUANTICK_LEDGER_FOLD`).
    pub(crate) fn autostart_folded_days(&mut self, tz: TzOffset) {
        let env = account_env!(self);
        self.report.autostart_folded_days(tz, &env);
    }

    /// Reveal `pages` pages of saved history (`QUANTICK_LEDGER_PAGES`).
    pub(crate) fn autostart_ledger_pages(&mut self, pages: usize) {
        self.report.autostart_ledger_pages(pages);
    }

    /// Open or collapse the report's trade list (`QUANTICK_PAPER_REPORT_LIST`).
    pub(crate) fn set_report_list_open(&mut self, open: bool) {
        self.report.set_report_list_open(open);
    }

    // ------------------------------------------------------------------
    // Import
    // ------------------------------------------------------------------

    /// Ask for a folder whose trades should be copied into the journal
    /// home, off the UI thread — the manual way in for legacy folders the
    /// startup consolidation cannot reach (an old working directory, a
    /// backup). One dialog at a time.
    pub(crate) fn start_import(&mut self) {
        if self.import_rx.is_some() {
            self.push_toast("SIM: an import is already running.".to_owned());
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
    pub(crate) fn poll_import(&mut self) {
        let Some(receiver) = &self.import_rx else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.import_rx = None;
        let Some(source) = choice else { return };
        let summary = crate::paper_home::consolidate_into(&self.dir, &[source]);
        self.push_toast(crate::paper_home::import_toast(&summary));
        let env = account_env!(self);
        self.report.history_imported(&env);
    }

    // ------------------------------------------------------------------
    // Export
    // ------------------------------------------------------------------

    /// Write everything the ledger lists (this session plus the saved
    /// history, in the ledger's scope) to one CSV, off the UI thread. The
    /// toast answers with the path or the failure.
    pub(crate) fn start_export(&mut self) {
        if self.export_rx.is_some() {
            self.push_toast("SIM: an export is already running.".to_owned());
            return;
        }
        // The saved half of the export, loaded if the ledger has not been
        // drawn yet: an export that silently skipped it because nobody had
        // opened the tab would write a history that is missing most of
        // itself. Cloned out before the session's own rows are appended,
        // since gathering those borrows this host again.
        let mut rows: Vec<HistoryRow> = {
            let env = account_env!(self);
            self.report.saved_rows(&env).to_vec()
        };
        rows.extend(
            self.venue
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
            self.push_toast("SIM: nothing to export yet - close a trade first.".to_owned());
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
    pub(crate) fn poll_export(&mut self) {
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
                self.push_toast(format!("Exported {count} trades to {}", elide_path(&path)));
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
                self.push_toast(
                    "SIM: could not write the export - see the log for the path.".to_owned(),
                );
            }
        }
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
                VenueEvent::Rejected(reason) => self.push_toast(format!("SIM: {reason}")),
                VenueEvent::BracketDropped { reason } => {
                    self.push_toast(format!("SIM: dropped at the fill - {reason}"));
                }
                VenueEvent::Filled(fill) => {
                    if matches!(fill.role, quantick_sim::FillRole::Entry(_)) {
                        self.push_toast(format!(
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
                        self.push_toast(format!(
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
                    self.push_toast(format!("SIM cancelled {}: target traded first", order.id));
                }
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::AccountOccupied,
                } => {
                    self.push_toast(format!(
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
                    self.push_toast(format!("SIM cancelled {}: its pair filled", order.id));
                }
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::PositionClosed,
                } => {
                    self.push_toast(format!(
                        "SIM cancelled {}: the position it protected is closed",
                        order.id
                    ));
                }
                VenueEvent::Cancelled {
                    order,
                    reason: quantick_sim::CancelReason::BracketReplaced,
                } => {
                    self.push_toast(format!("SIM cancelled {}: protection replaced", order.id));
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
    pub(crate) fn journal(&mut self, trade: &ClosedTrade) -> bool {
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
            self.push_toast(
                "SIM: could not save the trade history - see the log for the path.".to_owned(),
            );
        }
        written.is_ok()
    }
}

/// A toast-sized path: printed whole when short, elided to `…/file.csv`
/// past the limit — the file name always stays whole.
pub(crate) fn elide_path(path: &Path) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= EXPORT_PATH_ELIDE_CHARS {
        return text;
    }
    path.file_name().map_or(text, |name| {
        format!("…{}{}", std::path::MAIN_SEPARATOR, name.to_string_lossy())
    })
}

/// The export CSV: one merged, Excel-facing artifact — the journal stays
/// the machine-readable source of truth. Human-readable UTC stamps ride
/// beside the venue epoch, decimals always use `.`, and the running
/// equity is a column so a spreadsheet shows it without a formula.
pub(crate) fn export_csv(rows: &[HistoryRow]) -> String {
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

/// The session file for `stamp` under `folder`: the plain name when free,
/// else `stamp.rerun-N` — file names derive from venue time, so replaying
/// the same recording twice reproduces the same stamp, and the second run
/// must land beside the first instead of appending duplicate trades into
/// it.
pub(crate) fn free_session_path(folder: &Path, stamp: &str) -> PathBuf {
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

/// The step a notch walks when the trader has named none, as a fraction of
/// the mark.
///
/// Half a basis point: volatility scales with price, so one fraction serves
/// an instrument quoted at 78,000 and one quoted at 5. It is rounded up the
/// 1-2-5 ladder to a whole number of ticks, which lands on 5 points for
/// BTCUSDT near 78,000 and 10 for the mini index near 138,000 — in both
/// cases a twenty-to-forty point read is four to eight rolls away.
const RULER_DEFAULT_STEP_FRACTION: Decimal = Decimal::from_parts(5, 0, 0, false, 5);

/// Price precision for snapped drags before any print reveals the
/// instrument's own (two decimals, the crypto-major default).
const SNAP_FALLBACK_DECIMALS: u32 = 2;

/// How many `.rerun-N` session names one venue-time stamp may try — far
/// beyond any real journal, a backstop against a pathological folder.
const MAX_SESSION_RERUNS: usize = 999;

/// Longest export path a toast prints whole; past it the folder elides
/// and the file name stays.
const EXPORT_PATH_ELIDE_CHARS: usize = 64;

/// `YYYY-MM-DDTHH:MM:SSZ` in UTC — the export's human-readable stamp.
fn fmt_utc_iso(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}
