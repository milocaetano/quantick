//! Pine source window and its app-owned background compiler handle.
use super::{Surface, SurfaceEnv, SurfaceResponse};
use eframe::egui;
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};
use std::{sync::mpsc::Receiver, time::Instant};

pub(crate) type CompileResult = (u64, ScriptDocument, Option<String>);

pub(crate) struct ScriptEditorSurface {
    pub state: quantick_stores::script_editor::ScriptEditor,
    pub compilation: Option<Receiver<CompileResult>>,
    pub epoch: Instant,
    copy_name: String,
    keyboard: Vec<egui::Event>,
    keys: std::collections::HashSet<egui::Key>,
}
impl Default for ScriptEditorSurface {
    fn default() -> Self {
        Self {
            state: Default::default(),
            compilation: None,
            epoch: Instant::now(),
            copy_name: String::new(),
            keyboard: Vec::new(),
            keys: Default::default(),
        }
    }
}
fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}
impl ScriptEditorSurface {
    pub fn capture_keyboard(&mut self, ctx: &egui::Context) {
        if self.state.document.is_some() {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    if keyboard_event(event) {
                        self.keyboard.push(event.clone());
                        false
                    } else {
                        true
                    }
                });
                self.keys = std::mem::take(&mut input.keys_down);
            });
        }
    }
}
impl Surface for ScriptEditorSurface {
    fn id(&self) -> &'static str {
        "script-editor"
    }
    fn draw(&mut self, ctx: &egui::Context, _env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let Some(document) = self.state.document.as_ref() else {
            return SurfaceResponse::default();
        };
        ctx.input_mut(|input| {
            input.events.append(&mut self.keyboard);
            input.keys_down = std::mem::take(&mut self.keys);
        });
        let mut source = document.source.clone();
        let mut open = true;
        let mut actions = Vec::new();
        let screen = ctx.screen_rect().size();
        egui::Window::new(format!("{} - Pine source", document.title))
            .id(egui::Id::new(self.id()))
            .open(&mut open)
            .default_size(egui::vec2(
                680.0_f32.min(screen.x - 32.0),
                420.0_f32.min(screen.y - 80.0),
            ))
            .min_size(egui::vec2(240.0, 180.0))
            .max_size(egui::vec2(screen.x - 32.0, screen.y - 64.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(if document.read_only {
                        "Built-in · Read only"
                    } else {
                        "Editable copy · Saves automatically"
                    });
                    if !document.read_only && ui.button("Save").clicked() {
                        actions.push(ScriptEditorAction::Save);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Copy name");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.copy_name)
                            .hint_text("my-indicator.pine")
                            .desired_width(180.0),
                    );
                    if ui.button("Create editable copy").clicked() {
                        actions.push(ScriptEditorAction::Copy {
                            id: document.id.clone(),
                            new_id: self.copy_name.clone(),
                        });
                    }
                });
                ui.separator();
                egui::ScrollArea::both()
                    .max_height(ui.available_height() - 46.0)
                    .show(ui, |ui| {
                        let response = if document.read_only {
                            let mut text = document.source.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .code_editor()
                                    .desired_rows(16)
                                    .desired_width(f32::INFINITY),
                            )
                        } else {
                            ui.add(
                                egui::TextEdit::multiline(&mut source)
                                    .code_editor()
                                    .desired_rows(16)
                                    .desired_width(f32::INFINITY),
                            )
                        };
                        if response.changed() {
                            actions.push(ScriptEditorAction::ReplaceSource {
                                source: source.clone(),
                            });
                        }
                    });
                ui.separator();
                if let Some(error) = &self.state.diagnostics {
                    egui::ScrollArea::vertical()
                        .max_height(90.0)
                        .show(ui, |ui| {
                            ui.colored_label(crate::theme::get(ctx).sell(), error);
                        });
                    ui.label("The chart keeps the last valid version.");
                } else if self.state.compiling {
                    ui.label("Checking source…");
                } else if self.state.dirty {
                    ui.label("Waiting for typing to stop…");
                } else {
                    ui.label(if document.read_only {
                        "Create a copy to edit this source."
                    } else {
                        "Saved"
                    });
                }
            });
        if !open {
            actions.push(ScriptEditorAction::Close);
        }
        ctx.input_mut(|input| {
            input.events.retain(|event| !keyboard_event(event));
            input.keys_down.clear();
        });
        if self.state.dirty || self.state.compiling {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        SurfaceResponse {
            script_editor_actions: actions,
            ..Default::default()
        }
    }
}

pub(crate) fn draw<S: Surface + ?Sized>(
    surface: &mut S,
    ctx: &egui::Context,
    env: &SurfaceEnv<'_>,
) -> SurfaceResponse {
    surface.draw(ctx, env)
}
