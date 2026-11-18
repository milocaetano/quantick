use super::*;
use quantick_workspace::layout_document::LayoutBook;

struct Window {
    layouts: LayoutBook,
    canvas: CanvasLayout,
    ops: Vec<Op>,
}
impl Default for Window {
    fn default() -> Self {
        Self {
            layouts: LayoutBook::starter(vec![]),
            canvas: CanvasLayout::Single,
            ops: vec![],
        }
    }
}
impl Desk for Window {
    fn apply(&mut self, op: Op) {
        if let Op::Layout(layout) = op {
            self.canvas = layout;
        }
        self.ops.push(op);
    }
    fn has_layout(&mut self, name: &str) -> bool {
        self.layouts.by_name(name).is_some()
    }
    fn has_script(&mut self, _: &str) -> bool {
        false
    }
    fn flow_address(&mut self) -> (u64, u64) {
        (13, 72)
    }
    fn workspace_menu(&mut self, _: &str) -> bool {
        true
    }
    fn canvas_layout(&mut self) -> CanvasLayout {
        self.canvas
    }
}

fn captured<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    }
}

#[test]
fn a_layout_tab_switches_to_a_persisted_name_and_creates_a_missing_one() {
    let mut window = Window::default();
    let existing = window.layouts.active().name.clone();
    for (name, id) in [
        (existing.as_str(), layout::TAB_SWITCH_CAPABILITY_ID),
        ("Scalp", layout::TAB_CREATE_CAPABILITY_ID),
    ] {
        Scenario(&captured(&[("QUANTICK_LAYOUT_TAB", name)])).layout(&mut window);
        let [Op::Call((actual, input))] = window.ops.as_slice() else {
            panic!("expected a single registered call");
        };
        assert_eq!(*actual, id);
        assert_eq!(input["name"], name);
        window.ops.clear();
    }
}

#[test]
fn context_folds_observe_the_layout_selected_earlier_in_the_phase() {
    for (name, upper, lower, expected) in [
        ("time+time+flow", "1", "0", Some(true)),
        ("time+time+flow", "0", "1", Some(false)),
        ("time+time+flow", "1", "1", None),
        ("time+flow", "1", "0", None),
    ] {
        let mut window = Window::default();
        Scenario(&captured(&[
            ("QUANTICK_LAYOUT", name),
            ("QUANTICK_CONTEXT_UPPER_COLLAPSED", upper),
            ("QUANTICK_CONTEXT_LOWER_COLLAPSED", lower),
        ]))
        .after_replay(&mut window);
        let folds: Vec<_> = window
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FoldContext(upper) => Some(*upper),
                _ => None,
            })
            .collect();
        assert_eq!(folds, expected.into_iter().collect::<Vec<_>>());
    }
}

#[test]
fn restored_history_is_overridden_only_by_valid_inputs_in_launch_order() {
    let mut history = quantick_session::HistorySettings {
        progressive_history: true,
        history_reach: history_reach::HistoryReach::Sessions(2),
        history_reach_span_minutes: 60,
        venue_lead_in: true,
    };
    Scenario(&captured(&[
        ("QUANTICK_HISTORY_REACH_SPAN_MINUTES", "180"),
        ("QUANTICK_HISTORY_REACH", "span"),
        ("QUANTICK_PROGRESSIVE_HISTORY", "0"),
        ("QUANTICK_VENUE_LEAD_IN", "true"),
    ]))
    .history(&mut history);
    assert_eq!(history.history_reach, history_reach::HistoryReach::Hours(3));
    assert!(history.venue_lead_in);
    assert!(!history.progressive_history);
    let restored = history;
    Scenario(&captured(&[
        ("QUANTICK_HISTORY_REACH_SPAN_MINUTES", "-1"),
        ("QUANTICK_HISTORY_REACH", "typo"),
        ("QUANTICK_PROGRESSIVE_HISTORY", "on"),
    ]))
    .history(&mut history);
    assert_eq!(history, restored);
}

#[test]
fn price_range_and_absent_scenarios_target_only_the_requested_pane() {
    let mut window = Window::default();
    Scenario(&captured(&[])).before_seed(&mut window, false, None);
    Scenario(&captured(&[])).after_replay(&mut window);
    assert!(window.ops.is_empty());
    Scenario(&captured(&[("QUANTICK_PRICE_RANGE", " 90.5 : 110.5 ")])).after_replay(&mut window);
    let [Op::Call((id, input))] = window.ops.as_slice() else {
        panic!("expected a price-axis call");
    };
    assert_eq!(
        *id,
        quantick_control_schema::price_axis::PRICE_AXIS_CAPABILITY_ID
    );
    assert_eq!(input["tab_id"], "13");
    assert_eq!(input["pane_id"], "72");
    assert_eq!(input["mode"]["low"], "90.5");
    assert_eq!(input["mode"]["high"], "110.5");
}
