//! Layout popover A: the catalogue the capsule's miniature opens, side by
//! side — WITH FLOW on the left, TIMEFRAMES on the right — filled from
//! [`LAYOUT_PRESETS`] and nothing else, so a preset added to the registry
//! appears here without this file changing.
//!
//! The current layout wears the accent ring, the workspace's saved one a
//! "saved" tag, and the footer says the layout changed only while the two
//! differ. A pick goes back as [`ToolbarAction::SetLayout`], the one path.

use eframe::egui;

use super::ToolbarAction;
use crate::canvas_layout::{LAYOUT_PRESETS, LayoutPreset, PresetGroup};
use crate::theme;
use crate::widgets::CORNER_RADIUS;

/// One cell: the thumbnail and a name of up to two lines under it.
const CELL: egui::Vec2 = egui::vec2(70.0, 62.0);
/// The thumbnail inside a cell.
const THUMBNAIL: egui::Vec2 = egui::vec2(58.0, 34.0);
/// Gap between cells and around the column divider.
const GAP_PX: f32 = 10.0;
/// Padding inside the popover.
const PAD_PX: f32 = 12.0;
/// A cell's name.
const LABEL_PX: f32 = 11.0;
/// The ring around the current layout.
const RING_PX: f32 = 1.5;
/// How far below the capsule the popover hangs.
const DROP_PX: f32 = 4.0;

/// What the popover shows.
pub struct PopoverModel<'a> {
    /// The workspace whose layout this is.
    pub workspace: &'a str,
    /// The preset the canvas draws; `None` for a custom arrangement.
    pub current: Option<&'static LayoutPreset>,
    /// The preset the workspace saved.
    pub saved: Option<&'static LayoutPreset>,
    /// Auto save is off and the layout differs from the save.
    pub layout_unsaved: bool,
}

/// One drawn cell, for the observer scene.
pub type DrawnCell = (&'static LayoutPreset, egui::Rect);

/// Draw the popover under `anchor`'s right edge. Answers the cells drawn and
/// whether a press landed outside it (which closes it).
pub fn draw(
    ctx: &egui::Context,
    anchor: egui::Rect,
    model: &PopoverModel<'_>,
    actions: &mut Vec<ToolbarAction>,
) -> (Vec<DrawnCell>, bool) {
    let palette = theme::get(ctx);
    let mut cells = Vec::new();
    let area = egui::Area::new(egui::Id::new("layout_popover"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(anchor.right_bottom() + egui::vec2(0.0, DROP_PX))
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::menu(ui.style())
                .inner_margin(PAD_PX)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(GAP_PX, GAP_PX / 2.0);
                    let caption = |ui: &mut egui::Ui, text: String| {
                        ui.label(
                            egui::RichText::new(text)
                                .size(LABEL_PX)
                                .color(palette.text_muted()),
                        );
                    };
                    caption(ui, format!("LAYOUT OF {}", model.workspace.to_uppercase()));
                    ui.horizontal_top(|ui| {
                        for (group, title) in [
                            (PresetGroup::WithFlow, "WITH FLOW"),
                            (PresetGroup::Timeframes, "TIMEFRAMES"),
                        ] {
                            if group == PresetGroup::Timeframes {
                                ui.separator();
                            }
                            ui.vertical(|ui| {
                                caption(ui, title.to_owned());
                                let presets = LAYOUT_PRESETS.iter().filter(|p| p.group == group);
                                let presets: Vec<_> = presets.collect();
                                for row in presets.chunks(2) {
                                    ui.horizontal(|ui| {
                                        for preset in row {
                                            let rect = cell(ui, preset, model);
                                            if rect.1.clicked() {
                                                actions.push(ToolbarAction::SetLayout(preset));
                                            }
                                            cells.push((*preset, rect.0));
                                        }
                                    });
                                }
                            });
                        }
                    });
                    if model.layout_unsaved {
                        ui.separator();
                        ui.horizontal(|ui| {
                            caption(
                                ui,
                                format!(
                                    "Changed in \u{201c}{}\u{201d} \u{b7} not saved",
                                    model.workspace
                                ),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let keys = egui::RichText::new("Ctrl+S")
                                        .monospace()
                                        .color(palette.text_muted());
                                    ui.label(keys);
                                },
                            );
                        });
                    }
                });
        });
    let outside = ctx.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !anchor.contains(at) && !area.response.rect.contains(at))
    });
    (cells, outside)
}

/// One preset: its tree in miniature and its name, wrapped at " + ".
fn cell(
    ui: &mut egui::Ui,
    preset: &'static LayoutPreset,
    model: &PopoverModel<'_>,
) -> (egui::Rect, egui::Response) {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(CELL, egui::Sense::click());
    let response = response.on_hover_text(preset.label);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CORNER_RADIUS, palette.control());
    }
    if model.current == Some(preset) {
        painter.rect_stroke(
            rect,
            CORNER_RADIUS,
            egui::Stroke::new(RING_PX, palette.accent()),
        );
    }
    let thumb = egui::Rect::from_min_size(
        egui::pos2(rect.center().x - THUMBNAIL.x / 2.0, rect.top() + 4.0),
        THUMBNAIL,
    );
    super::capsule::paint_tree(painter, thumb, preset.tree);
    let name = preset
        .label
        .replacen(" + ", "\n+ ", usize::from(preset.label.len() > 12));
    painter.text(
        egui::pos2(rect.center().x, thumb.bottom() + 3.0),
        egui::Align2::CENTER_TOP,
        name,
        egui::FontId::proportional(LABEL_PX),
        palette.text_primary(),
    );
    if model.saved == Some(preset) {
        let tag = painter.layout_no_wrap(
            "saved".to_owned(),
            egui::FontId::proportional(9.0),
            palette.text_muted(),
        );
        let at = egui::Rect::from_min_size(
            egui::pos2(thumb.right() - tag.size().x - 4.0, thumb.top() - 5.0),
            tag.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect(
            at,
            4.0,
            palette.chrome(),
            egui::Stroke::new(1.0_f32, palette.border()),
        );
        painter.galley(at.min + egui::vec2(3.0, 1.0), tag, palette.text_muted());
    }
    (rect, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The popover reads the registry: no preset id is spelled here.
    #[test]
    fn the_popover_names_no_preset_of_its_own() {
        let source = include_str!("layout_popover.rs");
        for preset in LAYOUT_PRESETS {
            assert!(
                !source.contains(&format!("\"{}\"", preset.id)),
                "{}",
                preset.id
            );
        }
    }
}
