//! Where orders go: the simulator, or the broker account the MetaTrader
//! terminal is logged into, demo or real.
//!
//! One global route, decided here and nowhere else, with no clock and no
//! window: the host hands over what it reads ([`RouteFacts`]) and the time,
//! and carries out the [`RouteChange`] it gets back. Every rule a trader or
//! an agent can run into is a test of this module.
//!
//! - **The simulator is where every run starts.** A broker route is never
//!   remembered: [`AccountRoute::new`] is the restart.
//! - **DEMO is one action, REAL is a held gesture.** A REAL account arms only
//!   after its row is held for [`HOLD_TO_ARM_MS`]; releasing earlier cancels,
//!   and so does a frame whose painted row did not report the press still
//!   held. No named call can stand in for the hold.
//! - **An agent never arms.** The control plane may take the account back to
//!   the simulator - leaving the broker's exposure where it is, unless the
//!   config allows agent orders; arming DEMO or REAL from it is refused with
//!   a code, and its broker orders are refused unless the config allows them
//!   and the route is armed. What the trader left at the broker answers the
//!   trader's hand alone.
//! - **Leaving never hides money.** Going back to the simulator with a
//!   position or orders at the broker asks first: flatten them, or leave them
//!   there (their plates stay on the chart).
//! - **Anything that makes the route unsafe disarms it**, with the reason: the
//!   link lost, the terminal logged into another account or refused, broker
//!   routes switched off, or a chart whose source cannot route orders.

use quantick_trading::{AccountFacts, TradeMode, VenueNotice};

mod desk;
mod scene;
mod session;
mod sheet;

#[cfg(test)]
mod tests;

pub use desk::{AccountsPort, BrokerDesk, RoutePress, SourceFacts};
pub use scene::Scene;
pub use session::{BrokerSession, RouteCall, RouteHost, RouteRead, Staged};
pub use sheet::{
    AccountRow, Dot, LEAVE_CHOICES, LEAVE_PROMPT, NOTICE_LEAD, RouteNotice, RouteTone, RowGesture,
    RowState, SIM_TITLE, Sheet, dock_title,
};

/// How long a REAL row is held before it arms, in milliseconds.
pub const HOLD_TO_ARM_MS: i64 = 1_000;

/// Where orders go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    /// The deterministic paper simulator, filling from the tape.
    #[default]
    Sim,
    /// The broker account the terminal reports as demo.
    Demo,
    /// The broker account the terminal reports as real money.
    Real,
}

impl Route {
    /// The word every label leads with: `SIM`, `DEMO`, `REAL`.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Sim => "SIM",
            Self::Demo => "DEMO",
            Self::Real => "REAL",
        }
    }

    /// Whether orders on this route reach a broker.
    #[must_use]
    pub const fn is_broker(self) -> bool {
        !matches!(self, Self::Sim)
    }

    /// The broker route a terminal's trade mode allows: anything that may be
    /// real money is REAL.
    #[must_use]
    pub fn of_mode(mode: TradeMode) -> Self {
        if mode.is_real_money() {
            Self::Real
        } else {
            Self::Demo
        }
    }

    /// The stable wire name (`sim`, `demo`, `real`).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Sim => "sim",
            Self::Demo => "demo",
            Self::Real => "real",
        }
    }
}

/// Which account a terminal is logged into: its login and a fingerprint of
/// its server. Two REAL accounts share a route word; they never share this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AccountKey {
    pub login: u64,
    server: u64,
}

impl AccountKey {
    /// The key of `facts`. The server is folded with FNV-1a: fixed, so the
    /// same account always has the same key.
    #[must_use]
    pub fn of(facts: &AccountFacts) -> Self {
        let server = facts
            .server
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        Self {
            login: facts.login,
            server,
        }
    }
}

/// Who asks for a route change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// The trader's own hand on the window.
    Trader,
    /// Anything else: the control plane, an MCP client, an automation.
    Agent,
}

/// What the host reads before asking: the config, the chart's source and the
/// broker's own state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RouteFacts {
    /// `[broker.metatrader] enabled`.
    pub broker_enabled: bool,
    /// The active chart's source can route orders (`FeedCapabilities`).
    pub source_routes: bool,
    /// The broker route the terminal's account allows, once it has said.
    pub account: Option<Route>,
    /// Which account that is: a route armed on one never stays armed on
    /// another, of the same kind or not.
    pub account_key: Option<AccountKey>,
    /// The link to the broker is up.
    pub link_up: bool,
    /// A position or working orders sit at the broker.
    pub exposure: bool,
}

/// Why a route change was refused. Each has a stable code for the control
/// plane and the sentence a trader reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    BrokerOff,
    SourceCannotRoute,
    NoAccount,
    LinkDown,
    NotThisAccount,
    AgentCannotArm,
    RealNeedsHold,
    ExposureNeedsChoice,
    AgentOrdersOff,
    ParkedAtBroker,
}

impl Refusal {
    /// The stable code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::BrokerOff => "route.broker_off",
            Self::SourceCannotRoute => "route.source_cannot_route",
            Self::NoAccount => "route.no_account",
            Self::LinkDown => "route.link_down",
            Self::NotThisAccount => "route.not_this_account",
            Self::AgentCannotArm => "route.agent_cannot_arm",
            Self::RealNeedsHold => "route.real_needs_hold",
            Self::ExposureNeedsChoice => "route.exposure_needs_choice",
            Self::AgentOrdersOff => "route.agent_orders_off",
            Self::ParkedAtBroker => "route.parked_at_broker",
        }
    }

    /// The sentence.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Self::BrokerOff => {
                "broker routes are off - set [broker.metatrader] enabled = true to offer them"
            }
            Self::SourceCannotRoute => "this source does not route orders",
            Self::NoAccount => "the execution bridge has not reported an account yet",
            Self::LinkDown => "no link to the broker",
            Self::NotThisAccount => "the terminal is not logged into that kind of account",
            Self::AgentCannotArm => "only the trader's own hand arms a broker account",
            Self::RealNeedsHold => {
                "a REAL account arms only by holding its row in the Trading panel"
            }
            Self::ExposureNeedsChoice => {
                "a position or orders are open at the broker - flatten them first or leave them there"
            }
            Self::AgentOrdersOff => {
                "agents may not place broker orders - set [broker.metatrader] allow_agent_orders = true to allow it"
            }
            Self::ParkedAtBroker => {
                "orders left at the broker answer only the trader's own hand - close them in the window, or arm the account again"
            }
        }
    }
}

/// Why a broker route went back to the simulator on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disarm {
    LinkLost,
    AccountChanged,
    AccountRefused(&'static str),
    SourceCannotRoute,
    BrokerOff,
}

impl Disarm {
    /// The sentence the notice and the toast carry.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Self::LinkLost => "no link with the broker - disarmed",
            Self::AccountChanged => "the terminal changed account - disarmed",
            Self::AccountRefused(sentence) => sentence,
            Self::SourceCannotRoute => "this source does not route orders - disarmed",
            Self::BrokerOff => "broker routes are off - disarmed",
        }
    }
}

/// How to leave a broker route that still has exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// Close the position and cancel the orders at the broker, then route to
    /// the simulator.
    FlattenFirst,
    /// Route to the simulator and leave the broker's position and orders
    /// where they are; their plates stay on the chart.
    LeaveAtBroker,
}

/// One ask for a route: which, who asks, and how to leave the broker's
/// exposure behind when the ask is the simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRequest {
    pub route: Route,
    pub actor: Actor,
    pub leave: Option<Leave>,
}

impl RouteRequest {
    #[must_use]
    pub const fn new(route: Route, actor: Actor, leave: Option<Leave>) -> Self {
        Self {
            route,
            actor,
            leave,
        }
    }
}

/// What the host must carry out after a route decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteChange {
    /// Nothing moved.
    None,
    /// Route orders to the broker venue.
    ToBroker(Route),
    /// Route orders to the simulator; `flatten` closes the broker's
    /// exposure on the way.
    ToSim { flatten: bool },
}

/// The route and the gesture that arms it.
#[derive(Debug, Clone, Default)]
pub struct AccountRoute {
    route: Route,
    /// When the press on a REAL row began, while it is held.
    hold_started_ms: Option<i64>,
    /// The account the press began on, or the armed route is armed on.
    account_key: Option<AccountKey>,
    /// The last time the route went back on its own, and why.
    disarmed: Option<(Disarm, i64)>,
    /// A trader asked to leave with exposure and has not chosen how.
    leaving: bool,
}

impl AccountRoute {
    /// The simulator: how every run starts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn route(&self) -> Route {
        self.route
    }

    /// The last automatic disarm and when it happened.
    #[must_use]
    pub fn last_disarm(&self) -> Option<(Disarm, i64)> {
        self.disarmed
    }

    /// Whether a leave with exposure waits for the trader's choice.
    #[must_use]
    pub fn leaving(&self) -> bool {
        self.leaving
    }

    /// How far the REAL hold has come, `0..=1`, while a row is held.
    #[must_use]
    pub fn hold_progress(&self, now_ms: i64) -> Option<f32> {
        let started = self.hold_started_ms?;
        let held = now_ms.saturating_sub(started).max(0);
        Some((held as f32 / HOLD_TO_ARM_MS as f32).min(1.0))
    }

    /// Whether `route` could be armed now, whoever asks.
    ///
    /// # Errors
    ///
    /// The first reason it cannot.
    pub fn availability(facts: &RouteFacts, route: Route) -> Result<(), Refusal> {
        if !route.is_broker() {
            return Ok(());
        }
        if !facts.broker_enabled {
            return Err(Refusal::BrokerOff);
        }
        if !facts.source_routes {
            return Err(Refusal::SourceCannotRoute);
        }
        let Some(account) = facts.account else {
            return Err(Refusal::NoAccount);
        };
        if !facts.link_up {
            return Err(Refusal::LinkDown);
        }
        if account != route {
            return Err(Refusal::NotThisAccount);
        }
        Ok(())
    }

    /// Back to the simulator. With exposure at the broker the caller must
    /// say how to leave; asked without, the trader is shown the choice.
    ///
    /// # Errors
    ///
    /// [`Refusal::ExposureNeedsChoice`] while exposure waits for a choice.
    pub fn select_sim(
        &mut self,
        facts: &RouteFacts,
        leave: Option<Leave>,
    ) -> Result<RouteChange, Refusal> {
        self.hold_started_ms = None;
        if !self.route.is_broker() {
            // The trader saw the disarm and chose the simulator.
            self.leaving = false;
            self.disarmed = None;
            return Ok(RouteChange::None);
        }
        let flatten = match (facts.exposure, leave) {
            (true, None) => {
                self.leaving = true;
                return Err(Refusal::ExposureNeedsChoice);
            }
            (true, Some(Leave::FlattenFirst)) => true,
            _ => false,
        };
        self.route = Route::Sim;
        self.leaving = false;
        Ok(RouteChange::ToSim { flatten })
    }

    /// Withdraw a pending leave: the trader stays where they are.
    pub fn cancel_leave(&mut self) {
        self.leaving = false;
    }

    /// The DEMO account, in one action — the trader's alone.
    ///
    /// # Errors
    ///
    /// Why it cannot be armed.
    pub fn select_demo(
        &mut self,
        actor: Actor,
        facts: &RouteFacts,
    ) -> Result<RouteChange, Refusal> {
        if actor == Actor::Agent {
            return Err(Refusal::AgentCannotArm);
        }
        Self::availability(facts, Route::Demo)?;
        if self.route == Route::Demo {
            return Ok(RouteChange::None);
        }
        self.hold_started_ms = None;
        self.leaving = false;
        self.disarmed = None;
        self.route = Route::Demo;
        self.account_key = facts.account_key;
        Ok(RouteChange::ToBroker(Route::Demo))
    }

    /// A named request for `route`: the simulator for anyone, DEMO for the
    /// trader, REAL for nobody — it arms only by the held gesture.
    ///
    /// # Errors
    ///
    /// Why the request is refused.
    pub fn request(
        &mut self,
        route: Route,
        actor: Actor,
        facts: &RouteFacts,
        leave: Option<Leave>,
    ) -> Result<RouteChange, Refusal> {
        match (route, actor) {
            (Route::Sim, _) => self.select_sim(facts, leave),
            (Route::Demo | Route::Real, Actor::Agent) => Err(Refusal::AgentCannotArm),
            (Route::Demo, Actor::Trader) => self.select_demo(actor, facts),
            (Route::Real, Actor::Trader) => Err(Refusal::RealNeedsHold),
        }
    }

    /// The press on a REAL row begins.
    ///
    /// # Errors
    ///
    /// Why REAL cannot be armed; nothing is held then.
    pub fn hold_start(&mut self, now_ms: i64, facts: &RouteFacts) -> Result<(), Refusal> {
        Self::availability(facts, Route::Real)?;
        if self.route != Route::Real {
            self.hold_started_ms = Some(now_ms);
            self.account_key = facts.account_key;
        }
        Ok(())
    }

    /// The row is still held: arms once the hold has lasted
    /// [`HOLD_TO_ARM_MS`], if REAL is still available then.
    pub fn hold_update(&mut self, now_ms: i64, facts: &RouteFacts) -> RouteChange {
        let Some(started) = self.hold_started_ms else {
            return RouteChange::None;
        };
        if Self::availability(facts, Route::Real).is_err() || facts.account_key != self.account_key
        {
            // Pressed on one account, never armed on another.
            self.hold_started_ms = None;
            return RouteChange::None;
        }
        if now_ms.saturating_sub(started) < HOLD_TO_ARM_MS {
            return RouteChange::None;
        }
        self.hold_started_ms = None;
        self.leaving = false;
        self.disarmed = None;
        self.route = Route::Real;
        RouteChange::ToBroker(Route::Real)
    }

    /// The press is gone without a release the window saw: nothing arms.
    pub fn hold_cancel(&mut self) {
        self.hold_started_ms = None;
    }

    /// The press ends. Released before the hold completes, nothing arms.
    pub fn hold_release(&mut self, now_ms: i64, facts: &RouteFacts) -> RouteChange {
        let change = self.hold_update(now_ms, facts);
        self.hold_started_ms = None;
        change
    }

    /// Re-read the facts each frame: a route that has become unsafe goes
    /// back to the simulator, with the reason. A hold the facts no longer
    /// allow is dropped.
    pub fn observe(&mut self, facts: &RouteFacts, now_ms: i64) -> RouteChange {
        if self.hold_started_ms.is_some() && Self::availability(facts, Route::Real).is_err() {
            self.hold_started_ms = None;
        }
        if !self.route.is_broker() {
            return RouteChange::None;
        }
        let reason = match Self::availability(facts, self.route) {
            Ok(()) if facts.account_key == self.account_key => return RouteChange::None,
            // The same kind of account, but not the one that was armed.
            Ok(()) => Disarm::AccountChanged,
            Err(Refusal::BrokerOff) => Disarm::BrokerOff,
            Err(Refusal::SourceCannotRoute) => Disarm::SourceCannotRoute,
            Err(Refusal::LinkDown | Refusal::NoAccount) => Disarm::LinkLost,
            Err(_) => Disarm::AccountChanged,
        };
        self.disarm(reason, now_ms)
    }

    /// What the broker said about itself: a lost link, another account or
    /// a refused one disarms.
    pub fn on_notice(&mut self, notice: &VenueNotice, now_ms: i64) -> RouteChange {
        let reason = match notice {
            VenueNotice::LinkLost => Disarm::LinkLost,
            VenueNotice::AccountChanged => Disarm::AccountChanged,
            VenueNotice::AccountRefused(sentence) => Disarm::AccountRefused(sentence),
            _ => return RouteChange::None,
        };
        if !self.route.is_broker() {
            // Nothing armed to drop, but a hold in progress must not
            // complete on facts that just changed.
            self.hold_started_ms = None;
            return RouteChange::None;
        }
        self.disarm(reason, now_ms)
    }

    fn disarm(&mut self, reason: Disarm, now_ms: i64) -> RouteChange {
        self.route = Route::Sim;
        self.hold_started_ms = None;
        self.leaving = false;
        self.disarmed = Some((reason, now_ms));
        // Never flattened on the trader's behalf: protective prices already
        // rest at the broker, and a close nobody asked for is a trade.
        RouteChange::ToSim { flatten: false }
    }

    /// Why a broker order from `actor` is refused, if it is: on a broker
    /// route an agent places orders only where the config allows it.
    #[must_use]
    pub fn write_block(&self, actor: Actor, allow_agent_orders: bool) -> Option<Refusal> {
        (self.route.is_broker() && actor == Actor::Agent && !allow_agent_orders)
            .then_some(Refusal::AgentOrdersOff)
    }

    /// Why `actor` may not leave the broker route as `leave` asks, if it may
    /// not: flattening first is a broker order, so an agent leaves with the
    /// exposure where it is unless the config allows agent orders.
    #[must_use]
    pub fn leave_block(
        actor: Actor,
        leave: Option<Leave>,
        allow_agent_orders: bool,
    ) -> Option<Refusal> {
        (actor == Actor::Agent && leave == Some(Leave::FlattenFirst) && !allow_agent_orders)
            .then_some(Refusal::AgentOrdersOff)
    }
}
