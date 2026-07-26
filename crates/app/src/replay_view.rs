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
use crate::theme::{AMBER, CHROME, CONTROL, TEXT_MUTED, TEXT_PRIMARY, WARN};

/// The accent this feature owns: the same amber the chart already uses for the
/// backfill/live divider, so "this is not live data" reads the same way twice.
/// Borrowed rather than repeated, so the two can never drift apart.
const REPLAY_ACCENT: egui::Color32 = AMBER;

/// Height of the transport strip, in pixels — part of the status system, one
/// line directly above the status bar (`docs/ux/ui-design-model.md` §8).
pub const TRANSPORT_HEIGHT: f32 = 30.0;

/// Environment variable naming the folder the browser opens on.
pub const REPLAY_DIR_ENV: &str = "QUANTICK_REPLAY_DIR";

/// Height of the seek track, in pixels.
const TRACK_HEIGHT: f32 = 6.0;

/// Radius of the seek handle, resting and hovered.
const HANDLE_RADIUS: f32 = 4.5;
/// See [`HANDLE_RADIUS`].
const HANDLE_RADIUS_HOVERED: f32 = 6.0;

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
}

impl Default for ReplayView {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayView {
    /// A closed browser, opening on `QUANTICK_REPLAY_DIR` when it is set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            browser_open: false,
            folder: std::env::var(REPLAY_DIR_ENV).unwrap_or_default(),
            library: None,
            selected: None,
            loading: None,
            picker: None,
            error: None,
            show_format: false,
            speed: 1.0,
            autoplay: true,
            scrub: None,
        }
    }

    /// Open the browser, rescanning the folder it already knows.
    pub fn open_browser(&mut self) {
        self.browser_open = true;
        if self.library.is_none() && !self.folder.trim().is_empty() {
            self.rescan();
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
    pub fn draw(&mut self, ctx: &egui::Context, link: Option<&ReplayLink>) -> Option<ReplayAction> {
        self.poll_picker();
        let mut action = self.poll_loading();
        // The browser starts a load rather than returning an action: the
        // session is parsed on a worker, and `poll_loading` turns the result
        // into the `Open` a later frame carries.
        self.draw_browser(ctx);
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
    fn rescan(&mut self) {
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

    /// Draw the session browser, starting a load if one was asked for.
    fn draw_browser(&mut self, ctx: &egui::Context) {
        if !self.browser_open {
            return;
        }
        let mut open = true;
        let mut load_clicked = false;

        egui::Window::new("Market Replay")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(TEXT_PRIMARY);
                self.draw_folder_row(ui);
                ui.add_space(10.0);
                load_clicked = self.draw_session_list(ui);
                ui.add_space(8.0);
                self.draw_problems(ui);
                self.draw_format_help(ui);
                ui.add_space(6.0);
                load_clicked |= self.draw_start_row(ui);
            });

        self.browser_open = open;
        if load_clicked {
            self.load_selected();
        }
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
        let view = ReplayView::new();
        assert!(!view.browser_open);
        assert_eq!(view.speed, 1.0);
        assert!(view.autoplay);
        assert!(view.library.is_none());
        assert!(!view.is_loading(), "nothing is being parsed yet");
    }

    #[test]
    fn opening_the_browser_with_no_folder_scans_nothing() {
        let mut view = ReplayView::new();
        view.folder = String::new();
        view.open_browser();
        assert!(view.browser_open);
        assert!(view.library.is_none(), "no folder, nothing to scan");
    }

    #[test]
    fn scanning_a_missing_folder_reports_it_instead_of_failing() {
        let mut view = ReplayView::new();
        view.folder = "definitely/not/here".to_string();
        view.rescan();
        let library = view.library.as_ref().expect("scanned");
        assert!(library.sessions.is_empty());
        assert!(!library.problems.is_empty());
        assert!(view.selected.is_none());
    }
}
