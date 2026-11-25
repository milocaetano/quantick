//! The Strategies tab's buttons reach the session through the desk's own
//! command path. What a pause changes is `quantick_session::strategy_desk`'s,
//! tested there.

use serde_json::json;

use super::*;
use crate::app::control_host::ControlWindow;
use quantick_session::strategy_desk::DeskState;

/// Each instance's state, in the order the tab lists them.
fn states(app: &mut QuantickApp) -> Vec<DeskState> {
    let groups = quantick_control_handlers::strategy_desk::groups(app as &mut ControlWindow);
    let rows = groups.into_iter().flat_map(|group| group.rows);
    rows.map(|row| row.state).collect()
}

/// Every place `wanted` was painted, in paint order.
fn painted_at(output: &egui::FullOutput, wanted: &str) -> Vec<egui::Pos2> {
    fn walk(shape: &egui::Shape, wanted: &str, out: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                out.push(text.visual_bounding_rect().center());
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, wanted, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, wanted, &mut out);
    }
    out
}

#[test]
fn rail_pause_sends_the_desk_command() {
    let (mut app, _commands) = app_with_history(4);
    for _ in 0..2 {
        let input = json!({
            "script": "renko_poc_flip.pine",
            "side": "buy",
            "placement": { "kind": "anywhere" },
        });
        let origin = crate::control::ActionOrigin::Human;
        app.control_action("strategy.script.arm", 1, origin, input)
            .expect("the bundled flip arms from the default library");
    }
    assert_eq!(states(&mut app), [DeskState::Watching, DeskState::Watching]);

    app.surfaces.dock.open_tab(crate::dock::DockTab::Strategies);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    // The asset header's pause first, then one per row.
    let pauses = painted_at(&output, egui_phosphor::regular::PAUSE);
    assert_eq!(pauses.len(), 3, "one asset pause and two row pauses");
    click_chart(&mut app, &ctx, pauses[1]);
    // The click is kept on its frame and sent at the next one's settle.
    run_frame(&mut app, &ctx);

    assert_eq!(
        states(&mut app),
        [DeskState::Paused, DeskState::Watching],
        "the first row's pause paused that instance alone, once"
    );
}
