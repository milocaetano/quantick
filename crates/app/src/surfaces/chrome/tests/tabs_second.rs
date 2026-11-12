//! Test-only catalog item for the TabStrip slot.
use quantick_ui_model::chrome_tabs_second::{Effects, Intent, Model, State};
#[derive(Default)]
pub(crate) struct View;
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "chrome_tabs_second"
    }
    fn paint(
        &mut self,
        state: &State,
        _: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&crate::surfaces::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        state.visible.then_some(Intent::Add(1))
    }
}
pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::ChromeTabsSecond(effects)],
        ..Default::default()
    }
}
