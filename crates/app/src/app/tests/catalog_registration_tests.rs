//! A new surface docks through the catalog alone. The test-only `second`
//! surface is one model file (`ui-model/src/second.rs`), one view file
//! (`surfaces/tests/fixtures/second.rs`) and one `surface_catalog!` entry;
//! nothing in `QuantickApp` names it, yet a real frame paints it, routes its
//! intent through `update` and hands its effects to the host.

use super::*;
use quantick_ui_model::{HostEffect, Slot};

#[test]
fn new_surface_uses_only_catalog_registration() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let scope = app.runtime.tabs.windows().presented().0;
    let (state, view) = app.surfaces.second.scoped(scope);
    assert_eq!(*state, 0);
    assert_eq!(view.painted, None, "a disabled surface paints nothing");
    view.enabled = true;
    view.intent = Some((4, 17));

    run_frame(&mut app, &ctx);
    let (state, view) = app.surfaces.second.scoped(scope);
    assert_eq!(view.painted, Some(0), "the app frame drew its catalog slot");
    assert_eq!(*state, 8, "the emitted intent ran the model's update");

    run_frame(&mut app, &ctx);
    let (state, view) = app.surfaces.second.scoped(scope);
    assert_eq!(view.painted, Some(8));
    assert_eq!(*state, 8, "painting alone never runs update");

    // Effects take the catalog's conversion on the same scoped adapter the
    // frame drove: the typed effect and the converted host request.
    app.surfaces.second.scoped(scope).1.intent = Some((0, 17));
    let mut response = crate::surfaces::SurfaceResponse::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        response = app.surfaces.draw_slot_scoped(
            scope,
            Slot::Toolbar,
            &mut crate::surfaces::ViewContext { ctx, ui: None },
        );
    });
    assert_eq!(response.model_effects, vec![HostEffect::Second((8, 17))]);
    assert!(response.undo_drawing);
}
