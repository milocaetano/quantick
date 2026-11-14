use super::*;
use crate::surfaces::{AccessPanelEnv, Surfaces};
use quantick_control::id::PermissionId;

#[test]
fn access_panel_uses_registered_lifecycle_and_authoritative_host() {
    crate::control::ControlAccess::assert_native_panel_widgets();
    let mut surfaces = Surfaces::default();
    let mut access = crate::control::ControlAccess::new();
    let ctx = eframe::egui::Context::default();
    assert!(!surfaces.access_panel.model_state.open);
    surfaces.access_panel.update(Intent::Open);
    let _ = ctx.run(eframe::egui::RawInput::default(), |ctx| {
        let mut env = BorrowedSurfaceEnv::Access(AccessPanelEnv {
            access: Some(&mut access),
        });
        let result = surfaces.draw_slot_unscoped_with_env(
            Slot::MenuDialogs,
            &mut ViewContext { ctx, ui: None },
            Some(&mut env),
        );
        assert!(result.model_effects.is_empty());
    });
    assert!(ctx.memory(|memory| {
        memory
            .area_rect(eframe::egui::Id::new("control_access_panel"))
            .is_some()
    }));
    assert!(
        !access.needs_frame_service(),
        "opening the panel never enables the host"
    );
    surfaces.access_panel.with_updates(
        |response| assert!(response.model_effects.is_empty()),
        |_, session| {
            access.panel_request(
                &ctx,
                session,
                Intent::RequestPermission(PermissionId::new("observe.chart").unwrap(), false),
            );
            assert_eq!(
                session.state().facts,
                access.panel_facts(),
                "terminal receipt is settled before the next widget"
            );
            access.panel_request(&ctx, session, Intent::RequestDisable);
            assert!(!access.needs_frame_service());
            session.update(Intent::Close);
        },
    );
    assert!(!surfaces.access_panel.model_state.open);
}
