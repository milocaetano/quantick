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
    assert_eq!(number_in("zone-12", "zone"), Some(12));
    for name in [
        "zone-0", "zone-01", "zone-", "zone12", "zone-1a", "zones-1", "line-1",
    ] {
        assert_eq!(number_in(name, "zone"), None, "{name}");
    }
}

#[test]
fn the_next_name_starts_above_the_highest_used_and_never_repeats() {
    let mut floors = NameFloors::new();
    raise_floors(
        &mut floors,
        [
            ("rectangle", Some("zone-3")),
            ("rectangle", Some("congestion 108k")),
            ("trend-line", None),
        ],
    );
    assert_eq!(next_name(&mut floors, "rectangle"), "zone-4");
    assert_eq!(next_name(&mut floors, "rectangle"), "zone-5");
    assert_eq!(next_name(&mut floors, "trend-line"), "line-1");
    assert_eq!(next_name(&mut floors, "horizontal-line"), "line-2");
}
