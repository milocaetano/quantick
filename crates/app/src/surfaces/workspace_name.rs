//! The Save-as box, as a [`Surface`].
//!
//! A window rather than an inline menu field, because a menu closes the moment
//! focus moves and a name is several keystrokes long. Enter saves, Escape
//! cancels, and the field takes the keyboard on the frame it opens so the
//! trader can type without clicking into it first.
//!
//! This is the surface that exercises both halves of the port. It *reads*
//! something it does not own — the saved arrangements, to warn before one is
//! replaced — and takes it from [`SurfaceEnv`] rather than from the host. And
//! it *asks* for something it must not do itself — writing the workspace —
//! and says so through [`SurfaceResponse`]. Neither direction gives it a
//! reference to `QuantickApp`, which is the whole point: the surface stays
//! testable without an application, and the application keeps the write.

use eframe::egui;

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::{theme, ui_state};

/// Width of the Save-as box, in pixels. Wide enough that a name at the
/// [`ui_state::MAX_WORKSPACE_NAME`] limit reads in one line.
const WIDTH_PX: f32 = 280.0;

/// The Save-as box: one text field, Save and Cancel.
#[derive(Default)]
pub(crate) struct WorkspaceNameSurface {
    /// What has been typed so far, or `None` while the box is closed. The
    /// box being open *is* this being `Some`, so there is no second flag to
    /// disagree with it.
    entry: Option<String>,
    /// Whether the field still owes itself the keyboard.
    ///
    /// Focus is taken on the frame the box opens and not afterwards. Asking
    /// every frame would hold the keyboard for as long as the box is up, which
    /// matters most under `QUANTICK_WORKSPACE_NAME_BOX=1`: the box is open from
    /// launch, and every key a capture run sent would land in the name field
    /// instead of reaching the chart.
    focus_pending: bool,
}

impl WorkspaceNameSurface {
    /// Open the box on an empty name, with the keyboard owed to its field.
    pub fn open(&mut self) {
        self.entry = Some(String::new());
        self.focus_pending = true;
    }

    /// What has been typed, or `None` while the box is closed.
    #[cfg(test)]
    pub fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
    }
}

impl Surface for WorkspaceNameSurface {
    fn id(&self) -> &'static str {
        "workspace_name_box"
    }

    /// The box is behind the Workspace menu's *Save as*, which a scripted run
    /// cannot click. Goes through [`Self::open`], the call the menu entry
    /// makes.
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        if std::env::var("QUANTICK_WORKSPACE_NAME_BOX").is_ok_and(|value| value == "1") {
            self.open();
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let id = self.id();
        let Some(mut entry) = self.entry.take() else {
            return SurfaceResponse::default();
        };
        let mut save = false;
        let mut cancel = false;
        let mut open = true;
        egui::Window::new("Save workspace as")
            .id(egui::Id::new(id))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(WIDTH_PX);
                ui.label("A name you will recognise later.");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut entry)
                        .hint_text("scalp WIN")
                        .char_limit(ui_state::MAX_WORKSPACE_NAME)
                        .desired_width(f32::INFINITY),
                );
                if self.focus_pending {
                    field.request_focus();
                    self.focus_pending = false;
                }
                // Enter goes through the same gate as the Save button. Without
                // it, Enter on an empty field closes the box and asks the host
                // to save "", which it refuses with a "needs a name" toast —
                // the trader loses the box for an input the button had already
                // greyed out.
                if field.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && ui_state::clean_workspace_name(&entry).is_some()
                {
                    save = true;
                }
                // A name already in use replaces that bookmark. Saying so
                // before the click is the difference between "save as" and
                // "lose the arrangement I meant to keep".
                if let Some(clean) = ui_state::clean_workspace_name(&entry)
                    && env.bookmarks.iter().any(|held| held.name == clean)
                {
                    ui.label(
                        egui::RichText::new(format!("Replaces the saved \"{clean}\"."))
                            .color(theme::AMBER),
                    );
                }
                ui.horizontal(|ui| {
                    let named = ui_state::clean_workspace_name(&entry).is_some();
                    if ui
                        .add_enabled(named, egui::Button::new("Save"))
                        .on_disabled_hover_text("Type a name first")
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if save {
            return SurfaceResponse {
                save_workspace_as: Some(entry),
                ..SurfaceResponse::default()
            };
        }
        if !cancel && open {
            // Neither settled: keep what has been typed for the next frame.
            self.entry = Some(entry);
        }
        SurfaceResponse::default()
    }
}

crate::hooks::declare_hooks!["QUANTICK_WORKSPACE_NAME_BOX"];

#[cfg(test)]
mod tests {
    use super::*;

    /// The box being open is the entry being `Some`, so a fresh surface draws
    /// nothing and asks for nothing.
    #[test]
    fn a_closed_box_draws_nothing_and_asks_nothing() {
        let ctx = egui::Context::default();
        let mut surface = WorkspaceNameSurface::default();
        assert!(surface.entry().is_none());
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &SurfaceEnv::quiet(std::time::Instant::now()));
        });
        assert_eq!(response, SurfaceResponse::default());
    }

    /// Opening puts an empty name on screen, which is what makes the Save
    /// button start disabled rather than saving an unnamed arrangement.
    #[test]
    fn opening_starts_from_an_empty_name() {
        let mut surface = WorkspaceNameSurface::default();
        surface.open();
        assert_eq!(surface.entry(), Some(""));
    }

    /// What the trader typed survives a frame where they settled nothing —
    /// the box is several keystrokes long, and losing them on any frame that
    /// is not a click would make it unusable.
    #[test]
    fn an_unsettled_name_survives_the_frame() {
        let ctx = egui::Context::default();
        let mut surface = WorkspaceNameSurface::default();
        surface.open();
        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, &SurfaceEnv::quiet(std::time::Instant::now()));
        });
        assert!(
            surface.entry().is_some(),
            "the box stays open across a quiet frame"
        );
    }
}
