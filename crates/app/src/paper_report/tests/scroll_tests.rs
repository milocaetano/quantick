//! Real report layout and ordinary wheel input, with no assigned scroll offset.
use super::*;

fn text_rect(
    shapes: &[egui::epaint::ClippedShape],
    text: &str,
) -> Option<(egui::Rect, egui::Rect)> {
    shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(painted) if painted.galley.text() == text => {
            Some((painted.visual_bounding_rect(), shape.clip_rect))
        }
        _ => None,
    })
}

#[test]
fn enlarged_populated_report_footer_is_reached_by_wheel_input() {
    let ctx = egui::Context::default();
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(&format!("{}\n[typography.body]\nsize = 25\n[typography.monospace]\nsize = 24\n[typography.small]\nsize = 18", quantick_appearance::LIGHT)).unwrap();
    crate::theme::sync(&ctx, &appearance);
    assert_eq!(crate::theme::get(&ctx).body_font(12.5).size, 25.0);
    let mut state = ReportState {
        report_open: true,
        ..Default::default()
    };
    state.calendar.open = true;
    state.report = Some(LoadedHistory {
        rows: (0..40)
            .map(|i| {
                row(
                    if i == 39 { "LAST" } else { "TEST" },
                    None,
                    trade_at(1_792_580_296_000 + i * 1000, i + 1),
                )
            })
            .collect(),
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
        sessions: Default::default(),
    });
    let env = ReportEnv {
        symbol: "TEST",
        dir: std::path::Path::new("."),
        session_journal_paths: &[],
        session_trades: &[],
        session_trade_at: &[],
        session_trade_sources: &[],
        open: None,
        pnl: PnlReader::plain(),
    };
    let footnote = env.pnl.footnote();
    let mut initial = None;
    let mut final_footer = None;
    state.arm_scroll_probe("report:-20,rows:-24");
    let mut saw_last_row = false;
    let mut wheel_events = 0;
    for frame in 0..1000 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        state.push_scroll_probe(&mut input);
        wheel_events += input
            .events
            .iter()
            .filter(|event| matches!(event, egui::Event::MouseWheel { .. }))
            .count();
        let output = ctx.run(input, |ctx| {
            state.draw_window(ctx, TzOffset::new(0), &env);
        });
        if frame == 100 {
            initial = state.scroll_probe.1;
        }
        saw_last_row |=
            text_rect(&output.shapes, "LAST").is_some_and(|(rect, clip)| clip.contains_rect(rect));
        final_footer = text_rect(&output.shapes, &footnote);
    }
    let (before, clip_before) = initial.expect("the layout emits its clipped footer");
    assert!(
        state
            .scroll_probe
            .1
            .is_some_and(|(rect, clip)| clip.contains_rect(rect)),
        "wheel path must expose footer: before={initial:?} after={:?}, events={wheel_events}",
        state.scroll_probe.1
    );
    let (after, clip_after) = final_footer.expect("footer remains painted");
    assert!(
        !clip_before.contains_rect(before),
        "the fixture must require scrolling: {before:?} in {clip_before:?}"
    );
    assert!(
        clip_after.contains_rect(after),
        "normal wheel input must expose the complete footer: {after:?} in {clip_after:?}"
    );
    assert!(
        wheel_events <= 44 && wheel_events > 20,
        "bounded sequential wheel delivery: {wheel_events}"
    );
    assert!(
        saw_last_row,
        "actual nested trade viewport must expose its last row"
    );
    assert!(after.top() < before.top());
}
