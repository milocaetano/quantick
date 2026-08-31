//! The object manager: a non-modal list of every drawing with the named
//! per-object actions.
//!
//! It sends the same store commands the inspector and the keyboard do —
//! nothing here re-implements lock or delete rules. It is also the only place
//! a mark clamped to the edge of the series can be *found*: an off-series
//! object may be nowhere near the window the trader is looking at.

use eframe::egui;

use super::{
    DRAWING_MANAGER_DEFAULT_POSITION, DRAWING_MANAGER_GAP_PX, DrawingChromeAsk,
    DrawingChromeSurface, DrawingEnv, INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX,
    MANAGER_AREA_ID, clamp_into_chart,
};
use crate::theme;
use crate::toolrail::ToolboxDock;

/// The manager's own state.
#[derive(Default)]
pub(crate) struct Manager {
    /// Whether the window is on screen.
    pub open: bool,
    /// Whether it was on screen last frame, which is what tells an opening
    /// from a frame in the middle of a session: only the opening frame places
    /// the window.
    was_open: bool,
    /// The count-bearing gate is showing and awaiting its answer.
    confirm_delete_all: bool,
    #[cfg(test)]
    pub action_rects: Vec<(usize, &'static str, egui::Rect)>,
}

/// Where the object manager opens: one gap inboard of the rail's inner edge,
/// aligned with the rail's leading end, clamped into the chart — beside the
/// button that opened it in all four docks.
fn target_position(
    ctx: &egui::Context,
    chart: Option<egui::Rect>,
    dock: ToolboxDock,
) -> Option<egui::Pos2> {
    let chart = chart?;
    let size = ctx
        .memory(|memory| memory.area_rect(egui::Id::new(MANAGER_AREA_ID)))
        .map_or(
            egui::vec2(INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX),
            |rect| rect.size(),
        );
    let gap = DRAWING_MANAGER_GAP_PX;
    let position = match dock {
        ToolboxDock::Left | ToolboxDock::Top => egui::pos2(chart.left() + gap, chart.top() + gap),
        ToolboxDock::Bottom => egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
    };
    Some(clamp_into_chart(position, size, chart))
}

/// Draw this frame.
pub(crate) fn draw(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let mut ask = DrawingChromeAsk::default();
    if !chrome.manager.open {
        chrome.manager.was_open = false;
        return ask;
    }
    let just_opened = !chrome.manager.was_open;
    chrome.manager.was_open = true;
    #[cfg(test)]
    chrome.manager.action_rects.clear();
    let mut open = true;
    let mut confirm_delete_all = chrome.manager.confirm_delete_all;
    let rows = env.manager_rows;
    let count = rows.len();
    let authored = env.authored_objects;
    let mut window = egui::Window::new("Drawn objects")
        .id(egui::Id::new(MANAGER_AREA_ID))
        .open(&mut open)
        .default_pos(DRAWING_MANAGER_DEFAULT_POSITION)
        .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
        .collapsible(false)
        // Resizable, with the list scrolling below (audit M13): thirty
        // objects used to grow the window past the screen and put the footer
        // out of reach.
        .resizable(true);
    if just_opened
        && let Some(position) = target_position(ctx, env.focused_chart_area, env.toolbox_dock)
    {
        window = window.current_pos(position);
    }
    window.show(ctx, |ui| {
        if count == 0 {
            ui.label("No drawings yet.");
        }
        // One gesture back from an assistant that drew too much. It appears
        // only when there is something to take back, names the number, and is
        // a single undo entry.
        if authored > 0 {
            if ui
                .button(format!("Remove {authored} object(s) placed for you"))
                .on_hover_text(
                    "Removes every object an assistant placed on this chart. Ctrl+Z brings them back.",
                )
                .clicked()
            {
                ask.sweep_authored = true;
            }
            ui.add_space(4.0);
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                // Walked in reverse: the manager lists top-most first, the
                // same order hit-testing resolves overlap.
                for index in (0..count).rev() {
                    let row = &rows[index];
                    ui.horizontal(|ui| {
                        let mut label = egui::RichText::new(&row.name);
                        if row.hidden {
                            label = label.weak();
                        }
                        if ui.selectable_label(row.selected, label).clicked() {
                            ask.manager_select = Some(index);
                        }
                        if let Some(author) = &row.author {
                            ui.label(
                                egui::RichText::new("assistant")
                                    .small()
                                    .color(theme::TEXT_SUPPORT),
                            )
                            .on_hover_text(format!("Placed by {author}, not by you"));
                        }
                        if row.locked {
                            ui.label(egui::RichText::new("locked").small());
                        }
                        if row.hidden {
                            ui.label(egui::RichText::new("hidden").small());
                        }
                        // Which band an object is on, for the objects that are
                        // not on the candles. An object nothing on screen is
                        // showing — its indicator removed, hidden, collapsed
                        // or errored — is listed in amber and says which of
                        // those it is. It still exists; deleting it stays the
                        // trader's call.
                        if let Some(chip) = row.band.chip() {
                            let text = egui::RichText::new(chip).small();
                            match row.band.hint() {
                                Some(hint) => {
                                    ui.label(text.color(theme::AMBER)).on_hover_text(hint);
                                }
                                None => {
                                    ui.label(text);
                                }
                            }
                        }
                        if row.foreign_market {
                            // The one state the chart alone cannot explain:
                            // the mark resolves onto real bars, at a price
                            // that belonged to another instrument.
                            ui.label(egui::RichText::new("other market").small())
                                .on_hover_text(
                                    "Drawn while this tab showed a different instrument. The                                      moment still exists here; the price does not mean the                                      same thing",
                                );
                        }
                        if row.off_series {
                            // The mark outlived the bars it was drawn on and
                            // the chart fades it (§D7b). The list is where it
                            // can be found and removed, since a clamped object
                            // may be nowhere near the window the trader is
                            // looking at.
                            ui.label(egui::RichText::new("off series").small())
                                .on_hover_text(
                                    "Drawn at a moment this chart's bars do not cover. It is                                      shown at the nearest edge, faded, until you move or                                      delete it",
                                );
                        }
                        if row.shared {
                            // Which marks are global is a question the list
                            // must answer at a glance (Marina, §D7).
                            ui.label(egui::RichText::new("all charts").small())
                                .on_hover_text(
                                    "Also drawn on the other chart of this tab, at the same \
                                     moment in market time",
                                );
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let delete = ui.small_button("Delete");
                                #[cfg(test)]
                                chrome
                                    .manager
                                    .action_rects
                                    .push((index, "Delete", delete.rect));
                                if delete.clicked() {
                                    ask.manager_delete = Some(index);
                                }
                                let front = ui.small_button("Front");
                                #[cfg(test)]
                                chrome.manager.action_rects.push((index, "Front", front.rect));
                                if front.clicked() {
                                    ask.manager_bring_to_front = Some(index);
                                }
                                let lock =
                                    ui.small_button(if row.locked { "Unlock" } else { "Lock" });
                                #[cfg(test)]
                                chrome.manager.action_rects.push((index, "Lock", lock.rect));
                                if lock.clicked() {
                                    ask.manager_toggle_locked = Some(index);
                                }
                                let eye = ui.small_button(if row.hidden { "Show" } else { "Hide" });
                                #[cfg(test)]
                                chrome.manager.action_rects.push((index, "Eye", eye.rect));
                                if eye.clicked() {
                                    ask.manager_toggle_hidden = Some(index);
                                }
                            },
                        );
                    });
                }
            });
        ui.separator();
        if confirm_delete_all && count > 0 {
            // The count-bearing gate (audit M7): deleting everything is one
            // command, but never one stray click — and locked objects go too,
            // which the question says out loud.
            ui.horizontal(|ui| {
                ui.label(format!("Delete all {count} drawing(s), locked included?"));
                if ui.button("Delete all").clicked() {
                    ask.delete_all = true;
                    confirm_delete_all = false;
                }
                if ui.button("Keep").clicked() {
                    confirm_delete_all = false;
                }
            });
        } else {
            confirm_delete_all = false;
            ui.horizontal(|ui| {
                if ui.button("Show all").clicked() {
                    ask.show_all = true;
                }
                if ui.button("Unlock all").clicked() {
                    ask.unlock_all = true;
                }
                if count > 0 && ui.button("Delete all…").clicked() {
                    confirm_delete_all = true;
                }
            });
        }
    });
    chrome.manager.confirm_delete_all = confirm_delete_all;
    chrome.manager.open = open;
    ask
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART: egui::Rect =
        egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(900.0, 650.0));

    /// The window opens beside the button that opened it, in every dock —
    /// a list that appeared in a fixed corner would be nowhere near the hand.
    #[test]
    fn the_manager_opens_beside_the_rail_in_every_dock() {
        let ctx = egui::Context::default();
        let top = target_position(&ctx, Some(CHART), ToolboxDock::Top).expect("a chart");
        let left = target_position(&ctx, Some(CHART), ToolboxDock::Left).expect("a chart");
        assert_eq!(
            top, left,
            "a top rail and a left rail share a leading corner"
        );
        assert_eq!(top, egui::pos2(112.0, 62.0), "one gap inboard of the chart");
        let bottom = target_position(&ctx, Some(CHART), ToolboxDock::Bottom).expect("a chart");
        assert!(
            bottom.y > top.y,
            "a bottom rail opens the list at the bottom, where the button is"
        );
        assert!(CHART.contains(bottom), "and never outside the chart");
    }

    /// No laid-out chart, no placement — the window falls back to its own
    /// default rather than being put at an invented pixel.
    #[test]
    fn without_a_chart_there_is_no_placement() {
        let ctx = egui::Context::default();
        assert_eq!(target_position(&ctx, None, ToolboxDock::Left), None);
    }
}
