//! The **Get data** tab: pick a symbol, pick days, download them.
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
//! find out how big it is. Several days download one at a time, oldest
//! first, past the ones already on disk; [`quantick_replay::download_selection`]
//! decides that, and nothing several days fetch is opened on the chart.
//!
//! # No timeframe control, on purpose
//!
//! The run-up is fetched once as minute candles and folded to 2m/5m/15m by
//! `quantick_sources::candles`. Asking the trader to pick a timeframe *here* would be
//! asking the same question twice — the chart already has that control — and
//! would let the four views disagree with one another.

use eframe::egui;
use egui_phosphor::regular as icons;

use quantick_replay::download_selection::{
    DaySelection, DayStatus, days_in_month, month_name, next_month, previous_month, prints,
    size_label, tape_bytes, weekday_monday_first,
};

use crate::theme;
use quantick_feed::replay_desk::{BROKER_CLOCKS, CONTEXT_CHOICES, DayBeforeStage, Phase};
pub use quantick_feed::replay_desk::{GetDataAction, GetDataPanel};

/// Draw the tab; [`GetDataPanel::pump`] moves the downloads on.
///
/// `offered` are the instruments worth one click — the chart's own first,
/// then the catalogue, then whatever the replay folder already holds.
pub fn draw(panel: &mut GetDataPanel, ui: &mut egui::Ui, out_dir: &str, offered: &[String]) {
    draw_source_row(ui);
    ui.add_space(8.0);
    draw_symbol_row(panel, ui, out_dir);
    draw_offered_row(panel, ui, out_dir, offered);
    ui.add_space(8.0);
    draw_calendar(panel, ui);
    ui.add_space(8.0);
    draw_context_row(panel, ui);
    draw_day_before_row(panel, ui);
    draw_clock_row(panel, ui);
    ui.add_space(10.0);
    draw_estimate_and_button(panel, ui, out_dir);
}

fn draw_source_row(ui: &mut egui::Ui) {
    let palette = theme::get(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Source")
                .color(palette.text_muted())
                .small(),
        );
        ui.label(egui::RichText::new("MetaTrader 5").color(palette.text_primary()));
        ui.label(
            egui::RichText::new("— the terminal you are logged in to")
                .color(palette.text_muted())
                .small(),
        );
    });
}

fn draw_symbol_row(panel: &mut GetDataPanel, ui: &mut egui::Ui, out_dir: &str) {
    let palette = theme::get(ui.ctx());
    ui.label(
        egui::RichText::new("Symbol")
            .color(palette.text_muted())
            .small(),
    );
    let mut look_up = false;
    let busy = panel.is_busy();
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 120.0).max(100.0);
        let response = ui.add_sized(
            [width, 24.0],
            egui::TextEdit::singleline(&mut panel.symbol)
                .hint_text("WINQ26")
                .interactive(!busy)
                .desired_width(width),
        );
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            look_up = true;
        }
        if ui
            .add_enabled(
                !busy && !panel.symbol.trim().is_empty(),
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
    // The same guard the button beside it carries. Without it, Enter in
    // this field starts a probe mid-chain — and `start` clears the tape
    // being held for the day before, stranding a day that is already on
    // disk and that the browser is then never told about.
    if look_up && !panel.is_busy() {
        panel.probe(out_dir);
    }
    if matches!(panel.phase, Phase::Probing) {
        crate::loading::inline(ui, "asking the terminal which days it holds");
    }
}

/// The instruments one click away.
///
/// Typing `WINV26` correctly is a thing a trader should never have to do
/// to answer "what can I replay?" — the app already knows which contract
/// is on the chart, which ones the catalogue carries, and which ones are
/// sitting in the replay folder. The rolling contract changes every couple
/// of months, so this row is where "the current one, or the one before it"
/// actually lives.
fn draw_offered_row(
    panel: &mut GetDataPanel,
    ui: &mut egui::Ui,
    out_dir: &str,
    offered: &[String],
) {
    let palette = theme::get(ui.ctx());
    if offered.is_empty() {
        return;
    }
    let busy = panel.is_busy();
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Contracts")
                .color(palette.text_muted())
                .small(),
        );
        for symbol in offered {
            let on = panel.symbol.trim() == symbol;
            let response = ui.add_enabled(
                !busy,
                egui::SelectableLabel::new(on, egui::RichText::new(symbol).monospace()),
            );
            if response
                .on_hover_text("Ask the terminal which days it holds for this contract")
                .clicked()
            {
                picked = Some(symbol.clone());
            }
        }
    });
    if let Some(symbol) = picked {
        panel.pick_symbol(&symbol, out_dir);
    }
}

/// The calendar. Days the provider does not hold are dead — visibly so, and
/// not clickable — because finding out after a failed download is the worst
/// possible time to learn a day was a holiday.
fn draw_calendar(panel: &mut GetDataPanel, ui: &mut egui::Ui) {
    let palette = theme::get(ui.ctx());
    ui.label(
        egui::RichText::new("Days")
            .color(palette.text_muted())
            .small(),
    );
    // A retyped symbol invalidates the calendar rather than showing another
    // instrument's days under the new name — the fastest way to download
    // the wrong contract.
    let stale = panel.symbol.trim() != panel.available_for;
    // Taken out and put back rather than cloned: this runs every frame,
    // and copying four months of day strings to draw one month of numbers
    // is an allocation per frame for nothing.
    let Some(available) = panel.available.take().filter(|_| !stale) else {
        panel.days.selection.clear();
        egui::Frame::none()
            .fill(palette.replay_well())
            .inner_margin(10.0)
            .rounding(4.0)
            .show(ui, |ui| {
                crate::theme::supporting_label(
                    ui,
                    "Type a symbol and look up its days — quantick asks the \
                     terminal what it holds before you pick.",
                );
            });
        return;
    };

    egui::Frame::none()
        .fill(palette.replay_well())
        .inner_margin(8.0)
        .rounding(4.0)
        .show(ui, |ui| {
            let (year, month) = panel.month;
            ui.horizontal(|ui| {
                if ui.small_button(icons::CARET_LEFT).clicked() {
                    panel.month = previous_month(year, month);
                }
                ui.label(
                    egui::RichText::new(format!("{} {year}", month_name(month)))
                        .color(palette.text_primary()),
                );
                if ui.small_button(icons::CARET_RIGHT).clicked() {
                    panel.month = next_month(year, month);
                }
            });
            ui.add_space(4.0);

            let (year, month) = panel.month;
            egui::Grid::new("replay_get_data_calendar")
                .num_columns(7)
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    for name in ["M", "T", "W", "T", "F", "S", "S"] {
                        ui.label(
                            egui::RichText::new(name)
                                .color(palette.text_muted())
                                .small(),
                        );
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
                        let on_disk = panel.days.held.contains(&iso);
                        draw_day_cell(
                            ui,
                            &mut panel.days.selection,
                            &available,
                            day,
                            &iso,
                            on_disk,
                        );
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
                        .color(palette.amber())
                        .small(),
                );
                ui.label(
                    egui::RichText::new(
                        "· greyed days have no data at this broker · Shift+click adds a range",
                    )
                    .color(palette.text_muted())
                    .small(),
                );
            });
        });
    panel.available = Some(available);
}

fn draw_context_row(panel: &mut GetDataPanel, ui: &mut egui::Ui) {
    let palette = theme::get(ui.ctx());
    ui.label(
        egui::RichText::new("Run-up before that day")
            .color(palette.text_muted())
            .small(),
    );
    let busy = panel.is_busy();
    ui.horizontal(|ui| {
        for choice in CONTEXT_CHOICES {
            let selected = panel.context_sessions == choice;
            if ui
                .add_enabled(
                    !busy,
                    egui::SelectableLabel::new(selected, format!("{choice}")),
                )
                .on_hover_text(format!(
                    "{choice} trading session(s) of minute candles before the tape"
                ))
                .clicked()
            {
                panel.context_sessions = choice;
            }
        }
        ui.label(
            egui::RichText::new("sessions, as minute candles")
                .color(palette.text_muted())
                .small(),
        );
    });
    ui.label(
        egui::RichText::new(
            "1m, 2m, 5m and 15m all come from these. Broker candles carry no \
             order flow, so the chart shows none before the tape starts.",
        )
        .color(if palette.resolved.dark {
            palette.text_muted()
        } else {
            palette.text_support()
        }),
    );
}

/// The *day before* tick: whether the session before the chosen one is
/// fetched as a full tape, not just as the run-up's minute candles.
///
/// It sits directly under the run-up because it answers the sentence the
/// run-up ends on — *broker candles carry no order flow* — and a trader
/// reading that has just been told what is missing. Returns the new value
/// when it was changed here; the browser owns it, and draws the same
/// setting beside **Play session**.
fn draw_day_before_row(panel: &mut GetDataPanel, ui: &mut egui::Ui) {
    let palette = theme::get(ui.ctx());
    ui.add_space(8.0);
    let mut wanted = panel.day_before;
    let one_day = panel.days.selection.len() < 2;
    let changed = ui
        .add_enabled(one_day, egui::Checkbox::new(&mut wanted, "The day before, as a full tape"))
        .on_hover_text(
            "Downloads the previous session's prints too, and replays them in front of the day you picked",
        )
        .changed();
    // The mirror follows the tick on the frame it was clicked. The browser
    // that owns the setting is told below and answers next frame; without
    // this the box would draw its old state for one frame, which is a tick
    // that visibly does not take.
    if changed {
        panel.day_before = wanted;
        // Resolved again on the spot: the caption under the box has to
        // answer for the tick that was just clicked, not for the one the
        // frame started with.
        panel.day_before_target = panel.resolve_day_before();
        panel.day_before_change = Some(wanted);
    }
    ui.label(
        egui::RichText::new(panel.day_before_note()).color(if palette.resolved.dark {
            palette.text_muted()
        } else {
            palette.text_support()
        }),
    );
}

/// The broker-clock control, shown only once the provider has said it
/// cannot measure one.
///
/// Measuring needs a fresh tick, which needs an open market — and a replay
/// is downloaded after the close. Rather than send the trader to a command
/// line for a flag, the question is asked here, in the one place they are
/// already standing, and the answer is labelled as *stated* rather than
/// measured: the timestamps in the file are only as right as this is.
fn draw_clock_row(panel: &mut GetDataPanel, ui: &mut egui::Ui) {
    let palette = theme::get(ui.ctx());
    if !panel.clock_needed {
        return;
    }
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("Broker clock")
            .color(palette.text_muted())
            .small(),
    );
    ui.horizontal(|ui| {
        for (offset, label) in BROKER_CLOCKS {
            if ui
                .selectable_label(panel.declared_clock == Some(offset), label)
                .clicked()
            {
                panel.declared_clock = Some(offset);
            }
        }
    });
    ui.label(
        egui::RichText::new(
            "quantick could not measure it — the market is closed. This is what \
             the times in the file will be written in, so it is stated, not measured. \
             B3 brokers run at -03:00.",
        )
        .color(if palette.resolved.dark {
            palette.text_muted()
        } else {
            palette.text_support()
        }),
    );
}

fn draw_estimate_and_button(panel: &mut GetDataPanel, ui: &mut egui::Ui, out_dir: &str) {
    let palette = theme::get(ui.ctx());
    match &panel.phase {
        Phase::Failed { headline, fix } => {
            egui::Frame::none()
                .fill(palette.replay_error())
                .inner_margin(8.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(headline).color(palette.warn()));
                    if let Some(fix) = fix {
                        crate::theme::supporting_label(ui, fix);
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
                    prints(*ticks)
                ))
                .color(palette.amber()),
            );
            if !complete {
                ui.label(
                    egui::RichText::new(
                        "The broker held fewer sessions than asked for; the run-up is short.",
                    )
                    .color(palette.text_muted())
                    .small(),
                );
            }
            ui.add_space(6.0);
        }
        _ => {}
    }

    // Said beside the chosen day's result, never in place of it: the day
    // before is an addition, and its absence is not this download failing.
    match &panel.day_before_stage {
        Some(DayBeforeStage::Running { day, ticks }) => {
            // Its own row, with its own Cancel, and then out — exactly as
            // the first half does. Falling through to the button would
            // draw it disabled with a reason that is not the reason
            // ("pick a day from the calendar", with a day picked), and
            // would leave 70 MB downloading with nothing to stop it.
            ui.horizontal(|ui| {
                crate::loading::inline(
                    ui,
                    &format!("{day} — the day before, {} prints so far", prints(*ticks)),
                );
                if ui
                    .button("Cancel")
                    .on_hover_text("The day you picked is already downloaded and stays")
                    .clicked()
                    && let Some(job) = panel.job.as_ref()
                {
                    job.cancel();
                }
            });
            return;
        }
        Some(DayBeforeStage::Done { day, ticks }) => {
            ui.label(
                egui::RichText::new(format!(
                    "{} {day} came too, {} prints — it replays in front of the day you picked",
                    icons::CHECK,
                    prints(*ticks)
                ))
                .color(palette.amber())
                .small(),
            );
            ui.add_space(6.0);
        }
        Some(DayBeforeStage::Failed { day, headline }) => {
            ui.label(
                egui::RichText::new(format!("The day before ({day}) did not come: {headline}"))
                    .color(palette.warn())
                    .small(),
            );
            ui.label(
                egui::RichText::new("The day you picked is downloaded and will replay on its own.")
                    .color(palette.text_muted())
                    .small(),
            );
            ui.add_space(6.0);
        }
        None => {}
    }

    if draw_batch(panel, ui) {
        return;
    }
    if let Phase::Running { ticks } = panel.phase {
        ui.horizontal(|ui| {
            crate::loading::inline(
                ui,
                &format!("downloading — {} prints so far", prints(ticks)),
            );
            if ui.button("Cancel").clicked()
                && let Some(job) = panel.job.as_ref()
            {
                job.cancel();
            }
        });
        return;
    }

    let ready = !panel.days.selection.is_empty() && !panel.symbol.trim().is_empty();
    ui.horizontal(|ui| {
        if panel.days.selection.len() > 1 {
            let estimate = panel.days.estimate(panel.context_sessions);
            ui.label(
                egui::RichText::new(estimate)
                    .color(palette.text_muted())
                    .small(),
            );
        } else if let Some(day) = panel.days.selection.only() {
            // Two tapes cost twice one tape, and nobody may find that out
            // from a progress bar that keeps going after they expected it
            // to stop. Both days are named, because "2 tapes from the
            // 28th" is not what is about to be downloaded.
            let (days, tapes) = match panel.day_before_to_fetch() {
                Some(earlier) => (format!("{day} + {earlier}"), 2),
                None => (day.to_owned(), 1),
            };
            ui.label(
                egui::RichText::new(format!(
                    "Tape {days} · about {} · run-up {} session(s) · under 1 MB",
                    size_label(tape_bytes(tapes)),
                    panel.context_sessions
                ))
                .color(palette.text_muted())
                .small(),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = match panel.days.selection.len() {
                0 | 1 => "Download".to_string(),
                days => format!("Download {days} days"),
            };
            let button =
                egui::Button::new(egui::RichText::new(text).color(palette.replay_plate_ink()))
                    .fill(palette.replay_plate());
            if ui
                .add_enabled(
                    ready && !panel.days.nothing_to_fetch() && !panel.is_busy(),
                    button,
                )
                .on_disabled_hover_text(if panel.symbol.trim().is_empty() {
                    "Type a symbol first"
                } else if panel.days.nothing_to_fetch() {
                    "Every picked day is already here"
                } else if panel.available.is_none() {
                    "Look up the symbol's days first"
                } else {
                    "Pick days from the calendar"
                })
                .clicked()
            {
                panel.download(out_dir);
            }
        });
    });
}

/// A several-day download's rows, with Cancel while it runs and the
/// summary once it ends. True while it runs, so the button stays away.
fn draw_batch(panel: &mut GetDataPanel, ui: &mut egui::Ui) -> bool {
    let palette = theme::get(ui.ctx());
    let Some(batch) = panel.days.batch() else {
        return false;
    };
    let asked = batch.template().context_sessions;
    egui::ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            for row in batch.rows() {
                let colour = match row.status {
                    DayStatus::Running { .. } => {
                        crate::loading::inline(ui, &row.line(asked));
                        continue;
                    }
                    DayStatus::Refused { .. } => palette.warn(),
                    DayStatus::Fetched { .. } | DayStatus::AlreadyHere { .. } => palette.amber(),
                    DayStatus::Waiting | DayStatus::NotTried | DayStatus::Cancelled => {
                        palette.text_muted()
                    }
                };
                ui.label(egui::RichText::new(row.line(asked)).color(colour).small());
            }
        });
    let running = batch.running().is_some();
    if !running {
        ui.label(egui::RichText::new(batch.summary().line()).color(palette.amber()));
        ui.add_space(6.0);
    } else if ui
        .button("Cancel")
        .on_hover_text("Days already downloaded stay")
        .clicked()
    {
        panel.days.cancel();
        if let Some(job) = panel.job.as_ref() {
            job.cancel();
        }
    }
    running
}

/// One day in the calendar grid.
///
/// A free function taking the selection by reference rather than a method:
/// the grid runs inside a closure that already borrows the panel's day set,
/// and this is what lets the set be borrowed instead of copied every frame.
fn draw_day_cell(
    ui: &mut egui::Ui,
    selection: &mut DaySelection,
    offered: &std::collections::BTreeSet<String>,
    day: u32,
    iso: &str,
    on_disk: bool,
) {
    let palette = theme::get(ui.ctx());
    if !offered.contains(iso) {
        // Not a disabled button: there is nothing here to press, and a greyed
        // number reads as "no data" faster than a dead control does.
        ui.label(
            egui::RichText::new(format!("{day:2}"))
                .color(palette.text_muted())
                .weak(),
        );
        return;
    }
    let selected = selection.contains(iso);
    let colour = if on_disk {
        palette.amber()
    } else {
        palette.text_primary()
    };
    let response = ui
        .selectable_label(
            selected,
            egui::RichText::new(format!("{day:2}")).color(colour),
        )
        .on_hover_text(if on_disk {
            format!("{iso} — already on disk; downloading it alone replaces it")
        } else {
            iso.to_string()
        });
    if response.clicked() {
        selection.click(iso, offered, ui.input(|i| i.modifiers.shift));
    }
}
