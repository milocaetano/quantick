//! The settings surface must describe the projection currently being painted.

use super::*;
use quantick_orderflow::{BubbleSizeReference, LaneWindow};

type PaintedText = Vec<(String, egui::Rect)>;

fn collect_text(shape: egui::Shape, text: &mut PaintedText) {
    match shape {
        egui::Shape::Text(shape) => text.push((
            shape.galley.job.text.clone(),
            egui::Rect::from_min_size(shape.pos, shape.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, text);
            }
        }
        _ => {}
    }
}

fn paint(view: &mut OrderflowView, ctx: &egui::Context, events: Vec<egui::Event>) -> PaintedText {
    let output = ctx.run(
        egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_200.0, 3_000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| view.draw_bubble_controls(ui));
        },
    );
    let mut text = Vec::new();
    for shape in output.shapes {
        collect_text(shape.shape, &mut text);
    }
    text
}

fn open_controls(view: &mut OrderflowView) -> (egui::Context, PaintedText) {
    let ctx = egui::Context::default();
    ctx.style_mut(|style| {
        style.animation_time = 0.0;
        style.interaction.tooltip_delay = 0.0;
        style.interaction.show_tooltips_only_when_still = false;
    });
    let mut text = paint(view, &ctx, Vec::new());
    for title in ["live lane", "labels", "colours"] {
        let pos = text
            .iter()
            .find(|(label, _)| label == title)
            .unwrap_or_else(|| panic!("missing section {title}"))
            .1
            .center();
        for pressed in [true, false] {
            let _ = paint(
                view,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        text = paint(view, &ctx, vec![egui::Event::PointerGone]);
    }
    (ctx, text)
}

fn view(tape_only: bool) -> OrderflowView {
    let mut view = OrderflowView::new("TEST");
    view.config = HeatmapConfig::default();
    view.config.live_lane.tape_only = tape_only;
    view.config.live_lane.window = LaneWindow::Fixed { ms: 30_000 };
    view.config.volume_dots.enabled = tape_only;
    view.config.volume_dots.auto_full = true;
    view.config.bubbles.size_reference = BubbleSizeReference::VisibleMax;
    view.config.bubble_region_rows = 4;
    view
}

fn has(text: &PaintedText, label: &str) -> bool {
    text.iter().any(|(painted, _)| painted == label)
}

#[test]
fn native_tape_settings_hide_inactive_controls_and_explain_the_active_scale() {
    let mut view = view(true);
    let before = view.config.clone();
    let (_, text) = open_controls(&mut view);
    for inactive in [
        "cluster",
        "fold dust",
        "region height",
        "summarize closed bars",
        "smallest print px",
        "full size at",
        "Auto · largest in session",
        "side separation px",
        "width",
        "bubble size",
    ] {
        assert!(!has(&text, inactive), "inactive tape control: {inactive}");
    }
    for meaningful in [
        "Tape only (hide candles)",
        "auto: on",
        "Ignore opening burst in scale",
        "window",
        "duration",
        "biggest print px",
        "render style",
        "fill opacity",
        "readable from px",
        "boundary and live-edge line",
        "quantity inside the bubble",
        "buy",
        "sell",
    ] {
        assert!(has(&text, meaningful), "missing tape control: {meaningful}");
    }
    let text = text
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("dot full size qty"));
    assert!(text.contains("hide below qty"));
    assert!(text.contains("100 ms"));
    assert!(text.contains("one price tick"));
    assert!(text.contains("largest visible dot"));
    assert_eq!(
        view.config, before,
        "opening settings must not rewrite a preset"
    );
}

#[test]
fn legacy_bubble_settings_keep_their_controls_and_leave_the_crypto_look_unchanged() {
    let mut view = view(false);
    let before = view.config.clone();
    let (_, text) = open_controls(&mut view);
    for active in [
        "cluster",
        "fold dust",
        "region height",
        "summarize closed bars",
        "smallest print px",
        "full size at",
        "Auto · largest in session",
        "side separation px",
        "width",
        "bubble size",
    ] {
        assert!(has(&text, active), "missing legacy control: {active}");
    }
    assert!(!has(&text, "Ignore opening burst in scale"));
    assert_eq!(view.config, before);
}

#[test]
fn the_tape_quantity_tooltip_does_not_claim_to_resize_the_independent_candle_overlay() {
    let mut view = view(true);
    let before = view.config.clone();
    let (ctx, text) = open_controls(&mut view);
    let pos = text
        .iter()
        .find(|(label, _)| label.starts_with("dot full size qty"))
        .expect("the typed tape reference is visible")
        .1
        .center();
    // egui suppresses tooltips after a click until it observes movement.
    // Its velocity history needs three samples and its post-click grace is
    // 100 ms. Approach for eight 60 Hz frames, rather than teleporting back
    // after PointerGone, then let the tooltip's sizing frame settle.
    for offset in [-7.0, -6.0, -5.0, -4.0, -3.0, -2.0, -1.0, 0.0] {
        let _ = paint(
            &mut view,
            &ctx,
            vec![egui::Event::PointerMoved(pos + egui::vec2(offset, 0.0))],
        );
    }
    // Hit testing uses the previous frame; a tooltip Area then needs its
    // invisible sizing frame before its text can be painted. Keep hovering
    // through those frames with an explicit, deterministic UI clock.
    for _ in 0..3 {
        let _ = paint(&mut view, &ctx, Vec::new());
    }
    let text = paint(&mut view, &ctx, Vec::new());
    let hovered =
        ctx.interaction_snapshot(|state| state.hovered.iter().copied().collect::<Vec<_>>());
    let responses: Vec<_> = hovered.iter().map(|id| ctx.read_response(*id)).collect();
    let tooltip = text
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tooltip.contains("Typing a value turns auto off"),
        "hover must expose the reference tooltip at {pos:?}; hovered: {responses:?}; painted text: {tooltip}"
    );
    assert!(!tooltip.contains("both panes"));
    assert!(tooltip.contains("tape"));
    assert_eq!(view.config, before);
}
