//! Borrowed renderer input preserves retained and stateless tape output.

use super::*;
use crate::config::theme::OrderflowRenderStyle;
use crate::projection::{TapeDotFrame, TapeDotMemory, project_tape_frame};
use std::borrow::Cow;

fn assert_same_frame(actual: &TapeDotFrame, expected: &TapeDotFrame) {
    assert_eq!(actual.marks, expected.marks);
    assert_eq!(actual.max_radius, expected.max_radius);
    assert_eq!(actual.full_quantity, expected.full_quantity);
}

#[test]
fn borrowed_and_owned_tape_input_keep_the_same_complete_retained_frame() {
    let mut config = tape_config();
    config.volume_dots.ignore_opening_burst_in_scale = true;
    let history = tape(config.clone(), &[
        (1, 1_801, "95", "4", Side::Buy),
        (2, 3_011, "100", "0.1", Side::Buy),
        (3, 3_052, "100", "0.3", Side::Sell),
    ]);
    let timeline = chart(3_060, 1_500, None);
    let prices = prices("90", "110");
    let native = frame_at(&history, &timeline, prices, &tape_dots(100, 1));
    let original = native.aggressions.clone();
    let mut style = OrderflowRenderStyle::from_config(&config, [0, 0, 0, 255]);
    style.dot_sizing = Some(DotSizing { tape_column_px: 1.0, candle_column_px: 8.0, px_per_price: 20.0, typed_full: None });
    let geometry = TapeDotGeometry {
        left_x: timeline.locate_in_lane_clamped(1_560).unwrap().normalized,
        right_x: 1.0,
        width_px: 640.0,
        height_px: 400.0,
    };
    let mut owned_memory = TapeDotMemory::default();
    let mut borrowed_memory = TapeDotMemory::default();
    for now_ms in [3_060, 3_070, 3_110, 3_400] {
        let edge = crate::LiveEdge { now_ms, window_ms: 1_500, reference_ms: 1_500, on_newest_bar: true };
        let owned = project_tape_frame(native.aggressions.clone(), Some(&mut owned_memory), &style, geometry, Some((edge, 100)), Some(prices), native.tape_facts.as_deref()).unwrap();
        let borrowed = project_tape_frame(native.aggressions.as_slice(), Some(&mut borrowed_memory), &style, geometry, Some((edge, 100)), Some(prices), native.tape_facts.as_deref()).unwrap();
        assert_same_frame(&borrowed, &owned);
        assert_eq!(native.aggressions, original, "repositioning never mutates borrowed input");
    }
    let cow: Cow<'_, [AggressionPrimitive]> = Cow::Borrowed(&native.aggressions);
    let from_cow = project_tape_frame(cow, Some(&mut borrowed_memory), &style, geometry, None, Some(prices), native.tape_facts.as_deref()).unwrap();
    let from_vec = project_tape_frame(native.aggressions.clone(), None, &style, geometry, None, Some(prices), native.tape_facts.as_deref()).unwrap();
    assert_same_frame(&from_cow, &from_vec);
    assert_eq!(native.aggressions, original);
}

#[test]
fn stateless_tape_preview_copies_borrowed_input_before_repositioning() {
    let (config, _, marks) = fixture(3_060);
    let original = marks.clone();
    let mut style = OrderflowRenderStyle::from_config(&config, [0, 0, 0, 255]);
    style.dot_sizing = Some(DotSizing { tape_column_px: 1.0, candle_column_px: 8.0, px_per_price: 20.0, typed_full: None });
    let geometry = TapeDotGeometry { left_x: 0.0, right_x: 1.0, width_px: 640.0, height_px: 400.0 };
    let edge = crate::LiveEdge { now_ms: 3_400, window_ms: 1_500, reference_ms: 1_500, on_newest_bar: true };
    let current_prices = prices("0", "200");
    let owned = project_tape_frame(marks.clone(), None, &style, geometry, Some((edge, 250)), Some(current_prices), None).unwrap();
    let borrowed = project_tape_frame(marks.as_slice(), None, &style, geometry, Some((edge, 250)), Some(current_prices), None).unwrap();
    assert_same_frame(&borrowed, &owned);
    assert_ne!(borrowed.marks[0].x, original[0].x, "the fallback actually repositions");
    assert_eq!(marks, original);
    style.dot_sizing = None;
    assert!(project_tape_frame(marks.as_slice(), None, &style, geometry, None, None, None).is_none());
}
