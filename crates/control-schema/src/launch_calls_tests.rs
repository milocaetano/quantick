use super::*;

#[test]
fn a_price_range_becomes_a_manual_axis_on_the_named_pane() {
    let (id, input) = price_range(" 100.5 : 102 ", 3, 7).expect("a readable range");
    assert_eq!(id, price_axis::PRICE_AXIS_CAPABILITY_ID);
    let input: price_axis::PriceAxisInput = serde_json::from_value(input).expect("valid input");
    assert_eq!((input.tab_id.get(), input.pane_id.get()), (3, 7));
    assert_eq!(input.mode.range().expect("finite"), Some((100.5, 102.0)));
}

#[test]
fn an_unreadable_or_inverted_price_range_is_no_call() {
    for value in [
        "",
        "100",
        "a:b",
        "102:100",
        "100:100",
        "NaN:110",
        "90:inf",
        "90:110:120",
    ] {
        assert!(price_range(value, 1, 1).is_none(), "{value:?}");
    }
}

#[test]
fn the_tape_switch_reads_on_and_off_and_nothing_else() {
    let (id, input) = tape(" off ", 2, 5).expect("off is a call");
    assert_eq!(id, layers::SET_VISIBILITY_CAPABILITY_ID);
    let input: layers::SetVisibilityInput = serde_json::from_value(input).expect("valid input");
    assert_eq!(input.layer_id, ChartLayer::TapeChart.id());
    assert!(!input.visible);
    assert!(tape("on", 2, 5).is_some_and(|(_, input)| input["visible"] == true));
    assert!(tape("1", 2, 5).is_none());
}

#[test]
fn a_layout_tab_switches_to_a_known_name_and_creates_an_unknown_one() {
    assert_eq!(
        layout_tab("Scalp", true).0,
        layout::TAB_SWITCH_CAPABILITY_ID
    );
    let (id, input) = layout_tab("Scalp", false);
    assert_eq!(id, layout::TAB_CREATE_CAPABILITY_ID);
    assert_eq!(input, json!({ "name": "Scalp" }));
}

#[test]
fn a_switch_runs_its_call_only_on_one() {
    let id = layout::COLLAPSE_CAPABILITY_ID;
    assert_eq!(when_one(Some("1"), id), Some((id, json!({}))));
    assert_eq!(when_one(Some("0"), id), None);
    assert_eq!(when_one(Some("true"), id), None);
    assert_eq!(when_one(None, id), None);
}
