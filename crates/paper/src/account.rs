//! The paper-trading account: the money path, with no pixels in it.
//!
//! Everything here places, fills, protects, sizes and journals; nothing here
//! draws, reads a clock, opens a dialog or reads the environment. That is the
//! whole point of the crate boundary: the chart, the backtest and a bot drive
//! this one account, and an auditor asking whether a stop went on at the
//! right price reads this file and stops, rather than reading it through the
//! code that paints the stop.
//!
//! The account is *handed* what only its host can know (an [`AccountEnv`]:
//! what the trader typed, where the ruler stands) and *answers* with what it
//! cannot do itself (an [`AccountResponse`]: the acknowledgement to show, and
//! whether the journal just changed under a report that may be open).
//! Session files are named from venue time, never from a clock.

use std::path::{Path, PathBuf};

use quantick_engine::{Side, Trade};
use quantick_sim::{
    Bracket, CloseAmount, ClosedTrade, Command, OrderIntent, Simulator, TradingVenue, VenueEvent,
    history,
};
use rust_decimal::Decimal;

use crate::civil::civil_utc;
use crate::format::{PositionSummary, fmt_decimal, fmt_signed_points, position_word};
use crate::order_strategies::OrderStrategy;
use crate::risk_sizing::{Capital, InstrumentBook, RiskHook, RiskSettings};

mod journal;
mod orders;
mod risk;

pub use journal::{elide_path, export_csv};
pub use risk::RiskEditor;

/// What the account is handed that only the ticket can know.
///
/// Deliberately small. The account owns the venue, the journal folder, the
/// symbol and the instrument's precision, so none of those are here - it
/// would be handed back what it already knows. What it cannot learn is what
/// the trader typed and how far the ruler has been wound, and that is the
/// whole of this struct.
#[derive(Clone, Default)]
pub struct AccountEnv {
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
pub struct TicketForm {
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
    pub fn bracket(&self, side: Side, reference: Decimal) -> Bracket {
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
pub struct AccountResponse {
    /// The acknowledgement waiting to be shown, if there is one.
    ///
    /// One slot, and deliberately: the ticket's outbox always held one, so a
    /// healthy "closed" painting over a could-not-save warning is a decision
    /// this keeps exactly as it was. A queue here would change which message
    /// a trader sees.
    pub toast: Option<String>,
    /// A close was journaled since the host last asked.
    ///
    /// The report reads the journal from disk, so a host holding one open
    /// has to re-read it after a close or it shows yesterday until a manual
    /// refresh. The account cannot re-read what it does not hold; it says
    /// so here, and the host takes the flag with
    /// [`PaperAccount::take_journal_changed`].
    pub journal_changed: bool,
}

/// The scripted demo's only state: how many prints it has seen.
struct PaperDemo {
    prints: u64,
}

/// What a timeline reset did, for the host to narrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineReset {
    /// A position was open when the timeline was rebuilt under it.
    pub had_position: bool,
    /// Orders were resting or queued.
    pub had_orders: bool,
    /// Every close the reset forced reached the journal. `false` means a
    /// could-not-save warning is already waiting in the outbox, and a
    /// healthy acknowledgement must not paint over it.
    pub all_saved: bool,
}

/// Which side of a bracket a gesture is about.
///
/// The two legs are not symmetric — one caps the loss and one takes the
/// win — but every gesture that touches either does the same thing to it,
/// so they travel as one value rather than as a `bool` nobody can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    StopLoss,
    TakeProfit,
}
impl Leg {
    /// The two-letter word on the line's tag and on its handle.
    pub fn word(self) -> &'static str {
        match self {
            Self::StopLoss => "SL",
            Self::TakeProfit => "TP",
        }
    }

    /// The *other* leg's level within a bracket — the one the R:R read is
    /// measured against while this one is being dragged.
    pub fn other(self, bracket: Bracket) -> Option<Decimal> {
        match self {
            Self::StopLoss => bracket.take_profit(),
            Self::TakeProfit => bracket.stop_loss(),
        }
    }

    /// This leg's level within a bracket.
    pub fn level(self, bracket: Bracket) -> Option<Decimal> {
        match self {
            Self::StopLoss => bracket.stop_loss(),
            Self::TakeProfit => bracket.take_profit(),
        }
    }

    /// The bracket with this leg set to `level` (`None` clears it) and the
    /// other leg untouched — every amendment in this module goes through
    /// here, so "replace wholesale" can never accidentally drop the leg
    /// nobody was touching.
    pub fn applied(self, bracket: Bracket, level: Option<Decimal>) -> Bracket {
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
    pub fn sits_above_entry(self, side: Side) -> bool {
        matches!(
            (self, side),
            (Self::TakeProfit, Side::Buy) | (Self::StopLoss, Side::Sell)
        )
    }
}
/// The paper-trading account: everything the money path is made of,
/// and nothing the ticket draws.
pub struct PaperAccount {
    /// Where orders actually go. A [`TradingVenue`] rather than a
    /// `Simulator`, so the chart's gestures, the ticket and the
    /// control-plane actions are all written against the port a real
    /// broker will one day implement — see `quantick-trading`. Today the
    /// only venue constructed here is the deterministic paper simulator,
    /// and every surface still says `SIM`.
    venue: Box<dyn TradingVenue>,
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
    /// The named exit strategies the trader keeps, in their own order.
    strategies: Vec<OrderStrategy>,
    /// Which strategy the ticket is set to; `None` is the bare order the
    /// trader brackets by hand.
    selected_strategy: Option<usize>,
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
    tick_scale: u32,
    /// What one trade may lose, and whether an entry over it is refused.
    ///
    /// The policy half of risk sizing; the arithmetic belongs to the kernel.
    /// See [`crate::risk_sizing`].
    risk: RiskSettings,
    /// The practice capital, one amount per currency. Never summed across
    /// currencies and never converted between them.
    capital: Capital,
    /// What one point of each instrument is worth, by bare symbol. Declared
    /// by the trader; nothing here derives it from the tape.
    instrument_money: InstrumentBook,
    /// Money a launch hook asked for, waiting for the tab to learn which
    /// symbol it opens on. Spent once, on the first symbol.
    hook_money: Option<quantick_sim::InstrumentMoney>,
    /// Whether a launch hook set the risk per trade for this run.
    ///
    /// An environment variable is an explicit request for one run, so it
    /// outranks the sidecar - and the sidecar fan-out that follows
    /// construction has to be told, or it silently restores the stored
    /// settings over the ones the run asked for.
    risk_from_hook: bool,
    /// The scripted demo (`QUANTICK_PAPER_DEMO=1` in `app`), for screenshot
    /// and validation runs; `None` in normal use.
    demo: Option<PaperDemo>,
    /// Whether anything is listening for per-print simulator events (armed
    /// strategy instances). Off, `on_trade` buffers nothing — the hot path
    /// pays for the bot only while a bot exists.
    bot_listening: bool,
    /// Per-print events buffered for the strategy instances since the last
    /// drain. Only ever non-empty while `bot_listening`, and prints with
    /// nothing to report push nothing.
    bot_events: Vec<VenueEvent>,
    /// What this module asked its host to do. Drained by the ticket
    /// after every call; see [`AccountResponse`].
    outbox: AccountResponse,
}
impl PaperAccount {
    /// Take an entry at the market, protected by `ticket`.
    pub fn market(&mut self, side: Side, reference: Decimal, ticket: Bracket, env: &AccountEnv) {
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

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
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
    pub fn set_symbol(&mut self, symbol: &str) -> Option<bool> {
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
        Some(arriving)
    }

    /// An account journaling to `dir`, on the deterministic paper simulator,
    /// with no risk per trade standing and no scripted demo.
    ///
    /// The folder is resolved by the host — from its config, its environment
    /// or a folder picker — and handed over; this crate reads none of them.
    #[must_use]
    pub fn with_trades_dir(dir: PathBuf) -> Self {
        Self {
            venue: Box::new(Simulator::new()),
            symbol: String::new(),
            dir,
            journal_path: None,
            session_journal_paths: Vec::new(),
            session_trade_sources: Vec::new(),
            journal_warned: false,
            session_source: history::SessionSource::Live,
            strategies: Vec::new(),
            selected_strategy: None,
            tick_scale: 0,
            risk: RiskSettings::default(),
            capital: Capital::new(),
            instrument_money: InstrumentBook::new(),
            risk_from_hook: false,
            hook_money: None,
            demo: None,
            bot_listening: false,
            bot_events: Vec::new(),
            outbox: AccountResponse::default(),
        }
    }

    /// This account, with the risk per trade a launch hook asked for.
    ///
    /// An explicit request for one run, so it outranks whatever the host
    /// restores from its sidecar afterwards — [`Self::risk_from_hook`] is
    /// how the host knows not to. The hook's instrument money, if it named
    /// any, waits for the first symbol the host names.
    #[must_use]
    pub fn with_risk_hook(mut self, hook: RiskHook) -> Self {
        self.risk = hook.settings;
        self.capital = hook.capital;
        self.hook_money = hook.money;
        self.risk_from_hook = true;
        self
    }

    /// This account, running the scripted demo: a fixed sequence of ordinary
    /// simulator commands driven by print count (see [`Self::run_demo_step`]).
    #[must_use]
    pub fn with_demo(mut self) -> Self {
        self.demo = Some(PaperDemo { prints: 0 });
        self
    }

    /// Post an acknowledgement. An outbox, not a toast: this module owns no
    /// lane and no clock, and the message leaves through
    /// [`AccountResponse::toast`]. The ticket's own `show_toast` - the door
    /// `QUANTICK_TOAST=paper` knocks on - comes through here too, so there is
    /// one slot and one way into it.
    pub fn set_toast(&mut self, message: String) {
        self.outbox.toast = Some(message);
    }

    /// The acknowledgement waiting, if any, left where it is.
    ///
    /// Test support, published on purpose: a host's tests assert on what
    /// the account said without draining the slot the host's own frame
    /// loop drains. Production reads the outbox with [`Self::take_toast`].
    #[must_use]
    pub fn peek_toast(&self) -> Option<&String> {
        self.outbox.toast.as_ref()
    }

    /// Take the acknowledgement, if one is waiting.
    pub fn take_toast(&mut self) -> Option<String> {
        self.outbox.toast.take()
    }

    /// Whether a close was journaled since the last ask, clearing the flag.
    /// See [`AccountResponse::journal_changed`].
    pub fn take_journal_changed(&mut self) -> bool {
        std::mem::take(&mut self.outbox.journal_changed)
    }

    /// Point the journal at `dir` without acknowledging it or restarting the
    /// session file.
    ///
    /// Test support, published on purpose: a host's tests aim a freshly built
    /// account at a scratch folder, and an acknowledgement there would sit in
    /// the one toast slot the test is about to assert on. A trader's own pick
    /// goes through [`Self::set_trades_dir`], which says so.
    pub fn redirect_history_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }

    /// Where trades save right now.
    #[must_use]
    pub fn trades_dir(&self) -> &Path {
        &self.dir
    }

    /// The session file the next close appends to, once a close has named
    /// one; `None` before the first close and after every retarget.
    #[must_use]
    pub fn journal_path(&self) -> Option<&Path> {
        self.journal_path.as_deref()
    }

    /// Every file this session has journaled to — what a reader of the
    /// saved history excludes, because those trades are still in the venue.
    #[must_use]
    pub fn session_journal_paths(&self) -> &[PathBuf] {
        &self.session_journal_paths
    }

    /// Point the journal somewhere new, in-session — the panel's folder
    /// picker. Files already written stay exactly where they are; the next
    /// close opens a new session file under the new folder. Returns whether
    /// the folder changed, which is the host's cue to re-read any history
    /// it holds from the new home.
    pub fn set_trades_dir(&mut self, dir: PathBuf) -> bool {
        if self.dir == dir {
            return false;
        }
        self.dir = dir;
        self.journal_path = None;
        self.journal_warned = false;
        self.set_toast(format!("SIM: trades now save to {}", elide_path(&self.dir)));
        true
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

    /// Seed the mark from backfilled history — never fills (look-ahead).
    pub fn seed(&mut self, trade: &Trade) {
        self.observe_precision(trade);
        self.venue.seed(trade);
    }

    /// Fold one print's own precision into the instrument's, so the tick the
    /// ruler and the strategies step in is the smallest move the tape has
    /// shown rather than whatever the last print happened to look like.
    ///
    /// Per-trade path: one `normalize`, one comparison, no allocation.
    pub fn observe_precision(&mut self, trade: &Trade) {
        self.tick_scale = self.tick_scale.max(trade.price.normalize().scale());
    }

    /// Turn per-print event buffering for the strategy instances on or off.
    /// The tab flips it from whether any instance exists, so an idle chart
    /// never accumulates events nobody will drain.
    pub fn set_bot_listening(&mut self, listening: bool) {
        self.bot_listening = listening;
        if !listening {
            self.bot_events.clear();
        }
    }

    /// Everything the simulator reported on prints since the last drain,
    /// for the strategy instances to attribute by order id.
    #[must_use]
    pub fn drain_bot_events(&mut self) -> Vec<VenueEvent> {
        std::mem::take(&mut self.bot_events)
    }

    /// Whether the account is *clean* — the gate an armed strategy checks
    /// before firing. Not just "no position": a queued market entry or a
    /// resting order is a position about to exist, and two instances
    /// co-triggered by one bar must not both pass this gate and stack.
    /// A bot fires only into an account with no position, no resting
    /// orders and nothing queued — the human's included.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.venue.position().is_none()
            && self.venue.working_orders().is_empty()
            && self.venue.in_flight() == 0
    }

    /// The last price the venue was shown, or `None` before the first
    /// print — what a caller with no chart in front of it needs before it
    /// can name a price at all.
    #[must_use]
    pub fn mark_price(&self) -> Option<Decimal> {
        self.venue.mark_price()
    }

    /// One step of the scripted demo: a fixed command sequence by print
    /// count — an entry, brackets, a partial, a flatten, a resting order,
    /// a short round trip — so every surface has something honest to show.
    pub fn run_demo_step(&mut self) {
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
    pub fn ready(&self) -> bool {
        self.venue.mark_price().is_some()
    }

    /// The status-bar cell, honest about open versus flat: `SIM LONG 1 ·
    /// +2 pts` while a position is open (side, size and its open profit),
    /// `SIM +7 pts · flat` otherwise (the session's realized points). `None`
    /// while the simulator has never been touched.
    #[must_use]
    pub fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
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
    pub fn position_summary(&self) -> Option<PositionSummary> {
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
    pub fn close_position(&mut self) {
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
    pub fn close_button_label(&self) -> Option<String> {
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
    pub fn session_trades(&self) -> &[ClosedTrade] {
        self.venue.closed_trades()
    }

    /// The resting entry orders, in placement order — the simulator's own
    /// view, read-only.
    #[must_use]
    pub fn working_orders(&self) -> &[quantick_sim::Order] {
        self.venue.working_orders()
    }

    /// The instrument this account is aimed at. Empty before the host names
    /// the opening symbol.
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// The venue orders go to, read-only.
    #[must_use]
    pub fn venue(&self) -> &dyn TradingVenue {
        self.venue.as_ref()
    }

    /// The venue, for a caller that talks to it directly: a gesture that
    /// cancels or moves one order, or a harness that rests its own. What it
    /// answers must still come back through [`Self::handle_events`], or the
    /// journal, the acknowledgements and the bot buffer never hear of it.
    pub fn venue_mut(&mut self) -> &mut dyn TradingVenue {
        self.venue.as_mut()
    }

    /// The source rebuilt its timeline (replay seek, feed or symbol switch,
    /// restart): pending orders are swept and the position flattens at the
    /// last mark, labeled `reset` — never silently. Every forced close is
    /// journaled, the session file ends with the tape session, and the bot
    /// buffer is emptied so events from the torn-down timeline cannot leak
    /// into an instance's next life.
    pub fn reset_timeline(&mut self) -> TimelineReset {
        let had_position = self.venue.position().is_some();
        let had_orders = !self.venue.working_orders().is_empty() || self.venue.in_flight() > 0;
        let events = self.venue.reset();
        let mut all_saved = true;
        for event in &events {
            if let VenueEvent::Closed(trade) = event {
                all_saved &= self.journal(&trade.clone());
            }
        }
        // A reset ends the tape session, so it ends the file session too:
        // the next close opens a fresh file (same venue stamp lands as
        // `.rerun-N`). Without this, replaying the same recording again
        // without leaving replay appended run 2 into run 1's file.
        self.journal_path = None;
        self.bot_events.clear();
        TimelineReset {
            had_position,
            had_orders,
            all_saved,
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
    pub fn handle_events(&mut self, events: Vec<VenueEvent>) {
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
                    // to disk; the host must re-read now or its window shows
                    // yesterday until the manual refresh - the "my trade is
                    // missing" report. A flag and not the re-read itself:
                    // the report is the host's, and so is the decision to
                    // skip the work while no window is open.
                    self.outbox.journal_changed = true;
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

pub fn side_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

/// `YYYYMMDD-HHMMSS` in UTC from epoch milliseconds — session file names
/// derive from venue time, so the same replay run names the same file.
pub fn utc_compact(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(timestamp_ms);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

#[cfg(test)]
mod tests;
