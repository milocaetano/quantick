use std::collections::BTreeMap;

use super::*;
use crate::TabDirectory;
use crate::feed_source::FeedSelection;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_control_schema::feed_source::FeedSourceChoice;

struct Window {
    symbol: String,
    replay: bool,
    paper_open: bool,
    gesture: bool,
    offered: Vec<String>,
    switches: usize,
    carries: Vec<(String, String, String)>,
    /// What the venue does not list: the feed reports one of these as
    /// unlisted as soon as the tab is on it, and connects on anything else.
    delisted: Vec<String>,
    auto: AutoRoll,
}

impl Window {
    fn on(symbol: &str) -> Self {
        Self {
            symbol: symbol.into(),
            replay: false,
            paper_open: false,
            gesture: false,
            offered: vec!["AAAZ26".into(), "AAAG27".into(), "AAAJ27".into()],
            switches: 0,
            carries: Vec::new(),
            delisted: Vec::new(),
            auto: AutoRoll::default(),
        }
    }
}
impl TabDirectory for Window {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == 7).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        7
    }
}
impl FeedSourcePort for Window {
    fn feed_choices(&self) -> Vec<FeedSourceChoice> {
        vec![FeedSourceChoice {
            feed_id: "venue".into(),
            display_name: "Venue".into(),
            symbols: self.offered.clone(),
            contract_rolls: BTreeMap::from([
                ("AAAZ26".to_owned(), "AAAG27".to_owned()),
                ("AAAG27".to_owned(), "AAAJ27".to_owned()),
            ]),
        }]
    }
    fn feed_selection(&self, _index: usize) -> Option<FeedSelection> {
        Some(FeedSelection {
            feed_id: "venue".into(),
            symbol: self.symbol.clone(),
            replay: self.replay,
        })
    }
    fn select_feed(&mut self, index: usize, _feed_id: &str, symbol: &str) -> Option<FeedSelection> {
        self.switches += 1;
        self.symbol = symbol.into();
        // The feed's link disarms on connect; a listed contract connects.
        if !self.delisted.contains(&self.symbol) {
            self.auto.disarm();
        }
        self.feed_selection(index)
    }
}
impl ContractRollPort for Window {
    fn next_contract(&self, _feed_id: &str, symbol: &str) -> Option<String> {
        self.feed_choices()[0].contract_rolls.get(symbol).cloned()
    }
    fn feed_offers(&self, _feed_id: &str, symbol: &str) -> bool {
        self.offered.iter().any(|offered| offered == symbol)
    }
    fn paper_not_flat(&self, _index: usize) -> bool {
        self.paper_open
    }
    fn drawing_gesture_on(&self, _feed_id: &str, _from: &str, _to: &str) -> bool {
        self.gesture
    }
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize {
        // Like the real copy: the second time adds nothing.
        let carry = (feed_id.to_owned(), from.to_owned(), to.to_owned());
        let first = !self.carries.contains(&carry);
        self.carries.push(carry);
        if first { 3 } else { 0 }
    }
}

impl UnlistedContractPort for Window {
    fn tab_count(&self) -> usize {
        1
    }
    fn unlisted_contract(&self, _index: usize) -> Option<String> {
        (!self.replay && self.delisted.contains(&self.symbol)).then(|| self.symbol.clone())
    }
    fn auto_roll_mut(&mut self, _index: usize) -> Option<&mut AutoRoll> {
        Some(&mut self.auto)
    }
}

fn call(window: &mut Window, from: &str) -> Result<Value, ControlError> {
    roll(
        window,
        &mut FakeAccess::with_budget(0),
        &FakeWindow::assistant(),
        &json!({ "tab_id": "7", "from_symbol": from }),
    )
}

fn precondition(error: &ControlError) -> String {
    error
        .context
        .violated_precondition
        .as_ref()
        .expect("a roll refusal names its precondition")
        .to_string()
}

#[test]
fn a_roll_lands_on_the_configured_contract_after_carrying_the_drawings() {
    let mut window = Window::on("AAAZ26");
    let result = call(&mut window, "AAAZ26").unwrap();
    assert_eq!(result["symbol"], "AAAG27");
    assert_eq!(result["from_symbol"], "AAAZ26");
    assert_eq!(result["changed"], true);
    assert_eq!(result["drawings_carried"], 3);
    assert_eq!(window.switches, 1);
    assert_eq!(
        window.carries,
        [("venue".to_owned(), "AAAZ26".to_owned(), "AAAG27".to_owned())]
    );
}

#[test]
fn a_retry_after_a_lost_answer_does_not_roll_a_contract_further() {
    let mut window = Window::on("AAAZ26");
    call(&mut window, "AAAZ26").unwrap();
    let again = call(&mut window, "AAAZ26").unwrap();
    assert_eq!(again["symbol"], "AAAG27");
    assert_eq!(again["changed"], false);
    assert_eq!(again["drawings_carried"], 0);
    assert_eq!(window.switches, 1, "the retry rolled again");
}

#[test]
fn a_tab_switched_to_the_target_by_hand_still_gets_its_drawings() {
    let mut window = Window::on("AAAG27");
    let result = call(&mut window, "AAAZ26").unwrap();
    assert_eq!(result["changed"], false);
    assert_eq!(result["drawings_carried"], 3);
    assert_eq!(window.switches, 0);
}

#[test]
fn every_refusal_names_its_precondition_and_touches_nothing() {
    let refused = |window: &mut Window, from: &str, expected: &str| {
        let error = call(window, from).unwrap_err();
        assert_eq!(precondition(&error), expected);
        assert_eq!(window.switches, 0);
        assert!(window.carries.is_empty());
    };
    refused(
        &mut Window::on("AAAJ27"),
        "AAAJ27",
        ROLL_CONFIGURED_PRECONDITION,
    );
    refused(&mut Window::on("AAAJ27"), "OTHER", FROM_SYMBOL_PRECONDITION);
    let mut window = Window::on("AAAZ26");
    window.offered.retain(|symbol| symbol != "AAAG27");
    refused(&mut window, "AAAZ26", TARGET_OFFERED_PRECONDITION);
    let mut window = Window::on("AAAZ26");
    window.paper_open = true;
    refused(&mut window, "AAAZ26", PAPER_FLAT_PRECONDITION);
    let mut window = Window::on("AAAZ26");
    window.gesture = true;
    refused(&mut window, "AAAZ26", NO_GESTURE_PRECONDITION);
    let mut window = Window::on("AAAZ26");
    window.replay = true;
    refused(&mut window, "AAAZ26", LIVE_SOURCE_PRECONDITION);
}

#[test]
fn the_toolbar_asks_the_same_rule_the_capability_answers_with() {
    assert_eq!(
        target(&Window::on("AAAZ26"), 0),
        Some(Ok("AAAG27".to_owned()))
    );
    assert_eq!(
        target(&Window::on("AAAJ27"), 0),
        Some(Err(RollRefusal::NotConfigured))
    );
    let mut open = Window::on("AAAZ26");
    open.paper_open = true;
    assert_eq!(target(&open, 0), Some(Err(RollRefusal::PaperNotFlat)));
}

/// A tab on `symbol`, which the venue no longer lists, with `more` unlisted too.
fn unlisted_on(symbol: &str, more: &[&str]) -> Window {
    let mut window = Window::on(symbol);
    window.delisted = std::iter::once(symbol)
        .chain(more.iter().copied())
        .map(str::to_owned)
        .collect();
    window
}

#[test]
fn an_unlisted_contract_offers_its_configured_roll_and_the_click_lands_there() {
    let mut window = unlisted_on("AAAZ26", &[]);
    assert_eq!(notice_roll(&window, 0), Some(Ok("AAAG27".to_owned())));
    let toast = roll_from_notice(&mut window, 0, "AAAG27").unwrap();
    assert_eq!(toast, "AAAZ26 is no longer listed; rolled to AAAG27");
    assert_eq!(window.symbol, "AAAG27");
    assert_eq!(
        window.carries,
        [("venue".to_owned(), "AAAZ26".to_owned(), "AAAG27".to_owned())],
        "the same roll the palette runs: the drawings come along"
    );
    assert_eq!(
        roll_from_notice(&mut window, 0, "AAAG27"),
        Err("The feed no longer reports an unlisted contract".to_owned()),
        "a second press on a stale popup rolls nothing further"
    );
    assert_eq!(window.switches, 1);
}

#[test]
fn no_usable_successor_leaves_the_notice_as_it_was() {
    // Nothing reported unlisted.
    assert_eq!(notice_roll(&Window::on("AAAZ26"), 0), None);
    // The last configured contract has no successor.
    assert_eq!(notice_roll(&unlisted_on("AAAJ27", &[]), 0), None);
    // The successor is not offered on this feed.
    let mut window = unlisted_on("AAAZ26", &[]);
    window.offered.retain(|symbol| symbol != "AAAG27");
    assert_eq!(notice_roll(&window, 0), None);
    // A report about a contract the tab no longer shows.
    assert_eq!(unlisted_roll(&Window::on("AAAG27"), 0, "AAAZ26"), None);
    // A recording never rolls.
    let mut window = unlisted_on("AAAZ26", &[]);
    window.replay = true;
    assert_eq!(unlisted_roll(&window, 0, "AAAZ26"), None);
}

#[test]
fn a_refused_roll_names_its_reason_and_moves_nothing() {
    let mut window = unlisted_on("AAAZ26", &[]);
    window.paper_open = true;
    assert_eq!(
        notice_roll(&window, 0),
        Some(Err(RollRefusal::PaperNotFlat))
    );
    assert_eq!(
        roll_from_notice(&mut window, 0, "AAAG27"),
        Err(RollRefusal::PaperNotFlat.reason().to_owned())
    );
    assert_eq!(window.switches, 0);
    assert!(window.carries.is_empty());
}

#[test]
fn a_restored_tab_rolls_itself_along_the_chain_once_per_contract() {
    let mut window = unlisted_on("AAAZ26", &["AAAG27"]);
    assert!(
        settle_auto_rolls(&mut window).is_empty(),
        "a tab the trader opened by hand waits for the click"
    );
    assert_eq!(window.symbol, "AAAZ26");

    window.auto.arm();
    assert_eq!(
        settle_auto_rolls(&mut window),
        ["AAAZ26 is no longer listed; rolled to AAAG27"]
    );
    // The next contract is unlisted too: the next report rolls one more step.
    assert_eq!(
        settle_auto_rolls(&mut window),
        ["AAAG27 is no longer listed; rolled to AAAJ27"]
    );
    assert_eq!(window.symbol, "AAAJ27");
    assert!(settle_auto_rolls(&mut window).is_empty());
    assert!(
        !window.auto.is_armed(),
        "a feed that connected disarms the tab"
    );
    assert_eq!(window.switches, 2);
}

#[test]
fn an_automatic_roll_is_decided_once_even_when_refused() {
    let mut window = unlisted_on("AAAZ26", &[]);
    window.auto.arm();
    window.gesture = true;
    assert!(settle_auto_rolls(&mut window).is_empty());
    window.gesture = false;
    assert!(
        settle_auto_rolls(&mut window).is_empty(),
        "the refusal was the decision; the roll is the trader's now"
    );
    assert_eq!(window.switches, 0);
    assert_eq!(notice_roll(&window, 0), Some(Ok("AAAG27".to_owned())));
}

#[test]
fn a_recording_never_rolls_by_itself() {
    let mut window = unlisted_on("AAAZ26", &[]);
    window.auto.arm();
    window.replay = true;
    window.delisted.push("AAAZ26".to_owned());
    assert!(settle_auto_rolls(&mut window).is_empty());
    assert_eq!(unlisted_roll(&window, 0, "AAAZ26"), None);
    assert_eq!(window.switches, 0);
}

#[test]
fn a_pressed_roll_runs_on_the_next_settle_once() {
    let mut window = unlisted_on("AAAZ26", &[]);
    window.auto.press("AAAG27");
    assert_eq!(
        settle_auto_rolls(&mut window),
        ["AAAZ26 is no longer listed; rolled to AAAG27"],
        "a hand needs no armed tab"
    );
    assert_eq!(window.symbol, "AAAG27");
    assert!(
        settle_auto_rolls(&mut window).is_empty(),
        "the press ran once"
    );

    let mut refused = unlisted_on("AAAZ26", &[]);
    refused.paper_open = true;
    refused.auto.press("AAAG27");
    assert_eq!(
        settle_auto_rolls(&mut refused),
        [RollRefusal::PaperNotFlat.reason()],
        "a refused press says why instead"
    );
    assert_eq!(refused.switches, 0);
}
