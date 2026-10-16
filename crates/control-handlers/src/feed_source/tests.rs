use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_control::registry::RevisionPolicy;
use quantick_control_host::{actions::ActionRegistry, journal::EventActor};
use serde_json::json;

struct Window {
    feed: String,
    symbol: String,
    replay: bool,
    switches: usize,
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
impl FeedSourcePort for Window {
    fn feed_choices(&self) -> Vec<FeedSourceChoice> {
        vec![
            FeedSourceChoice {
                feed_id: "spot".into(),
                display_name: "Spot".into(),
                symbols: vec!["BTCUSDT".into()],
            },
            FeedSourceChoice {
                feed_id: "futures".into(),
                display_name: "Futures".into(),
                symbols: vec!["BTCUSDT.p".into()],
            },
        ]
    }
    fn feed_selection(&self, _index: usize) -> Option<FeedSelection> {
        Some(FeedSelection {
            feed_id: self.feed.clone(),
            symbol: self.symbol.clone(),
            replay: self.replay,
        })
    }
    fn select_feed(&mut self, index: usize, feed_id: &str, symbol: &str) -> Option<FeedSelection> {
        self.switches += 1;
        self.feed = feed_id.into();
        self.symbol = symbol.into();
        self.feed_selection(index)
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
    let mut window = Window {
        feed: "spot".into(),
        symbol: "BTCUSDT".into(),
        replay: false,
        switches: 0,
    };
    for invalid in [
        json!({"tab_id":"8","feed_id":"futures","symbol":"BTCUSDT.p"}),
        json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT"}),
        json!({"tab_id":"7","feed_id":"missing","symbol":"BTCUSDT.p"}),
        json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT.p","extra":true}),
    ] {
        assert!(call(&mut window, invalid).is_err());
        assert_eq!(window.switches, 0);
        assert_eq!(window.symbol, "BTCUSDT");
    }
    let input = json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT.p"});
    window.replay = true;
    assert!(
        call(&mut window, input.clone())
            .unwrap_err()
            .message
            .contains("recorded session")
    );
    assert_eq!(window.switches, 0);
    window.replay = false;
    assert_eq!(call(&mut window, input.clone()).unwrap()["changed"], true);
    assert_eq!(call(&mut window, input).unwrap()["timeline_kept"], true);
    assert_eq!(window.switches, 1);
    assert_eq!(window.symbol, "BTCUSDT.p");
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
        let mut window = Window {
            feed: "spot".into(),
            symbol: "BTCUSDT".into(),
            replay: false,
            switches: 0,
        };
        let mut access = FakeAccess::with_budget(0);
        let input = json!({"tab_id":"7","feed_id":"futures","symbol":"BTCUSDT.p"});
        let result = select(&mut window, &mut access, &actor, &input).expect("the switch answers");
        select(&mut window, &mut access, &actor, &input).expect("the repeat answers");
        assert_eq!(window.switches, 1);
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
