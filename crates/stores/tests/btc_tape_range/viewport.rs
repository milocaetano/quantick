//! Identical public recordings through fixed and visible-relative presentation.
use super::*;
use quantick_orderflow::projection::TapeBubbleDisplay;

fn adaptive_recording(name: &str, csv: &str) -> Recording {
    let mut recording = Recording::new(name, csv);
    let preset = if name.starts_with("spot") {
        "Binance BTC Spot spheres"
    } else {
        "Binance BTC perpetual spheres"
    };
    recording.config.bubbles.viewport_scale = bubble_presets::embedded()
        .get(preset)
        .unwrap()
        .bubbles
        .viewport_scale;
    recording
}

fn display(recording: &Recording, frame: &TapeDotFrame, view: TapeDotView) -> TapeBubbleDisplay {
    let price_span = (view.prices.high - view.prices.low).to_f64().unwrap();
    TapeBubbleDisplay::resolve(
        &recording.config.bubbles,
        frame.marks.iter().filter(|mark| {
            visible(mark, view) && mark.price >= view.prices.low && mark.price <= view.prices.high
        }),
        f64::from(view.geometry.width_px) / view.window_ms as f64,
        f64::from(view.geometry.height_px) / price_span,
        frame.max_radius.min(view.geometry.width_px / 2.0),
    )
    .unwrap()
}

fn visible(mark: &AggressionPrimitive, view: TapeDotView) -> bool {
    (view.geometry.left_x..=view.geometry.right_x).contains(&mark.x)
        && (0.0..=1.0).contains(&mark.y)
}

#[test]
fn btc_visible_presentation_preserves_recorded_groups_sides_and_replay() {
    for (name, csv) in recordings() {
        let fixed = Recording::new(name, csv);
        let adaptive = adaptive_recording(name, csv);
        let (_, fixed_middle, fixed_frame) = fixed.replay();
        let (_, middle, frame) = adaptive.replay();
        assert_eq!(middle.marks, fixed_middle.marks, "{name}: causal prefix");
        assert_eq!(frame.marks, fixed_frame.marks, "{name}: final groups");
        let ids = assert_facts(&adaptive, &frame.marks);
        assert_eq!(ids.len(), adaptive.prints.len(), "{name}");
        let view = adaptive.view(adaptive.start_ms + WINDOW_MS, 640.0, 400.0);
        let normal = display(&adaptive, &frame, view);
        let largest = frame.marks.iter().map(|mark| mark.quantity).max().unwrap();
        assert_eq!(
            normal.full_quantity, largest,
            "{name}: final visible maximum"
        );
        for mark in &frame.marks {
            let radius = normal.draw(mark.quantity).1;
            let expected = (mark.quantity / largest).to_f64().unwrap();
            let actual = f64::from(radius / normal.max_radius).powi(2);
            assert!(
                (actual - expected).abs() < 0.000001,
                "{name}: pure proportional area"
            );
        }
        let mut compressed = view;
        compressed.geometry.width_px *= 0.5;
        compressed.geometry.height_px *= 0.5;
        let smaller = display(&adaptive, &frame, compressed);
        assert!(
            smaller.max_radius < normal.max_radius,
            "{name}: compressed camera"
        );
        let common = smaller.max_radius / normal.max_radius;
        for mark in &frame.marks {
            assert!(
                (smaller.draw(mark.quantity).1 / normal.draw(mark.quantity).1 - common).abs()
                    < 0.000001
            );
        }
        assert_eq!(
            display(&adaptive, &frame, view),
            normal,
            "{name}: camera A-B-A"
        );
        let (_, replay_middle, replay_frame) = adaptive.replay();
        assert_eq!(replay_middle.marks, middle.marks, "{name}: replay prefix");
        assert_eq!(replay_frame.marks, frame.marks, "{name}: replay final");
        assert_eq!(display(&adaptive, &replay_frame, view), normal);
    }
}

#[test]
#[ignore = "manual identical-recording diameter and presentation-cost report; run without builds"]
fn report_viewport_btc_tape_range() {
    eprintln!(
        "BTC_VIEWPORT quantiles=min,p10,p25,p50,p75,p90,p95,max; logical_pixel_diameters; presentation timing excludes aggregation and app frames"
    );
    for (name, csv) in recordings() {
        let fixed = Recording::new(name, csv);
        let adaptive = adaptive_recording(name, csv);
        let (memory, _, _) = adaptive.replay();
        for (camera, width, height, window) in [
            ("normal", 640.0, 400.0, 60_000),
            ("compressed_y", 640.0, 200.0, 60_000),
            ("narrow", 250.0, 400.0, 60_000),
            ("zoom_in", 640.0, 800.0, 30_000),
        ] {
            let mut view = adaptive.view(adaptive.start_ms + WINDOW_MS, width, height);
            view.window_ms = window;
            let current = adaptive.draw(&mut memory.clone(), view);
            assert_facts(&adaptive, &current.marks);
            let shown: Vec<_> = current
                .marks
                .iter()
                .filter(|mark| visible(mark, view))
                .collect();
            let sizing = display(&adaptive, &current, view);
            let mut by_quantity = shown.clone();
            by_quantity.sort_by_key(|mark| mark.quantity);
            let upper_half = &by_quantity[by_quantity.len() / 2..];
            let new_diameter =
                |mark: &&AggressionPrimitive| f64::from(2.0 * sizing.draw(mark.quantity).1);
            let old_diameter =
                |mark: &&AggressionPrimitive| f64::from(fixed.diameter(&current, mark));
            let started = Instant::now();
            for _ in 0..1000 {
                std::hint::black_box(display(&adaptive, &current, view));
            }
            let resolve_us = started.elapsed().as_secs_f64() * 1000.0;
            eprintln!(
                "BTC_VIEWPORT name={name:?} camera={camera} groups={} quantity={} buys={} Qv={} gain={:.6} quantities=[{}] before_diameters=[{}] after_diameters=[{}] upper_half_before=[{}] upper_half_after=[{}] resolver_mean_us={resolve_us:.3}",
                shown.len(),
                shown.iter().map(|mark| mark.quantity).sum::<Decimal>(),
                shown.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
                sizing.full_quantity,
                sizing.radius_gain,
                distribution(shown.iter().map(|mark| mark.quantity.to_f64().unwrap())),
                distribution(shown.iter().map(old_diameter)),
                distribution(shown.iter().map(new_diameter)),
                distribution(upper_half.iter().map(old_diameter)),
                distribution(upper_half.iter().map(new_diameter)),
            );
        }
    }
}
