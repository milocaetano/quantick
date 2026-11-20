use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;
use quantick_ui_model::strategy_describe::{
    Command, Effects, Intent, MAX_TEXT_BYTES, Model, Phase, State, Turn,
};
mod preview;
mod protocol;
pub(crate) mod provider;

#[derive(Default)]
pub(crate) struct View {
    pub(crate) worker: provider::Worker,
    pub(crate) command: Option<Command>,
    pub(crate) library: Option<std::path::PathBuf>,
    pub(crate) viewport: egui::ViewportId,
    text: String,
    name: String,
    hidden: Option<u64>,
    keyboard: Vec<egui::Event>,
    keys: Option<std::collections::HashSet<egui::Key>>,
}
fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Ime(_)
            | egui::Event::Paste(_)
            | egui::Event::Cut
            | egui::Event::Copy
    )
}
impl View {
    pub(crate) fn open(&mut self, viewport: egui::ViewportId) {
        self.viewport = viewport;
        self.text.clear();
        self.name.clear();
        self.hidden = None;
    }
}
impl super::View<Model> for View {
    fn capture_keyboard(&mut self, state: &State, ctx: &egui::Context) {
        self.keyboard.clear();
        self.keys = None;
        let field = match state.status.phase {
            Phase::Ready | Phase::Question => "message",
            Phase::Preview => "filename",
            _ => return,
        };
        if ctx.viewport_id() != self.viewport
            || self.hidden == Some(state.status.revision)
            || !ctx.memory(|memory| memory.has_focus(egui::Id::new(self.id()).with(field)))
        {
            return;
        }
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                if keyboard_event(event) {
                    self.keyboard.push(event.clone());
                    false
                } else {
                    true
                }
            });
            self.keys = Some(std::mem::take(&mut input.keys_down));
        });
    }
    fn id(&self) -> &'static str {
        "strategy_describe"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let status = &state.status;
        if let (Some(context), Some(env)) = (state.context(), env) {
            if context.tab != env.active_tab {
                return None;
            }
            self.viewport = target.context().viewport_id();
        }
        if target.context().viewport_id() != self.viewport
            || matches!(status.phase, Phase::Unavailable | Phase::Closed)
            || self.hidden == Some(status.revision)
        {
            return None;
        }
        let revision = status.revision;
        let mut open = true;
        let ctx = target.context();
        let typing = self.keys.is_some();
        if let Some(keys) = self.keys.take() {
            ctx.input_mut(|input| {
                input.events.append(&mut self.keyboard);
                input.keys_down = keys;
            });
        }
        egui::Window::new("Describe a strategy")
            .id(egui::Id::new(self.id()))
            .open(&mut open)
            .order(egui::Order::Foreground)
            .default_width(460.0)
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(chart) = state.context() {
                    ui.strong(format!(
                        "{} · {} · {}",
                        chart.symbol, chart.feed, chart.bars
                    ));
                    if let Some(drawing) = chart.placement.drawing() {
                        ui.label(format!("Alarm region #{}", drawing.0));
                    }
                }
                egui::ScrollArea::vertical()
                    .max_height(440.0)
                    .show(ui, |ui| {
                        for turn in &status.transcript {
                            match turn {
                                Turn::Trader(s) => {
                                    ui.strong("You");
                                    ui.label(s);
                                }
                                Turn::Assistant(s) => {
                                    ui.strong("Strategy assistant");
                                    ui.label(s);
                                }
                            }
                        }
                        ui.label(&status.message);
                        if let Some(draft) = &status.draft {
                            ui.heading("Alarm preview");
                            ui.label(&draft.summary);
                            if let Some(chart) = state.context() {
                                preview::plot(ui, &chart.history, &draft.markers);
                            }
                            for marker in &draft.markers {
                                ui.label(format!(
                                    "{:?} · slot {} · {} UTC",
                                    marker.side,
                                    marker.slot,
                                    quantick_civil::fmt_offset_minute(
                                        marker.close_time,
                                        quantick_civil::TzOffset::new(0)
                                    )
                                ));
                            }
                            ui.collapsing("Generated Pine source", |ui| {
                                ui.monospace(&draft.source);
                            });
                        }
                    });
                match status.phase {
                    Phase::Ready | Phase::Question => {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.text)
                                .id(egui::Id::new("strategy_describe").with("message"))
                                .desired_rows(3)
                                .hint_text("Describe the conditions for your alarm")
                                .char_limit(MAX_TEXT_BYTES),
                        );
                        if ui
                            .add_enabled(
                                !self.text.trim().is_empty() && self.text.len() <= MAX_TEXT_BYTES,
                                egui::Button::new("Send"),
                            )
                            .clicked()
                        {
                            self.command = Some(Command::Submit {
                                revision,
                                text: std::mem::take(&mut self.text),
                            });
                        }
                    }
                    Phase::Waiting => {
                        ui.spinner();
                        ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    }
                    Phase::Preview => {
                        ui.label("Strategy file name");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.name)
                                .id(egui::Id::new("strategy_describe").with("filename"))
                                .hint_text("my-alarm.pine")
                                .char_limit(128),
                        );
                        if ui
                            .add_enabled(
                                !self.name.trim().is_empty(),
                                egui::Button::new("Save and arm alarms"),
                            )
                            .clicked()
                        {
                            self.command = Some(Command::Save {
                                revision,
                                id: std::mem::take(&mut self.name),
                            });
                        }
                    }
                    Phase::Failed | Phase::Saved | Phase::Cancelled
                        if ui.button("New conversation").clicked() =>
                    {
                        self.command = Some(Command::Open);
                    }
                    _ => {}
                }
                if state.context().is_some() && ui.button("Cancel conversation").clicked() {
                    self.command = Some(Command::Cancel { revision });
                }
            });
        if typing {
            ctx.input_mut(|input| {
                input.events.retain(|event| !keyboard_event(event));
                input.keys_down.clear();
            });
        }
        if !open {
            self.hidden = Some(revision);
            if state.context().is_some() {
                self.hidden = Some(revision + 1);
                self.command = Some(Command::Cancel { revision });
            }
        }
        None
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) -> Option<Intent> {
        if self.worker.available()
            && crate::hooks::captured::var("QUANTICK_STRATEGY_DESCRIBE").as_deref() == Some("1")
        {
            self.command = Some(Command::Open);
        }
        None
    }
}
pub(crate) fn effects(effect: Effects) -> SurfaceResponse {
    SurfaceResponse::effect_if(
        effect != Effects::None,
        super::HostEffect::StrategyDescribe(effect),
    )
}
#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_STRATEGY_DESCRIBE"];
