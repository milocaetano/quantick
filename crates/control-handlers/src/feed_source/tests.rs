use super::*;
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
        &mut (),
        &crate::test_support::FakeWindow::assistant(),
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

#[test]
fn source_selection_declares_the_reload_permissions_and_effects() {
    let descriptor = quantick_control_schema::feed_source::descriptor();
    let reload = quantick_control_schema::recovery::descriptor(
        "feed.reload",
        "Reload",
        "Reload",
        true,
        json!({}),
    );
    assert_eq!(descriptor.required_permissions, reload.required_permissions);
    assert_eq!(descriptor.effect, reload.effect);
    assert_eq!(descriptor.risk_flags, reload.risk_flags);
    assert!(!descriptor.reversible);
}
