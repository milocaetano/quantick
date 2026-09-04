//! The REC control and its readouts — the toolbar button with its popover,
//! the chart-corner chip and the recorded-days list.
//!
//! One module because they say one thing in three places, and every word
//! comes from [`RecordingView`] so the three cannot disagree. The button
//! sits beside the symbol, not beside the bar kind: recording is a fact
//! about the asset, and switching the pane to `tick` must leave it exactly
//! where it was, saying exactly what it said.

use eframe::egui;

use crate::deal_recording::{DealRecordingAction, RecState, RecordingView, fmt_count, fmt_hms};
use crate::feed_notice;
use crate::state::BarKind;
use crate::tab::Tab;
use crate::theme;
use quantick_feed::stall::Stall;

/// The popover's width, in points.
const POPOVER_WIDTH_PX: f32 = 360.0;
/// The REC button's width, whatever it says: the count grows a digit, the
/// stale state counts seconds, and the controls to its right — the bar kind,
/// BUY and SELL — must not move under a hand that has learned where they are.
pub const REC_BUTTON_MIN_WIDTH_PX: f32 = 196.0;
// The corner chip's geometry is the offline chip's, one set of numbers:
// the two sit side by side in the same corner and must read as one pill.
use crate::feed_notice::{
    CHIP_DOT_DIAMETER_PX as CHIP_DOT_PX, CHIP_DOT_GAP_PX as CHIP_GAP_PX, CHIP_HEIGHT_PX,
    CHIP_LABEL_PT, CHIP_MARGIN_PX, CHIP_PAD_PX,
};

/// The popover's persistent id, so a hook can open it the way a click does.
fn popup_id(ui: &egui::Ui) -> egui::Id {
    ui.make_persistent_id("deal_recording_popover")
}

/// The button's colours for a state: (fill, ink, border).
fn palette(state: RecState) -> (egui::Color32, egui::Color32, egui::Color32) {
    match state {
        RecState::Recording => (theme::REC, theme::CHIP_INK, theme::REC),
        RecState::Stale => (theme::AMBER, theme::CHIP_INK, theme::AMBER),
        RecState::Off => (theme::CONTROL, theme::TEXT_MUTED, theme::BORDER),
        RecState::Recorded => (theme::CONTROL, theme::TEXT_MUTED, theme::TEXT_FAINT),
        RecState::Unsupported => (theme::CONTROL, theme::TEXT_FAINT, theme::BORDER),
    }
}

/// Draw the REC button beside the symbol, and its popover when open.
///
/// `pane_kind` is what the focused pane is drawing, so the popover can offer
/// *Show as trades* only when that would change something. `request_open`
/// opens the popover this frame through the same call a click makes.
pub fn draw_button(
    ui: &mut egui::Ui,
    view: &RecordingView,
    pane_kind: BarKind,
    request_open: bool,
) -> Option<DealRecordingAction> {
    if !view.supported() {
        return None;
    }
    let mut action = None;
    let (fill, ink, border) = palette(view.state);
    let text = egui::RichText::new(view.button_label()).color(ink).strong();
    let button = ui
        .add(
            egui::Button::new(text)
                .min_size(egui::vec2(REC_BUTTON_MIN_WIDTH_PX, 0.0))
                .fill(fill)
                .stroke(egui::Stroke::new(1.0_f32, border))
                .rounding(egui::Rounding::same(3.0)),
        )
        .on_hover_text(view.headline());

    let id = popup_id(ui);
    if request_open {
        ui.memory_mut(|memory| memory.open_popup(id));
    }
    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    egui::popup::popup_below_widget(
        ui,
        id,
        &button,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(POPOVER_WIDTH_PX);
            draw_popover(ui, view, pane_kind, &mut action);
        },
    );
    action
}

/// The popover's body: what is happening, where it is written, what to do.
fn draw_popover(
    ui: &mut egui::Ui,
    view: &RecordingView,
    pane_kind: BarKind,
    action: &mut Option<DealRecordingAction>,
) {
    ui.label(egui::RichText::new(view.headline()).strong());
    ui.label(
        egui::RichText::new(
            "MetaTrader reports the exchange's own deal counter. quantick reads it every \
             poll and cuts a trades bar every N deals — where ProfitChart's Trades chart cuts.",
        )
        .color(theme::TEXT_MUTED)
        .small(),
    );
    ui.add_space(4.0);
    egui::Grid::new("deal_recording_facts")
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            let row = |ui: &mut egui::Ui, key: &str, value: String| {
                ui.label(egui::RichText::new(key).color(theme::TEXT_MUTED));
                ui.label(egui::RichText::new(value).monospace());
                ui.end_row();
            };
            row(
                ui,
                "session deals",
                view.reading
                    .map_or_else(|| "none yet".to_owned(), fmt_count),
            );
            row(
                ui,
                "first reading",
                view.first_reading_ms
                    .map_or_else(|| "—".to_owned(), |ms| fmt_hms(ms, view.tz_minutes)),
            );
            row(
                ui,
                "file since",
                view.since_ms
                    .map_or_else(|| "—".to_owned(), |ms| fmt_hms(ms, view.tz_minutes)),
            );
            row(ui, "resolution", "one poll, ≈ 3 to 12 deals".to_owned());
            // The name here, the folder on its own row: a full path would
            // stretch the popover across the chart it sits over.
            if let Some(path) = &view.path {
                row(
                    ui,
                    "file",
                    format!(
                        "{} · {} lines",
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        view.written
                    ),
                );
            }
            // The tail of the path, not the whole: a home directory would
            // stretch the popover across the pane it sits over. The full
            // path is one hover away, and Open the folder is right below.
            let tail: Vec<String> = view
                .dir
                .components()
                .rev()
                .take(2)
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            ui.label(egui::RichText::new("folder").color(theme::TEXT_MUTED));
            ui.label(egui::RichText::new(format!("…/{}", tail.join("/"))).monospace())
                .on_hover_text(view.dir.display().to_string());
            ui.end_row();
            row(
                ui,
                "this pane",
                if pane_kind == BarKind::Trades {
                    "trades".to_owned()
                } else {
                    format!("{} · switch to trades to cut by deals", pane_kind.label())
                },
            );
        });
    if let Some(error) = &view.error {
        ui.label(egui::RichText::new(error).color(theme::WARN).small());
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Prints before the recording started have no deal count and form no trades \
             bar. Days you did not record open as tick or time only.",
        )
        .color(theme::TEXT_MUTED)
        .small(),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        match view.state {
            RecState::Recording | RecState::Stale => {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Stop recording").color(theme::WARN),
                    ))
                    .clicked()
                {
                    *action = Some(DealRecordingAction::Stop);
                    ui.close_menu();
                }
            }
            RecState::Off | RecState::Recorded => {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Start recording")
                                .color(theme::CHIP_INK)
                                .strong(),
                        )
                        .fill(theme::REC),
                    )
                    .clicked()
                {
                    *action = Some(DealRecordingAction::Start);
                    ui.close_menu();
                }
            }
            RecState::Unsupported => {
                ui.label(
                    egui::RichText::new("no deal counter on this source yet")
                        .small()
                        .color(theme::TEXT_MUTED),
                )
                .on_hover_text("a recorded day still opens from the list below");
            }
        }
        if pane_kind != BarKind::Trades
            && view.deal_count_available()
            && ui.button("Show as trades").clicked()
        {
            *action = Some(DealRecordingAction::ShowAsTrades);
            ui.close_menu();
        }
        if ui.button("Open the folder").clicked() {
            *action = Some(DealRecordingAction::OpenFolder);
        }
    });
    if !view.days.is_empty() {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("RECORDED DAYS")
                .small()
                .color(theme::TEXT_MUTED),
        );
        draw_days(ui, view, action);
    }
}

/// The recorded days, oldest first, each loadable into the panes.
///
/// Also drawn inside the toolbar's history menu, so the trader who is
/// paging the tape back finds the readings that go with it in the same
/// place.
pub fn draw_days(
    ui: &mut egui::Ui,
    view: &RecordingView,
    action: &mut Option<DealRecordingAction>,
) {
    for (index, day) in view.days.iter().enumerate() {
        let loaded = view.loaded_days.contains(&day.day);
        let recording_now = view.path.as_deref() == Some(day.path.as_path());
        ui.horizontal(|ui| {
            let text = format!(
                "{}  {}  {}",
                day.day,
                day.coverage(view.tz_minutes),
                day.label(view.tz_minutes)
            );
            let label =
                egui::RichText::new(text)
                    .monospace()
                    .small()
                    .color(if loaded || recording_now {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_MUTED
                    });
            if recording_now {
                ui.label(label)
                    .on_hover_text("recording: today's file, its readings are on the chart");
            } else if loaded {
                ui.label(label)
                    .on_hover_text("loaded: its trades bars are on the chart");
            } else if ui
                .add(egui::Button::new(label).frame(false))
                .on_hover_text("load this day's readings, so its prints cut as trades bars")
                .clicked()
            {
                *action = Some(DealRecordingAction::LoadDay(index));
            }
        });
    }
}

/// What the chart-corner chip says for one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealChip {
    pub text: String,
    pub hover: String,
    pub tone: RecState,
}

/// The chip for a pane, or none when the pane has nothing to say — a pane
/// not drawing `trades` on a feed that is not recording.
#[must_use]
pub fn chip_for(
    view: &RecordingView,
    pane_kind: BarKind,
    reading_in_pane: bool,
    uncounted_prints: u64,
) -> Option<DealChip> {
    let since = view
        .since_ms
        .map(|ms| fmt_hms(ms, view.tz_minutes))
        .unwrap_or_else(|| "—".to_owned());
    let (text, hover) = match view.state {
        RecState::Recording if view.since_ms.is_none() => (
            "recording · waiting for the counter".to_owned(),
            format!(
                "{} is being recorded; no reading has arrived yet this session",
                view.symbol
            ),
        ),
        RecState::Recording => (
            format!("recording · {since} →"),
            format!(
                "{} deals counted since {since} and written to disk",
                view.symbol
            ),
        ),
        RecState::Stale => (
            "recording · counter stale".to_owned(),
            "the tape flows but the deal counter has not moved; trades bars wait".to_owned(),
        ),
        RecState::Recorded => (
            format!("recorded · {}", view.loaded_days.join(", ")),
            "the deal counts on this chart come from a recording; nothing is being written"
                .to_owned(),
        ),
        RecState::Off | RecState::Unsupported => {
            if pane_kind != BarKind::Trades {
                return None;
            }
            if view.state == RecState::Unsupported {
                // A trades pane on a feed with no counter — a spec restored
                // from a workspace or a config before the hello, or a quoted
                // instrument — is a blank chart unless something says why.
                (
                    "no deal count · this source has no deal counter".to_owned(),
                    "trades bars need MetaTrader B3's session deal counter; this feed declares none"
                        .to_owned(),
                )
            } else if view.reading.is_some() {
                // Readings arrive and cut bars whether or not REC is on;
                // what REC adds is the file. Say exactly that.
                (
                    "counting · not written to disk".to_owned(),
                    "bars are cut from the live counter; press REC to keep them for a restart"
                        .to_owned(),
                )
            } else {
                (
                    "no deal count".to_owned(),
                    "press REC to count from now, or load a recorded day".to_owned(),
                )
            }
        }
    };
    let mut text = text;
    let mut hover = hover;
    if pane_kind == BarKind::Trades && uncounted_prints > 0 {
        text = format!(
            "{text} · {} prints before have no count",
            fmt_count(uncounted_prints)
        );
        hover = format!(
            "{hover}. {} prints came before the first reading and form no trades bar",
            fmt_count(uncounted_prints)
        );
    }
    if pane_kind == BarKind::Trades && !reading_in_pane && view.state == RecState::Recording {
        hover = format!("{hover}. No reading has reached this pane yet");
    }
    Some(DealChip {
        text,
        hover,
        tone: view.state,
    })
}

/// The chart corner for one tab: the chip, when the tab has something to
/// say, placed left of the offline chip's corner so the two never overlap.
pub fn draw_corner(ui: &mut egui::Ui, area: egui::Rect, tab: &Tab, stall: Option<&Stall>) {
    let Some(chip) = tab.deal_chip() else {
        return;
    };
    let offline = feed_notice::report(&tab.notice, stall).is_some_and(|report| report.is_offline());
    let right = if offline {
        feed_notice::chip_rect(ui.painter(), area).left()
    } else {
        area.right()
    };
    draw_chip(ui, area, right, &chip);
}

/// Draw the chip in the chart's bottom-right corner, left of `right_edge`,
/// and return its rectangle for the scene.
pub fn draw_chip(
    ui: &mut egui::Ui,
    area: egui::Rect,
    right_edge: f32,
    chip: &DealChip,
) -> egui::Rect {
    let painter = ui.painter();
    let label = painter.layout_no_wrap(
        chip.text.clone(),
        egui::FontId::proportional(CHIP_LABEL_PT),
        theme::TEXT_PRIMARY,
    );
    let width =
        (2.0 * CHIP_PAD_PX + CHIP_DOT_PX + CHIP_GAP_PX + label.size().x.ceil()).min(area.width());
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            right_edge - CHIP_MARGIN_PX - width,
            area.bottom() - CHIP_MARGIN_PX - CHIP_HEIGHT_PX,
        ),
        egui::vec2(width, CHIP_HEIGHT_PX),
    )
    // Never past the chart, like the offline chip: a narrow pane clips the
    // pill rather than letting it overrun the axis.
    .intersect(area);
    let response = ui
        .interact(
            rect,
            ui.id().with("deal_recording_chip"),
            egui::Sense::hover(),
        )
        .on_hover_text(&chip.hover);
    let painter = ui.painter();
    let fill = if response.hovered() {
        theme::CONTROL
    } else {
        theme::CHROME
    };
    painter.rect(
        rect,
        egui::Rounding::same(CHIP_HEIGHT_PX / 2.0),
        fill,
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    let dot_center = egui::pos2(
        rect.left() + CHIP_PAD_PX + CHIP_DOT_PX / 2.0,
        rect.center().y,
    );
    let (dot, ink) = match chip.tone {
        RecState::Recording => (theme::REC, theme::TEXT_PRIMARY),
        RecState::Stale => (theme::AMBER, theme::TEXT_PRIMARY),
        RecState::Recorded => (theme::TEXT_FAINT, theme::TEXT_PRIMARY),
        RecState::Off | RecState::Unsupported => (egui::Color32::TRANSPARENT, theme::TEXT_MUTED),
    };
    if chip.tone == RecState::Off || chip.tone == RecState::Unsupported {
        painter.circle_stroke(
            dot_center,
            CHIP_DOT_PX / 2.0,
            egui::Stroke::new(1.0_f32, theme::TEXT_FAINT),
        );
    } else if chip.tone == RecState::Recorded {
        painter.rect_filled(
            egui::Rect::from_center_size(dot_center, egui::vec2(CHIP_DOT_PX, CHIP_DOT_PX)),
            egui::Rounding::same(1.0),
            dot,
        );
    } else {
        painter.circle_filled(dot_center, CHIP_DOT_PX / 2.0, dot);
    }
    let label = painter.layout_no_wrap(
        chip.text.clone(),
        egui::FontId::proportional(CHIP_LABEL_PT),
        ink,
    );
    painter.galley(
        egui::pos2(
            rect.left() + CHIP_PAD_PX + CHIP_DOT_PX + CHIP_GAP_PX,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        ink,
    );
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn view(state: RecState) -> RecordingView {
        RecordingView {
            symbol: "WINV26".to_owned(),
            state,
            reading: Some(2_301_455),
            since_ms: Some(1_788_436_800_000),
            first_reading_ms: Some(1_788_436_800_000),
            counter_age_ms: Some(50),
            written: 10,
            path: None,
            dir: PathBuf::from("deals"),
            error: None,
            days: std::rc::Rc::from(Vec::new()),
            loaded_days: vec!["2026-09-03".to_owned()],
            tz_minutes: -180,
        }
    }

    #[test]
    fn the_chip_names_the_three_states_and_the_uncounted_prints() {
        let recording = chip_for(&view(RecState::Recording), BarKind::Trades, true, 0).unwrap();
        assert_eq!(recording.text, "recording · 09:00:00 →");
        let with_uncounted =
            chip_for(&view(RecState::Recording), BarKind::Trades, true, 1_204_118).unwrap();
        assert_eq!(
            with_uncounted.text,
            "recording · 09:00:00 → · 1 204 118 prints before have no count"
        );
        let recorded = chip_for(&view(RecState::Recorded), BarKind::Tick, false, 0).unwrap();
        assert_eq!(recorded.text, "recorded · 2026-09-03");
        let counting = chip_for(&view(RecState::Off), BarKind::Trades, true, 0).unwrap();
        assert_eq!(counting.text, "counting · not written to disk");
        let mut no_reading = view(RecState::Off);
        no_reading.reading = None;
        let off_on_trades = chip_for(&no_reading, BarKind::Trades, false, 0).unwrap();
        assert_eq!(off_on_trades.text, "no deal count");
    }

    #[test]
    fn a_tick_pane_on_a_feed_that_is_not_recording_shows_no_chip() {
        assert!(chip_for(&view(RecState::Off), BarKind::Tick, false, 0).is_none());
        let mut unsupported = view(RecState::Unsupported);
        unsupported.loaded_days.clear();
        assert!(chip_for(&unsupported, BarKind::Tick, false, 0).is_none());
        let blank = chip_for(&unsupported, BarKind::Trades, false, 0).unwrap();
        assert_eq!(
            blank.text,
            "no deal count · this source has no deal counter"
        );
    }
}
