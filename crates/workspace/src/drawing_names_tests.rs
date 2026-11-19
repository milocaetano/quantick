use super::*;

#[test]
fn rectangles_are_zones_lines_are_lines_and_the_rest_keep_their_id() {
    assert_eq!(prefix_for("rectangle"), "zone");
    assert_eq!(prefix_for("trend-line"), "line");
    assert_eq!(prefix_for("horizontal-ray"), "line");
    assert_eq!(prefix_for("fib-retracement"), "fib-retracement");
}

#[test]
fn only_a_prefix_dash_and_a_positive_number_read_as_a_generated_name() {
    assert_eq!(split_generated("zone-12"), Some(("zone", 12)));
    assert_eq!(
        split_generated("fib-retracement-3"),
        Some(("fib-retracement", 3))
    );
    for name in ["zone-0", "zone-01", "zone-", "zone12", "zone-1a", "-1"] {
        assert_eq!(split_generated(name), None, "{name}");
    }
    assert!(is_generated_for("line-2", "trend-line"));
    assert!(!is_generated_for("zone-2", "trend-line"));
}

#[test]
fn the_next_name_starts_above_the_highest_used_and_never_repeats() {
    let mut floors = NameFloors::new();
    raise_floors(&mut floors, ["zone-3", "congestion 108k"]);
    assert_eq!(next_name(&mut floors, "rectangle"), "zone-4");
    assert_eq!(next_name(&mut floors, "rectangle"), "zone-5");
    assert_eq!(next_name(&mut floors, "trend-line"), "line-1");
    assert_eq!(next_name(&mut floors, "horizontal-line"), "line-2");
}

#[test]
fn a_typed_name_shaped_like_another_kinds_raises_that_kinds_floor() {
    let mut floors = NameFloors::new();
    // A line the trader called `zone-7`: the next zone must not be `zone-7`.
    raise_floors(&mut floors, ["zone-7"]);
    assert_eq!(next_name(&mut floors, "rectangle"), "zone-8");
}
