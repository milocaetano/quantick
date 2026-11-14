//! Tab-strip pixels and hit rectangles over the cached registered model.
use crate::surfaces::{HostEffect, SurfaceEnv, SurfaceResponse, ViewContext};
use quantick_ui_model::chrome_tab_strip::{Effects, Intent, Model, State};
#[derive(Default)]
pub(crate) struct View {
    pub drawn: crate::tabstrip::StripDrawn,
    pub root: Option<std::sync::Arc<crate::theme::Palette>>,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "tab_strip"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let snapshot = state.snapshot.as_ref()?;
        let ui = target.ui.as_deref_mut()?;
        let chips: Vec<_> = snapshot
            .chips
            .iter()
            .map(|chip| crate::tabstrip::TabChip {
                id: chip.id,
                label: &chip.label,
                feed: chip.feed,
            })
            .collect();
        self.drawn.allow_close_last = snapshot.allow_close_last;
        let root = self
            .root
            .clone()
            .unwrap_or_else(|| crate::theme::get(ui.ctx()));
        crate::tabstrip::draw(
            ui,
            &chips,
            snapshot.active,
            &snapshot.surface,
            (&mut self.drawn, &root),
        )
        .map(Intent::Action)
    }
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    if effects.0.is_none() {
        return SurfaceResponse::default();
    }
    SurfaceResponse {
        model_effects: vec![HostEffect::ChromeTabStrip(effects)],
        ..Default::default()
    }
}
