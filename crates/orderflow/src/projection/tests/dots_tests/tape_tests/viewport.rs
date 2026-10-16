//! Display normalization follows final visible groups, never the merger's scale.

use super::*;
use crate::projection::{TapeBubbleDisplay, TapeDotMemory, TapeDotView};
use crate::{BubbleSizeReference, ViewportBubbleScale};

#[test]
fn a_collapsed_tape_span_never_grows_its_bubbles_back_to_full_size() {
    let mut config = style();
    config.bubbles.max_radius = 15.0;
    let mut previous = f32::INFINITY;
    for width in [31.0, 30.1, 30.01, 30.0, 24.0] {
        let geometry =
            crate::projection::TapeHorizontalGeometry::resolve(width, 600.0, &config.bubbles);
        let display = TapeBubbleDisplay::resolve(
            &config.bubbles,
            std::iter::empty(),
            f64::from(geometry.span_px) / 6000.0,
            4.0,
            geometry.max_radius,
        )
        .unwrap();
        let radius = display.draw(Decimal::ONE).1;
        assert!(
            radius <= previous,
            "shrinking width {width} must not enlarge the bubbles"
        );
        if width <= 30.0 {
            assert_eq!(radius, 0.0, "no time span remains between the fixed insets");
        }
        previous = radius;
    }
}

fn style() -> HeatmapConfig {
    let mut config = tape_config();
    config.live_lane.stable_size = true;
    config.bubbles.size_reference = BubbleSizeReference::Fixed;
    config.bubbles.size_reference_quantity = 8.0;
    config.bubbles.viewport_scale = Some(ViewportBubbleScale {
        x_units_per_pixel: 200.0,
        price_units_per_pixel: 1.0,
    });
    config
}

fn sizing() -> DotSizing {
    DotSizing {
        native_tape: true,
        tape_column_px: 1.0,
        candle_column_px: 1.0,
        px_per_price: 1.0,
        typed_full: Some(Decimal::ONE),
    }
}

fn camera() -> TapeDotView {
    TapeDotView {
        now_ms: 20_000,
        window_ms: 20_000,
        dot_window_ms: 100,
        evicted_through_ms: None,
        prices: prices("90", "110"),
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 600.0,
            height_px: 400.0,
        },
    }
}

fn visible(marks: &[AggressionPrimitive]) -> impl Iterator<Item = &AggressionPrimitive> {
    marks
        .iter()
        .filter(|mark| (0.0..=1.0).contains(&mark.x) && (0.0..=1.0).contains(&mark.y))
}

#[test]
fn visible_tape_areas_preserve_group_facts_and_camera_round_trip() {
    let config = style();
    let history = recorded(
        config.clone(),
        &[
            (1, 3_011, "95", "1", Side::Buy),
            (2, 8_011, "100", "4", Side::Sell),
            (3, 13_011, "105", "16", Side::Buy),
        ],
    );
    let timeline = chart(20_000, 20_000, None);
    let source = frame_at(&history, &timeline, camera().prices, &tape_dots(100, 1));
    let native: Vec<_> = source
        .aggressions
        .into_iter()
        .filter(|mark| mark.live)
        .collect();
    let mut memory = TapeDotMemory::default();
    let frame = memory.project(
        &native,
        camera(),
        sizing(),
        &config.bubbles,
        &config.live_lane,
        &[],
    );
    let facts = frame.marks.clone();
    assert_eq!(facts.len(), 3);
    assert_eq!(
        facts.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        dec("21")
    );
    assert_eq!(
        facts.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
        dec("17")
    );
    let resolve = |px_per_ms, px_per_price, envelope| {
        TapeBubbleDisplay::resolve(
            &config.bubbles,
            visible(&frame.marks),
            px_per_ms,
            px_per_price,
            envelope,
        )
        .unwrap()
    };
    let a = resolve(0.02, 20.0, 15.0);
    assert_eq!(a.full_quantity, dec("16"));
    let radii = [1, 4, 16].map(|q| a.draw(q.into()).1);
    assert!((radii[1] / radii[0] - 2.0).abs() < 0.00001);
    assert!((radii[2] / radii[1] - 2.0).abs() < 0.00001);
    for b in [
        resolve(0.01, 20.0, 15.0),
        resolve(0.02, 10.0, 15.0),
        resolve(0.02, 20.0, 5.0),
    ] {
        assert!(b.max_radius < a.max_radius);
        for q in [1, 4, 16] {
            assert!(
                (b.draw(q.into()).1 / a.draw(q.into()).1 - b.max_radius / a.max_radius).abs()
                    < 0.00001
            );
        }
        assert_eq!(resolve(0.02, 20.0, 15.0), a);
    }
    assert_eq!(frame.marks, facts);
    let legacy = crate::BubbleStyle {
        viewport_scale: None,
        ..config.bubbles.clone()
    };
    let legacy_frame = TapeDotMemory::default().project(
        &native,
        camera(),
        sizing(),
        &legacy,
        &config.live_lane,
        &[],
    );
    assert_eq!(
        legacy_frame.marks, frame.marks,
        "display policy cannot change membership, sides, quantities or centers"
    );
    assert_eq!(legacy_frame.full_quantity, frame.full_quantity);
    assert_eq!(legacy_frame.max_radius, frame.max_radius);
    assert!(TapeBubbleDisplay::resolve(&legacy, visible(&frame.marks), 0.02, 20.0, 15.0).is_none());
}

#[test]
fn visible_tape_reference_ignores_hidden_whales_and_other_panes() {
    let config = style();
    let (_, _, mut marks) = fixture(3_400);
    marks[0].quantity = dec("4");
    marks[0].x = 0.5;
    marks[0].y = 0.5;
    let mut whale = marks[0].clone();
    whale.quantity = dec("1000");
    for (live, x, y) in [(true, -0.1, 0.5), (true, 0.5, 1.1), (false, 0.5, 0.5)] {
        whale.live = live;
        whale.x = x;
        whale.y = y;
        marks.truncate(1);
        marks.push(whale.clone());
        let scale =
            TapeBubbleDisplay::resolve(&config.bubbles, visible(&marks), 0.02, 20.0, 15.0).unwrap();
        assert_eq!(scale.full_quantity, dec("4"));
    }
    whale.live = true;
    whale.x = 0.5;
    whale.y = 0.5;
    marks[1] = whale;
    let scale =
        TapeBubbleDisplay::resolve(&config.bubbles, visible(&marks), 0.02, 20.0, 15.0).unwrap();
    assert_eq!(scale.full_quantity, dec("1000"));
    assert!(scale.draw(dec("4")).1 < scale.draw(dec("1000")).1);
}

/// The Tape gain magnifies the drawn disc, so the time and price insets pad
/// for the magnified disc; where the pane cannot fit it the held gain is
/// lowered instead, and gain 1 resolves exactly what it always did.
#[test]
fn the_tape_insets_hold_the_magnified_largest_disc() {
    let mut config = style();
    config.bubbles.max_radius = 15.0;
    config.bubbles.outline_width = 0.0;
    let at = |gain: f32, width: f32| {
        let mut bubbles = config.bubbles.clone();
        bubbles.display_scale.tape = gain;
        crate::projection::TapeHorizontalGeometry::resolve(width, 600.0, &bubbles)
    };
    let plain = at(1.0, 300.0);
    assert_eq!(plain.display_gain, 1.0);
    for gain in [1.37, 3.0] {
        let geometry = at(gain, 300.0);
        assert_eq!(geometry.max_radius, plain.max_radius, "sizing is untouched");
        assert_eq!(geometry.display_gain, gain);
        let drawn = geometry.max_radius * geometry.display_gain;
        assert!(geometry.inset_px >= drawn && geometry.price_inset_px >= drawn);
        assert!(geometry.inset_px > plain.inset_px);
    }
    let narrow = at(3.0, 60.0);
    assert!(narrow.display_gain < 3.0, "a narrow pane holds less gain");
    assert!(narrow.inset_px >= narrow.max_radius * narrow.display_gain);
    assert_eq!(at(0.5, 300.0).display_gain, 0.5);
}
