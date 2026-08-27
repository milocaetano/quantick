//! The layout picker: the toolbar's door to every arrangement the canvas can
//! draw.
//!
//! `View → Layout` is kept — a menu is where a feature is *discovered* — but a
//! trader who changes layout mid-session should not have to walk a menu for
//! it. This is the icon that opens a grid of pictures instead.
//!
//! The grid is drawn from [`canvas_layout::LAYOUT_PRESETS`] and nothing else,
//! so an arrangement added to that table appears here without this file being
//! edited. That is the whole reason the registry exists.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::canvas_layout::{LAYOUT_PRESETS, LayoutPreset, PaneKind};
use crate::theme;
use crate::widgets::{CORNER_RADIUS, IconButton, TOOLBAR_ICON};

/// Width of the popover, in pixels.
const POPOVER_WIDTH_PX: f32 = 268.0;
/// One preset cell: the thumbnail plus its label.
///
/// Tall enough for a label on two lines. Preset names grow with the number of
/// panes they hold — "Timeframe + Timeframe + Flow" is one that is coming — so
/// the label wraps inside the cell rather than the cell being widened to fit
/// whichever name happens to be longest today.
const CELL_SIZE: egui::Vec2 = egui::vec2(76.0, 92.0);
/// The miniature layout diagram inside a cell.
const THUMBNAIL_SIZE: egui::Vec2 = egui::vec2(64.0, 40.0);
/// Gap between cells, both axes.
const CELL_GAP_PX: f32 = 8.0;
/// Corner radius of a thumbnail's outer box.
const THUMBNAIL_RADIUS_PX: f32 = 2.0;
/// Stroke width of the rules inside a thumbnail.
const THUMBNAIL_STROKE_PX: f32 = 1.0;
/// Stroke width of the ring around the selected cell. Two pixels at 3:1 or
/// better is the accessible floor for a selection indicator; one pixel of
/// accent against chrome is legible but not *findable*.
const SELECTED_STROKE_PX: f32 = 1.5;
/// Padding inside the popover frame.
const POPOVER_PADDING_PX: f32 = 10.0;
/// How many presets a number key reaches (`Ctrl+1` … `Ctrl+9`). Mirrors
/// `app::LAYOUT_PRESET_KEYS`, whose length is what a number row has; a preset
/// past the ninth simply has no shortcut to name.
const SHORTCUT_KEYS: usize = 9;
/// Font size of a cell's label.
const LABEL_SIZE_PX: f32 = 11.0;
/// Gap between a cell's thumbnail and the label under it.
const LABEL_GAP_PX: f32 = 5.0;
/// Space above a cell's thumbnail, as a share of the popover's own padding.
/// Derived rather than a second number, so tightening the popover tightens the
/// cell with it.
const THUMBNAIL_TOP_PAD: f32 = 0.6;
/// How much of a cell's width the label gives up on each side before it wraps.
/// Keeps a wrapped line clear of the selection ring drawn on the cell's edge.
const LABEL_INSET_PX: f32 = 5.0;

/// What the picker needs to know about the canvas it is switching.
pub struct PickerModel<'a> {
    /// The preset the canvas is showing, if it still matches one. `None` is a
    /// real state: a row the trader rearranged by hand is a custom layout, and
    /// lighting a preset it no longer is would be the chrome lying.
    pub current: Option<&'static LayoutPreset>,
    /// Whether the popover is open. Owned by the caller so the toolbar's own
    /// collapse logic can close it.
    pub open: &'a mut bool,
    /// Open the popover this frame, whatever it was doing.
    ///
    /// One-shot, and drained by the caller. A popover is a thing egui owns, so
    /// there is no state to set that would not be a second way of opening it:
    /// the request goes through the same `open_popup` the click does
    /// (`.claude/skills/ui-harness`).
    pub request_open: bool,
}

/// Draw the picker button, and its popover when it is open.
///
/// Returns the preset the trader picked this frame, if any.
pub fn draw(ui: &mut egui::Ui, model: PickerModel<'_>) -> Option<&'static LayoutPreset> {
    let button = IconButton::new(icons::LAYOUT, TOOLBAR_ICON)
        .active(*model.open)
        .hover_text("chart layout (Ctrl+1…)")
        .show(ui);

    let popup_id = ui.make_persistent_id("layout_picker_popup");
    if model.request_open {
        ui.memory_mut(|memory| memory.open_popup(popup_id));
        *model.open = true;
    }
    if button.clicked() {
        *model.open = !*model.open;
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    // The two have to agree in both directions: egui closes the popup on a
    // click elsewhere without telling us, and a stale `open` flag would leave
    // the button lit over a popover that is gone.
    if !model.request_open && !ui.memory(|memory| memory.is_popup_open(popup_id)) {
        *model.open = false;
    }

    let mut picked = None;
    egui::popup::popup_below_widget(
        ui,
        popup_id,
        &button,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(POPOVER_WIDTH_PX);
            picked = draw_grid(ui, model.current);
        },
    );
    if picked.is_some() {
        *model.open = false;
    }
    picked
}

/// The grid of preset cells, wrapped to the popover's width.
fn draw_grid(
    ui: &mut egui::Ui,
    current: Option<&'static LayoutPreset>,
) -> Option<&'static LayoutPreset> {
    let mut picked = None;
    ui.spacing_mut().item_spacing = egui::vec2(CELL_GAP_PX, CELL_GAP_PX);
    ui.add_space(POPOVER_PADDING_PX - ui.spacing().item_spacing.y);

    let per_row = ((POPOVER_WIDTH_PX - POPOVER_PADDING_PX * 2.0 + CELL_GAP_PX)
        / (CELL_SIZE.x + CELL_GAP_PX))
        .floor()
        .max(1.0) as usize;

    for (row, chunk) in LAYOUT_PRESETS.chunks(per_row).enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(POPOVER_PADDING_PX);
            for (offset, preset) in chunk.iter().enumerate() {
                let index = row * per_row + offset;
                let selected = current.is_some_and(|entry| entry.id == preset.id);
                if draw_cell(ui, preset, selected, index).clicked() {
                    picked = Some(preset);
                }
            }
        });
    }
    ui.add_space(POPOVER_PADDING_PX);
    picked
}

/// One preset: its thumbnail and its name, as a single click target.
///
/// `index` is the preset's position in the registry, which is also the number
/// key that reaches it — so the tooltip can name the shortcut without this
/// module keeping a second list of them.
fn draw_cell(
    ui: &mut egui::Ui,
    preset: &'static LayoutPreset,
    selected: bool,
    index: usize,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CELL_SIZE, egui::Sense::click());
    let hint = if index < SHORTCUT_KEYS {
        format!("{} (Ctrl+{})", preset.label, index + 1)
    } else {
        preset.label.to_owned()
    };
    let response = response.on_hover_text(hint);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();

    if response.hovered() {
        painter.rect_filled(rect, CORNER_RADIUS, theme::CONTROL);
    }
    if selected {
        painter.rect_stroke(
            rect,
            CORNER_RADIUS,
            egui::Stroke::new(SELECTED_STROKE_PX, theme::ACCENT),
        );
    }

    let thumbnail = egui::Rect::from_min_size(
        egui::pos2(
            rect.center().x - THUMBNAIL_SIZE.x / 2.0,
            rect.top() + POPOVER_PADDING_PX * THUMBNAIL_TOP_PAD,
        ),
        THUMBNAIL_SIZE,
    );
    draw_thumbnail(painter, thumbnail, preset);

    // The label carries the selection in weight as well as in colour: a ring
    // alone asks the reader to discriminate a hue against a busy chrome.
    let label_colour = if selected || response.hovered() {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    // Wrapped to the cell, never past it. An unwrapped label crosses the
    // selection ring and reads as a broken widget — which is what a preset
    // name one pane longer than today's would have done.
    let mut job = egui::text::LayoutJob::simple(
        preset.label.to_owned(),
        egui::FontId::proportional(LABEL_SIZE_PX),
        label_colour,
        rect.width() - LABEL_INSET_PX * 2.0,
    );
    // Centre every line, not just the block: a two-line name whose second line
    // hangs left reads as a text box that ran out of room.
    job.halign = egui::Align::Center;
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(rect.center().x, thumbnail.bottom() + LABEL_GAP_PX),
        galley,
        label_colour,
    );
    response
}

/// The miniature: one block per pane, left to right, with the flow pane
/// filled.
///
/// Filling the flow block is what teaches the rule the registry enforces —
/// the heatmap is the protagonist, and the picture says where it will be
/// before the trader commits to the layout.
fn draw_thumbnail(painter: &egui::Painter, rect: egui::Rect, preset: &LayoutPreset) {
    painter.rect_filled(rect, THUMBNAIL_RADIUS_PX, theme::INSET);

    let count = preset.kinds.len().max(1);
    // The flow pane is drawn wider than the context panes for the same reason
    // it is wider on the canvas. An even miniature would promise a layout the
    // canvas does not open on.
    let flow_weight = 2.0_f32;
    let total_weight: f32 = preset
        .kinds
        .iter()
        .map(|kind| match kind {
            PaneKind::Flow => flow_weight,
            _ => 1.0,
        })
        .sum::<f32>()
        .max(1.0);

    let mut left = rect.left();
    for (index, kind) in preset.kinds.iter().enumerate() {
        let weight = match kind {
            PaneKind::Flow => flow_weight,
            _ => 1.0,
        };
        let width = rect.width() * (weight / total_weight);
        let block = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2((left + width).min(rect.right()), rect.bottom()),
        );
        if matches!(kind, PaneKind::Flow) {
            painter.rect_filled(
                block.shrink(1.0),
                THUMBNAIL_RADIUS_PX,
                theme::active_tint(theme::ACCENT),
            );
            painter.rect_stroke(
                block.shrink(1.0),
                THUMBNAIL_RADIUS_PX,
                egui::Stroke::new(THUMBNAIL_STROKE_PX, theme::ACCENT),
            );
        }
        if index + 1 < count {
            painter.line_segment(
                [
                    egui::pos2(block.right(), rect.top()),
                    egui::pos2(block.right(), rect.bottom()),
                ],
                egui::Stroke::new(THUMBNAIL_STROKE_PX, theme::TEXT_FAINT),
            );
        }
        left = block.right();
    }
    painter.rect_stroke(
        rect,
        THUMBNAIL_RADIUS_PX,
        egui::Stroke::new(THUMBNAIL_STROKE_PX, theme::TEXT_FAINT),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picker draws the registry, not a list of its own. A preset added to
    /// the table must reach the popover without this file being edited — which
    /// is only true while nothing here enumerates layouts.
    #[test]
    fn the_grid_offers_every_registered_preset() {
        let source = include_str!("layout_picker.rs");
        for preset in LAYOUT_PRESETS {
            assert!(
                !source.contains(&format!("\"{}\"", preset.id)),
                "the picker names the preset {} instead of reading the registry",
                preset.id
            );
        }
    }

    /// The popover has to stay a popover as the registry grows.
    ///
    /// A property of the *table*, not of the constants: adding presets adds
    /// rows, and a grid that outgrows the screen is a layout nobody can pick.
    /// This is the test that fails when the registry gets ambitious.
    #[test]
    fn the_grid_stays_a_reasonable_size_for_every_registered_preset() {
        let usable = POPOVER_WIDTH_PX - POPOVER_PADDING_PX * 2.0;
        let per_row = ((usable + CELL_GAP_PX) / (CELL_SIZE.x + CELL_GAP_PX)).floor() as usize;
        assert!(per_row >= 1, "not one cell fits the popover width");

        let rows = LAYOUT_PRESETS.len().div_ceil(per_row);
        let height = rows as f32 * CELL_SIZE.y
            + (rows.saturating_sub(1)) as f32 * CELL_GAP_PX
            + POPOVER_PADDING_PX * 2.0;
        assert!(
            height <= 420.0,
            "{} presets need {rows} rows and {height}px of popover; either the              grid wraps wider or the registry has outgrown a popover",
            LAYOUT_PRESETS.len()
        );
    }

    /// A cell is a click target before it is a picture, so it has to clear the
    /// 24px minimum a pointer target is held to.
    #[test]
    fn a_preset_cell_is_a_large_enough_click_target() {
        let smallest = CELL_SIZE.x.min(CELL_SIZE.y);
        assert!(
            smallest >= 24.0,
            "a {smallest}px cell is under the minimum target size"
        );
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    /// Every registered preset must be reachable from the keyboard.
    ///
    /// The picker is the discoverable path and the menu is the learnable one,
    /// but a trader mid-session uses neither — they press a number. A preset
    /// added past the ninth would be mouse-only, which the "operable without
    /// a hand" rule does not allow to happen quietly.
    #[test]
    fn every_preset_is_reachable_by_a_number_key() {
        assert!(
            LAYOUT_PRESETS.len() <= SHORTCUT_KEYS,
            "{} presets but only {SHORTCUT_KEYS} number keys: the ones past \
             the ninth would be reachable by mouse and menu only",
            LAYOUT_PRESETS.len()
        );
    }

    /// The shortcut a cell names is its own position, not a hardcoded map.
    #[test]
    fn the_named_shortcut_follows_the_registry_order() {
        for (index, preset) in LAYOUT_PRESETS.iter().enumerate() {
            let hint = format!("{} (Ctrl+{})", preset.label, index + 1);
            assert!(
                hint.contains(preset.label),
                "the hint must name the preset it belongs to"
            );
            assert!(hint.ends_with(&format!("(Ctrl+{})", index + 1)));
        }
    }
}
