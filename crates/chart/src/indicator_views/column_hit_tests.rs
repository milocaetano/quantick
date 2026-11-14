use super::*;
use crate::indicator_views::IndicatorViews;
use quantick_indicator_session::IndicatorEvent;
use quantick_indicators::{Indicator, PreviewFrame, native::Volume};

fn fixture() -> IndicatorViews {
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("native.volume");
    views.apply(IndicatorEvent::Rebuilt {
        slot,
        descriptor: Volume::default().descriptor().clone(),
        columns: vec![
            vec![10.0, f64::NAN, 20.0, 20.0],
            vec![f64::NAN, 20.0, f64::NAN, f64::NAN],
            vec![f64::NAN; 4],
        ],
        rows: 4,
        bar_paint: Vec::new(),
        inputs: Vec::new(),
        stale: None,
    });
    views
}

fn hit(view: &IndicatorView, pick: &ColumnPick) -> bool {
    let scale = view
        .bottom_scale(
            pick.visible.start,
            pick.visible.end,
            pick.partial_slot,
            0.0,
            400.0,
        )
        .unwrap();
    (0..2).any(|index| {
        view.column_hit(index, pick, |row| row as f32 * 20.0, |v| scale.y(v)) == Some(true)
    })
}

#[test]
fn alternating_column_bodies_hit_but_invisible_connections_miss() {
    let views = fixture();
    let view = &views.all()[0];
    let mut pick = ColumnPick {
        visible: 0..4,
        partial_slot: None,
        candle_width: 10.0,
        pointer: (0.0, 375.0),
    };
    assert!(hit(view, &pick));
    pick.pointer = (20.0, 350.0);
    assert!(hit(view, &pick));
    pick.pointer = (50.0, 300.0);
    assert!(
        !hit(view, &pick),
        "no top-to-top segment between same-color bars"
    );
    pick.pointer = (44.0, 350.0);
    assert!(hit(view, &pick), "painted 0.8 candle-width boundary");
    pick.pointer.0 += 0.01;
    assert!(!hit(view, &pick));
    pick.visible = 3..4;
    pick.pointer = (40.0, 350.0);
    assert!(!hit(view, &pick), "offscreen cells cannot be picked");
}

#[test]
fn preview_requires_alignment_visibility_and_visible_plot() {
    let mut views = fixture();
    let view = &mut views.views[0];
    view.preview = Some(PreviewFrame::new(vec![40.0, f64::NAN, f64::NAN]));
    let mut pick = ColumnPick {
        visible: 0..5,
        partial_slot: Some(4),
        candle_width: 10.0,
        pointer: (80.0, 350.0),
    };
    assert!(hit(view, &pick));
    pick.partial_slot = Some(5);
    assert!(!hit(view, &pick), "worker lag must not invent a column");
    pick.partial_slot = Some(4);
    pick.visible = 0..4;
    assert!(!hit(view, &pick));
    pick.visible = 0..5;
    view.style.set(
        0,
        crate::indicator_style::PlotOverride {
            visible: Some(false),
            ..Default::default()
        },
    );
    assert!(!hit(view, &pick));
    view.hidden = true;
    pick.pointer = (20.0, 350.0);
    assert!(!hit(view, &pick));
    assert_eq!(
        view.column_hit(2, &pick, |_| 0.0, |_| 0.0),
        None,
        "SMA keeps its line picker"
    );
}
