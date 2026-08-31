//! The assistant's popup, as a [`Surface`].
//!
//! Raised over the control plane by `quantick_notify` and dismissed by the
//! trader. It owns the one thing it draws, so moving it here took its state
//! off `QuantickApp` with it — the point of the port, and the reason this
//! file's existence is the whole registration.

use eframe::egui;

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::control::AgentPopup;
use crate::theme;

/// How far below the top edge the assistant's popup opens, clear of the menu
/// row and the toolbar.
const TOP_MARGIN_PX: f32 = 96.0;
/// Widest the assistant's popup gets, so a long message wraps instead of
/// covering the chart.
const MAX_WIDTH_PX: f32 = 360.0;
/// Room between the message and the attribution line under it.
const SPACING_PX: f32 = 6.0;

/// The assistant's popup: at most one waiting to be read.
#[derive(Default)]
pub(crate) struct AgentPopupSurface {
    pending: Option<AgentPopup>,
}

impl AgentPopupSurface {
    /// Raise a popup, replacing any still on screen. Last message wins: the
    /// assistant speaks to a trader who is watching a market, and a queue
    /// would show them a stale line while the current one waits.
    pub fn show(&mut self, popup: AgentPopup) {
        self.pending = Some(popup);
    }

    /// The popup currently on screen, if any.
    #[cfg(test)]
    pub fn pending(&self) -> Option<&AgentPopup> {
        self.pending.as_ref()
    }
}

impl Surface for AgentPopupSurface {
    fn id(&self) -> &'static str {
        "agent_popup"
    }

    fn draw(&mut self, ctx: &egui::Context, _env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let id = self.id();
        let mut open = true;
        let mut dismissed = false;
        // Borrowed, never cloned, for the reason the toast gives beside its own
        // borrow — and more so here, because the popup has no deadline: it is
        // painted every frame until the trader dismisses it, so two minutes on
        // screen at 60 Hz would be ~21,600 copies of three strings that never
        // change. The borrow ends with this block, before `pending` is cleared.
        {
            let Some(popup) = self.pending.as_ref() else {
                return SurfaceResponse::default();
            };
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
                            .color(theme::TEXT_SUPPORT),
                    );
                    if ui.button("Dismiss").clicked() {
                        dismissed = true;
                    }
                });
        }
        if dismissed || !open {
            self.pending = None;
        }
        SurfaceResponse::default()
    }

    /// The real popup arrives over the control plane from a connected
    /// assistant, so a scripted capture has no way to raise one — which is
    /// exactly the case `ui-harness` says a hook must cover. It goes through
    /// [`Self::show`], the same call `quantick_notify` makes.
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        if std::env::var("QUANTICK_AGENT_POPUP").is_ok_and(|value| value == "1") {
            self.show(AgentPopup {
                title: "Assistant".to_string(),
                message: "Volume at 108k is three times the session median.".to_string(),
                author: "capture".to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup(message: &str) -> AgentPopup {
        AgentPopup {
            title: "Assistant".to_string(),
            message: message.to_string(),
            author: "tester".to_string(),
        }
    }

    /// The surface starts empty, so an application that never hears from the
    /// assistant paints no window.
    #[test]
    fn a_new_surface_has_nothing_to_show() {
        assert!(AgentPopupSurface::default().pending().is_none());
    }

    /// Last message wins, rather than queueing behind one the trader has not
    /// dismissed.
    #[test]
    fn a_second_popup_replaces_the_first() {
        let mut surface = AgentPopupSurface::default();
        surface.show(popup("first"));
        surface.show(popup("second"));
        assert_eq!(
            surface.pending().map(|p| p.message.as_str()),
            Some("second")
        );
    }
}
