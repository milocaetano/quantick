//! Native siblings share an egui Context but must retain their own panel bounds.
use super::*;

pub(super) fn model(replaying: bool) -> StatusModel {
    StatusModel {
        workspace: Some("Scalp".to_owned()),
        modified: true,
        venue: if replaying {
            "Recorded session"
        } else {
            "Offline validation (no bridge)"
        }
        .to_owned(),
        symbol: if replaying { "WINZ26" } else { "WDOU26" }.to_owned(),
        replay: replaying.then_some(ReplayFigures {
            speed: 10.0,
            progress: 0.4,
        }),
        feed_arrival_ms: None,
        tape_age_ms: None,
        feed_latency: None,
        deal_recording: None,
        deal_chip: None,
        side_note: Some("side: inferred (tick rule)".to_owned()),
        side_detail: None,
        sim_pnl: None,
        price_auto: true,
        fps: Some(60.0),
        frame_avg_ms: Some(16.7),
        frame_cpu_ms: Some(4.2),
        show_perf: true,
        saves_off: None,
    }
}

fn paint(
    ctx: &egui::Context,
    id: egui::ViewportId,
    size: egui::Vec2,
    model: &StatusModel,
) -> egui::Rect {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let mut input = egui::RawInput {
        viewport_id: id,
        screen_rect: Some(screen),
        ..Default::default()
    };
    input
        .viewports
        .entry(id)
        .or_default()
        .native_pixels_per_point = Some(1.5);
    let mut canvas = egui::Rect::NOTHING;
    let output = ctx.run(input, |ctx| {
        draw(ctx, model, TzOffset::default(), &StatusLead::default());
        canvas = egui::CentralPanel::default()
            .show(ctx, |_| {})
            .response
            .rect;
    });
    let painted = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == theme::CHROME => Some(rect.rect),
            _ => None,
        })
        .expect("the actual status background is painted");
    assert_eq!(painted.left(), screen.left());
    assert_eq!(painted.right(), screen.right());
    assert!(
        (canvas.bottom() - painted.top()).abs() < 0.01,
        "canvas and status must meet"
    );
    painted
}

#[test]
fn alternating_native_viewports_do_not_borrow_the_siblings_wrapped_status_height() {
    let ctx = egui::Context::default();
    let replay = model(true);
    let offline = model(false);
    let wide = (
        egui::ViewportId::from_hash_of("wide"),
        egui::vec2(1650.0, 760.0),
        &replay,
    );
    let narrow = (
        egui::ViewportId::from_hash_of("narrow"),
        egui::vec2(650.0, 700.0),
        &offline,
    );
    let mut settled = Vec::new();
    for (id, size, model) in [wide, narrow] {
        paint(&ctx, id, size, model);
        let rect = paint(&ctx, id, size, model);
        assert!((rect.bottom() - size.y).abs() < 0.01);
        settled.push(rect);
    }
    assert!(
        settled[1].height() > settled[0].height(),
        "the sibling must actually wrap"
    );
    for _ in 0..4 {
        for ((id, size, model), expected) in [wide, narrow].into_iter().zip(&settled) {
            let rect = paint(&ctx, id, size, model);
            assert_eq!(
                rect, *expected,
                "a sibling must not change the painted boundary"
            );
            assert!(
                (rect.bottom() - size.y).abs() < 0.01,
                "no unpainted client rows"
            );
        }
    }
}
