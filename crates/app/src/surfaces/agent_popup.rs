//! Paint the assistant notification state and emit dismissal intents.
//!
//! Raised over the control plane by `quantick_notify` and dismissed by the trader.

use eframe::egui;

use super::{SurfaceEnv, SurfaceResponse, ViewContext};
#[cfg(any(feature = "scenario-harness", test))]
use crate::control::AgentPopup;
use crate::theme;
use quantick_ui_model::agent_popup::{Intent, Model, State};

/// How far below the top edge the assistant's popup opens, clear of the menu
/// row and the toolbar.
const TOP_MARGIN_PX: f32 = 96.0;
/// Widest the assistant's popup gets, so a long message wraps instead of
/// covering the chart.
const MAX_WIDTH_PX: f32 = 360.0;
/// Room between the message and the attribution line under it.
const SPACING_PX: f32 = 6.0;

#[derive(Default)]
pub(crate) struct View;

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "agent_popup"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        _env: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let ctx = target.context();
        let palette = theme::get(ctx);
        let id = self.id();
        let mut open = true;
        let mut dismissed = false;
        // Borrowed, never cloned, for the reason the toast gives beside its own
        // borrow — and more so here, because the popup has no deadline: it is
        // painted every frame until the trader dismisses it, so two minutes on
        // screen at 60 Hz would be ~21,600 copies of three strings that never
        // change. The borrow ends with this block, before `pending` is cleared.
        {
            let popup = state.pending()?;
            egui::Window::new(&popup.title)
                .id(egui::Id::new(id))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, TOP_MARGIN_PX))
                .show(ctx, |ui| {
                    ui.set_max_width(MAX_WIDTH_PX);
                    ui.label(&popup.message);
                    ui.add_space(SPACING_PX);
                    ui.label(
                        egui::RichText::new(format!("Sent by {}", popup.author))
                            .small()
                            .color(palette.text_support()),
                    );
                    if ui.button("Dismiss").clicked() {
                        dismissed = true;
                    }
                });
        }
        if dismissed {
            Some(Intent::Dismiss)
        } else if !open {
            Some(Intent::WindowClosed)
        } else {
            None
        }
    }

    /// The real popup arrives over the control plane from a connected
    /// assistant, so a scripted capture has no way to raise one — which is
    /// exactly the case `ui-harness` says a hook must cover. It goes through
    /// the same model intent used by `quantick_notify`.
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) -> Option<Intent> {
        if crate::hooks::captured::var("QUANTICK_AGENT_POPUP").is_some_and(|value| value == "1") {
            Some(Intent::Show(AgentPopup {
                title: "Assistant".to_string(),
                message: "Volume at 108k is three times the session median.".to_string(),
                author: "capture".to_string(),
            }))
        } else {
            None
        }
    }
}

pub(crate) fn effects((): ()) -> SurfaceResponse {
    SurfaceResponse::default()
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_AGENT_POPUP"];

#[cfg(test)]
#[path = "agent_popup/tests.rs"]
mod tests;
