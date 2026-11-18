use super::*;
use crate::TabDirectory;
use crate::contract_roll::AutoRoll;
use crate::market_tab::RecoveredFeed;
use crate::market_tab::{LentTab, MarketTab, MarketTabHost};
use crate::test_support::{FAKE_DEFAULT_REACH, FakeAccess, FakeWindow, fake_config};
use quantick_control::registry::RevisionPolicy;
use quantick_control_host::{actions::ActionRegistry, journal::EventActor};
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot};
use quantick_sources::history_reach::HistoryReach;
use quantick_sources::recovery::Recovery;
use quantick_stores::config::AppConfig;
use serde_json::json;

/// The one tab: its pair, and how many switches it took.
struct Market {
    feed: String,
    symbol: String,
    replay: bool,
    switches: usize,
}
impl MarketTab for Market {
    fn market(&self) -> FeedSelection {
        FeedSelection {
            feed_id: self.feed.clone(),
            symbol: self.symbol.clone(),
            replay: self.replay,
        }
    }
    fn switch_market(&mut self, _config: &AppConfig, feed_id: &str, symbol: &str) {
        self.switches += 1;
        self.feed = feed_id.into();
        self.symbol = symbol.into();
    }
    fn paper_open(&self) -> bool {
        false
    }
    fn unlisted(&self) -> Option<String> {
        None
    }
    fn roll_memory(&mut self) -> &mut AutoRoll {
        unreachable!("a source switch never reads the roll memory")
    }
    fn recover(&mut self, _config: &AppConfig, _recovery: Recovery) -> RecoveredFeed {
        unreachable!("a source switch never recovers the feed")
    }
    fn pages_history(&self, _config: &AppConfig) -> bool {
        false
    }
    fn history(&mut self, _config: &AppConfig, _act: HistoryAct) -> HistoryOutcome {
        unreachable!("a source switch never presses History")
    }
    fn session(&self) -> SessionSnapshot {
        unreachable!("a source switch never presses History")
    }
}

/// A window of one tab, id 7, over a spot and a futures feed.
struct Window {
    tab: Market,
    config: AppConfig,
    default: HistoryReach,
}
impl Window {
    fn on(feed: &str, symbol: &str) -> Self {
        Self {
            tab: Market {
                feed: feed.into(),
                symbol: symbol.into(),
                replay: false,
                switches: 0,
            },
            config: fake_config(&[
                ("spot", &["BTCUSDT"], &[]),
                ("futures", &["BTCUSDT.p"], &[]),
            ]),
            default: FAKE_DEFAULT_REACH,
        }
    }
}
impl TabDirectory for Window {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, id: u64) -> Option<usize> {
        (id == 7).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        7
    }
}
impl crate::OpenTabs for Window {
    type Tab = Market;
    fn tab_count(&self) -> usize {
        1
    }
    fn open_tab(&self, _index: usize) -> Option<&Market> {
        Some(&self.tab)
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn rail_magnet(&self) -> bool {
        false
    }
}
impl MarketTabHost for Window {
    fn market_tab_mut(&mut self, _index: usize) -> Option<LentTab<'_>> {
        Some(LentTab {
            tab: &mut self.tab,
            config: &self.config,
            history_default: &mut self.default,
        })
    }
    fn drawing_gesture_on(&self, _feed_id: &str, _from: &str, _to: &str) -> bool {
        false
    }
    fn carry_market_drawings(&mut self, _feed_id: &str, _from: &str, _to: &str) -> usize {
        0
    }
}
fn call(window: &mut Window, input: Value) -> Result<Value, ControlError> {
    select(
        window,
        &mut FakeAccess::with_budget(0),
        &FakeWindow::assistant(),
        &input,
    )
}
#[test]
fn source_selection_validates_before_mutation_and_retries_without_another_switch() {
    let mut window = Window::on("spot", "BTCUSDT");
    for invalid in [
        json!({"tab_id":"8","feed_id":"futures","symbol":"BTCUSDT.p"}),
        json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT"}),
        json!({"tab_id":"7","feed_id":"missing","symbol":"BTCUSDT.p"}),
        json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT.p","extra":true}),
    ] {
        assert!(call(&mut window, invalid).is_err());
        assert_eq!(window.tab.switches, 0);
        assert_eq!(window.tab.symbol, "BTCUSDT");
    }
    let input = json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT.p"});
    window.tab.replay = true;
    assert!(
        call(&mut window, input.clone())
            .unwrap_err()
            .message
            .contains("recorded session")
    );
    assert_eq!(window.tab.switches, 0);
    window.tab.replay = false;
    assert_eq!(call(&mut window, input.clone()).unwrap()["changed"], true);
    assert_eq!(call(&mut window, input).unwrap()["timeline_kept"], true);
    assert_eq!(window.tab.switches, 1);
    assert_eq!(window.tab.symbol, "BTCUSDT.p");
}

/// Both acts that throw a timeline away say so in the descriptor a client
/// reads before calling: irreversible, behind `cockpit.recover`, carrying
/// `timeline_rebuilt`, with a stale-input text that names the journal event.
/// Neither claims `destructive`: the registry couples it to a `Required`
/// revision this host's strict tier refuses to receive, so the claim would be
/// a guarantee nothing delivers. The reconnect beside them keeps the trader's
/// work and stays reversible.
#[test]
fn source_selection_and_reload_declare_their_rebuild_without_a_revision_promise() {
    let mut sources = ActionRegistry::<Window, FakeAccess>::new();
    register(&mut sources).expect("the source descriptor is valid");
    let mut recoveries = ActionRegistry::<FakeWindow, FakeAccess>::new();
    crate::recovery::register(&mut recoveries).expect("the recovery descriptors are valid");
    let descriptors: Vec<_> = sources
        .descriptors()
        .chain(recoveries.descriptors())
        .cloned()
        .collect();
    let named = |id: &str| {
        descriptors
            .iter()
            .find(|descriptor| descriptor.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is registered"))
            .clone()
    };
    let source = named("feed.source.set");
    let reload = named("feed.reload");
    for descriptor in [&source, &reload] {
        let id = descriptor.id.as_str();
        assert!(!descriptor.destructive, "{id} promises no revision check");
        assert!(!descriptor.reversible, "{id} cannot be undone");
        assert_eq!(
            descriptor.revision_policy,
            RevisionPolicy::OptionalForAdditive,
            "{id}"
        );
        let safety = descriptor
            .stale_input_safety
            .as_deref()
            .unwrap_or_else(|| panic!("{id} states why a stale caller is bounded"));
        for event in ["feed.timeline.rebuilt", "feed.source.changed"] {
            assert!(safety.contains(event), "{id} names {event}");
        }
        assert!(
            descriptor
                .risk_flags
                .iter()
                .any(|flag| flag.as_str() == "timeline_rebuilt"),
            "{id} carries timeline_rebuilt"
        );
        let permissions: Vec<_> = descriptor
            .required_permissions
            .iter()
            .map(|permission| permission.as_str().to_owned())
            .collect();
        assert_eq!(permissions, ["cockpit", "cockpit.recover"], "{id}");
    }
    assert_eq!(source.effect, reload.effect);
    let reconnect = named("feed.reconnect");
    assert!(!reconnect.destructive, "a reconnect keeps the timeline");
    assert!(reconnect.reversible);
}

/// Who switched a tab's market, and from what to what, is on the record,
/// whoever it was: the trader's own call and an assistant's journal the same
/// event under their own names. Repeating the active pair switches nothing
/// and records nothing.
#[test]
fn a_changed_source_journals_who_switched_it_from_what_to_what() {
    for actor in [FakeWindow::trader(), FakeWindow::assistant()] {
        let mut window = Window::on("spot", "BTCUSDT");
        let mut access = FakeAccess::with_budget(0);
        let input = json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT.p"});
        let result = select(&mut window, &mut access, &actor, &input).expect("the switch answers");
        select(&mut window, &mut access, &actor, &input).expect("the repeat answers");
        assert_eq!(window.tab.switches, 1);
        assert_eq!(access.events.len(), 1, "one switch, one record");
        let event = &access.events[0];
        assert_eq!(event.module_id.as_str(), "feed");
        assert_eq!(event.kind.as_str(), "feed.source.changed");
        assert_eq!(event.actor, Some(EventActor::from(&actor)));
        let expected = json!({
            "tab_id": "7",
            "feed_id": "futures",
            "symbol": "BTCUSDT.p",
            "from_feed_id": "spot",
            "from_symbol": "BTCUSDT",
            "changed": true,
            "timeline_kept": false,
        });
        assert_eq!(event.payload, json!({ "feed_source": expected }));
        assert_eq!(result, expected, "the caller reads what the journal reads");
    }
}

/// `tab.market.set` is `feed.source.set` under the tab's market names: the
/// same checks, one switch, the same journal row, and a repeat moves nothing.
#[test]
fn tab_market_set_is_the_source_switch_under_market_names() {
    let mut window = Window::on("spot", "BTCUSDT");
    let mut access = FakeAccess::with_budget(0);
    let actor = FakeWindow::assistant();
    let mut set =
        |window: &mut Window, input: Value| set_market(window, &mut access, &actor, &input);
    for invalid in [
        json!({"tab":"8","feed":"futures","symbol":"BTCUSDT.p"}),
        json!({"tab":"7","feed":"futures","symbol":"BTCUSDT"}),
        json!({"tab":"7","feed_id":"futures","symbol":"BTCUSDT.p"}),
    ] {
        assert!(set(&mut window, invalid).is_err());
    }
    assert_eq!(window.tab.switches, 0, "a refusal switches nothing");
    let input = json!({"tab":"7","feed":"futures","symbol":"BTCUSDT.p"});
    let switched = set(&mut window, input.clone()).expect("the switch answers");
    assert_eq!(
        switched,
        json!({
            "tab": "7",
            "feed": "futures",
            "symbol": "BTCUSDT.p",
            "from_feed": "spot",
            "from_symbol": "BTCUSDT",
            "changed": true,
        })
    );
    let repeat = set(&mut window, input).expect("the repeat answers");
    assert_eq!(repeat["changed"], false);
    assert_eq!(window.tab.switches, 1);
    assert_eq!(access.events.len(), 1, "one switch, one record");
    assert_eq!(access.events[0].kind.as_str(), "feed.source.changed");
    let mut registry = ActionRegistry::<Window, FakeAccess>::new();
    register(&mut registry).expect("both descriptors are valid");
    let ids: Vec<_> = registry
        .descriptors()
        .map(|d| d.id.as_str().to_owned())
        .collect();
    assert_eq!(ids, ["feed.source.set", "tab.market.set"]);
}
