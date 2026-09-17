use quantick_layers::{
    ChartLayer as L, LayerFacts, LayerRegistry, LayerSource, LayerState, OrderflowSwitch,
    SavedLayers, blocks,
};

#[test]
fn opening_visibility_is_not_an_undoable_change_and_cannot_claim_shared_or_preset_state() {
    use quantick_layers::VisibilityWrite;
    let mut state = LayerState::default();
    let facts = LayerFacts::default();
    let opening = state
        .initialize(L::Drawings, false, facts)
        .unwrap()
        .unwrap();
    assert_eq!(opening.source, LayerSource::Drawings);
    assert_eq!(opening.write, VisibilityWrite::Opening);
    assert!(!opening.visible);
    assert_eq!(
        state.set(L::Drawings, false).unwrap().unwrap().write,
        VisibilityWrite::Change
    );
    assert_eq!(state.initialize(L::Grid, false, facts), Ok(None));
    assert_eq!(state.initialize(L::Heatmap, false, facts), Ok(None));
    assert_eq!(
        state.initialize(
            L::LaneMarks,
            false,
            LayerFacts {
                flow_pane: true,
                ..facts
            }
        ),
        Ok(None)
    );
    assert_eq!(state.initialize(L::Crosshair, false, facts), Ok(None));
    assert!(!state.requested(L::Crosshair));
}

#[test]
fn live_strip_eligibility_owns_source_policy_but_not_renderer_width() {
    let mut state = LayerState::default();
    state.set(L::LiveStrip, true).unwrap();
    let requested = state.requested(L::LiveStrip);
    for (book_capture, traded_volume, expected) in [
        (false, false, false),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let facts = LayerFacts {
            flow_pane: true,
            book_capture,
            traded_volume,
            ..Default::default()
        };
        assert_eq!(
            LayerState::effective(L::LiveStrip, requested, facts),
            expected
        );
        assert!(!LayerState::effective(
            L::LiveStrip,
            requested,
            LayerFacts {
                flow_pane: false,
                ..facts
            }
        ));
    }
    assert!(
        state.requested(L::LiveStrip),
        "unavailability never erases the requested choice"
    );
}

#[test]
fn capability_reason_precedence_and_requested_depth_are_preserved() {
    let mut facts = LayerFacts::default();
    assert_eq!(
        LayerState::blocked(L::TapeHeatmap, facts),
        Some(blocks::WRONG_PANE)
    );
    facts.flow_pane = true;
    assert_eq!(
        LayerState::blocked(L::TapeHeatmap, facts),
        Some(blocks::TAPE_OFF)
    );
    facts.tape_on = true;
    assert_eq!(
        LayerState::blocked(L::TapeHeatmap, facts),
        Some(blocks::NO_BOOK)
    );
    facts.book_capture = true;
    assert_eq!(LayerState::blocked(L::TapeHeatmap, facts), None);
    assert!(!LayerState::visible(L::TapeHeatmap, true, facts));
    facts.capture_enabled = true;
    assert!(LayerState::visible(L::TapeHeatmap, true, facts));
    assert_eq!(
        LayerState::blocked(L::BookStatus, facts),
        Some(blocks::DEPTH_MAP_HIDDEN)
    );
}

#[test]
fn local_switches_and_external_effects_have_one_authority() {
    let mut state = LayerState::default();
    assert!(!state.requested(L::BackfillDivider));
    assert!(state.requested(L::Crosshair));
    assert_eq!(state.set(L::Crosshair, false).unwrap(), None);
    assert!(!state.requested(L::Crosshair));
    let effect = state.set(L::Heatmap, true).unwrap().unwrap();
    assert_eq!(
        effect.source,
        LayerSource::Orderflow(OrderflowSwitch::Depth)
    );
    assert!(effect.visible);
    assert_eq!(
        state.local_mask(),
        LayerState::default().local_mask() & !LayerRegistry::default().bit(L::Crosshair).unwrap()
    );
}

#[test]
fn switching_tabs_rebaselines_without_persisting_another_charts_opinion() {
    let mut saved = SavedLayers::default();
    assert_eq!(saved.observe(10, 12), None);
    assert_eq!(saved.observe(10, 14), Some(2));
    saved.record(14);
    assert_eq!(saved.observe(10, 14), None);
    assert_eq!(saved.observe(11, 0), None);
    assert_eq!(saved.observe(11, 1), Some(1));
}

#[test]
fn scope_and_persistence_filter_restores_without_overwriting_other_owners() {
    let facts = LayerFacts::default();
    assert!(!LayerState::restorable(L::Grid, facts));
    assert!(!LayerState::restorable(L::Heatmap, facts));
    assert!(LayerState::restorable(L::Footprint, facts));
    assert!(!L::LaneMarks.persisted());
    assert_eq!(LayerRegistry::default().resolve("future_layer"), None);
}
