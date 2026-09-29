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
    // As the tape quantity tooltip test does: approach after the click, so
    // egui's post-click grace and velocity history let the tooltip open, then
    // hold still through the tooltip's sizing frame.
    for offset in [-7.0, -6.0, -5.0, -4.0, -3.0, -2.0, -1.0, 0.0] {
        let _ = paint(
            &mut view,
            &ctx,
            vec![egui::Event::PointerMoved(at + egui::vec2(offset, 0.0))],
        );
    }
    for _ in 0..3 {
        let _ = paint(&mut view, &ctx, Vec::new());
    }
    let hover = paint(&mut view, &ctx, Vec::new());
    assert!(
        hover
            .iter()
            .any(|(painted, _)| painted.contains("tape only") && painted.contains("native tape")),
        "the hover text says tape only implies it: {hover:?}"
    );
}

/// What the layer policy says of the native tape switch for `view`, from the
/// facts the pane hands it.
fn native_block(view: &OrderflowView) -> Option<quantick_layers::LayerBlock> {
    quantick_layers::LayerState::blocked(
        quantick_layers::ChartLayer::NativeTape,
        quantick_layers::LayerFacts {
            flow_pane: true,
            tape_on: view.config.lane_enabled(),
            tape_only: view.config.tape_only(),
            native_tape: view.config.native_tape(),
            volume_dots: view.config.volume_dots.enabled,
            ..Default::default()
        },
    )
}

/// Click the native tape box, then hover it until its tooltip paints, and
/// return the text painted then.
fn click_then_hover(view: &mut OrderflowView) -> PaintedText {
    let (ctx, text) = open_controls(view);
    let at = text
        .iter()
        .find(|(painted, _)| painted == NATIVE_TAPE)
        .expect("the native tape control")
        .1
        .center();
    for pressed in [true, false] {
        let _ = paint(
            view,
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
    for offset in [-7.0, -6.0, -5.0, -4.0, -3.0, -2.0, -1.0, 0.0] {
        let _ = paint(
            view,
            &ctx,
            vec![egui::Event::PointerMoved(at + egui::vec2(offset, 0.0))],
        );
    }
    for _ in 0..3 {
        let _ = paint(view, &ctx, Vec::new());
    }
    paint(view, &ctx, Vec::new())
}

/// The box is the layer's second door and opens only where the layer call
/// does. Blocked — tape only, the tape off, volume dots off — it can be
/// neither ticked nor unticked, and its hover text is the reason the layer
/// call names; open, a click moves the request.
#[test]
fn a_blocked_native_tape_layer_locks_its_box_with_the_layers_reason() {
    let beside = || {
        let mut view = view(true);
        view.config.live_lane.tape_only = false;
        view.config.live_lane.native_tape = true;
        view
    };
    let mut open = beside();
    assert_eq!(native_block(&open), None);
    let _ = click_then_hover(&mut open);
    assert!(
        !open.config.live_lane.native_tape,
        "an open box moves the request"
    );
    let blocks: [(&str, fn(&mut HeatmapConfig)); 3] = [
        ("tape only", |config| config.live_lane.tape_only = true),
        ("the tape off", |config| config.live_lane.enabled = false),
        ("volume dots off", |config| {
            config.volume_dots.enabled = false
        }),
    ];
    for (why, block) in blocks {
        let mut view = beside();
        block(&mut view.config);
        let reason = native_block(&view)
            .unwrap_or_else(|| panic!("{why} blocks the layer"))
            .explanation;
        let before = view.config.clone();
        let hover = click_then_hover(&mut view);
        assert_eq!(view.config, before, "{why}: the box is locked");
        assert!(
            hover.iter().any(|(painted, _)| painted == reason),
            "{why}: the hover text is the layer's reason {reason:?}: {hover:?}"
        );
    }
}
