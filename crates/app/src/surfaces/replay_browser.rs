//! Registered global replay browser. Receivers and native mechanics stay here.
use super::{Adapter, HostEffect, SurfaceResponse, ViewContext};
use crate::theme;
use crate::{
    replay_get_data::{GetDataAction, GetDataPanel},
    replay_view::{
        MarketMenu, ReplayAction, TRANSPORT_HEIGHT, badge, seek_track, speed_chip, thousands,
    },
};
use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_feed::ReplayControl;
use quantick_feed::{ReplayLink, ReplayOptions, ReplayRequest};
use quantick_replay::{ParseOptions, Session, SessionError, library};
use quantick_replay::{clock::SPEEDS, format};
use quantick_ui_model::{
    UpdateSession,
    replay_browser::{BrowserTab, Effects, Intent, LocalEffect, Model, State},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, TryRecvError},
    },
};
pub(crate) type ReplayBrowser = Adapter<Model, View>;
pub(crate) struct ReplayIo {
    loading: Option<Receiver<Result<Session, SessionError>>>,
    picker: Option<Receiver<Option<PathBuf>>>,
    loaded: Option<Session>,
    pub(crate) get_data: GetDataPanel,
}
impl Default for ReplayIo {
    fn default() -> Self {
        Self {
            loading: None,
            picker: None,
            loaded: None,
            get_data: GetDataPanel::new(crate::paper_home::shelf_dir().as_deref()),
        }
    }
}
#[derive(Default)]
pub(crate) struct View {
    pub(crate) io: ReplayIo,
    pub(crate) scrub: Option<f32>,
    pass_link: Option<ReplayLink>,
    transport: Option<ReplayAction>,
}
/// Every ordinary intent executes one local operation, then settles its terminal receipt.
pub(crate) fn dispatch(session: &mut UpdateSession<'_, Model>, io: &mut ReplayIo, intent: Intent) {
    session.with_local_effects(
        &mut |mut effects| {
            let receipt = effects.local.take().map(|operation| io.execute(operation));
            (effects, receipt)
        },
        |session| session.update(intent),
    );
}
impl ReplayIo {
    fn execute(&mut self, operation: LocalEffect) -> Intent {
        match operation {
            LocalEffect::Scan { folder } => {
                let root = PathBuf::from(folder.trim());
                let library = library::scan(&root);
                tracing::info!(target: "quantick::app", schema_version = 1_u8, event_code = "REPLAY_FOLDER_SCANNED", folder = %root.display(), sessions = library.sessions.len(), problems = library.problems.len(), "scanned a replay folder");
                self.get_data.folder_rescanned();
                Intent::ScanCompleted(library)
            }
            LocalEffect::PickFolder { folder } => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("quantick-replay-picker".into())
                    .spawn(move || {
                        let start = PathBuf::from(folder.trim());
                        let mut dialog = rfd::FileDialog::new().set_title("Choose a replay folder");
                        if start.is_dir() {
                            dialog = dialog.set_directory(&start);
                        }
                        let _ = tx.send(dialog.pick_folder());
                    })
                    .expect("spawn folder picker thread");
                self.picker = Some(rx);
                Intent::PickerStarted
            }
            LocalEffect::Load { path, day_before } => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("quantick-replay-load".into())
                    .spawn(move || {
                        let loaded = if day_before {
                            Session::load_with_day_before(&path, ParseOptions::default())
                        } else {
                            Session::load(&path, ParseOptions::default())
                        };
                        let _ = tx.send(loaded);
                    })
                    .expect("spawn replay loader thread");
                self.loading = Some(rx);
                self.loaded = None;
                Intent::LoadStarted
            }
            LocalEffect::EnterDownload {
                folder,
                symbol,
                explicit,
            } => {
                if explicit && let Some(symbol) = symbol.as_deref() {
                    self.get_data.set_symbol(symbol);
                }
                self.get_data.arrive(&folder, symbol.as_deref());
                Intent::DownloadEntered
            }
        }
    }
    fn picker_event(&mut self) -> Option<Intent> {
        let result = self.picker.as_ref()?.try_recv();
        match result {
            Ok(folder) => {
                self.picker = None;
                Some(Intent::PickerCompleted(
                    folder.map(|path| path.display().to_string()),
                ))
            }
            Err(TryRecvError::Disconnected) => {
                self.picker = None;
                Some(Intent::PickerCompleted(None))
            }
            Err(TryRecvError::Empty) => None,
        }
    }
    fn loading_event(&mut self) -> Option<Intent> {
        let result = self.loading.as_ref()?.try_recv();
        let result = match result {
            Ok(Ok(session)) => {
                if let Some(problem) = session.day_before_problem.as_ref() {
                    tracing::warn!(target: "quantick::app", schema_version = 1_u8, event_code = "REPLAY_DAY_BEFORE_UNUSABLE", session = %session.label(), detail = problem.detail.as_str(), advice = problem.advice, "the day before this session will not join it");
                }
                self.loaded = Some(session);
                Ok(())
            }
            Ok(Err(error)) => Err(format!("{error}\n{}", error.advice())),
            Err(TryRecvError::Disconnected) => {
                Err("the loader stopped before finishing".to_owned())
            }
            Err(TryRecvError::Empty) => return None,
        };
        self.loading = None;
        Some(Intent::LoadCompleted(result))
    }
}
impl View {
    fn pump(&mut self, session: &mut UpdateSession<'_, Model>, ctx: &egui::Context) {
        if let Some(event) = self.io.picker_event() {
            dispatch(session, &mut self.io, event);
        }
        let landed = self
            .io
            .get_data
            .pump(session.state().library.as_ref(), session.state().day_before);
        if self.io.get_data.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        let event = match landed {
            Some(GetDataAction::Downloaded(path)) => Some(Intent::Downloaded(path)),
            Some(GetDataAction::Fetched) => Some(Intent::DownloadRefreshed),
            None => None,
        };
        if let Some(event) = event {
            dispatch(session, &mut self.io, event);
        }
        if let Some(event) = self.io.loading_event() {
            dispatch(session, &mut self.io, event);
        }
    }
}
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "replay_browser"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&super::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
    fn paint_updated(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        _: Option<&super::SurfaceEnv<'_>>,
    ) {
        // One supplied link is consumed at entry, retaining its live playback atomics.
        let link = self.pass_link.take();
        self.transport = None;
        self.pump(session, target.context());
        BrowserPaint {
            session,
            view: self,
        }
        .draw_browser(target.context());
        if let Some(link) = link {
            self.transport = self.draw_transport(target.context(), &link);
        }
    }
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    assert!(
        effects.local.is_none(),
        "replay local effects must settle before host conversion"
    );
    SurfaceResponse {
        model_effects: vec![HostEffect::ReplayBrowser(effects)],
        ..Default::default()
    }
}
/// Consume the completed payload once. Successful loading precedes transport input.
pub(crate) fn take_action(
    browser: &mut ReplayBrowser,
    response: SurfaceResponse,
) -> Option<ReplayAction> {
    let mut open = None;
    for effect in response.model_effects {
        if let HostEffect::ReplayBrowser(effects) = effect {
            open = open.or(effects.open_loaded);
        }
    }
    let action = open.and_then(|(speed, autoplay)| {
        browser.view.io.loaded.take().map(|session| {
            ReplayAction::Open(Box::new(ReplayRequest {
                session: Arc::new(session),
                options: ReplayOptions {
                    speed,
                    autoplay,
                    ..Default::default()
                },
            }))
        })
    });
    let transport = browser.view.transport.take();
    action.or(transport)
}
pub(crate) trait ReplayBrowserExt {
    fn initialize(&mut self, stored: Option<&str>, day_before: Option<bool>);
    #[cfg(test)]
    fn new_browser(stored: Option<&str>, day_before: Option<bool>) -> Self;
    fn dispatch(&mut self, intent: Intent) -> Effects;
    fn open_browser(&mut self);
    fn open_format_help(&mut self);
    fn day_before(&self) -> bool;
    fn stored_day_before(&self) -> Option<bool>;
    fn stored_pick(&self) -> Option<&str>;
    fn is_loading(&self) -> bool;
    fn download_provider(&self) -> crate::config::ProviderKind;
    fn take_folder_change(&mut self) -> Option<Option<String>>;
    fn take_day_before_change(&mut self) -> Option<bool>;
    fn bind_pass(&mut self, link: Option<&ReplayLink>, market: &MarketMenu<'_>);
    #[cfg(any(feature = "scenario-harness", test))]
    fn stage_day_before(&mut self, enabled: bool);
    #[cfg(any(feature = "scenario-harness", test))]
    fn folder_in_use(&self) -> &str;
    #[cfg(any(feature = "scenario-harness", test))]
    fn open_get_data(&mut self, symbol: Option<&str>);
    #[cfg(any(feature = "scenario-harness", test))]
    fn autostart(&mut self, speed: f32, day: Option<&str>, play: bool) -> bool;
    #[cfg(test)]
    fn browser_is_open(&self) -> bool;
}
impl ReplayBrowserExt for ReplayBrowser {
    fn initialize(&mut self, stored: Option<&str>, day_before: Option<bool>) {
        self.dispatch(Intent::Initialize {
            folder: crate::replay_home::resolve(stored),
            stored: stored.map(str::to_owned),
            day_before,
        });
    }
    #[cfg(test)]
    fn new_browser(stored: Option<&str>, day_before: Option<bool>) -> Self {
        let mut browser = Self::new(View::default(), effects)
            .in_slots(&[quantick_ui_model::Slot::EarlyEdgePanels]);
        browser.initialize(stored, day_before);
        browser
    }
    fn dispatch(&mut self, intent: Intent) -> Effects {
        let mut result = Effects::default();
        self.with_updates(
            |response| {
                for effect in response.model_effects {
                    if let HostEffect::ReplayBrowser(effect) = effect {
                        if effect.folder_change.is_some() {
                            result.folder_change = effect.folder_change;
                        }
                        if effect.day_before_change.is_some() {
                            result.day_before_change = effect.day_before_change;
                        }
                        if effect.selected_day.is_some() {
                            result.selected_day = effect.selected_day;
                        }
                        result.load_started |= effect.load_started;
                        result.open_loaded = result.open_loaded.or(effect.open_loaded);
                    }
                }
            },
            |view, session| dispatch(session, &mut view.io, intent),
        );
        result
    }
    fn open_browser(&mut self) {
        self.dispatch(Intent::OpenBrowser);
    }
    fn open_format_help(&mut self) {
        self.dispatch(Intent::OpenFormatHelp);
    }
    fn day_before(&self) -> bool {
        self.model_state.day_before
    }
    fn stored_day_before(&self) -> Option<bool> {
        self.model_state.day_before_stored
    }
    fn stored_pick(&self) -> Option<&str> {
        self.model_state.stored.as_deref()
    }
    fn is_loading(&self) -> bool {
        self.model_state.loading.is_some()
    }
    fn download_provider(&self) -> crate::config::ProviderKind {
        self.view.io.get_data.provider()
    }
    fn take_folder_change(&mut self) -> Option<Option<String>> {
        self.dispatch(Intent::TakeFolderChange).folder_change
    }
    fn take_day_before_change(&mut self) -> Option<bool> {
        self.dispatch(Intent::TakeDayBeforeChange).day_before_change
    }
    fn bind_pass(&mut self, link: Option<&ReplayLink>, market: &MarketMenu<'_>) {
        self.view.pass_link = link.cloned();
        self.model_state
            .observe_market(market.current, &market.catalogue);
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn stage_day_before(&mut self, enabled: bool) {
        self.dispatch(Intent::StageDayBefore(enabled));
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn folder_in_use(&self) -> &str {
        &self.model_state.folder
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn open_get_data(&mut self, symbol: Option<&str>) {
        self.open_browser();
        self.dispatch(Intent::EnterDownload {
            symbol: symbol.map(str::to_owned),
            explicit: true,
        });
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn autostart(&mut self, speed: f32, day: Option<&str>, play: bool) -> bool {
        self.dispatch(Intent::Rescan);
        self.dispatch(Intent::AutostartSelection {
            speed,
            day: day.map(str::to_owned),
            play,
        })
        .load_started
    }
    #[cfg(test)]
    fn browser_is_open(&self) -> bool {
        self.model_state.browser_open
    }
}
pub(crate) struct BrowserPaint<'a, 'b> {
    pub session: &'a mut UpdateSession<'b, Model>,
    pub view: &'a mut View,
}
impl BrowserPaint<'_, '_> {
    fn dispatch(&mut self, intent: Intent) {
        dispatch(self.session, &mut self.view.io, intent);
    }
    /// Draw the session browser, starting a load if one was asked for.
    pub(crate) fn draw_browser(&mut self, ctx: &egui::Context) {
        let palette = theme::get(ctx);
        if !self.session.state().browser_open {
            return;
        }
        let mut open = true;
        let mut load_clicked = false;
        let mut day_before_change = None;

        egui::Window::new("Market Replay")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(palette.text_primary());
                self.draw_tab_strip(ui);
                ui.add_space(8.0);
                self.draw_folder_row(ui);
                ui.add_space(10.0);
                match self.session.state().tab {
                    BrowserTab::Sessions => {
                        load_clicked = self.draw_session_list(ui);
                        ui.add_space(8.0);
                        self.draw_problems(ui);
                        self.draw_format_help(ui);
                        ui.add_space(6.0);
                        load_clicked |= self.draw_start_row(ui);
                    }
                    BrowserTab::GetData => {
                        let state = self.session.state();
                        crate::replay_get_data::draw(
                            &mut self.view.io.get_data,
                            ui,
                            &state.folder,
                            &state.offered_symbols,
                        );
                        // Drained apart from the action, so a tick and a
                        // landed tape in the same frame do not compete for the
                        // one slot an action has.
                        day_before_change = self.view.io.get_data.take_day_before_change();
                    }
                }
            });

        self.dispatch(Intent::SetOpen(open));
        // The download half and the session list edit one setting, so the tick
        // made on either row is applied to the browser that owns it.
        if let Some(enabled) = day_before_change {
            self.dispatch(Intent::SetDayBefore(enabled));
        }
        if load_clicked {
            self.dispatch(Intent::LoadSelected);
        }
    }

    /// The two tabs. A download in flight is announced on its tab, so switching
    /// to the session list never loses sight of it.
    fn draw_tab_strip(&mut self, ui: &mut egui::Ui) {
        let mut entered_get_data = false;
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    self.session.state().tab == BrowserTab::Sessions,
                    "My sessions",
                )
                .clicked()
            {
                self.dispatch(Intent::SelectSessions);
            }
            let label = if self.view.io.get_data.is_busy() {
                format!("Get data  {}", icons::ARROWS_CLOCKWISE)
            } else {
                "Get data".to_string()
            };
            if ui
                .selectable_label(self.session.state().tab == BrowserTab::GetData, label)
                .on_hover_text("Download a session from MetaTrader 5")
                .clicked()
            {
                entered_get_data = self.session.state().tab != BrowserTab::GetData;
            }
        });
        // Arriving on the tab *is* the question "what can I download?", so it
        // is asked here rather than left behind a button the trader has to
        // find. Only on arrival: staying on the tab must not re-ask.
        if entered_get_data {
            self.dispatch(Intent::EnterDownload {
                symbol: None,
                explicit: false,
            });
        }
    }

    pub(crate) fn draw_folder_row(&mut self, ui: &mut egui::Ui) {
        let palette = theme::get(ui.ctx());
        ui.label(
            egui::RichText::new("Replay folder")
                .color(palette.text_muted())
                .small(),
        );
        let mut folder = self.session.state().folder.clone();
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 110.0).max(120.0);
            let response = ui.add_sized(
                [width, 24.0],
                egui::TextEdit::singleline(&mut folder)
                    .hint_text("…/replay")
                    .desired_width(width),
            );
            if response.changed() {
                self.dispatch(Intent::SetFolderDraft(folder.clone()));
            }
            // Typing is not choosing; pressing Enter on what was typed is.
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.dispatch(Intent::ChooseFolder);
            }
            if ui.button("Browse…").clicked() {
                self.dispatch(Intent::PickFolder);
            }
            // Re-reading the same folder answers "did anything land?" — it
            // says nothing about where recordings should live, so it scans
            // without recording a choice.
            if ui
                .button(icons::ARROW_CLOCKWISE)
                .on_hover_text("Scan this folder again")
                .clicked()
            {
                self.dispatch(Intent::Rescan);
            }
        });
        if self.session.state().picker_pending {
            ui.horizontal(|ui| {
                crate::loading::inline(ui, "waiting for the folder dialog");
            });
        }
    }

    /// Draw the session list. Returns whether one was double-clicked, which is
    /// the same request as picking it and pressing **Play session**.
    fn draw_session_list(&mut self, ui: &mut egui::Ui) -> bool {
        let palette = theme::get(ui.ctx());
        let mut open_requested = false;
        let mut selection = None;
        ui.label(
            egui::RichText::new("Sessions")
                .color(palette.text_muted())
                .small(),
        );
        let frame = egui::Frame::none()
            .fill(palette.replay_well())
            .inner_margin(8.0)
            .rounding(4.0);

        frame.show(ui, |ui| {
            ui.set_min_height(150.0);
            let Some(library) = self.session.state().library.as_ref() else {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Choose the folder holding your recorded sessions.")
                            .color(palette.text_muted()),
                    );
                });
                return;
            };
            if library.sessions.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No sessions in this folder.").color(palette.warn()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "quantick reads one CSV per session day, in a folder per instrument.",
                        )
                        .color(palette.text_muted())
                        .small(),
                    );
                });
                return;
            }

            egui::ScrollArea::vertical()
                .max_height(180.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, entry) in library.sessions.iter().enumerate() {
                        let selected = self.session.state().selected == Some(index);
                        let response = ui.selectable_label(
                            selected,
                            egui::RichText::new(format!(
                                "{}      {}",
                                entry.label(),
                                entry.size_label()
                            )),
                        );
                        if response.clicked() {
                            selection = Some(index);
                        }
                        if response.double_clicked() {
                            selection = Some(index);
                            open_requested = true;
                        }
                        if !entry.notes.is_empty() {
                            for note in &entry.notes {
                                ui.label(
                                    egui::RichText::new(format!("    {note}"))
                                        .color(palette.text_muted())
                                        .small(),
                                );
                            }
                        }
                    }
                });
        });
        if let Some(index) = selection {
            self.dispatch(Intent::Select(index));
        }
        open_requested
    }

    fn draw_problems(&mut self, ui: &mut egui::Ui) {
        let palette = theme::get(ui.ctx());
        let Some(library) = self.session.state().library.as_ref() else {
            return;
        };
        if library.problems.is_empty() {
            return;
        }
        let header = format!(
            "{} file(s) were not loaded",
            library
                .problems
                .iter()
                .filter(|p| p.path.is_some())
                .count()
                .max(1)
        );
        egui::CollapsingHeader::new(egui::RichText::new(header).color(palette.warn()))
            .default_open(library.sessions.is_empty())
            .show(ui, |ui| {
                for problem in &library.problems {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(problem.subject())
                                .color(palette.text_primary())
                                .monospace(),
                        );
                        ui.label(egui::RichText::new(&problem.detail).color(palette.text_muted()));
                    });
                    ui.label(
                        egui::RichText::new(format!("    {}", problem.advice()))
                            .color(palette.text_muted())
                            .small(),
                    );
                    ui.add_space(4.0);
                }
            });
    }

    pub(crate) fn draw_format_help(&mut self, ui: &mut egui::Ui) {
        let palette = theme::get(ui.ctx());
        let label = if self.session.state().show_format {
            "Hide the file format"
        } else {
            "Show the file format"
        };
        if ui
            .link(egui::RichText::new(label).color(if palette.resolved.dark {
                palette.text_muted()
            } else {
                palette.text_support()
            }))
            .clicked()
        {
            self.dispatch(Intent::SetShowFormat(!self.session.state().show_format));
        }
        if !self.session.state().show_format {
            return;
        }
        egui::Frame::none()
            .fill(palette.replay_inset())
            .inner_margin(8.0)
            .rounding(4.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .id_salt("replay_format_help")
                    .show(ui, |ui| {
                        for paragraph in format::FORMAT_HELP.split("\n\n") {
                            let rich = egui::RichText::new(paragraph)
                                .monospace()
                                .text_style(egui::TextStyle::Body)
                                .color(if palette.resolved.dark {
                                    palette.text_muted()
                                } else {
                                    palette.text_support()
                                });
                            ui.add(egui::Label::new(rich).selectable(true).wrap());
                            ui.add_space(6.0);
                        }
                    });
            });
    }

    /// The bottom row: speed to start at, and the button that starts playback.
    /// Returns whether the load was asked for.
    fn draw_start_row(&mut self, ui: &mut egui::Ui) -> bool {
        let palette = theme::get(ui.ctx());
        if let Some(error) = &self.session.state().error {
            egui::Frame::none()
                .fill(palette.replay_error())
                .inner_margin(8.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(error).color(palette.warn()).small());
                });
            ui.add_space(6.0);
        }

        let mut clicked = false;
        // Two rows, not one. The speed chips, two ticks and the button did not
        // fit the dialog's width, and what a right-to-left button takes is
        // taken from the label beside it: the day-before tick was drawn cut in
        // half. Speeds and the button first, because that is the pair a trader
        // ends on, then the two questions about how it opens.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Start at")
                    .color(palette.text_muted())
                    .small(),
            );
            for speed in SPEEDS {
                let selected = (self.session.state().speed - speed).abs() < f32::EPSILON;
                if speed_chip(ui, speed, selected).clicked() {
                    self.dispatch(Intent::SetSpeed(speed));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(loading) = &self.session.state().loading {
                    crate::loading::inline(ui, &format!("loading {}", loading));
                    return;
                }
                let ready = self.session.state().selected_entry().is_some();
                let button = egui::Button::new(
                    egui::RichText::new("Play session").color(palette.replay_plate_ink()),
                )
                .fill(palette.replay_plate());
                if ui
                    .add_enabled(ready, button)
                    .on_disabled_hover_text("Pick a session from the list first")
                    .clicked()
                {
                    clicked = true;
                }
            });
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let mut autoplay = self.session.state().autoplay;
            if ui.checkbox(&mut autoplay, "and play")
                .on_hover_text("Leave unticked to open the session paused on its first print").changed() { self.dispatch(Intent::SetAutoplay(autoplay)); }
            ui.add_space(8.0);
            let mut day_before = self.session.state().day_before;
            if ui
                .checkbox(&mut day_before, "with the day before")
                .on_hover_text(
                    "Join the previous session's tape in front of this one, already played, so the chart opens with yesterday's order flow behind the day you picked",
                )
                .changed()
            {
                self.dispatch(Intent::SetDayBefore(day_before));
            }
        });
        clicked
    }
}
impl View {
    /// The transport strip: one 30 px row directly above the status bar while
    /// a session is playing — part of the status system, not an island (§8).
    pub(crate) fn draw_transport(
        &mut self,
        ctx: &egui::Context,
        link: &ReplayLink,
    ) -> Option<ReplayAction> {
        let palette = theme::get(ctx);
        let mut action = None;
        let status = &link.status;

        egui::TopBottomPanel::bottom(egui::Id::new((ctx.viewport_id(), "replay_transport")))
            .exact_height(TRANSPORT_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(palette.chrome())
                    .inner_margin(egui::Margin::symmetric(10.0, 3.0)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(palette.text_primary());
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui
                        .button(icons::SKIP_BACK)
                        .on_hover_text("Back to the first print")
                        .clicked()
                    {
                        action = Some(ReplayAction::Control(ReplayControl::Restart));
                    }
                    // The button states what it does, rather than toggling
                    // blind: it reads the same status it draws, so a click
                    // cannot invert a state that changed between frames.
                    let playing = status.is_playing() && !status.is_finished();
                    let (label, hint, control) = if playing {
                        (icons::PAUSE, "Pause (Space)", ReplayControl::Pause)
                    } else {
                        (icons::PLAY, "Play (Space)", ReplayControl::Play)
                    };
                    if ui
                        .button(egui::RichText::new(label).color(palette.amber()))
                        .on_hover_text(hint)
                        .clicked()
                    {
                        action = Some(ReplayAction::Control(control));
                    }

                    badge(ui, "REPLAY");
                    ui.label(egui::RichText::new(link.label()).color(palette.text_primary()));
                    // Two days on one chart is never left to be inferred from
                    // the bars: the recording says which day was joined in
                    // front of it, or why none was when one was there to join.
                    if let Some(joined) = link.session.day_before_label() {
                        ui.label(egui::RichText::new(format!("+ {joined}")).color(palette.text_muted()).small())
                            .on_hover_text(
                                "The day before, joined in front of this session: its order flow is already on the chart, and play starts on the day you picked",
                            );
                    } else if let Some(problem) = link.session.day_before_problem.as_ref() {
                        ui.label(egui::RichText::new("no day before").color(palette.warn()).small())
                            .on_hover_text(format!("{}\n{}", problem.detail, problem.advice));
                    }
                    ui.label(
                        egui::RichText::new(link.clock_text(self.scrub))
                            .monospace()
                            .color(palette.amber()),
                    );

                    for speed in SPEEDS {
                        let selected = (status.speed() - speed).abs() < f32::EPSILON;
                        if speed_chip(ui, speed, selected).clicked() {
                            action = Some(ReplayAction::Control(ReplayControl::SetSpeed(speed)));
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(icons::X)
                            .on_hover_text("Close the replay and go back to the live feed")
                            .clicked()
                        {
                            action = Some(ReplayAction::Close);
                        }
                        // Counted over the session being rehearsed, like the
                        // seek track beside it: a joined day is context the
                        // chart was handed, and counting its prints here would
                        // open a fresh replay at "half played". Asked of the
                        // link rather than worked out here, so the control
                        // plane cannot answer the same question differently.
                        let (played, total) = link.day_prints();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} / {} prints",
                                thousands(played),
                                thousands(total)
                            ))
                            .color(palette.text_muted())
                            .small(),
                        );
                        // The seek track takes every pixel left between the
                        // speed chips and the counts.
                        if let Some(fraction) = seek_track(ui, status.progress(), &mut self.scrub) {
                            action = Some(ReplayAction::Control(ReplayControl::SeekToFraction(
                                fraction,
                            )));
                        }
                        if self.scrub.is_some() {
                            let (layer, id) = (ui.layer_id(), ui.id().with("scrub_time"));
                            let time = link.clock_text(self.scrub);
                            egui::show_tooltip_at_pointer(ctx, layer, id, |ui| ui.monospace(time));
                        }
                    });
                });
            });

        // Space toggles playback, the way every media transport behaves. Only
        // while nothing has keyboard focus, so typing in a field is untouched.
        if ctx.memory(|m| m.focused().is_none()) && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            action = Some(ReplayAction::Control(ReplayControl::TogglePlay));
        }
        action
    }
}

#[cfg(test)]
#[path = "replay_browser/tests.rs"]
mod tests;
