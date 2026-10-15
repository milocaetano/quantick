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
    assert!(state.frame_for(&source, look).is_none());
    state.record(&source, Some(&display), look);
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, look).unwrap(),
        &display
    ));
    let replaced = Arc::new((*source).clone());
    assert!(
        state.frame_for(&replaced, look).is_none(),
        "identical revisions do not identify an allocation"
    );
    let changed = Some((
        TickBubbleStyle {
            max_radius: 48.0,
            ..tick
        },
        HeatmapTheme::default(),
    ));
    assert!(state.frame_for(&source, changed).is_none());
    let legacy = Some((
        TickBubbleStyle {
            viewport_scale: None,
            ..tick
        },
        HeatmapTheme::default(),
    ));
    assert!(Arc::ptr_eq(
        &state.frame_for(&source, legacy).unwrap(),
        &source
    ));
    state.clear();
    assert!(state.frame_for(&source, look).is_none());
    state.record(&source, Some(&display), look);
    state.record(&source, None, look);
    assert!(state.frame_for(&source, look).is_none());
}
