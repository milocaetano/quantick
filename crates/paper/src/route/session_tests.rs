//! The route session over a connector double: when the venue is built,
//! what a staged capture shows and what a named call is answered.

use super::*;

/// The tabs as a window hands them to the session: the config and chart
/// facts, a clock the test sets, and the toasts it was told.
struct Host {
    tabs: Tabs,
    source: SourceFacts,
    now_ms: i64,
    toasts: Vec<String>,
}

impl Host {
    fn new(source: SourceFacts) -> Self {
        Self {
            tabs: Tabs::new(),
            source,
            now_ms: 0,
            toasts: Vec::new(),
        }
    }
}

impl AccountsPort for Host {
    fn active_tab(&self) -> u64 {
        self.tabs.active_tab()
    }
    fn account(&self, key: u64) -> Option<&PaperAccount> {
        self.tabs.account(key)
    }
    fn account_mut(&mut self, key: u64) -> Option<&mut PaperAccount> {
        self.tabs.account_mut(key)
    }
}

impl RouteHost for Host {
    fn source(&self) -> SourceFacts {
        self.source
    }
    fn now_ms(&self) -> i64 {
        self.now_ms
    }
    fn ticket_quantity(&self) -> Option<Decimal> {
        None
    }
    fn toast(&mut self, text: String) {
        self.toasts.push(text);
    }
}

/// What the connector double did, shared with the test.
#[derive(Default)]
struct Wiring {
    built: usize,
    revived: usize,
    severed: bool,
    script: Option<Rc<RefCell<Script>>>,
}

/// Builds a broker of `mode`, or nothing when `mode` is `None` (the port is
/// taken).
struct Connector {
    mode: Option<TradeMode>,
    link: Option<LinkState>,
    wiring: Rc<RefCell<Wiring>>,
}

impl quantick_trading::BrokerConnector for Connector {
    fn connect(&mut self, _symbol: &str) -> Option<Box<dyn BrokerVenue>> {
        let (venue, script) = broker(self.mode?);
        script.borrow_mut().link = self.link;
        let mut wiring = self.wiring.borrow_mut();
        wiring.built += 1;
        wiring.script = Some(script);
        Some(venue)
    }
    fn sever(&mut self) {
        self.wiring.borrow_mut().severed = true;
    }
    fn revive(&mut self, _symbol: &str) {
        self.wiring.borrow_mut().revived += 1;
    }
}

fn session(
    mode: Option<TradeMode>,
    link: Option<LinkState>,
) -> (BrokerSession, Rc<RefCell<Wiring>>) {
    let wiring = Rc::new(RefCell::new(Wiring::default()));
    let connector = Connector {
        mode,
        link,
        wiring: Rc::clone(&wiring),
    };
    (BrokerSession::new(Box::new(connector), false), wiring)
}

#[test]
fn the_list_builds_the_venue_once_and_only_while_broker_routes_are_offered() {
    let (mut session, wiring) = session(Some(TradeMode::Real), None);
    let mut host = Host::new(SourceFacts::default());
    session.press(&mut host, RoutePress::OpenList);
    session.frame(&mut host);
    assert_eq!(wiring.borrow().built, 0, "broker routes are off");
    assert!(session.sheet().is_none());

    host.source = LIVE;
    session.frame(&mut host);
    assert_eq!(
        wiring.borrow().built,
        0,
        "nothing is built until the list opens"
    );
    session.press(&mut host, RoutePress::ToggleList);
    session.press(&mut host, RoutePress::ToggleList);
    session.press(&mut host, RoutePress::OpenList);
    assert_eq!(wiring.borrow().built, 1, "one terminal, one venue");
    let sheet = session.sheet().expect("broker routes are offered");
    assert!(sheet.open);
    assert_eq!(sheet.rows[1].title, "XP · 51234567 · REAL");
}

#[test]
fn the_broker_row_says_connecting_until_the_terminal_names_its_account() {
    let (mut session, _wiring) = session(Some(TradeMode::Real), Some(LinkState::NeverConnected));
    let mut host = Host::new(LIVE);
    session.press(&mut host, RoutePress::OpenList);
    let row = &session.sheet().expect("offered").rows[1];
    assert_eq!(row.hint, "connecting…");
    assert_eq!(row.state, RowState::Off);

    let (mut session, _wiring) = session_without_terminal();
    session.press(&mut host, RoutePress::OpenList);
    let row = &session.sheet().expect("offered").rows[1];
    assert_eq!(row.subs, ["not connected"]);
}

fn session_without_terminal() -> (BrokerSession, Rc<RefCell<Wiring>>) {
    session(None, None)
}

#[test]
fn a_named_demo_request_builds_the_venue_and_arms_for_the_trader_only() {
    let (mut session, wiring) = session(Some(TradeMode::Demo), None);
    let mut host = Host::new(LIVE);
    session.frame(&mut host);
    let mut call = RouteCall::new(&mut session, Box::new(Host::new(LIVE)));
    assert_eq!(
        call.request(RouteRequest::new(Route::Demo, Actor::Agent, None)),
        Err(Refusal::AgentCannotArm)
    );
    assert_eq!(
        call.request(RouteRequest::new(Route::Demo, Actor::Trader, None)),
        Ok(())
    );
    let RouteRead {
        sheet,
        route,
        broker_enabled,
    } = call.read();
    assert_eq!((route, broker_enabled), (Route::Demo, true));
    assert_eq!(sheet.notice.tone, RouteTone::Demo);
    assert_eq!(
        call.write_block(Actor::Agent),
        Some(Refusal::AgentOrdersOff)
    );
    assert_eq!(wiring.borrow().built, 1);
}

/// A host whose capture staged `scene`, over a connector wired to the test.
struct StagedHost(Host, Option<Scene>, Rc<RefCell<Wiring>>);

impl AccountsPort for StagedHost {
    fn active_tab(&self) -> u64 {
        self.0.active_tab()
    }
    fn account(&self, key: u64) -> Option<&PaperAccount> {
        self.0.account(key)
    }
    fn account_mut(&mut self, key: u64) -> Option<&mut PaperAccount> {
        self.0.account_mut(key)
    }
}

impl RouteHost for StagedHost {
    fn source(&self) -> SourceFacts {
        self.0.source
    }
    fn now_ms(&self) -> i64 {
        self.0.now_ms
    }
    fn ticket_quantity(&self) -> Option<Decimal> {
        None
    }
    fn toast(&mut self, text: String) {
        self.0.toasts.push(text);
    }
    fn staged(&self) -> Option<Staged> {
        let connector = Connector {
            mode: Some(TradeMode::Real),
            link: None,
            wiring: Rc::clone(&self.2),
        };
        Some((self.1, Some(Box::new(connector))))
    }
}

#[test]
fn a_staged_lost_link_arms_over_a_replay_then_drops_and_disarms() {
    let scene = Scene::parse(Some("lost"), None, None);
    let replay = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: true,
    };
    let (mut session, configured) = session(Some(TradeMode::Demo), None);
    let staged = Rc::new(RefCell::new(Wiring::default()));
    let mut host = StagedHost(Host::new(replay), scene, Rc::clone(&staged));
    session.frame(&mut host);
    assert_eq!(session.sheet().expect("offered").tone, RouteTone::Real);
    assert_eq!(staged.borrow().built, 1, "the staged connector stands in");
    assert_eq!(configured.borrow().built, 0);
    assert!(!staged.borrow().severed);
    session.frame(&mut host);
    assert!(staged.borrow().severed, "armed, the staged link drops");
}

#[test]
fn a_staged_broker_scene_waits_for_a_mark_and_opens_at_whole_points() {
    let wiring = Rc::new(RefCell::new(Wiring::default()));
    let connect = |demo: bool, entry: Decimal| -> Box<dyn quantick_trading::BrokerConnector> {
        assert!(demo);
        assert_eq!(entry, Decimal::from(125_431));
        let (mode, link) = (Some(TradeMode::Demo), None);
        Box::new(Connector {
            mode,
            link,
            wiring: Rc::clone(&wiring),
        })
    };
    let list = [None, Some("1"), None];
    let (scene, connector) = Scene::stage(list, None, |_, _| unreachable!()).expect("no broker");
    assert!(scene.is_some_and(|scene| scene.list) && connector.is_none());
    let demo = [Some("demo"), None, None];
    assert!(Scene::stage(demo, None, |_, _| unreachable!()).is_none());
    let mark = Some(Decimal::new(1_254_306, 1));
    let (scene, connector) = Scene::stage(demo, mark, connect).expect("a mark");
    assert!(scene.is_some_and(|scene| scene.demo()) && connector.is_some());
}

#[test]
fn a_lost_link_takes_the_status_cell_while_its_account_is_shown() {
    let scene = Scene::parse(Some("real"), None, None);
    let (mut session, _configured) = session(Some(TradeMode::Demo), None);
    let staged = Rc::new(RefCell::new(Wiring::default()));
    let mut host = StagedHost(Host::new(LIVE), scene, Rc::clone(&staged));
    session.frame(&mut host);
    assert_eq!(session.status_cell(), None, "the link is up");
    go_long(host.0.tabs.active());
    let script = staged.borrow().script.clone().expect("built");
    script.borrow_mut().link = Some(LinkState::Lost);
    let lost = VenueEvent::Notice(VenueNotice::LinkLost);
    script.borrow_mut().said.push(lost);
    host.0.now_ms = 3_000;
    session.frame(&mut host);
    let status = session.sheet().and_then(|sheet| sheet.status.clone());
    assert!(status.is_some(), "the shown account says its link is down");
    let cell = status.map(|status| (status, std::cmp::Ordering::Less));
    assert_eq!(session.status_cell(), cell);
}

#[test]
fn an_agent_disarms_only_by_leaving_at_the_broker_unless_allowed() {
    for allow in [false, true] {
        let wiring = Rc::new(RefCell::new(Wiring::default()));
        let connector = Connector {
            mode: Some(TradeMode::Demo),
            link: None,
            wiring: Rc::clone(&wiring),
        };
        let mut session = BrokerSession::new(Box::new(connector), allow);
        let mut host = Host::new(LIVE);
        session
            .request(
                &mut host,
                RouteRequest::new(Route::Demo, Actor::Trader, None),
            )
            .expect("DEMO arms for the trader");
        go_long(host.tabs.active());
        let flatten = RouteRequest::new(Route::Sim, Actor::Agent, Some(Leave::FlattenFirst));
        let answer = session.request(&mut host, flatten);
        if allow {
            assert_eq!(answer, Ok(()));
            continue;
        }
        assert_eq!(answer, Err(Refusal::AgentOrdersOff));
        assert_eq!(session.read(&host).route, Route::Demo, "still armed");
        host.tabs.active().on_trade(&print(2, 101));
        assert!(
            host.tabs.active().venue().position().is_some(),
            "no agent flattened the broker"
        );
        let leave = RouteRequest::new(Route::Sim, Actor::Agent, Some(Leave::LeaveAtBroker));
        assert_eq!(session.request(&mut host, leave), Ok(()));
        assert_eq!(session.read(&host).route, Route::Sim);
    }
}
#[test]
fn a_broker_request_after_the_link_is_lost_revives_what_runs_beside_the_venue() {
    let (mut session, wiring) = session(Some(TradeMode::Demo), None);
    let mut host = Host::new(LIVE);
    session
        .request(
            &mut host,
            RouteRequest::new(Route::Demo, Actor::Trader, None),
        )
        .expect("DEMO arms for the trader");
    session.press(&mut host, RoutePress::OpenList);
    assert_eq!(wiring.borrow().revived, 0, "a live link needs nothing");

    // The bridge process died: the link is lost and the route disarms.
    let script = wiring.borrow().script.clone().expect("built");
    script.borrow_mut().link = Some(LinkState::Lost);
    session.frame(&mut host);
    assert_eq!(session.read(&host).route, Route::Sim);

    assert_eq!(
        session.request(
            &mut host,
            RouteRequest::new(Route::Demo, Actor::Trader, None)
        ),
        Err(Refusal::LinkDown),
        "not until the new bridge says hello"
    );
    assert_eq!(wiring.borrow().revived, 1, "the request relaunches it");
    session.press(&mut host, RoutePress::OpenList);
    assert_eq!(wiring.borrow().revived, 2, "so does opening the list");
    assert_eq!(wiring.borrow().built, 1, "the same venue, waiting");
}
