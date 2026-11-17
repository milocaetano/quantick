//! The one broker venue and the account it routes through.
//!
//! A terminal is logged into one account, so there is one broker venue per
//! run. It sits in exactly one place: built but unattached, or inside the
//! account of the chart it routes for. [`BrokerDesk`] keeps it there, follows
//! the active chart while a broker route is armed, parks it (plates still on
//! the chart) when the route goes back with exposure open, and takes it back
//! once that account is idle. The host hands over its tabs as an
//! [`AccountsPort`] and the time; nothing here reads a clock or a window.

use quantick_trading::{BrokerVenue, LinkState, VenueNotice};

use super::{AccountKey, AccountRoute, Actor, Leave, Refusal, Route, RouteChange, RouteFacts};
use crate::PaperAccount;

/// The host's tabs, as the desk reaches them: which one is active, and each
/// one's account by a key the host keeps stable for the tab's life.
pub trait AccountsPort {
    /// The key of the tab whose chart is active.
    fn active_tab(&self) -> u64;
    /// The account of the tab under `key`, while it is open.
    fn account(&self, key: u64) -> Option<&PaperAccount>;
    /// The same, to act on.
    fn account_mut(&mut self, key: u64) -> Option<&mut PaperAccount>;
}

/// What the trader did to the route block in the Trading panel or the
/// toolbar notice: the window turns a press into one of these and the desk
/// decides what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePress {
    /// Open or close the account list.
    ToggleList,
    /// Open the account list (the toolbar notice).
    OpenList,
    /// A one-click row.
    Click(Route),
    /// The REAL row is pressed.
    HoldStart,
    /// The REAL row was painted this frame and is still pressed. A frame
    /// without it ends the hold: a row no longer painted (the panel
    /// collapsed, the tab switched) cannot report a release either.
    HoldStill,
    /// The REAL row is let go.
    HoldRelease,
    /// The choice asked for when leaving with exposure; `None` stays.
    Leave(Option<Leave>),
}

/// What the host knows about the active chart and the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceFacts {
    /// `[broker.metatrader] enabled`.
    pub broker_enabled: bool,
    /// The active chart's `FeedCapabilities::order_routing`.
    pub source_routes: bool,
    /// The active chart plays a recording.
    pub replay: bool,
}

/// The route, the broker venue and where it sits.
#[derive(Default)]
pub struct BrokerDesk {
    route: AccountRoute,
    /// Built and attached to no account.
    loose: Option<Box<dyn BrokerVenue>>,
    /// The tab whose account holds it.
    home: Option<u64>,
    /// The account list is open in the Trading panel.
    pub sheet_open: bool,
    /// A disarm or refusal the trader has not been told about yet.
    toast: Option<String>,
    /// The held row reported itself still pressed since the last frame.
    hold_seen: bool,
}

impl BrokerDesk {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn route(&self) -> &AccountRoute {
        &self.route
    }

    /// Whether a broker venue has been built this run.
    #[must_use]
    pub fn has_venue(&self) -> bool {
        self.loose.is_some() || self.home.is_some()
    }

    /// Hand over the venue the host built. Ignored when one exists: there is
    /// one terminal, so there is one venue.
    pub fn install(&mut self, venue: Box<dyn BrokerVenue>) {
        if !self.has_venue() {
            self.loose = Some(venue);
        }
    }

    /// The broker venue, wherever it sits.
    #[must_use]
    pub fn venue<'a>(&'a self, port: &'a dyn AccountsPort) -> Option<&'a dyn BrokerVenue> {
        if let Some(loose) = &self.loose {
            return Some(loose.as_ref());
        }
        port.account(self.home?)?.broker()
    }

    /// The route's facts: the config and the chart's source, and what the
    /// broker reports about itself.
    #[must_use]
    pub fn facts(&self, port: &dyn AccountsPort, source: SourceFacts) -> RouteFacts {
        let venue = self.venue(port);
        let account = venue
            .and_then(BrokerVenue::account)
            .filter(|facts| facts.margin_mode.nets());
        RouteFacts {
            broker_enabled: source.broker_enabled,
            source_routes: source.source_routes,
            account: account.map(|facts| Route::of_mode(facts.trade_mode)),
            account_key: account.map(AccountKey::of),
            link_up: venue.is_some_and(|venue| venue.link_state() == LinkState::Up),
            exposure: venue.is_some_and(|venue| !venue.is_idle()),
        }
    }

    /// One frame: drain the broker, let the route react to what it said and
    /// to the facts, and put the venue where the route wants it.
    pub fn frame(&mut self, port: &mut dyn AccountsPort, source: SourceFacts, now_ms: i64) {
        let before = self.route.last_disarm();
        let was = self.route.route();
        // A held REAL row arms the frame its hold completes, as long as the
        // row reported itself still pressed since the last frame.
        if self.route.hold_progress(now_ms).is_some() {
            if std::mem::take(&mut self.hold_seen) {
                self.hold(port, source, now_ms, false);
            } else {
                self.route.hold_cancel();
            }
        }
        for notice in self.poll(port, now_ms) {
            let change = self.route.on_notice(&notice, now_ms);
            self.apply(port, change);
        }
        let facts = self.facts(port, source);
        let change = self.route.observe(&facts, now_ms);
        self.apply(port, change);
        self.place(port);
        if let Some((reason, _)) = self.route.last_disarm()
            && self.route.last_disarm() != before
        {
            self.toast = Some(super::sheet::disarm_toast(was, reason));
        }
    }

    /// A named or clicked request for `route`.
    ///
    /// # Errors
    ///
    /// The route's refusal, also left as the toast for a trader's request.
    pub fn request(
        &mut self,
        port: &mut dyn AccountsPort,
        source: SourceFacts,
        request: (Route, Actor, Option<Leave>),
    ) -> Result<(), Refusal> {
        let (route, actor, leave) = request;
        let facts = self.facts(port, source);
        let change = self.route.request(route, actor, &facts, leave)?;
        self.apply(port, change);
        self.place(port);
        Ok(())
    }

    /// The press on the REAL row begins.
    ///
    /// # Errors
    ///
    /// Why REAL cannot be armed now.
    pub fn hold_start(
        &mut self,
        port: &dyn AccountsPort,
        source: SourceFacts,
        now_ms: i64,
    ) -> Result<(), Refusal> {
        let facts = self.facts(port, source);
        self.route.hold_start(now_ms, &facts)?;
        self.hold_seen = true;
        Ok(())
    }

    /// The REAL row is still held (`released` false) or let go.
    pub fn hold(
        &mut self,
        port: &mut dyn AccountsPort,
        source: SourceFacts,
        now_ms: i64,
        released: bool,
    ) {
        let facts = self.facts(port, source);
        let change = if released {
            self.route.hold_release(now_ms, &facts)
        } else {
            self.route.hold_update(now_ms, &facts)
        };
        if change != RouteChange::None {
            self.sheet_open = false;
        }
        self.apply(port, change);
        self.place(port);
    }

    /// Carry out what the trader pressed. A refusal is left as the toast,
    /// in the route's words.
    pub fn press(
        &mut self,
        port: &mut dyn AccountsPort,
        source: SourceFacts,
        press: RoutePress,
        now_ms: i64,
    ) {
        let refused = match press {
            RoutePress::ToggleList => {
                self.sheet_open = !self.sheet_open;
                Ok(())
            }
            RoutePress::OpenList => {
                self.sheet_open = true;
                Ok(())
            }
            RoutePress::Click(route) => self
                .request(port, source, (route, Actor::Trader, None))
                .map(|()| self.sheet_open = self.route.leaving()),
            RoutePress::HoldStart => self.hold_start(port, source, now_ms),
            RoutePress::HoldStill => {
                self.hold_seen = self.route.hold_progress(now_ms).is_some();
                Ok(())
            }
            RoutePress::HoldRelease => {
                self.hold(port, source, now_ms, true);
                Ok(())
            }
            RoutePress::Leave(None) => {
                self.route.cancel_leave();
                Ok(())
            }
            RoutePress::Leave(Some(leave)) => self
                .request(port, source, (Route::Sim, Actor::Trader, Some(leave)))
                .map(|()| self.sheet_open = false),
        };
        if let Err(refusal) = refused
            && refusal != Refusal::ExposureNeedsChoice
        {
            self.toast = Some(format!("{}: {}", Route::Sim.word(), refusal.sentence()));
        }
    }

    /// Withdraw a pending leave.
    pub fn cancel_leave(&mut self) {
        self.route.cancel_leave();
    }

    /// Why an order from `actor` on the active chart is refused, if it is.
    /// Every write that reaches a broker venue is judged: an agent's only
    /// on an armed route the config opened to agents, and never on a venue
    /// parked with what the trader left at the broker.
    #[must_use]
    pub fn write_block(
        &self,
        port: &dyn AccountsPort,
        actor: Actor,
        allow_agent_orders: bool,
    ) -> Option<Refusal> {
        if actor == Actor::Trader {
            return None;
        }
        if let Some(refusal) = self.route.write_block(actor, allow_agent_orders) {
            return Some(refusal);
        }
        let parked = !self.route.route().is_broker()
            && port
                .account(port.active_tab())
                .is_some_and(PaperAccount::broker_shown);
        parked.then_some(Refusal::ParkedAtBroker)
    }

    /// The sentence waiting for the trader, once.
    pub fn take_toast(&mut self) -> Option<String> {
        self.toast.take()
    }

    /// Drain the venue wherever it sits and hand it the time, so a link
    /// with a heartbeat can be declared lost while it says nothing.
    fn poll(&mut self, port: &mut dyn AccountsPort, now_ms: i64) -> Vec<VenueNotice> {
        if let Some(loose) = self.loose.as_mut() {
            let mut events = loose.poll();
            events.extend(loose.tick(now_ms));
            return events
                .into_iter()
                .filter_map(|event| match event {
                    quantick_trading::VenueEvent::Notice(notice) => Some(notice),
                    _ => None,
                })
                .collect();
        }
        let Some(home) = self.home else {
            return Vec::new();
        };
        match port.account_mut(home) {
            Some(account) => account.poll_broker(now_ms),
            // The tab closed and took the venue with it; the next broker
            // request builds a fresh one.
            None => {
                self.home = None;
                Vec::new()
            }
        }
    }

    fn apply(&mut self, port: &mut dyn AccountsPort, change: RouteChange) {
        let RouteChange::ToSim { flatten } = change else {
            return;
        };
        let Some(account) = self.home.and_then(|home| port.account_mut(home)) else {
            return;
        };
        if flatten {
            // Still routed: the flatten reaches the broker, then it parks.
            let _ = account.flatten();
        }
        account.park_broker();
    }

    /// Keep the venue where the route wants it: inside the active chart's
    /// account while armed; parked where it was while exposure stays open;
    /// unattached once that account is idle.
    fn place(&mut self, port: &mut dyn AccountsPort) {
        let active = port.active_tab();
        if self.route.route().is_broker() {
            if self.home == Some(active) {
                if let Some(account) = port.account_mut(active) {
                    account.resume_broker();
                }
                return;
            }
            let venue = match self.loose.take() {
                Some(venue) => Some(venue),
                None => self
                    .home
                    .and_then(|home| port.account_mut(home))
                    .and_then(PaperAccount::detach_broker),
            };
            match (venue, port.account_mut(active)) {
                (Some(venue), Some(account)) => {
                    account.attach_broker(venue);
                    self.home = Some(active);
                }
                (venue, _) => {
                    self.loose = venue;
                    self.home = None;
                }
            }
            return;
        }
        let Some(home) = self.home else { return };
        let Some(account) = port.account_mut(home) else {
            self.home = None;
            return;
        };
        account.park_broker();
        if account.broker().is_none_or(|broker| broker.is_idle()) {
            self.loose = account.detach_broker();
            self.home = None;
        }
    }
}
