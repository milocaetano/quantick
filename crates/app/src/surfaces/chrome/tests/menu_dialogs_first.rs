//! Test-only row preceding the access panel in the MenuDialogs slot.
chrome_probe_view!(chrome_menu_dialogs_first, ChromeMenuDialogsFirst);

#[test]
fn menu_dialogs_paint_in_catalog_order_after_menu() {
    use crate::surfaces::{AccessPanelEnv, BorrowedSurfaceEnv, Surfaces, ViewContext};
    use quantick_ui_model::{HostEffect, Slot};

    let mut surfaces = Surfaces::default();
    surfaces
        .chrome_menu_first
        .update(quantick_ui_model::chrome_menu_first::Intent::Show(true));
    surfaces
        .chrome_menu_second
        .update(quantick_ui_model::chrome_menu_second::Intent::Show(true));
    surfaces.chrome_menu_dialogs_first.update(
        quantick_ui_model::chrome_menu_dialogs_first::Intent::Show(true),
    );
    surfaces.chrome_menu_dialogs_second.update(
        quantick_ui_model::chrome_menu_dialogs_second::Intent::Show(true),
    );
    let ctx = eframe::egui::Context::default();
    let mut host = crate::control::ControlAccess::new();
    // Inactive and active panels must preserve the independent row order.
    for (pass, open, has_host) in [(1, false, true), (2, true, false), (3, true, true)] {
        let _ = ctx.run(Default::default(), |ctx| {
            surfaces.access_panel.update(if open {
                quantick_ui_model::access_panel::Intent::Open
            } else {
                quantick_ui_model::access_panel::Intent::Close
            });
            let mut target = ViewContext { ctx, ui: None };
            let mut observed = surfaces
                .draw_slot_unscoped(Slot::MenuBar, &mut target)
                .model_effects;
            assert_eq!(
                surfaces.chrome_menu_dialogs_first.model_state.value,
                pass - 1
            );
            assert_eq!(
                surfaces.chrome_menu_dialogs_second.model_state.value,
                pass - 1
            );
            let mut env = BorrowedSurfaceEnv::Access(AccessPanelEnv {
                access: has_host.then_some(&mut host),
            });
            observed.extend(
                surfaces
                    .draw_slot_unscoped_with_env(Slot::MenuDialogs, &mut target, Some(&mut env))
                    .model_effects,
            );
            assert_eq!(
                observed,
                vec![
                    HostEffect::ChromeMenuFirst(quantick_ui_model::chrome_menu_first::Effects(
                        Some(pass)
                    )),
                    HostEffect::ChromeMenuSecond(quantick_ui_model::chrome_menu_second::Effects(
                        Some(pass)
                    )),
                    HostEffect::ChromeMenuDialogsFirst(
                        quantick_ui_model::chrome_menu_dialogs_first::Effects(Some(pass))
                    ),
                    HostEffect::ChromeMenuDialogsSecond(
                        quantick_ui_model::chrome_menu_dialogs_second::Effects(Some(pass))
                    ),
                ],
                "menu rows finish before both dialog rows, in catalog order"
            );
            assert_eq!(surfaces.access_panel.model_state.open, open);
            assert_eq!(
                ctx.memory(|memory| memory
                    .area_rect(eframe::egui::Id::new("control_access_panel"))
                    .is_some()),
                open && has_host,
                "the real panel is painted between the independent catalog rows"
            );
        });
    }
    let result = surfaces.draw_slot_unscoped_with_env(
        Slot::MenuDialogs,
        &mut ViewContext {
            ctx: &ctx,
            ui: None,
        },
        None,
    );
    assert_eq!(
        result.model_effects,
        vec![
            HostEffect::ChromeMenuDialogsFirst(
                quantick_ui_model::chrome_menu_dialogs_first::Effects(Some(4))
            ),
            HostEffect::ChromeMenuDialogsSecond(
                quantick_ui_model::chrome_menu_dialogs_second::Effects(Some(4))
            ),
        ],
        "the generic pass also traverses both rows without a borrowed facet"
    );
}
