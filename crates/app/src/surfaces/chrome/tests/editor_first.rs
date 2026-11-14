//! Test-only item for the indicator editor phase.
chrome_probe_view!(chrome_editor_first, ChromeEditorFirst);

#[test]
fn indicator_editor_dialogs_use_catalog_order_before_legends() {
    use crate::surfaces::{Surfaces, ViewContext};
    use quantick_ui_model::{HostEffect, Slot};
    let mut surfaces = Surfaces::default();
    surfaces.chrome_editor_first.update(quantick_ui_model::chrome_editor_first::Intent::Show(true));
    surfaces.chrome_editor_second.update(quantick_ui_model::chrome_editor_second::Intent::Show(true));
    let ctx = eframe::egui::Context::default();
    for pass in 1..=2 {
        let result = surfaces.draw_slot_unscoped_with_env(Slot::EditorDialogs, &mut ViewContext { ctx: &ctx, ui: None }, None);
        assert_eq!(result.model_effects, vec![
            HostEffect::ChromeEditorFirst(quantick_ui_model::chrome_editor_first::Effects(Some(pass))),
            HostEffect::ChromeEditorSecond(quantick_ui_model::chrome_editor_second::Effects(Some(pass))),
        ], "both rows run in catalog order with the editor closed and no host authority");
    }
}
