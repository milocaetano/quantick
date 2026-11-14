//! Paint the toolbar from its registered, cached model.
use crate::toolbar;
use eframe::egui;
use quantick_ui_model::chrome_toolbar::{Effects, Intent, Model, State};
#[derive(Default)]
pub(crate) struct View {
    pub capsule: toolbar::capsule::CapsuleRects,
    pub door: Option<egui::Rect>,
    pub history: Option<egui::Rect>,
    pub recording_menu: bool,
    pub popovers: Option<quantick_ui_model::canvas_popovers::Projection>,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "toolbar"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&crate::surfaces::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let popovers = self.popovers.take()?;
        let state = state.snapshot.as_ref()?;
        let capsule = &state.capsule;
        let mut model = toolbar::ToolbarModel {
            window_menu: toolbar::WindowMenuDoor {
                open: capsule.door_open,
                hover: capsule.door_hover.clone(),
                rect: &mut self.door,
            },
            capsule: toolbar::capsule::CapsuleModel {
                tree: &capsule.tree,
                workspace: &capsule.workspace,
                unsaved: capsule.unsaved,
                layout_unsaved: capsule.marks.layout_unsaved,
                menu_open: capsule.menu_open,
                popover_open: popovers.layout_open,
                add_open: popovers.add_open,
                compact: false,
            },
            capsule_rects: &mut self.capsule,
            snapshot: state,
            deal_recording_menu: std::mem::take(&mut self.recording_menu),
            history_menu_rect: &mut self.history,
        };
        let actions = toolbar::draw(target.context(), &mut model);
        (!actions.is_empty()).then_some(Intent::Actions(actions))
    }
}
pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::ChromeToolbar(effects)],
        ..Default::default()
    }
}
