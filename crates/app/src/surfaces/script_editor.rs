//! Pine source window and its app-owned background compiler handle.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};
use quantick_ui_model::script_editor::{Effect, Intent, Model, State};
use std::{sync::mpsc::Receiver, time::Instant};

pub(crate) type CompileResult = (
    u64,
    ScriptDocument,
    Option<String>,
    Vec<quantick_indicators::InputSpec>,
);

pub(crate) struct View {
    pub compilation: Option<(u64, Receiver<CompileResult>)>,
    pub epoch: Instant,
    source: String,
    source_revision: Option<u64>,
    keyboard: Vec<egui::Event>,
    keys: std::collections::HashSet<egui::Key>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            compilation: None,
            epoch: Instant::now(),
            source: String::new(),
            source_revision: None,
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
impl super::View<Model> for View {
    fn capture_keyboard(&mut self, state: &State, ctx: &egui::Context) {
        if state.editor().document.is_some() {
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
    fn id(&self) -> &'static str {
        "script-editor"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let ctx = target.context();
        let env = env?;
        let editor = state.editor();
        let Some(document) = editor.document.as_ref() else {
            self.source.clear();
            self.source_revision = None;
            return None;
        };
        if document.read_only {
            self.source.clear();
            self.source_revision = None;
        } else if self.source_revision != Some(editor.revision) {
            self.source.clone_from(&document.source);
            self.source_revision = Some(editor.revision);
        }
        let mut copy_name = state.copy_name.clone();
        ctx.input_mut(|input| {
            input.events.append(&mut self.keyboard);
            input.keys_down = std::mem::take(&mut self.keys);
        });
        let mut open = true;
        let mut actions = Vec::new();
        let mut add_to_chart = None;
        let screen = ctx.screen_rect().size();
        egui::Window::new(format!("{} - Pine source", document.title))
            .id(egui::Id::new(self.id()))
            .order(egui::Order::Foreground)
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
                    if ui.button("Add to chart").clicked()
                        && let Some(index) = env
                            .picker_scripts
                            .iter()
                            .position(|entry| entry.name == document.id)
                    {
                        add_to_chart = Some(index);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Copy name");
                    ui.add(
                        egui::TextEdit::singleline(&mut copy_name)
                            .hint_text("my-indicator.pine")
                            .desired_width(180.0),
                    );
                    if ui.button("Create editable copy").clicked() {
                        actions.push(ScriptEditorAction::Copy {
                            id: document.id.clone(),
                            new_id: copy_name.clone(),
                        });
                    }
                });
                ui.separator();
                let footer_height = if editor.diagnostics.is_some() {
                    128.0
                } else {
                    46.0
                };
                egui::ScrollArea::both()
                    .id_salt("script-editor-source")
                    .max_height((ui.available_height() - footer_height).max(40.0))
                    .show(ui, |ui| {
                        let mut readonly = document.source.as_str();
                        let buffer: &mut dyn egui::TextBuffer = if document.read_only {
                            &mut readonly
                        } else {
                            &mut self.source
                        };
                        let response = ui.add(
                            egui::TextEdit::multiline(buffer)
                                .code_editor()
                                .desired_rows(16)
                                .desired_width(f32::INFINITY),
                        );
                        if response.changed() {
                            self.source_revision = None;
                            actions.push(ScriptEditorAction::ReplaceSource {
                                source: self.source.clone(),
                            });
                        }
                    });
                ui.separator();
                if let Some(error) = &editor.diagnostics {
                    egui::ScrollArea::vertical()
                        .id_salt("script-editor-diagnostics")
                        .max_height(90.0)
                        .show(ui, |ui| {
                            ui.colored_label(crate::theme::get(ctx).sell(), error);
                        });
                    ui.label("The chart keeps the last valid version.");
                } else if editor.validation_pending() && !editor.dirty {
                    ui.label("Checking source…");
                } else if editor.dirty {
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
        if editor.dirty || editor.validation_pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        (!actions.is_empty() || add_to_chart.is_some() || copy_name != state.copy_name).then_some(
            Intent::Requests {
                actions,
                add_to_chart,
                copy_name,
            },
        )
    }
}

pub(crate) fn effects(effects: Vec<Effect>) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    let mut host = Vec::new();
    for effect in effects {
        match effect {
            Effect::Requests {
                actions,
                add_to_chart,
            } => {
                response.script_editor_actions.extend(actions);
                response
                    .indicator_actions
                    .extend(add_to_chart.map(crate::toolbar::ToolbarAction::AddScriptIndicator));
            }
            effect => host.push(effect),
        }
    }
    if !host.is_empty() {
        response
            .model_effects
            .push(quantick_ui_model::HostEffect::ScriptEditor(host));
    }
    response
}
