//! The flow pane's lights: the bulb beside the gear, its named call, its scene
//! entry and its place in the saved tab.
use super::*;
use quantick_appearance::ChartSurface;
use quantick_control_schema::orderflow_settings::{LIGHTS_CAPABILITY_ID, LIGHTS_DESCRIPTION};

const BULB: HeaderControl = HeaderControl::Settings(PanelControl::Lights);

fn in_theme(ctx: &egui::Context, source: &str) -> (QuantickApp, impl Sized) {
    let (mut app, ends) = flow_fixture(ctx);
    app.runtime
        .appearance
        .apply_candidate(source, false)
        .unwrap();
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    (app, ends)
}

fn lit(app: &QuantickApp) -> bool {
    app.active_tab().flow_pane.surface() == ChartSurface::LitFlow
}

fn scene_entry(app: &QuantickApp) -> Option<quantick_control_schema::scene::SceneControlSnapshot> {
    let id = format!(
        "pane.{}.header.{}",
        app.active_tab().flow_pane.id,
        BULB.id_in_header()
    );
    crate::control::scene_snapshot(app)
        .controls
        .into_iter()
        .find(|entry| entry.control_id == id)
}

/// Light paints a bulb left of the gear; a click lights the pane, which then
/// paints with the lit surface's palette, and a second click puts it out. The
/// scene lists it with its description and its call.
#[test]
fn the_bulb_stands_left_of_the_gear_and_a_click_lights_the_pane() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::LIGHT);
    let [tape, bulb, gear] = [TAPE, BULB, GEAR].map(|control| flow_control(&app, control));
    assert!(
        tape.right() < bulb.left() && bulb.right() < gear.left(),
        "{bulb:?} {gear:?}"
    );
    let entry = scene_entry(&app).expect("the scene lists the bulb");
    assert!(!entry.selected);
    assert_eq!(entry.description.as_deref(), Some(LIGHTS_DESCRIPTION));
    assert_eq!(entry.capability_id.as_deref(), Some(LIGHTS_CAPABILITY_ID));
    assert!(!lit(&app));

    click_chart(&mut app, &ctx, bulb.center());
    run_frame(&mut app, &ctx);
    assert!(lit(&app));
    assert_eq!(scene_entry(&app).map(|entry| entry.selected), Some(true));
    let palette = app.active_tab().flow_pane.palette(&ctx);
    assert!(palette.resolved.orderflow.surface_bound);
    assert_eq!(
        palette.color(quantick_appearance::Token::ChartCanvas),
        egui::Color32::WHITE
    );

    let bulb = flow_control(&app, BULB).center();
    click_chart(&mut app, &ctx, bulb);
    run_frame(&mut app, &ctx);
    assert!(!lit(&app));
}

/// Dark offers no bulb, and its call refuses and keeps nothing.
#[test]
fn dark_has_no_bulb_and_refuses_the_call() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::DARK);
    assert_eq!(control_rect(&app, PaneSide::Flow, BULB), None);
    assert!(scene_entry(&app).is_none());
    let refused = settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true }));
    assert!(refused.is_err());
    assert!(!app.active_tab().tape().settings_panel.lights_on());
}

/// The call sets, reads back, repeats without effect and refuses bad input;
/// the settings scope reports the same value.
#[test]
fn the_lights_call_reads_back_retries_safely_and_refuses_bad_input() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::LIGHT);
    let first = settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    assert_eq!(
        (first["lights_on"].clone(), first["changed"].clone()),
        (json!(true), json!(true))
    );
    let again = settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    assert_eq!(
        (again["lights_on"].clone(), again["changed"].clone()),
        (json!(true), json!(false))
    );
    for bad in [
        json!({}),
        json!({ "on": 1 }),
        json!({ "on": true, "pane_id": "1" }),
    ] {
        assert!(settings_call(&mut app, LIGHTS_CAPABILITY_ID, bad).is_err());
    }
    assert!(lit(&app));
    let scope = observer_scope("orderflow.settings");
    let capture = crate::control::standard_registry()
        .unwrap()
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    assert_eq!(
        capture.scopes[&scope].value["tabs"][0]["lights_on"],
        json!(true)
    );
}

/// The lights are saved with the tab and come back on a restart.
#[test]
fn the_lights_survive_a_save_and_a_restart() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::LIGHT);
    settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    let (tabs, chrome) = app.arrangement_state().capture_arrangement();
    assert!(tabs[0].orderflow_panel.lights_on());
    let written = toml::to_string(&tabs[0]).unwrap();
    let read: ui_state::SavedTab = toml::from_str(&written).unwrap();

    let fresh_ctx = egui::Context::default();
    let (mut fresh, _fresh_ends) = in_theme(&fresh_ctx, quantick_appearance::LIGHT);
    assert!(!lit(&fresh));
    fresh
        .arrangement_adapter()
        .restore_workspace(ui_state::Workspace::new(
            true,
            None,
            0,
            vec![read],
            Some(chrome),
        ));
    assert!(lit(&fresh));
}

/// A canvas layout change only stops drawing the flow pane: hidden behind a
/// lone time chart and shown again, it keeps its lights and so does the call's
/// readback.
#[test]
fn the_lights_survive_a_layout_that_hides_the_flow_pane() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::LIGHT);
    settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    for layout in [
        CanvasLayout::Time,
        CanvasLayout::TimeAndFlow,
        CanvasLayout::Time,
        CanvasLayout::TimeTimeAndFlow,
        CanvasLayout::Single,
    ] {
        app.active_tab_mut().set_layout(layout);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert!(lit(&app), "{layout:?}");
    }
    let read = settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    assert_eq!(
        (read["lights_saved"].clone(), read["changed"].clone()),
        (json!(true), json!(false))
    );
}

/// Lit, every trail candle sits on a plate in the lit casing ink; off, no
/// plate is painted at all.
#[test]
fn lit_candles_sit_on_plates_and_unlit_ones_on_none() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::LIGHT);
    let ink = crate::theme::surface(&ctx, ChartSurface::LitFlow).casing();
    let plates = |output: &egui::FullOutput| {
        flat_shapes(output)
            .iter()
            .filter(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == ink))
            .count()
    };
    let off = run_frame(&mut app, &ctx);
    assert_eq!(plates(&off), 0);
    settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    run_frame(&mut app, &ctx);
    let on = run_frame(&mut app, &ctx);
    assert!(plates(&on) >= 2, "a body plate and a wick plate per candle");
}

/// Lit, every plate is painted before any candle, so no plate covers a
/// neighbour: the whole-pixel candle pass is one run of rects, plates first.
#[test]
fn every_plate_is_painted_before_any_candle() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = in_theme(&ctx, quantick_appearance::LIGHT);
    set_flow_bars(&mut app, BarSpec::Tick(1));
    let ink = crate::theme::surface(&ctx, ChartSurface::LitFlow).casing();
    settings_call(&mut app, LIGHTS_CAPABILITY_ID, json!({ "on": true })).unwrap();
    run_frame(&mut app, &ctx);
    let shapes = flat_shapes(&run_frame(&mut app, &ctx));
    let plate = |shape: &egui::Shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == ink);
    let first = shapes.iter().position(plate).expect("a plate");
    let pass: Vec<bool> = (shapes[first..].iter())
        .map_while(|shape| matches!(shape, egui::Shape::Rect(_)).then(|| plate(shape)))
        .collect();
    let plates = pass.iter().take_while(|is_plate| **is_plate).count();
    assert!(plates >= 4, "two candles' plates at least: {pass:?}");
    assert!(pass[plates..].iter().all(|is_plate| !is_plate), "{pass:?}");
}
