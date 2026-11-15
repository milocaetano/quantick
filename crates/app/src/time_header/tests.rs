use super::*;

/// The default has to be one of the offered presets, or the pane opens on
/// a value its bars menu lists under Custom… alone.
#[test]
fn the_default_interval_is_one_of_the_named_presets() {
    assert!(
        quantick_engine::bar_registry::TIME_PRESETS
            .iter()
            .any(|(_, ms)| *ms == DEFAULT_INTERVAL_MS)
    );
}

/// A strip too narrow for the bars control cuts it: the indicators button
/// after it stays whole, or a hand could not reach it.
#[test]
fn a_narrow_strip_keeps_the_indicators_button_whole() {
    let ctx = egui::Context::default();
    let mut strip = egui::Rect::NOTHING;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(600.0, 200.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            strip = egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(160.0, HEIGHT_PX));
            draw(ui, strip, 0, 0.0, |ui| {
                ui.label("Time · 1m and a long tail past the strip");
            });
        });
    });
    let button: egui::Rect = ctx
        .data(|data| data.get_temp(crate::pane_header::indicators_id(0)))
        .expect("the button drew");
    assert!(strip.contains_rect(button), "{button:?} left {strip:?}");
}

/// Every preset the bars menu lists is a value the interval drag beside it
/// accepts, and reads as the spec it sets.
#[test]
fn every_preset_lies_inside_the_shared_interval_domain() {
    for (label, ms) in quantick_engine::bar_registry::TIME_PRESETS {
        assert!(
            quantick_engine::bar_registry::is_time_interval(ms),
            "{label} is outside the interval domain the drag shares"
        );
        assert_eq!(
            quantick_engine::fmt_time_interval(ms),
            label,
            "a preset reads as the spec it sets"
        );
    }
}

/// A typed `1mo` or `3mo` sets the month, never four weeks; a drag past
/// four weeks stops there, and below the shortest interval at it.
#[test]
fn a_typed_month_sets_the_month_and_never_four_weeks() {
    use quantick_engine::time_bucket::admit_interval;
    let (min, max) = (
        crate::state::MIN_TIME_INTERVAL_MS,
        crate::state::MAX_TIME_INTERVAL_MS,
    );
    for typed in ["1mo", "3mo", "12mo"] {
        let ms = quantick_engine::parse_time_interval(typed).expect("an interval");
        assert_eq!(admit_interval(ms, min, max), ms, "{typed}");
        assert_eq!(quantick_engine::fmt_time_interval(ms), typed);
    }
    assert_eq!(admit_interval(max + 1, min, max), max, "four weeks at most");
    assert_eq!(admit_interval(0, min, max), min);
    let week = quantick_engine::time_bucket::WEEK_MS;
    assert_eq!(admit_interval(week, min, max), week);
}
