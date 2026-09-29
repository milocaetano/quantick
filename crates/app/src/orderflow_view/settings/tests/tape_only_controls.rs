//! The settings surface must describe the projection currently being painted.

use super::*;
use quantick_orderflow::{BubbleSizeReference, LaneWindow};

type PaintedText = Vec<(String, egui::Rect)>;

fn paint(view: &mut OrderflowView, ctx: &egui::Context, events: Vec<egui::Event>) -> PaintedText {
    let output = ctx.run(
        egui::RawInput {
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
    output
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Text(text) => Some((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            _ => None,
        })
        .collect()
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
    let _ = paint(&mut view, &ctx, vec![egui::Event::PointerMoved(pos)]);
    let text = paint(&mut view, &ctx, Vec::new());
    let tooltip = text
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tooltip.contains("Typing a value turns auto off"),
        "hover must expose the reference tooltip"
    );
    assert!(!tooltip.contains("both panes"));
    assert!(tooltip.contains("tape"));
    assert_eq!(view.config, before);
}
