//! Beside the candles, the native tape's settings keep the divider's width
//! and offer the native switch next to tape only.

use super::*;

const NATIVE_TAPE: &str = "Native tape (execution time and price)";

/// Every check mark painted: egui draws one as a three-point open line.
fn check_marks(view: &mut OrderflowView, ctx: &egui::Context) -> Vec<egui::Rect> {
    let output = ctx.run(
        egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_200.0, 3_000.0),
            )),
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
            egui::Shape::Path(path) if path.points.len() == 3 && !path.closed => {
                Some(egui::Rect::from_points(&path.points))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn native_split_settings_keep_the_width_and_the_native_controls() {
    let mut view = view(true);
    view.config.live_lane.tape_only = false;
    view.config.live_lane.native_tape = true;
    let before = view.config.clone();
    let (_, text) = open_controls(&mut view);
    for inactive in ["cluster", "summarize closed bars", "bubble size"] {
        assert!(!has(&text, inactive), "inactive tape control: {inactive}");
    }
    for meaningful in [
        NATIVE_TAPE,
        "Tape only (hide candles)",
        "Ignore opening burst in scale",
        "width",
        "window",
    ] {
        assert!(has(&text, meaningful), "missing control: {meaningful}");
    }
    assert_eq!(
        view.config, before,
        "opening settings must not rewrite a preset"
    );
}

/// With tape only on the pane draws the native tape whatever the switch
/// says, so the checkbox reads checked, cannot be unticked from there, and
/// its hover text says why.
#[test]
fn tape_only_shows_the_native_tape_checked_and_locked() {
    let mut view = view(true);
    assert!(
        !view.config.live_lane.native_tape,
        "never asked for directly"
    );
    let before = view.config.clone();
    let (ctx, text) = open_controls(&mut view);
    let label = text
        .iter()
        .find(|(painted, _)| painted == NATIVE_TAPE)
        .expect("the native tape control")
        .1;
    assert!(
        check_marks(&mut view, &ctx)
            .iter()
            .any(|mark| mark.right() < label.left()
                && mark.center().y > label.top()
                && mark.center().y < label.bottom()),
        "tape only implies the native tape, so its box reads checked"
    );
    let at = label.center();
    for pressed in [true, false] {
        let _ = paint(
            &mut view,
            &ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(view.config, before, "locked while tape only is on");
    let mut hover = Vec::new();
    for _ in 0..3 {
        hover = paint(&mut view, &ctx, vec![egui::Event::PointerMoved(at)]);
    }
    assert!(
        hover
            .iter()
            .any(|(painted, _)| painted.contains("tape only") && painted.contains("native tape")),
        "the hover text says tape only implies it"
    );
}
