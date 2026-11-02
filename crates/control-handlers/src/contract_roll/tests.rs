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
    offered: Vec<String>,
    switches: usize,
    carries: Vec<(String, String, String)>,
}

impl Window {
    fn on(symbol: &str) -> Self {
        Self {
            symbol: symbol.into(),
            replay: false,
            paper_open: false,
            offered: vec!["AAAZ26".into(), "AAAG27".into(), "AAAJ27".into()],
            switches: 0,
            carries: Vec::new(),
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
        self.feed_selection(index)
    }
}
impl ContractRollPort for Window {
    fn paper_position_open(&self, _index: usize) -> bool {
        self.paper_open
    }
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize {
        self.carries.push((feed_id.into(), from.into(), to.into()));
        3
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
    assert_eq!(window.switches, 1, "the retry rolled again");
    assert_eq!(window.carries.len(), 1);
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
    assert_eq!(target(&open, 0), Some(Err(RollRefusal::PaperPositionOpen)));
}
