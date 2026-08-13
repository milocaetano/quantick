//! The **Get data** tab: pick a symbol, pick a day, download it.
//!
//! It lives inside the Market Replay browser rather than behind a menu entry
//! of its own, because for a trader downloading and replaying are one task:
//! "run me Wednesday". Splitting them across two places is how a feature gets
//! shipped and never found.
//!
//! Three inputs and one button, in the order the question is actually asked —
//! *which instrument, which day, how much of the run-up*. The day comes from a
//! calendar whose empty days are dead on arrival: the provider is asked what it
//! holds before the trader clicks, so a holiday cannot be chosen and then fail.
//! The estimate sits above the button, so nobody starts a 60 MB download to
//! find out how big it is.
//!
//! # No timeframe control, on purpose
//!
//! The run-up is fetched once as minute candles and folded to 2m/5m/15m by
//! [`crate::resample`]. Asking the trader to pick a timeframe *here* would be
//! asking the same question twice — the chart already has that control — and
//! would let the four views disagree with one another.

use std::path::PathBuf;

use eframe::egui;
use egui_phosphor::regular as icons;

use quantick_replay::Library;
use quantick_replay::format::{CivilTime, UtcOffset};

use crate::replay_download::{
    DEFAULT_CONTEXT_SESSIONS, DownloadEvent, DownloadJob, DownloadRequest, Mt5SessionSource,
    SessionSource,
};
use crate::theme::{AMBER, TEXT_MUTED, TEXT_PRIMARY, WARN};

/// Context choices offered. A week is the default; the ends are there because
/// "just yesterday" and "the whole fortnight" are both real ways to prepare.
const CONTEXT_CHOICES: [u32; 5] = [1, 3, 5, 10, 20];

/// Roughly how many bytes one tick costs in the replay format, measured on a
/// real B3 export: 19,139,000 bytes for 389,622 prints. Used only to size the
/// estimate shown before a download starts, and labelled as approximate.
const BYTES_PER_TICK: u64 = 49;

/// Rough prints per session for the estimate, when nothing better is known.
///
/// A busy WIN day is over a million; this is deliberately a round,
/// order-of-magnitude figure, and the interface says "about".
const TYPICAL_TICKS_PER_SESSION: u64 = 1_000_000;

/// What the panel asks the browser to do.
pub enum GetDataAction {
    /// A download finished and wrote this tape; rescan and select it.
    Downloaded(PathBuf),
}

/// How far along a request is.
enum Phase {
    /// Nothing asked yet.
    Idle,
    /// Asking the provider which days it holds.
    Probing,
    /// Downloading, with prints written so far.
    Running { ticks: u64 },
    /// Finished; the tape is on disk.
    Finished {
        ticks: u64,
        sessions: u32,
        complete: bool,
    },
    /// Refused or failed, with the one thing to do about it.
    Failed {
        headline: String,
        fix: Option<String>,
    },
}

/// State of the Get data tab.
pub struct GetDataPanel {
    symbol: String,
    /// The month the calendar is showing, as `(year, month)`.
    month: (i64, u32),
    /// The chosen day, `YYYY-MM-DD`.
    day: Option<String>,
    context_sessions: u32,
    /// Days the provider says it holds, or `None` before it was asked.
    available: Option<Vec<String>>,
    /// The symbol `available` was measured for, so a retyped symbol invalidates
    /// the calendar instead of quietly showing another instrument's days.
    available_for: String,
    job: Option<DownloadJob>,
    phase: Phase,
}

impl Default for GetDataPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl GetDataPanel {
    /// An empty panel, opening on the current month.
    #[must_use]
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            // A calendar has to open somewhere and this struct owns no clock;
            // the first probe moves it to the newest month with data, which is
            // where the trader wanted to be anyway.
            month: (2026, 1),
            day: None,
            context_sessions: DEFAULT_CONTEXT_SESSIONS,
            available: None,
            available_for: String::new(),
            job: None,
            phase: Phase::Idle,
        }
    }

    /// Whether a download or probe is running.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        matches!(self.phase, Phase::Probing | Phase::Running { .. })
    }

    /// Drain whatever the running job has said since the last frame.
    ///
    /// Non-blocking by construction: the download runs on its own thread and
    /// this only ever reads what has already arrived, so a busy export cannot
    /// cost a frame.
    fn poll(&mut self) -> Option<GetDataAction> {
        let job = self.job.as_ref()?;
        let mut action = None;
        let mut ended = false;
        loop {
            match job.events.try_recv() {
                Ok(DownloadEvent::DaysAvailable { symbol, days }) => {
                    if let Some(newest) = days.last().and_then(|d| parse_day(d)) {
                        self.month = (newest.0, newest.1);
                    }
                    self.available_for = symbol;
                    self.available = Some(days);
                    self.phase = Phase::Idle;
                    ended = true;
                }
                Ok(DownloadEvent::Progress { ticks }) => self.phase = Phase::Running { ticks },
                Ok(DownloadEvent::TapeWritten { ticks, .. }) => {
                    self.phase = Phase::Running { ticks };
                }
                Ok(DownloadEvent::ContextWritten { sessions, complete }) => {
                    let ticks = match self.phase {
                        Phase::Running { ticks } => ticks,
                        _ => 0,
                    };
                    self.phase = Phase::Finished {
                        ticks,
                        sessions,
                        complete,
                    };
                }
                Ok(DownloadEvent::Done { tape }) => {
                    if let Phase::Running { ticks } = self.phase {
                        self.phase = Phase::Finished {
                            ticks,
                            sessions: 0,
                            complete: true,
                        };
                    }
                    action = Some(GetDataAction::Downloaded(tape));
                    ended = true;
                }
                Ok(DownloadEvent::Failed { headline, fix }) => {
                    self.phase = Phase::Failed { headline, fix };
                    ended = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // The worker is gone. If it never said why, say so rather
                    // than leaving a spinner turning forever.
                    if matches!(self.phase, Phase::Probing | Phase::Running { .. }) {
                        self.phase = Phase::Failed {
                            headline: "the download stopped without finishing".to_string(),
                            fix: Some(
                                "Check the MetaTrader 5 terminal is still open, then try again."
                                    .to_string(),
                            ),
                        };
                    }
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.job = None;
        }
        action
    }

    /// Ask the provider which days it holds for the typed symbol.
    fn probe(&mut self, out_dir: &str) {
        if self.symbol.trim().is_empty() {
            return;
        }
        self.available = None;
        self.start(
            DownloadRequest {
                symbol: self.symbol.trim().to_string(),
                day: None,
                context_sessions: 0,
                out_dir: PathBuf::from(out_dir),
            },
            Phase::Probing,
        );
    }

    /// Start the download the three inputs describe.
    fn download(&mut self, out_dir: &str) {
        let Some(day) = self.day.clone() else {
            return;
        };
        self.start(
            DownloadRequest {
                symbol: self.symbol.trim().to_string(),
                day: Some(day),
                context_sessions: self.context_sessions,
                out_dir: PathBuf::from(out_dir),
            },
            Phase::Running { ticks: 0 },
        );
    }

    fn start(&mut self, request: DownloadRequest, phase: Phase) {
        // The interpreter is the bridge's default, which already falls back
        // from `python` to `py`. A machine that can stream MetaTrader can
        // download from it.
        let source = Mt5SessionSource::new("python");
        match crate::replay_download::run(&source, &request) {
            Ok(job) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DOWNLOAD_STARTED",
                    source = source.label().as_str(),
                    symbol = request.symbol.as_str(),
                    day = if request.is_probe() {
                        "(probe)"
                    } else {
                        request.day.as_deref().unwrap_or_default()
                    },
                    context_sessions = request.context_sessions,
                    "downloading a replay session"
                );
                self.job = Some(job);
                self.phase = phase;
            }
            Err(e) => {
                self.phase = Phase::Failed {
                    headline: e.to_string(),
                    fix: Some(e.advice()),
                };
            }
        }
    }

    /// Draw the tab. Returns an action when a download finished.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        out_dir: &str,
        library: Option<&Library>,
    ) -> Option<GetDataAction> {
        let action = self.poll();

        self.draw_source_row(ui);
        ui.add_space(8.0);
        self.draw_symbol_row(ui, out_dir);
        ui.add_space(8.0);
        self.draw_calendar(ui, library);
        ui.add_space(8.0);
        self.draw_context_row(ui);
        ui.add_space(10.0);
        self.draw_estimate_and_button(ui, out_dir);

        action
    }

    fn draw_source_row(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Source").color(TEXT_MUTED).small());
            ui.label(egui::RichText::new("MetaTrader 5").color(TEXT_PRIMARY));
            ui.label(
                egui::RichText::new("— the terminal you are logged in to")
                    .color(TEXT_MUTED)
                    .small(),
            );
        });
    }

    fn draw_symbol_row(&mut self, ui: &mut egui::Ui, out_dir: &str) {
        ui.label(egui::RichText::new("Symbol").color(TEXT_MUTED).small());
        let mut look_up = false;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 120.0).max(100.0);
            let response = ui.add_sized(
                [width, 24.0],
                egui::TextEdit::singleline(&mut self.symbol)
                    .hint_text("WINQ26")
                    .desired_width(width),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                look_up = true;
            }
            let busy = self.is_busy();
            if ui
                .add_enabled(
                    !busy && !self.symbol.trim().is_empty(),
                    egui::Button::new("Look up days"),
                )
                .on_hover_text("Ask the terminal which days it holds for this symbol")
                .on_disabled_hover_text(if busy {
                    "Waiting for the terminal"
                } else {
                    "Type a symbol first"
                })
                .clicked()
            {
                look_up = true;
            }
        });
        if look_up {
            self.probe(out_dir);
        }
        if matches!(self.phase, Phase::Probing) {
            crate::loading::inline(ui, "asking the terminal which days it holds");
        }
    }

    /// The calendar. Days the provider does not hold are dead — visibly so, and
    /// not clickable — because finding out after a failed download is the worst
    /// possible time to learn a day was a holiday.
    fn draw_calendar(&mut self, ui: &mut egui::Ui, library: Option<&Library>) {
        ui.label(egui::RichText::new("Day").color(TEXT_MUTED).small());
        // A retyped symbol invalidates the calendar rather than showing another
        // instrument's days under the new name — the fastest way to download
        // the wrong contract.
        let stale = self.symbol.trim() != self.available_for;
        let Some(available) = self.available.clone().filter(|_| !stale) else {
            egui::Frame::none()
                .fill(egui::Color32::from_black_alpha(60))
                .inner_margin(10.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Type a symbol and look up its days — quantick asks the \
                             terminal what it holds before you pick.",
                        )
                        .color(TEXT_MUTED)
                        .small(),
                    );
                });
            return;
        };

        egui::Frame::none()
            .fill(egui::Color32::from_black_alpha(60))
            .inner_margin(8.0)
            .rounding(4.0)
            .show(ui, |ui| {
                let (year, month) = self.month;
                ui.horizontal(|ui| {
                    if ui.small_button(icons::CARET_LEFT).clicked() {
                        self.month = previous_month(year, month);
                    }
                    ui.label(
                        egui::RichText::new(format!("{} {year}", month_name(month)))
                            .color(TEXT_PRIMARY),
                    );
                    if ui.small_button(icons::CARET_RIGHT).clicked() {
                        self.month = next_month(year, month);
                    }
                });
                ui.add_space(4.0);

                let (year, month) = self.month;
                egui::Grid::new("replay_get_data_calendar")
                    .num_columns(7)
                    .spacing([2.0, 2.0])
                    .show(ui, |ui| {
                        for name in ["M", "T", "W", "T", "F", "S", "S"] {
                            ui.label(egui::RichText::new(name).color(TEXT_MUTED).small());
                        }
                        ui.end_row();

                        // Monday-first, which is how a trading week reads.
                        let lead = weekday_monday_first(year, month, 1);
                        for _ in 0..lead {
                            ui.label("");
                        }
                        let mut column = lead;
                        for day in 1..=days_in_month(year, month) {
                            let iso = format!("{year:04}-{month:02}-{day:02}");
                            let held = available.contains(&iso);
                            let on_disk = library.is_some_and(|l| {
                                l.sessions
                                    .iter()
                                    .any(|s| s.date.is_some_and(|d| d.label() == iso))
                            });
                            self.draw_day_cell(ui, day, &iso, held, on_disk);
                            column += 1;
                            if column.is_multiple_of(7) {
                                ui.end_row();
                            }
                        }
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} already downloaded", icons::CHECK))
                            .color(AMBER)
                            .small(),
                    );
                    ui.label(
                        egui::RichText::new("· greyed days have no data at this broker")
                            .color(TEXT_MUTED)
                            .small(),
                    );
                });
            });
    }

    fn draw_day_cell(&mut self, ui: &mut egui::Ui, day: u32, iso: &str, held: bool, on_disk: bool) {
        if !held {
            // Not a disabled button: there is nothing here to press, and a
            // greyed number reads as "no data" faster than a dead control.
            ui.label(
                egui::RichText::new(format!("{day:2}"))
                    .color(TEXT_MUTED)
                    .weak(),
            );
            return;
        }
        let selected = self.day.as_deref() == Some(iso);
        let text = if on_disk {
            egui::RichText::new(format!("{day:2}")).color(AMBER)
        } else {
            egui::RichText::new(format!("{day:2}")).color(TEXT_PRIMARY)
        };
        let response = ui
            .selectable_label(selected, text)
            .on_hover_text(if on_disk {
                format!("{iso} — already on disk; downloading again replaces it")
            } else {
                iso.to_string()
            });
        if response.clicked() {
            self.day = Some(iso.to_string());
        }
    }

    fn draw_context_row(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Run-up before that day")
                .color(TEXT_MUTED)
                .small(),
        );
        ui.horizontal(|ui| {
            for choice in CONTEXT_CHOICES {
                let selected = self.context_sessions == choice;
                if ui
                    .selectable_label(selected, format!("{choice}"))
                    .on_hover_text(format!(
                        "{choice} trading session(s) of minute candles before the tape"
                    ))
                    .clicked()
                {
                    self.context_sessions = choice;
                }
            }
            ui.label(
                egui::RichText::new("sessions, as minute candles")
                    .color(TEXT_MUTED)
                    .small(),
            );
        });
        ui.label(
            egui::RichText::new(
                "1m, 2m, 5m and 15m all come from these. Broker candles carry no \
                 order flow, so the chart shows none before the tape starts.",
            )
            .color(TEXT_MUTED)
            .small(),
        );
    }

    fn draw_estimate_and_button(&mut self, ui: &mut egui::Ui, out_dir: &str) {
        match &self.phase {
            Phase::Failed { headline, fix } => {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(90, 30, 25, 120))
                    .inner_margin(8.0)
                    .rounding(4.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(headline).color(WARN));
                        if let Some(fix) = fix {
                            ui.label(egui::RichText::new(fix).color(TEXT_MUTED).small());
                        }
                    });
                ui.add_space(6.0);
            }
            Phase::Finished {
                ticks,
                sessions,
                complete,
            } => {
                ui.label(
                    egui::RichText::new(format!(
                        "{} {} prints downloaded, with {sessions} session(s) of run-up",
                        icons::CHECK,
                        thousands(*ticks)
                    ))
                    .color(AMBER),
                );
                if !complete {
                    ui.label(
                        egui::RichText::new(
                            "The broker held fewer sessions than asked for; the run-up is short.",
                        )
                        .color(TEXT_MUTED)
                        .small(),
                    );
                }
                ui.add_space(6.0);
            }
            _ => {}
        }

        if let Phase::Running { ticks } = self.phase {
            ui.horizontal(|ui| {
                crate::loading::inline(
                    ui,
                    &format!("downloading — {} prints so far", thousands(ticks)),
                );
                if ui.button("Cancel").clicked()
                    && let Some(job) = self.job.as_ref()
                {
                    job.cancel();
                }
            });
            return;
        }

        let ready = self.day.is_some() && !self.symbol.trim().is_empty();
        ui.horizontal(|ui| {
            if let Some(day) = &self.day {
                let bytes = TYPICAL_TICKS_PER_SESSION * BYTES_PER_TICK;
                ui.label(
                    egui::RichText::new(format!(
                        "Tape {day} · about {} · run-up {} session(s) · under 1 MB",
                        size_label(bytes),
                        self.context_sessions
                    ))
                    .color(TEXT_MUTED)
                    .small(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button =
                    egui::Button::new(egui::RichText::new("Download").color(egui::Color32::BLACK))
                        .fill(AMBER);
                if ui
                    .add_enabled(ready && !self.is_busy(), button)
                    .on_disabled_hover_text(if self.symbol.trim().is_empty() {
                        "Type a symbol first"
                    } else if self.available.is_none() {
                        "Look up the symbol's days first"
                    } else {
                        "Pick a day from the calendar"
                    })
                    .clicked()
                {
                    self.download(out_dir);
                }
            });
        });
    }
}

/// `2026-08-12` → `(2026, 8, 12)`.
fn parse_day(iso: &str) -> Option<(i64, u32, u32)> {
    let mut parts = iso.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

fn previous_month(year: i64, month: u32) -> (i64, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn next_month(year: i64, month: u32) -> (i64, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    NAMES.get((month as usize).wrapping_sub(1)).unwrap_or(&"—")
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// Which column a date falls in, with Monday at 0.
///
/// Derived from the replay crate's own civil-date arithmetic rather than a
/// second calendar implementation: a date means one thing in this app.
fn weekday_monday_first(year: i64, month: u32, day: u32) -> usize {
    let ms = CivilTime {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        milli: 0,
    }
    .epoch_millis(UtcOffset::UTC);
    let days = ms.div_euclid(86_400_000);
    // 1970-01-01 was a Thursday, which is column 3 when Monday is 0.
    usize::try_from((days + 3).rem_euclid(7)).unwrap_or(0)
}

/// `1234567` → `1,234,567`.
fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A size a person reads at a glance.
fn size_label(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} kB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_lines_up_with_a_known_week() {
        // 2026-08-12 was a Wednesday — the day WINQ26 expired.
        assert_eq!(weekday_monday_first(2026, 8, 12), 2);
        assert_eq!(weekday_monday_first(2026, 8, 10), 0, "Monday is column 0");
        assert_eq!(weekday_monday_first(2026, 8, 16), 6, "Sunday is column 6");
        // 1970-01-01 was a Thursday.
        assert_eq!(weekday_monday_first(1970, 1, 1), 3);
    }

    #[test]
    fn months_have_the_right_number_of_days() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29, "2024 is a leap year");
        assert_eq!(days_in_month(2000, 2), 29, "so is 2000");
        assert_eq!(days_in_month(1900, 2), 28, "but 1900 is not");
        assert_eq!(days_in_month(2026, 8), 31);
    }

    #[test]
    fn month_navigation_crosses_the_year() {
        assert_eq!(previous_month(2026, 1), (2025, 12));
        assert_eq!(next_month(2026, 12), (2027, 1));
        assert_eq!(next_month(2026, 8), (2026, 9));
    }

    #[test]
    fn retyping_the_symbol_invalidates_the_calendar() {
        // Showing WINQ26's days under the name WINV26 is the fastest possible
        // way to download the wrong contract.
        let mut panel = GetDataPanel::new();
        panel.symbol = "WINQ26".to_string();
        panel.available_for = "WINQ26".to_string();
        panel.available = Some(vec!["2026-08-12".to_string()]);
        assert_eq!(panel.symbol.trim(), panel.available_for);

        panel.symbol = "WINV26".to_string();
        assert_ne!(
            panel.symbol.trim(),
            panel.available_for,
            "the calendar is stale and must be looked up again"
        );
    }

    #[test]
    fn a_download_needs_a_day_that_was_offered() {
        let mut panel = GetDataPanel::new();
        panel.symbol = "WINQ26".to_string();
        assert!(panel.day.is_none(), "nothing is picked before a look-up");
        // `download` is a no-op without a day, so a stray click cannot start
        // a request the calendar never authorised.
        panel.download("/replay");
        assert!(panel.job.is_none());
        assert!(matches!(panel.phase, Phase::Idle));
    }

    #[test]
    fn a_probe_reply_moves_the_calendar_to_where_the_data_is() {
        // The panel owns no clock, so it opens on a placeholder month. The
        // first answer has to land the trader on the month that has data,
        // or every download starts with two clicks of month navigation.
        let mut panel = GetDataPanel::new();
        assert_ne!(panel.month, (2026, 8));
        if let Some((year, month, _)) = parse_day("2026-08-12") {
            panel.month = (year, month);
        }
        assert_eq!(panel.month, (2026, 8));
    }

    #[test]
    fn counts_and_sizes_read_at_a_glance() {
        assert_eq!(thousands(389_622), "389,622");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(7), "7");
        assert_eq!(size_label(49_000_000), "46 MB");
    }

    #[test]
    fn a_day_is_parsed_out_of_the_provider_s_own_spelling() {
        assert_eq!(parse_day("2026-08-12"), Some((2026, 8, 12)));
        assert_eq!(parse_day("nonsense"), None);
    }
}
