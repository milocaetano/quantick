use quantick_layers::ChartLayer;
#[test]
fn every_menu_string_reads_as_a_sentence() {
    for layer in ChartLayer::ALL {
        for (what, text) in [("label", layer.label()), ("hint", layer.hint())] {
            assert!(
                !text.contains("  "),
                "{}'s {what} has a hole in it: {text:?}",
                layer.id()
            );
            assert!(
                !text.trim().is_empty() && text.trim() == text,
                "{}'s {what} is padded or empty: {text:?}",
                layer.id()
            );
        }
    }
}

#[test]
fn ids_are_unique_and_round_trip_through_from_id() {
    let mut ids: Vec<&str> = ChartLayer::ALL.iter().map(|layer| layer.id()).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count, "two layers share a persisted id");
    for layer in ChartLayer::ALL {
        assert_eq!(ChartLayer::from_id(layer.id()), Some(layer));
    }
    assert_eq!(ChartLayer::from_id("no_such_layer"), None);
}
