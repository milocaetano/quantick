//! The window's one route session: the desk, the connector that builds its
//! venue, and the sheet the panel and the toolbar paint.
//!
//! The host hands over a [`RouteHost`] — its tabs, the config and chart
//! facts, the time and its toast — once a frame, and with a press or a named
//! call when one comes. Everything else is decided here: when the venue is
//! built (the account list opening, or a named route request, and only while
//! broker routes are offered), what a staged capture shows, and the words
//! painted until the next change.

use quantick_trading::{BrokerConnector, BrokerVenue};
use rust_decimal::Decimal;

use super::desk::{AccountsPort, BrokerDesk, RoutePress, SourceFacts};
use super::scene::Scene;
use super::sheet::Sheet;
use super::{AccountRoute, Actor, Leave, Refusal, Route};

/// The host as the session reaches it: its tabs' accounts, and what only a
/// window reads.
pub trait RouteHost: AccountsPort {
    /// The config and the active chart, read now.
    fn source(&self) -> SourceFacts;
    /// The time, in Unix milliseconds. The session never reads a clock.
    fn now_ms(&self) -> i64;
    /// The size the active chart's ticket would send.
    fn ticket_quantity(&self) -> Option<Decimal>;
    /// Tell the trader `text` on the active chart.
    fn toast(&mut self, text: String);
    /// What a capture staged, asked each frame until it answers; `None`
    /// while it cannot say yet. A host with no capture harness stages
    /// nothing.
    fn staged(&self) -> Option<Staged> {
        Some((None, None))
    }
}

/// A staged scene, and the connector its venue is built through instead of
/// the configured one.
pub type Staged = (Option<Scene>, Option<Box<dyn BrokerConnector>>);

/// A connector for a host with no broker: builds nothing.
struct NoBroker;

impl BrokerConnector for NoBroker {
    fn connect(&mut self, _symbol: &str) -> Option<Box<dyn BrokerVenue>> {
        None
    }
}

/// The route, the one broker venue and what builds it.
pub struct BrokerSession {
    desk: BrokerDesk,
    connector: Box<dyn BrokerConnector>,
    /// `[broker.metatrader] allow_agent_orders`.
    allow_agent_orders: bool,
    /// What the panel and the toolbar paint, as of the last change; `None`
    /// while broker routes are not offered.
    sheet: Option<Sheet>,
    /// A staged capture's scene: `None` until the host has looked.
    staged: Option<Option<Scene>>,
}

impl Default for BrokerSession {
    fn default() -> Self {
        Self::new(Box::new(NoBroker), false)
    }
}

impl BrokerSession {
    /// A session that builds its venue through `connector` when a route
    /// first needs one.
    #[must_use]
    pub fn new(connector: Box<dyn BrokerConnector>, allow_agent_orders: bool) -> Self {
        Self {
            desk: BrokerDesk::new(),
            connector,
            allow_agent_orders,
            sheet: None,
            staged: None,
        }
    }

    /// One frame: the staged scene if any, the desk's frame, its toast, and
    /// the sheet painted until the next change.
    pub fn frame(&mut self, host: &mut dyn RouteHost) {
        if self.staged.is_none()
            && let Some((scene, connector)) = host.staged()
        {
            if let Some(connector) = connector {
                self.connector = connector;
            }
            self.staged = Some(scene);
        }
        let source = self.source(host);
        if source.broker_enabled {
            let now_ms = host.now_ms();
            self.stage(host, source, now_ms);
            self.desk.frame(host, source, now_ms);
            if let Some(toast) = self.desk.take_toast() {
                host.toast(toast);
            }
        }
        self.refresh(host);
    }

    /// Carry out a press on the route block or the toolbar notice. Opening
    /// the list is what first builds the venue: its rows are the terminal's
    /// own account facts.
    pub fn press(&mut self, host: &mut dyn RouteHost, press: RoutePress) {
        if matches!(press, RoutePress::OpenList | RoutePress::ToggleList) {
            self.ensure_venue(host);
        }
        let (source, now_ms) = (self.source(host), host.now_ms());
        self.desk.press(host, source, press, now_ms);
        self.refresh(host);
    }

    /// A named request for a route, judged by the route.
    ///
    /// # Errors
    ///
    /// The route's refusal.
    pub fn request(
        &mut self,
        host: &mut dyn RouteHost,
        request: (Route, Actor, Option<Leave>),
    ) -> Result<(), Refusal> {
        // Asking for a broker route needs the terminal's account; going back
        // to the simulator never starts a bridge.
        if request.0.is_broker() {
            self.ensure_venue(host);
        }
        let (_, actor, leave) = request;
        if let Some(refusal) = AccountRoute::leave_block(actor, leave, self.allow_agent_orders) {
            return Err(refusal);
        }
        let source = self.source(host);
        let answer = self.desk.request(host, source, request);
        self.refresh(host);
        answer
    }

    /// Why an order from `actor` on the active chart is refused now, if it
    /// is. See [`BrokerDesk::write_block`].
    #[must_use]
    pub fn write_block(&self, port: &dyn AccountsPort, actor: Actor) -> Option<Refusal> {
        self.desk.write_block(port, actor, self.allow_agent_orders)
    }

    /// The route block's words, while broker routes are offered.
    #[must_use]
    pub fn sheet(&self) -> Option<&Sheet> {
        self.sheet.as_ref()
    }

    /// The status bar's P&L cell while a broker account is shown without its
    /// link: the sheet's status, ranked below a profit so it reads as one.
    #[must_use]
    pub fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        let status = self.sheet.as_ref()?.status.clone()?;
        Some((status, std::cmp::Ordering::Less))
    }

    /// The route as a named read answers it, offered or not: the sheet as of
    /// now, the route in force and whether broker routes are offered.
    #[must_use]
    pub fn read(&self, host: &dyn RouteHost) -> (Sheet, Route, bool) {
        let source = self.source(host);
        let sheet = self
            .desk
            .sheet(host, source, host.ticket_quantity(), host.now_ms());
        (sheet, self.desk.route().route(), source.broker_enabled)
    }

    /// Whether a broker venue has been built this run.
    #[must_use]
    pub fn has_venue(&self) -> bool {
        self.desk.has_venue()
    }

    /// The host's facts, with a staged scene's stand-in: the chart's tape
    /// playing a live MetaTrader one.
    fn source(&self, host: &dyn RouteHost) -> SourceFacts {
        let mut source = host.source();
        if let Some(Some(scene)) = self.staged
            && scene.stands_in_live()
        {
            (source.source_routes, source.replay) = (true, false);
        }
        source
    }

    fn stage(&mut self, host: &mut dyn RouteHost, source: SourceFacts, now_ms: i64) {
        let Some(Some(scene)) = self.staged else {
            return;
        };
        if scene.needs_broker() {
            self.ensure_venue(host);
        }
        if self.desk.stage(host, source, &scene, now_ms) {
            self.connector.sever();
        }
    }

    /// Build the venue for the active chart's symbol, once, and only while
    /// broker routes are offered.
    fn ensure_venue(&mut self, host: &dyn RouteHost) {
        if self.desk.has_venue() || !host.source().broker_enabled {
            return;
        }
        let Some(account) = host.account(host.active_tab()) else {
            return;
        };
        if let Some(venue) = self.connector.connect(account.symbol()) {
            self.desk.install(venue);
        }
    }

    fn refresh(&mut self, host: &dyn RouteHost) {
        let (sheet, _, offered) = self.read(host);
        self.sheet = offered.then_some(sheet);
    }
}

/// The session and the host's tabs, joined for one named call.
pub struct RouteCall<'a> {
    session: &'a mut BrokerSession,
    host: Box<dyn RouteHost + 'a>,
}

impl<'a> RouteCall<'a> {
    #[must_use]
    pub fn new(session: &'a mut BrokerSession, host: Box<dyn RouteHost + 'a>) -> Self {
        Self { session, host }
    }

    /// One frame. See [`BrokerSession::frame`].
    pub fn frame(&mut self) {
        self.session.frame(&mut *self.host);
    }

    /// A press on the route block or the notice. See
    /// [`BrokerSession::press`].
    pub fn press(&mut self, press: RoutePress) {
        self.session.press(&mut *self.host, press);
    }

    /// The route as a named read answers it. See [`BrokerSession::read`].
    #[must_use]
    pub fn read(&self) -> (Sheet, Route, bool) {
        self.session.read(&*self.host)
    }

    /// A named request for a route. See [`BrokerSession::request`].
    ///
    /// # Errors
    ///
    /// The route's refusal.
    pub fn request(&mut self, request: (Route, Actor, Option<Leave>)) -> Result<(), Refusal> {
        self.session.request(&mut *self.host, request)
    }

    /// Why an order from `actor` on the active chart is refused now, if it
    /// is.
    #[must_use]
    pub fn write_block(&self, actor: Actor) -> Option<Refusal> {
        self.session.write_block(&*self.host, actor)
    }
}
