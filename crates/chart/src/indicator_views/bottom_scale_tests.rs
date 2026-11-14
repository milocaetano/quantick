use super::super::*;
use quantick_indicator_session::IndicatorEvent;
use quantick_indicators::{Indicator, PreviewFrame, native::Volume};

fn views() -> IndicatorViews {
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("native.volume");
    views.apply(IndicatorEvent::Rebuilt {
        slot,
        descriptor: Volume::default().descriptor().clone(),
        columns: vec![
            vec![10.0, f64::NAN, 1000.0],
            vec![f64::NAN, 20.0, f64::NAN],
            vec![f64::NAN; 3],
        ],
        rows: 3,
        bar_paint: Vec::new(),
        inputs: Vec::new(),
        stale: None,
    });
    views
}

#[test]
fn bottom_scale_is_zero_anchored_visible_only_and_keeps_overlay_out_of_panes() {
    let views = views();
    assert_eq!(views.visible_overlays().count(), 1);
    assert_eq!(views.visible_panes().count(), 0);
    let scale = views.all()[0]
        .bottom_scale(0, 2, None, 100.0, 500.0)
        .unwrap();
    assert_eq!(scale.y(0.0), 500.0);
    assert_eq!(scale.y(20.0), 400.0);
}

#[test]
fn scale_includes_only_aligned_visible_preview_and_visible_plots() {
    let mut views = views();
    let view = &mut views.views[0];
    view.preview = Some(PreviewFrame::new(vec![2000.0, f64::NAN, f64::NAN]));
    assert_eq!(
        view.bottom_scale(0, 2, None, 0.0, 400.0).unwrap().y(20.0),
        300.0
    );
    assert_eq!(
        view.bottom_scale(0, 4, Some(3), 0.0, 400.0)
            .unwrap()
            .y(2000.0),
        300.0
    );
    assert_eq!(
        view.bottom_scale(0, 3, Some(4), 0.0, 400.0)
            .unwrap()
            .y(1000.0),
        300.0
    );
    view.style.set(
        0,
        crate::indicator_style::PlotOverride {
            visible: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(
        view.bottom_scale(0, 4, Some(3), 0.0, 400.0)
            .unwrap()
            .y(20.0),
        300.0
    );
}

#[test]
fn capability_units_and_unavailable_are_explicit_but_zero_is_available() {
    let mut views = views();
    views.label_volume(false);
    assert_eq!(views.all()[0].label(), "Volume · Ticks");
    views.label_volume(true);
    assert_eq!(views.all()[0].label(), "Volume");
    views.views[0].preview = Some(PreviewFrame::new(vec![f64::NAN; 3]));
    views.label_volume(false);
    assert_eq!(views.all()[0].label(), "Volume · Ticks · Unavailable");
    views.views[0].preview = Some(PreviewFrame::new(vec![0.0, f64::NAN, f64::NAN]));
    views.label_volume(false);
    assert_eq!(views.all()[0].label(), "Volume · Ticks");
}

#[test]
fn headline_follows_falling_and_forming_volume_without_stale_fallback() {
    let mut views = views();
    views.views[0].preview = Some(PreviewFrame::new(vec![f64::NAN, 37.0, 20.0]));
    assert_eq!(views.all()[0].headline_value(), Some(37.0));
    views.views[0].preview = Some(PreviewFrame::new(vec![f64::NAN; 3]));
    assert_eq!(views.all()[0].headline_value(), None);
}

#[test]
fn offscreen_preview_does_not_flatten_visible_history() {
    let mut views = views();
    views.views[0].preview = Some(PreviewFrame::new(vec![1e9, f64::NAN, f64::NAN]));
    let scale = views.all()[0]
        .bottom_scale(0, 2, Some(3), 0.0, 400.0)
        .unwrap();
    assert_eq!(scale.y(20.0), 300.0);
}

#[test]
fn fractional_and_zero_volume_keep_the_same_zero_baseline() {
    let mut views = views();
    views.views[0].columns = vec![vec![1e-20, 0.0], vec![f64::NAN; 2], vec![f64::NAN; 2]];
    let scale = views.all()[0].bottom_scale(0, 2, None, 0.0, 400.0).unwrap();
    assert_eq!(scale.y(1e-20), 300.0);
    assert_eq!(scale.y(0.0), 400.0);
    let zero = views.all()[0].bottom_scale(1, 2, None, 0.0, 400.0).unwrap();
    assert_eq!(zero.y(0.0), 400.0);
}
