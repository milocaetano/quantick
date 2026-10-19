use super::*;
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowReference, FlowTapeView, project_flow_tape},
};
use quantick_orderflow::{HeatmapTheme, TickBubbleStyle, TickReferenceMode};

fn source() -> Arc<FlowTapeFrame> {
    Arc::new(project_flow_tape(
        [],
        1,
        0,
        0,
        FlowTapeView {
            first_slot: 0,
            end_slot: 1,
            clip_left: 0.into(),
            clip_right: 1.into(),
            width_px: 200.0,
            height_px: 100.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::Typed(1.into()),
            radius_limit: 32.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    ))
}

#[test]
fn display_readback_requires_the_same_source_and_look_and_clears_on_reset() {
    let source = source();
    let display = Arc::new(source.viewport_relative(5.0, |_| true));
    let tick = TickBubbleStyle {
        reference_quantity: 1.0,
        reference_mode: TickReferenceMode::VisibleMax,
        viewport_scale: Some(ViewportBubbleScale {
            x_units_per_pixel: 1.0 / 22.0,
            price_units_per_pixel: 1.0,
        }),
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let look = Some((tick, HeatmapTheme::default()));
    let mut state = FlowPaintState::default();
    assert!(state.frame_for(&source, look, 1.0).is_none());
    state.record(&source, Some(&display), look, 1.0);
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, look, 1.0).unwrap(),
        &display
    ));
    let replaced = Arc::new((*source).clone());
    assert!(
        state.frame_for(&replaced, look, 1.0).is_none(),
        "identical revisions do not identify an allocation"
    );
    let changed = Some((
        TickBubbleStyle {
            max_radius: 48.0,
            ..tick
        },
        HeatmapTheme::default(),
    ));
    assert!(state.frame_for(&source, changed, 1.0).is_none());
    let legacy = Some((
        TickBubbleStyle {
            viewport_scale: None,
            ..tick
        },
        HeatmapTheme::default(),
    ));
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, legacy, 1.0).unwrap(),
        &source
    ));
    state.clear();
    assert!(state.frame_for(&source, look, 1.0).is_none());
    state.record(&source, Some(&display), look, 1.0);
    state.record(&source, None, look, 1.0);
    assert!(state.frame_for(&source, look, 1.0).is_none());
}

#[test]
fn manual_legacy_scale_requires_matching_paint_without_changing_the_look() {
    let source = source();
    let display = Arc::new((*source).clone());
    let mut state = FlowPaintState::default();
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, None, 1.0).unwrap(),
        &source
    ));
    assert!(state.frame_for(&source, None, 0.5).is_none());
    state.record(&source, Some(&display), None, 0.5);
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, None, 0.5).unwrap(),
        &display
    ));
    assert!(state.frame_for(&source, None, 2.0).is_none());
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, None, 1.0).unwrap(),
        &source
    ));
}
