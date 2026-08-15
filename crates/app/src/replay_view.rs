//! Market Replay's interface: the session browser and the transport bar.
//!
//! Two rules shape it. The chart is the product, so replay adds exactly one menu
//! entry and one bar that exists only while a recording is playing — nothing is
//! added to the controls a live chart already carries. And a folder that does
//! not load must say why in the interface, not in a log: the browser names the
//! file, the line, what was found and what to change, with the whole format one
//! click away.
//!
//! The view owns no playback state. It reads [`ReplayLink`] (atomics, published
//! by the worker) and returns a [`ReplayAction`] for the app to carry out, so
//! drawing can never block the thread releasing trades.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;
use egui_phosphor::regular as icons;

use quantick_replay::clock::SPEEDS;
use quantick_replay::format::{self, UtcOffset};
use quantick_replay::{Library, ParseOptions, Session, SessionEntry, SessionError, library};

use crate::feed::{ReplayControl, ReplayLink, ReplayOptions, ReplayRequest};
use crate::replay_get_data::{GetDataAction, GetDataPanel};
use crate::theme::{AMBER, CHROME, CONTROL, TEXT_MUTED, TEXT_PRIMARY, WARN};

/// The accent this feature owns: the same amber the chart already uses for the
/// backfill/live divider, so "this is not live data" reads the same way twice.
/// Borrowed rather than repeated, so the two can never drift apart.
const REPLAY_ACCENT: egui::Color32 = AMBER;

/// Height of the transport strip, in pixels — part of the status system, one
/// line directly above the status bar (`docs/ux/ui-design-model.md` §8).
pub const TRANSPORT_HEIGHT: f32 = 30.0;

// The folder this browser opens on is resolved by [`crate::replay_home`],
// which owns the whole order — the `QUANTICK_REPLAY_DIR` hook, the trader's
// stored pick, the documents home. The view is handed the answer rather than
// reading the environment itself, so there is one place where "which folder"
// is decided and nowhere for a second opinion to appear.

/// Environment variable that opens the browser on its **Get data** tab.
///
/// `1` opens it on the chart's own instrument — what clicking the tab does —
/// and any other value states the symbol outright, so a scripted run reaches
/// the calendar of a *named* contract from a fresh launch with no clicks.
pub const GET_DATA_ENV: &str = "QUANTICK_REPLAY_GET_DATA";

/// Height of the seek track, in pixels.
const TRACK_HEIGHT: f32 = 6.0;

/// Radius of the seek handle, resting and hovered.
const HANDLE_RADIUS: f32 = 4.5;
/// See [`HANDLE_RADIUS`].
const HANDLE_RADIUS_HOVERED: f32 = 6.0;

/// The instruments the browser can offer without the trader typing one.
///
/// Passed in per frame rather than held, because both halves change under the
/// app's feet: the chart's symbol follows the active tab, and the catalogue
/// grows when a contract is added from the source picker.
pub struct MarketMenu<'a> {
    /// What the active chart is showing — the likeliest answer to "which
    /// instrument", and the one the Get data tab fills itself with.
    pub current: Option<&'a str>,
    /// What the download source's catalogue lists, in its own order.
    pub catalogue: &'a [String],
}

/// What the replay interface asks the app to do.
pub enum ReplayAction {
    /// Start playing this session.
    Open(Box<ReplayRequest>),
    /// Drive the session that is already playing.
    Control(ReplayControl),
    /// Leave replay and go back to the live feed.
    Close,
}

/// A load running on a worker thread, with the label to show while it runs.
struct Loading {
    label: String,
    result: Receiver<Result<Session, SessionError>>,
}

/// The two halves of the browser.
///
/// One window, because for a trader "get me Wednesday" and "play Wednesday"
/// are the same errand. A separate menu entry for downloading would be a
/// feature shipped and never found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserTab {
    /// Recordings already on disk.
    Sessions,
    /// Download a new one from a provider.
    GetData,
}

/// The session browser's state. Owned by the app, drawn every frame.
pub struct ReplayView {
    browser_open: bool,
    folder: String,
    library: Option<Library>,
    selected: Option<usize>,
    loading: Option<Loading>,
    picker: Option<Receiver<Option<PathBuf>>>,
    error: Option<String>,
    show_format: bool,
    speed: f32,
    autoplay: bool,
    /// Where the seek handle is being held, while it is being held.
    ///
    /// Seeking rebuilds the chart from the session's history, which is work
    /// proportional to the whole recording — so a drag shows this position and
    /// only commits it when the handle is let go. See [`seek_track`].
    scrub: Option<f32>,
    /// Which half of the browser is showing.
    tab: BrowserTab,
    /// The download half's state, kept across tab switches so a running
    /// download is not lost by looking at the session list.
    get_data: GetDataPanel,
    /// The folder as it was last written to the workspace file.
    ///
    /// Kept so a change can be spotted without writing the file every frame:
    /// the pick is persisted the moment it is made, not only on a clean exit,
    /// because "I chose the folder and it forgot again" is the report this
    /// whole path exists to answer.
    remembered: String,
    /// Whether the folder in use has moved away from [`Self::remembered`].
    ///
    /// Raised by [`Self::rescan`] rather than by every keystroke: typing a path
    /// is not choosing one, and a flag on the text field would write the
    /// workspace file once per character.
    folder_dirty: bool,
}

impl Default for ReplayView {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ReplayView {
    /// A closed browser on the folder [`crate::replay_home`] resolves: the
    /// environment hook, else the trader's `stored` pick, else the documents
    /// home. Never nowhere.
    #[must_use]
    pub fn new(stored: Option<&str>) -> Self {
        let folder = crate::replay_home::resolve(stored);
        Self {
            browser_open: false,
            remembered: folder.clone(),
            folder,
            library: None,
            selected: None,
            loading: None,
            picker: None,
            error: None,
            show_format: false,
            speed: 1.0,
            autoplay: true,
            scrub: None,
            tab: BrowserTab::Sessions,
            get_data: GetDataPanel::new(),
            folder_dirty: false,
        }
    }

    /// Open the browser, rescanning the folder it already knows.
    pub fn open_browser(&mut self) {
        self.browser_open = true;
        if self.library.is_none() && !self.folder.trim().is_empty() {
            self.rescan();
        }
    }

    /// The folder the workspace should record: the last one actually put to
    /// use, never the half-typed contents of the field.
    ///
    /// A trader who starts typing a path and closes the window has not chosen
    /// anything; writing `D:\ta` down would lose the folder that was working.
    #[must_use]
    pub fn remembered_folder(&self) -> &str {
        &self.remembered
    }

    /// The folder to write down, once, when the trader has pointed the browser
    /// somewhere new. `None` when nothing has changed since the last write.
    ///
    /// Taken rather than read so one pick is written once, on the frame it was
    /// made — a standing choice must survive a crash, not just a clean exit.
    pub fn take_folder_change(&mut self) -> Option<String> {
        if !self.folder_dirty {
            return None;
        }
        self.folder_dirty = false;
        self.remembered = self.folder.trim().to_string();
        Some(self.remembered.clone())
    }

    /// Open the browser on its **Get data** tab, optionally with a symbol
    /// already typed and looked up.
    ///
    /// The same states a click produces: the tab, the typed symbol, and — so
    /// the calendar is reachable from a fresh launch with no hand on the mouse
    /// — the day look-up that a press of **Look up days** would run.
    pub fn open_get_data(&mut self, symbol: Option<&str>) {
        self.open_browser();
        self.tab = BrowserTab::GetData;
        if let Some(symbol) = symbol {
            self.get_data.set_symbol(symbol);
            let folder = self.folder.clone();
            self.get_data.look_up_days(&folder);
        }
    }

    /// Show the format reference on its own, from the Help menu.
    pub fn open_format_help(&mut self) {
        self.browser_open = true;
        self.show_format = true;
    }

    /// Whether a session is being parsed on a worker right now — what the
    /// app's loading overlay mirrors.
    #[must_use]
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Scan the configured folder and start its first session, without opening
    /// the browser.
    ///
    /// The same path a click takes — scan, select, load on a worker — so what a
    /// scripted run exercises is what a person gets. Returns whether a session
    /// was found to load; a folder that yields nothing opens the browser
    /// instead, where the reason is already spelled out.
    pub fn autostart(&mut self, speed: f32) -> bool {
        self.speed = speed;
        self.autoplay = true;
        self.rescan();
        if self.selected_entry().is_none() {
            self.browser_open = true;
            return false;
        }
        self.load_selected();
        true
    }

    /// Draw the browser and, while a session is playing, the transport bar.
    ///
    /// Returns at most one action per frame — a person clicks one thing at a
    /// time, and folding several into a frame would let a stale click win.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        link: Option<&ReplayLink>,
        market: &MarketMenu<'_>,
    ) -> Option<ReplayAction> {
        self.poll_picker();
        let mut action = self.poll_loading();
        // The browser starts a load rather than returning an action: the
        // session is parsed on a worker, and `poll_loading` turns the result
        // into the `Open` a later frame carries.
        self.draw_browser(ctx, market);
        if let Some(link) = link
            && let Some(from_transport) = self.draw_transport(ctx, link)
        {
            action = action.or(Some(from_transport));
        }
        action
    }

    /// Take the result of a finished background load.
    fn poll_loading(&mut self) -> Option<ReplayAction> {
        let loading = self.loading.as_ref()?;
        match loading.result.try_recv() {
            Ok(Ok(session)) => {
                self.loading = None;
                self.browser_open = false;
                self.error = None;
                Some(ReplayAction::Open(Box::new(ReplayRequest {
                    session: Arc::new(session),
                    options: ReplayOptions {
                        speed: self.speed,
                        autoplay: self.autoplay,
                        ..ReplayOptions::default()
                    },
                })))
            }
            Ok(Err(e)) => {
                self.error = Some(format!("{e}\n{}", e.advice()));
                self.loading = None;
                None
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.error = Some("the loader stopped before finishing".to_string());
                self.loading = None;
                None
            }
        }
    }

    /// Take the folder the native dialog came back with.
    fn poll_picker(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        match picker.try_recv() {
            Ok(Some(folder)) => {
                self.folder = folder.display().to_string();
                self.picker = None;
                self.rescan();
            }
            Ok(None) | Err(TryRecvError::Disconnected) => self.picker = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Scan the folder in the text field.
    ///
    /// Putting a folder to use is what makes it the trader's choice, so this is
    /// also where the workspace is told to remember it.
    fn rescan(&mut self) {
        if self.folder.trim() != self.remembered {
            self.folder_dirty = true;
        }
        let root = PathBuf::from(self.folder.trim());
        let library = library::scan(&root);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_FOLDER_SCANNED",
            folder = %root.display(),
            sessions = library.sessions.len(),
            problems = library.problems.len(),
            "scanned a replay folder"
        );
        self.selected = (!library.sessions.is_empty()).then_some(0);
        self.library = Some(library);
        self.error = None;
    }

    /// Ask the operating system for a folder, off the UI thread so the dialog
    /// never freezes a frame.
    fn browse(&mut self) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let start = PathBuf::from(self.folder.trim());
        std::thread::Builder::new()
            .name("quantick-replay-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose a replay folder");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = tx.send(dialog.pick_folder());
            })
            .expect("spawn folder picker thread");
        self.picker = Some(rx);
    }

    /// Start loading the selected session on a worker thread.
    fn load_selected(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let path = entry.path.clone();
        std::thread::Builder::new()
            .name("quantick-replay-load".into())
            .spawn(move || {
                let _ = tx.send(Session::load(&path, ParseOptions::default()));
            })
            .expect("spawn replay loader thread");
        self.loading = Some(Loading {
            label: entry.label(),
            result: rx,
        });
        self.error = None;
    }

    fn selected_entry(&self) -> Option<&SessionEntry> {
        let library = self.library.as_ref()?;
        library.sessions.get(self.selected?)
    }

    /// Every instrument the Get data tab can offer with one click.
    ///
    /// The chart's own first — it is what the trader is looking at — then the
    /// catalogue, then the contracts already sitting in the replay folder, so
    /// last month's expired contract stays one click away long after it leaves
    /// the broker's Market Watch. Deduplicated, order preserved.
    fn offered_symbols(&self, market: &MarketMenu<'_>) -> Vec<String> {
        let mut offered: Vec<String> = Vec::new();
        let mut push = |symbol: &str| {
            let symbol = symbol.trim();
            if !symbol.is_empty() && !offered.iter().any(|held| held == symbol) {
                offered.push(symbol.to_string());
            }
        };
        if let Some(current) = market.current {
            push(current);
        }
        for symbol in market.catalogue {
            push(symbol);
        }
        if let Some(library) = self.library.as_ref() {
            for entry in &library.sessions {
                push(&entry.symbol);
            }
        }
        offered
    }

    /// Draw the session browser, starting a load if one was asked for.
    fn draw_browser(&mut self, ctx: &egui::Context, market: &MarketMenu<'_>) {
        if !self.browser_open {
            return;
        }
        let mut open = true;
        let mut load_clicked = false;
        let mut downloaded = None;

        egui::Window::new("Market Replay")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(TEXT_PRIMARY);
                self.draw_tab_strip(ui, market);
                ui.add_space(8.0);
                self.draw_folder_row(ui);
                ui.add_space(10.0);
                match self.tab {
                    BrowserTab::Sessions => {
                        load_clicked = self.draw_session_list(ui);
                        ui.add_space(8.0);
                        self.draw_problems(ui);
                        self.draw_format_help(ui);
                        ui.add_space(6.0);
                        load_clicked |= self.draw_start_row(ui);
                    }
                    BrowserTab::GetData => {
                        let folder = self.folder.clone();
                        let offered = self.offered_symbols(market);
                        downloaded = self
                            .get_data
                            .draw(ui, &folder, self.library.as_ref(), &offered)
                            .map(|GetDataAction::Downloaded(tape)| tape);
                    }
                }
            });

        self.browser_open = open;
        // A finished download lands the trader where the session now is: back
        // on the list, with the new day scanned and already picked. Making
        // them find it again would be the one avoidable step in the whole flow.
        if let Some(tape) = downloaded {
            self.rescan();
            self.select_path(&tape);
            self.tab = BrowserTab::Sessions;
        }
        if load_clicked {
            self.load_selected();
        }
    }

    /// The two tabs. A download in flight is announced on its tab, so switching
    /// to the session list never loses sight of it.
    fn draw_tab_strip(&mut self, ui: &mut egui::Ui, market: &MarketMenu<'_>) {
        let mut entered_get_data = false;
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.tab == BrowserTab::Sessions, "My sessions")
                .clicked()
            {
                self.tab = BrowserTab::Sessions;
            }
            let label = if self.get_data.is_busy() {
                format!("Get data  {}", icons::ARROWS_CLOCKWISE)
            } else {
                "Get data".to_string()
            };
            if ui
                .selectable_label(self.tab == BrowserTab::GetData, label)
                .on_hover_text("Download a session from MetaTrader 5")
                .clicked()
            {
                entered_get_data = self.tab != BrowserTab::GetData;
                self.tab = BrowserTab::GetData;
            }
        });
        // Arriving on the tab *is* the question "what can I download?", so it
        // is asked here rather than left behind a button the trader has to
        // find. Only on arrival: staying on the tab must not re-ask.
        if entered_get_data {
            let folder = self.folder.clone();
            self.get_data.arrive(&folder, market.current);
        }
    }

    /// Select the session at `path`, if the scan found it.
    fn select_path(&mut self, path: &std::path::Path) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        self.selected = library.sessions.iter().position(|s| s.path == path);
    }

    fn draw_folder_row(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Replay folder")
                .color(TEXT_MUTED)
                .small(),
        );
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 110.0).max(120.0);
            let response = ui.add_sized(
                [width, 24.0],
                egui::TextEdit::singleline(&mut self.folder)
                    .hint_text("…/replay")
                    .desired_width(width),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.rescan();
            }
            if ui.button("Browse…").clicked() {
                self.browse();
            }
            if ui
                .button(icons::ARROW_CLOCKWISE)
                .on_hover_text("Scan this folder again")
                .clicked()
            {
                self.rescan();
            }
        });
        if self.picker.is_some() {
            ui.horizontal(|ui| {
                crate::loading::inline(ui, "waiting for the folder dialog");
            });
        }
    }

    /// Draw the session list. Returns whether one was double-clicked, which is
    /// the same request as picking it and pressing **Play session**.
    fn draw_session_list(&mut self, ui: &mut egui::Ui) -> bool {
        let mut open_requested = false;
        ui.label(egui::RichText::new("Sessions").color(TEXT_MUTED).small());
        let frame = egui::Frame::none()
            .fill(egui::Color32::from_black_alpha(60))
            .inner_margin(8.0)
            .rounding(4.0);

        frame.show(ui, |ui| {
            ui.set_min_height(150.0);
            let Some(library) = self.library.as_ref() else {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Choose the folder holding your recorded sessions.")
                            .color(TEXT_MUTED),
                    );
                });
                return;
            };
            if library.sessions.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No sessions in this folder.").color(WARN));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "quantick reads one CSV per session day, in a folder per instrument.",
                        )
                        .color(TEXT_MUTED)
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
                        let selected = self.selected == Some(index);
                        let response = ui.selectable_label(
                            selected,
                            egui::RichText::new(format!(
                                "{}      {}",
                                entry.label(),
                                entry.size_label()
                            )),
                        );
                        if response.clicked() {
                            self.selected = Some(index);
                        }
                        if response.double_clicked() {
                            self.selected = Some(index);
                            open_requested = true;
                        }
                        if !entry.notes.is_empty() {
                            for note in &entry.notes {
                                ui.label(
                                    egui::RichText::new(format!("    {note}"))
                                        .color(TEXT_MUTED)
                                        .small(),
                                );
                            }
                        }
                    }
                });
        });
        open_requested
    }

    fn draw_problems(&mut self, ui: &mut egui::Ui) {
        let Some(library) = self.library.as_ref() else {
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
        egui::CollapsingHeader::new(egui::RichText::new(header).color(WARN))
            .default_open(library.sessions.is_empty())
            .show(ui, |ui| {
                for problem in &library.problems {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(problem.subject())
                                .color(TEXT_PRIMARY)
                                .monospace(),
                        );
                        ui.label(egui::RichText::new(&problem.detail).color(TEXT_MUTED));
                    });
                    ui.label(
                        egui::RichText::new(format!("    {}", problem.advice()))
                            .color(TEXT_MUTED)
                            .small(),
                    );
                    ui.add_space(4.0);
                }
            });
    }

    fn draw_format_help(&mut self, ui: &mut egui::Ui) {
        let label = if self.show_format {
            "Hide the file format"
        } else {
            "Show the file format"
        };
        if ui
            .link(egui::RichText::new(label).color(TEXT_MUTED).small())
            .clicked()
        {
            self.show_format = !self.show_format;
        }
        if !self.show_format {
            return;
        }
        egui::Frame::none()
            .fill(egui::Color32::from_black_alpha(90))
            .inner_margin(8.0)
            .rounding(4.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .id_salt("replay_format_help")
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format::FORMAT_HELP)
                                .monospace()
                                .small()
                                .color(TEXT_MUTED),
                        );
                    });
            });
    }

    /// The bottom row: speed to start at, and the button that starts playback.
    /// Returns whether the load was asked for.
    fn draw_start_row(&mut self, ui: &mut egui::Ui) -> bool {
        if let Some(error) = &self.error {
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(90, 30, 25, 120))
                .inner_margin(8.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(error).color(WARN).small());
                });
            ui.add_space(6.0);
        }

        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Start at").color(TEXT_MUTED).small());
            for speed in SPEEDS {
                let selected = (self.speed - speed).abs() < f32::EPSILON;
                if speed_chip(ui, speed, selected).clicked() {
                    self.speed = speed;
                }
            }
            ui.add_space(8.0);
            ui.checkbox(&mut self.autoplay, "and play")
                .on_hover_text("Leave unticked to open the session paused on its first print");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(loading) = &self.loading {
                    crate::loading::inline(ui, &format!("loading {}", loading.label));
                    return;
                }
                let ready = self.selected_entry().is_some();
                let button = egui::Button::new(
                    egui::RichText::new("Play session").color(egui::Color32::BLACK),
                )
                .fill(REPLAY_ACCENT);
                if ui
                    .add_enabled(ready, button)
                    .on_disabled_hover_text("Pick a session from the list first")
                    .clicked()
                {
                    clicked = true;
                }
            });
        });
        clicked
    }

    /// The transport strip: one 30 px row directly above the status bar while
    /// a session is playing — part of the status system, not an island (§8).
    fn draw_transport(&mut self, ctx: &egui::Context, link: &ReplayLink) -> Option<ReplayAction> {
        let mut action = None;
        let status = &link.status;
        let timezone = link.session.timezone();

        egui::TopBottomPanel::bottom("replay_transport")
            .exact_height(TRANSPORT_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(CHROME)
                    .inner_margin(egui::Margin::symmetric(10.0, 3.0)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(TEXT_PRIMARY);
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
                        .button(egui::RichText::new(label).color(REPLAY_ACCENT))
                        .on_hover_text(hint)
                        .clicked()
                    {
                        action = Some(ReplayAction::Control(control));
                    }

                    badge(ui, "REPLAY");
                    ui.label(egui::RichText::new(link.label()).color(TEXT_PRIMARY));
                    ui.label(
                        egui::RichText::new(clock_text(status.position_ms(), timezone))
                            .monospace()
                            .color(REPLAY_ACCENT),
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
                        ui.label(
                            egui::RichText::new(format!(
                                "{} / {} prints",
                                thousands(status.played()),
                                thousands(status.total())
                            ))
                            .color(TEXT_MUTED)
                            .small(),
                        );
                        // The seek track takes every pixel left between the
                        // speed chips and the counts.
                        if let Some(fraction) = seek_track(ui, status.progress(), &mut self.scrub) {
                            action = Some(ReplayAction::Control(ReplayControl::SeekToFraction(
                                fraction,
                            )));
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

/// A small speed button, e.g. `10×`.
fn speed_chip(ui: &mut egui::Ui, speed: f32, selected: bool) -> egui::Response {
    let text = if speed.fract() == 0.0 {
        format!("{speed:.0}×")
    } else {
        format!("{speed}×")
    };
    let mut rich = egui::RichText::new(text).monospace().small();
    if selected {
        rich = rich.color(egui::Color32::BLACK).strong();
    }
    let mut button = egui::Button::new(rich).min_size(egui::vec2(30.0, 18.0));
    if selected {
        button = button.fill(REPLAY_ACCENT);
    }
    ui.add(button)
}

/// The amber `REPLAY` tag: the one loud thing on the bar.
fn badge(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(REPLAY_ACCENT)
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .rounding(3.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(egui::Color32::BLACK)
                    .small()
                    .strong(),
            );
        });
}

/// The seek track. Returns the fraction to jump to, once per gesture.
///
/// A held handle only moves `scrub`, which is what the track draws; the jump is
/// emitted when the handle is let go, or immediately on a click. That bound is
/// not cosmetic: every seek rebuilds the chart from the session's history, so
/// emitting one per dragged frame would re-ingest the whole recording at frame
/// rate — hundreds of megabytes a second on a full trading day.
fn seek_track(ui: &mut egui::Ui, progress: f32, scrub: &mut Option<f32>) -> Option<f32> {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, TRACK_HEIGHT + 8.0),
        egui::Sense::click_and_drag(),
    );
    let track = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - TRACK_HEIGHT / 2.0),
        egui::vec2(width, TRACK_HEIGHT),
    );

    // Keep the last position the pointer held: on the frame the button is
    // released egui may already report no interaction position, and that frame
    // is exactly the one carrying the seek.
    if let Some(pointer) = response.interact_pointer_pos() {
        *scrub = Some(((pointer.x - track.left()) / width.max(1.0)).clamp(0.0, 1.0));
    }
    let held = response.dragged() || response.is_pointer_button_down_on();
    let shown = match *scrub {
        Some(fraction) if held => fraction,
        _ => progress.clamp(0.0, 1.0),
    };

    let painter = ui.painter();
    painter.rect_filled(track, 3.0, CONTROL);
    let filled = egui::Rect::from_min_size(track.min, egui::vec2(width * shown, TRACK_HEIGHT));
    painter.rect_filled(filled, 3.0, REPLAY_ACCENT);
    painter.circle_filled(
        egui::pos2(filled.right(), track.center().y),
        if response.hovered() || held {
            HANDLE_RADIUS_HOVERED
        } else {
            HANDLE_RADIUS
        },
        REPLAY_ACCENT,
    );

    if response.drag_stopped() || response.clicked() {
        return scrub.take();
    }
    if !held {
        *scrub = None;
    }
    None
}

/// `HH:MM:SS` of a replay position, read in the session's own timezone — the
/// clock the trader watched that day, not the one on this machine.
fn clock_text(position_ms: i64, timezone: UtcOffset) -> String {
    let civil = format::civil_parts(position_ms, timezone);
    format!("{:02}:{:02}:{:02}", civil.hour, civil.minute, civil.second)
}

/// Group a count so 231190 reads as 231 190.
///
/// A plain space, not a narrow no-break one: egui's bundled fonts have no glyph
/// for U+202F and draw the missing-character box instead, which turns a count
/// into `231□190`.
fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_for_reading() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1_000), "1 000");
        assert_eq!(thousands(231_190), "231 190");
    }

    #[test]
    fn the_clock_reads_in_the_sessions_own_timezone() {
        // 2026-03-16 13:01:08 UTC is 10:01:08 on a B3 clock.
        let ms = 1_773_666_068_000;
        assert_eq!(clock_text(ms, UtcOffset::UTC), "13:01:08");
        assert_eq!(
            clock_text(ms, UtcOffset::parse("-03:00").unwrap()),
            "10:01:08"
        );
    }

    #[test]
    fn a_fresh_view_is_closed_and_starts_at_one_times() {
        let view = ReplayView::new(None);
        assert!(!view.browser_open);
        assert_eq!(view.speed, 1.0);
        assert!(view.autoplay);
        assert!(view.library.is_none());
        assert!(!view.is_loading(), "nothing is being parsed yet");
    }

    #[test]
    fn opening_the_browser_with_no_folder_scans_nothing() {
        let mut view = ReplayView::new(None);
        view.folder = String::new();
        view.open_browser();
        assert!(view.browser_open);
        assert!(view.library.is_none(), "no folder, nothing to scan");
    }

    /// The report this whole change answers: the trader points the browser at
    /// their recordings, and the app has something to write down instead of
    /// starting the next launch on nowhere.
    #[test]
    fn a_folder_put_to_use_is_offered_for_remembering_once() {
        let mut view = ReplayView::new(None);
        view.folder = "D:/tape".to_string();
        view.rescan();
        assert_eq!(view.take_folder_change().as_deref(), Some("D:/tape"));
        assert_eq!(
            view.take_folder_change(),
            None,
            "one pick is written once, not on every frame that follows it"
        );
        assert_eq!(view.remembered_folder(), "D:/tape");
    }

    /// Typing is not choosing. A half-typed path must never reach the file and
    /// replace the folder that was working.
    #[test]
    fn typing_in_the_field_alone_changes_nothing() {
        let mut view = ReplayView::new(Some("D:/tape"));
        view.folder = "D:/ta".to_string();
        assert_eq!(view.take_folder_change(), None);
        assert_eq!(view.remembered_folder(), "D:/tape");
    }

    /// A stored pick opens the browser where the trader left it, with no
    /// environment variable in sight.
    #[test]
    fn a_stored_pick_is_where_the_browser_opens() {
        let view = ReplayView::new(Some("D:/tape"));
        assert_eq!(view.folder, "D:/tape");
        assert_eq!(view.remembered_folder(), "D:/tape");
    }

    /// The chart's own contract comes first, the catalogue next, and what is
    /// already on disk after that — each named once.
    #[test]
    fn the_offered_contracts_lead_with_the_chart_and_never_repeat() {
        let mut view = ReplayView::new(None);
        view.library = Some(library::scan(std::path::Path::new("definitely/not/here")));
        let catalogue = ["WINQ26".to_owned(), "WINV26".to_owned()];
        let offered = view.offered_symbols(&MarketMenu {
            current: Some("WINV26"),
            catalogue: &catalogue,
        });
        assert_eq!(offered, vec!["WINV26".to_owned(), "WINQ26".to_owned()]);
    }

    #[test]
    fn scanning_a_missing_folder_reports_it_instead_of_failing() {
        let mut view = ReplayView::new(None);
        view.folder = "definitely/not/here".to_string();
        view.rescan();
        let library = view.library.as_ref().expect("scanned");
        assert!(library.sessions.is_empty());
        assert!(!library.problems.is_empty());
        assert!(view.selected.is_none());
    }
}
