use super::*;
use crate::config::dressing::flow::{CONTEXT_OPACITY, PEAK_OPACITY, ordinary_colors};

#[test]
fn caption_reports_current_reference() {
    let mut frame = frame(&[trade(1000, 100, 32100, Side::Buy)], 200.0);
    frame.effective_reference = Some(32100.into());
    let text = caption_text(Some(&frame), FlowProgress::default(), true).unwrap();
    assert!(text.contains("scale 32.1K"));
    assert!(text.contains("brightness rises with regional volume"));
    assert!(text.contains("area = regional gross volume"));
    frame.effective_reference = Some(450.into());
    assert!(
        caption_text(Some(&frame), FlowProgress::default(), true)
            .unwrap()
            .contains("scale 450")
    );
    assert!(caption_text(Some(&frame), FlowProgress::default(), false).is_none());
}

#[test]
fn placement_threshold_is_exact_and_absent_without_a_positive_reference() {
    let mut frame = frame(&[trade(1000, 100, 13, Side::Buy)], 200.0);
    frame.effective_reference = Some(13.into());
    assert_eq!(frame.large_region_threshold(), Some(Decimal::new(325, 2)));
    for reference in [None, Some(Decimal::ZERO), Some((-1).into())] {
        frame.effective_reference = reference;
        assert_eq!(frame.large_region_threshold(), None);
    }
}

#[test]
fn ordinary_brightness_is_gradual_above_quarter_reference_and_preserves_endpoints() {
    let mut frame = frame(&[trade(1000, 100, 400, Side::Buy)], 200.0);
    frame.effective_reference = Some(400.into());
    let before = frame.clone();
    let opacity = |quantity| {
        let mut dot = frame.dots[0].clone();
        dot.mark.quantity = Decimal::from(quantity);
        frame.ordinary_region_opacity(&dot)
    };
    let levels = [0, 99, 100, 101, 200, 250, 300, 399, 400, 401].map(opacity);
    assert!(levels.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(opacity(0), CONTEXT_OPACITY);
    assert_eq!(opacity(100), CONTEXT_OPACITY);
    assert!((opacity(250) - 0.385).abs() < 0.000001);
    assert_eq!(opacity(400), PEAK_OPACITY);
    assert_eq!(opacity(401), PEAK_OPACITY);
    for (before, after) in [(100, 101), (399, 400)] {
        let step = opacity(after) - opacity(before);
        assert!(step > 0.0 && step < 0.002, "no binary brightness jump");
    }
    assert_eq!(frame, before);
}

#[test]
fn equal_volume_keeps_its_colour_when_camera_geometry_and_radius_change() {
    let mut frame = frame(
        &[
            trade(1000, 100, 200, Side::Buy),
            trade(2000, 101, 200, Side::Sell),
        ],
        200.0,
    );
    frame.effective_reference = Some(400.into());
    let expected = ordinary_colors(
        [19, 23, 34, 255],
        frame.ordinary_region_opacity(&frame.dots[0]),
    );
    frame.view.width_px *= 8.0;
    frame.view.height_px *= 16.0;
    frame.dots[1].candle_position += Decimal::from(100);
    frame.dots[1].mark.x = 500.0;
    frame.dots[1].mark.y = -200.0;
    frame.dots[1].radius *= 3.0;
    assert_eq!(
        ordinary_colors(
            [19, 23, 34, 255],
            frame.ordinary_region_opacity(&frame.dots[1])
        ),
        expected
    );
}

#[test]
fn missing_reference_uses_ordinary_volume_and_extreme_ratios_stay_finite() {
    let mut frame = frame(
        &[
            trade(1000, 100, 10000, Side::Buy),
            trade(2000, 100, 400, Side::Sell),
            trade(3000, 100, 200, Side::Buy),
        ],
        200.0,
    );
    frame.dots[0].opening_oversized = true;
    frame.effective_reference = Some(400.into());
    let expected = frame.ordinary_region_opacity(&frame.dots[2]);
    for reference in [None, Some(Decimal::ZERO), Some(-Decimal::ONE)] {
        frame.effective_reference = reference;
        assert_eq!(frame.ordinary_region_opacity(&frame.dots[2]), expected);
    }
    let mut dot = frame.dots[2].clone();
    for (quantity, reference, expected) in [
        (Decimal::MAX, Decimal::new(1, 28), PEAK_OPACITY),
        (Decimal::new(1, 28), Decimal::MAX, CONTEXT_OPACITY),
        (Decimal::MAX, Decimal::MAX, PEAK_OPACITY),
        (-Decimal::ONE, Decimal::ONE, CONTEXT_OPACITY),
    ] {
        dot.mark.quantity = quantity;
        frame.effective_reference = Some(reference);
        let opacity = frame.ordinary_region_opacity(&dot);
        assert!(opacity.is_finite());
        assert_eq!(opacity, expected);
    }
    frame.effective_reference = None;
    frame
        .dots
        .iter_mut()
        .for_each(|dot| dot.opening_oversized = true);
    assert_eq!(frame.ordinary_region_opacity(&dot), CONTEXT_OPACITY);
    frame.dots.clear();
    assert_eq!(frame.ordinary_region_opacity(&dot), CONTEXT_OPACITY);
}

#[test]
fn oversized_region_caption_remains_visible_and_separates_total_from_opening() {
    let mut frame = frame(&[trade(1000, 100, 32100, Side::Buy)], 200.0);
    frame.dots[0].opening_oversized = true;
    frame.dots[0].opening_quantity = 31000.into();
    let text = caption_text(Some(&frame), FlowProgress::default(), false).unwrap();
    assert!(text.contains("total 32100, recorded opening 31000"));
    assert!(text.contains("first daily regions"));
    assert!(text.contains("area proportional"));
}
