//! The Levels tab both Fib tools mount in the inspector.
//!
//! One view component per section, each owning its own inputs and reporting
//! what it changed: the preset rows, the level list, the display options and
//! the "new objects" defaults. [`draw_levels_tab`] lays them out in order and
//! folds their results into the single `edited` flag the inspector coalesces
//! into one undo step.

use eframe::egui;

use super::super::{Drawing, PresetHost};
use super::{
    Extend, FibLevelSpec, FibPayload, LabelMode, LabelPosition, MAX_BAND_ALPHA,
    MAX_LEVEL_LABEL_LEN, RATIO_DUPLICATE_TOLERANCE, builtin_presets, parse_ratio,
};
use crate::drawings::DrawingPayload;

/// The Levels tab both Fib tools mount in the inspector. Returns whether the
/// payload or the geometry changed (folded into the shared undo coalescing).
pub(in crate::drawings) fn draw_levels_tab(
    ui: &mut egui::Ui,
    drawing: &mut Drawing,
    host: &mut dyn PresetHost,
) -> bool {
    let tool_id = drawing.tool.id();
    let anchor_prices: Vec<f64> = drawing.points.iter().map(|point| point.price).collect();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FibPayload>() else {
        return false;
    };

    let mut edited = BuiltinPresetRow {
        payload: &mut *payload,
    }
    .show(ui);
    edited |= CustomPresetRows {
        payload: &mut *payload,
        host: &mut *host,
        tool_id,
    }
    .show(ui);
    ui.separator();

    edited |= LevelList {
        payload: &mut *payload,
        earlier_edit: edited,
    }
    .show(ui);
    ui.separator();

    let display = DisplayOptions {
        payload: &mut *payload,
        anchor_prices: &anchor_prices,
    }
    .show(ui);
    edited |= display.edited;
    if edited {
        payload.touch();
    }
    if display.swap_anchors && drawing.points.len() >= 2 {
        drawing.points.swap(0, 1);
        edited = true;
    }

    // The same two calls the Style tab makes, offered on the tab where the
    // configuration is actually built. A trader who has just laid out their
    // levels and colours is standing exactly where "and from now on" is worth
    // asking; sending them to another tab for it is how this ended up being
    // redone every single session.
    ui.separator();
    NewObjectDefaults { drawing, host }.show(ui);

    edited
}

/// Built-in presets: read-only ratio sets, applied as one edit.
struct BuiltinPresetRow<'a> {
    payload: &'a mut FibPayload,
}

impl BuiltinPresetRow<'_> {
    fn show(self, ui: &mut egui::Ui) -> bool {
        let mut edited = false;
        ui.horizontal(|ui| {
            ui.label("Preset");
            for preset in builtin_presets(self.payload.kind) {
                if ui.small_button(preset.name).clicked() {
                    self.payload.apply_preset(preset);
                    edited = true;
                }
            }
        });
        edited
    }
}

/// Custom presets via the host: apply, save (with explicit overwrite),
/// delete, and the explicit per-kind default for new objects.
struct CustomPresetRows<'a> {
    payload: &'a mut FibPayload,
    host: &'a mut dyn PresetHost,
    tool_id: &'static str,
}

impl CustomPresetRows<'_> {
    fn show(self, ui: &mut egui::Ui) -> bool {
        let Self {
            payload,
            host,
            tool_id,
        } = self;
        let mut edited = false;
        let customs = host.custom_preset_names(tool_id);
        ui.horizontal(|ui| {
            let selected_id = ui.id().with("custom-preset");
            let mut selected: String =
                ui.data_mut(|data| data.get_temp(selected_id).unwrap_or_default());
            egui::ComboBox::from_id_salt(selected_id)
                .selected_text(if selected.is_empty() {
                    "Custom presets"
                } else {
                    selected.as_str()
                })
                .show_ui(ui, |ui| {
                    for name in &customs {
                        ui.selectable_value(&mut selected, name.clone(), name);
                    }
                });
            ui.data_mut(|data| data.insert_temp(selected_id, selected.clone()));
            let has_selection = customs.contains(&selected);
            if ui
                .add_enabled(has_selection, egui::Button::new("Apply").small())
                .clicked()
                && let Some(value) = host.load_custom_preset(tool_id, &selected)
                && payload.import_preset(&value)
            {
                edited = true;
            }
            if ui
                .add_enabled(has_selection, egui::Button::new("Delete").small())
                .clicked()
            {
                host.delete_custom_preset(tool_id, &selected);
            }
            if ui
                .add_enabled(has_selection, egui::Button::new("Set as default").small())
                .on_hover_text("New objects of this tool start from this preset")
                .clicked()
            {
                host.set_default_preset(tool_id, Some(selected.clone()));
            }
        });
        ui.horizontal(|ui| {
            let name_id = ui.id().with("preset-name");
            let mut name: String = ui.data_mut(|data| data.get_temp(name_id).unwrap_or_default());
            ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text("Preset name")
                    .desired_width(120.0),
            );
            let confirm_id = ui.id().with("preset-overwrite");
            let pending: bool = ui.data_mut(|data| data.get_temp(confirm_id).unwrap_or(false));
            let trimmed = name.trim();
            let valid = !trimmed.is_empty() && trimmed.len() <= MAX_LEVEL_LABEL_LEN;
            if pending {
                ui.label("Overwrite?");
                if ui.small_button("Yes").clicked() {
                    if let Some(value) = payload.export_preset() {
                        host.save_custom_preset(tool_id, trimmed, value, true);
                    }
                    ui.data_mut(|data| data.insert_temp(confirm_id, false));
                }
                if ui.small_button("No").clicked() {
                    ui.data_mut(|data| data.insert_temp(confirm_id, false));
                }
            } else if ui
                .add_enabled(valid, egui::Button::new("Save preset").small())
                .clicked()
                && let Some(value) = payload.export_preset()
                && !host.save_custom_preset(tool_id, trimmed, value, false)
            {
                // The name exists: overwriting a custom needs explicit consent.
                ui.data_mut(|data| data.insert_temp(confirm_id, true));
            }
            ui.data_mut(|data| data.insert_temp(name_id, name));
            if host.default_preset(tool_id).is_some() && ui.small_button("Clear default").clicked()
            {
                host.set_default_preset(tool_id, None);
            }
        });
        edited
    }
}

/// The level list: one [`LevelRow`] per level, then the add / sort /
/// restore row.
///
/// A refused edit's message outlives its frame (audit M12): kept in temp
/// storage until the next successful edit, because a label that lives only
/// inside the `.changed()` branch is visible for ~16 ms — the typed ratio
/// silently reverts and nothing explains why.
struct LevelList<'a> {
    payload: &'a mut FibPayload,
    /// Whether a section above already edited the payload this frame; a
    /// successful edit anywhere on the tab clears the sticky refusal.
    earlier_edit: bool,
}

impl LevelList<'_> {
    fn show(self, ui: &mut egui::Ui) -> bool {
        let Self {
            payload,
            earlier_edit,
        } = self;
        let error_id = ui.id().with("fib_level_error");
        let sticky_error: Option<&'static str> = ui.data_mut(|data| data.get_temp(error_id));
        let mut edited = false;
        let mut error: Option<&'static str> = None;
        let mut remove: Option<usize> = None;
        let single_level = payload.levels.len() == 1;
        let single_visible = payload.visible_count() <= 1;
        let mut ratio_edits: Vec<(usize, f64)> = Vec::new();
        for (index, level) in payload.levels.iter_mut().enumerate() {
            let row = LevelRow {
                index,
                level,
                single_level,
                single_visible,
            }
            .show(ui);
            edited |= row.edited;
            if row.error.is_some() {
                error = row.error;
            }
            if let Some(ratio) = row.ratio_edit {
                ratio_edits.push((index, ratio));
            }
            if row.remove {
                remove = Some(index);
            }
        }
        for (index, ratio) in ratio_edits {
            if payload.is_duplicate(ratio, Some(index)) {
                error = Some("That ratio already exists.");
            } else {
                payload.levels[index].ratio = ratio;
                edited = true;
            }
        }
        if let Some(index) = remove {
            payload.levels.remove(index);
            edited = true;
        }
        // A fresh refusal replaces the sticky one; a successful edit clears
        // it; otherwise it stays on screen.
        let shown = error.or(if edited || earlier_edit {
            None
        } else {
            sticky_error
        });
        ui.data_mut(|data| match shown {
            Some(message) => data.insert_temp(error_id, message),
            None => data.remove::<&'static str>(error_id),
        });
        if let Some(message) = shown {
            ui.colored_label(crate::theme::WARN, message);
        }
        ui.horizontal(|ui| {
            if ui.small_button("+ Add level").clicked() {
                let mut candidate = 0.5;
                while payload.is_duplicate(candidate, None) {
                    candidate += 0.1;
                }
                payload.levels.push(FibLevelSpec::new(candidate));
                edited = true;
            }
            if ui.small_button("Sort").clicked() {
                payload
                    .levels
                    .sort_by(|left, right| left.ratio.total_cmp(&right.ratio));
                edited = true;
            }
            if ui.small_button("Restore").clicked() {
                payload.apply_preset(&builtin_presets(payload.kind)[0]);
                edited = true;
            }
        });
        edited
    }
}

/// One level's row: visible / ratio / label / colour / fill / delete.
///
/// Edits that only touch this level apply in place; the ratio (which must
/// not duplicate a sibling) and the delete are reported back for the list
/// to apply once every row has drawn.
struct LevelRow<'a> {
    index: usize,
    level: &'a mut FibLevelSpec,
    single_level: bool,
    single_visible: bool,
}

/// What one [`LevelRow`] asks of the list.
#[derive(Default)]
struct LevelRowOutcome {
    edited: bool,
    /// The last refusal this row produced this frame.
    error: Option<&'static str>,
    ratio_edit: Option<f64>,
    remove: bool,
}

impl LevelRow<'_> {
    fn show(self, ui: &mut egui::Ui) -> LevelRowOutcome {
        let Self {
            index,
            level,
            single_level,
            single_visible,
        } = self;
        let mut out = LevelRowOutcome::default();
        ui.horizontal(|ui| {
            let mut visible = level.visible;
            if ui
                .checkbox(&mut visible, "")
                .on_hover_text("Show this level")
                .changed()
            {
                if !visible && single_visible && level.visible {
                    out.error = Some("At least one level stays visible.");
                } else {
                    level.visible = visible;
                    out.edited = true;
                }
            }
            // Ratio as text: ".618", "0,618" and "61.8%" all parse.
            let ratio_id = ui.id().with(("ratio", index));
            let mut buffer: String = ui.data_mut(|data| {
                data.get_temp(ratio_id)
                    .unwrap_or_else(|| format!("{:.3}", level.ratio))
            });
            let response = ui.add(
                egui::TextEdit::singleline(&mut buffer)
                    .desired_width(56.0)
                    .font(egui::TextStyle::Monospace),
            );
            if response.lost_focus() {
                match parse_ratio(&buffer) {
                    Some(ratio) if (ratio - level.ratio).abs() >= RATIO_DUPLICATE_TOLERANCE => {
                        out.ratio_edit = Some(ratio);
                    }
                    Some(_) => {}
                    None => out.error = Some("Ratios read like 0.618 or 61.8%."),
                }
                buffer = format!("{:.3}", level.ratio);
            }
            ui.data_mut(|data| data.insert_temp(ratio_id, buffer));
            // Custom label (empty = automatic).
            let mut label = level.label.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut label)
                        .hint_text("auto")
                        .desired_width(80.0),
                )
                .changed()
            {
                label.truncate(MAX_LEVEL_LABEL_LEN);
                level.label = label;
                out.edited = true;
            }
            // Colour override; the small x returns to the inherited colour.
            let mut color = level.color.map_or(drawing_color_of(ui), |[r, g, b]| {
                egui::Color32::from_rgb(r, g, b)
            });
            if ui.color_edit_button_srgba(&mut color).changed() {
                level.color = Some([color.r(), color.g(), color.b()]);
                out.edited = true;
            }
            if level.color.is_some()
                && ui
                    .small_button("x")
                    .on_hover_text("Inherit colour")
                    .clicked()
            {
                level.color = None;
                out.edited = true;
            }
            let mut fill = level.fill_to_next;
            if ui
                .checkbox(&mut fill, "fill")
                .on_hover_text("Fill to the next visible level")
                .changed()
            {
                level.fill_to_next = fill;
                out.edited = true;
            }
            if ui
                .add_enabled(!single_level, egui::Button::new("\u{00d7}").small())
                .on_hover_text("Delete this level")
                .clicked()
            {
                out.remove = true;
            }
        });
        out
    }
}

/// Bands, labels, extension and scale, plus the A/B swap.
struct DisplayOptions<'a> {
    payload: &'a mut FibPayload,
    anchor_prices: &'a [f64],
}

/// What [`DisplayOptions`] changed: the payload, and whether the trader
/// asked to swap the A and B anchors (geometry the payload does not own).
struct DisplayOutcome {
    edited: bool,
    swap_anchors: bool,
}

impl DisplayOptions<'_> {
    fn show(self, ui: &mut egui::Ui) -> DisplayOutcome {
        let Self {
            payload,
            anchor_prices,
        } = self;
        let mut edited = ui
            .add(
                egui::Slider::new(&mut payload.band_alpha, 0..=MAX_BAND_ALPHA).text("band opacity"),
            )
            .changed();
        let mut swap_anchors = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("labels")
                .selected_text(payload.label_mode.describe())
                .show_ui(ui, |ui| {
                    for mode in LabelMode::ALL {
                        edited |= ui
                            .selectable_value(&mut payload.label_mode, mode, mode.describe())
                            .changed();
                    }
                });
            egui::ComboBox::from_label("position")
                .selected_text(payload.label_position.describe())
                .show_ui(ui, |ui| {
                    for position in LabelPosition::ALL {
                        edited |= ui
                            .selectable_value(
                                &mut payload.label_position,
                                position,
                                position.describe(),
                            )
                            .changed();
                    }
                });
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("extend")
                .selected_text(payload.extend.describe())
                .show_ui(ui, |ui| {
                    for extend in Extend::ALL {
                        edited |= ui
                            .selectable_value(&mut payload.extend, extend, extend.describe())
                            .changed();
                    }
                });
            let log_allowed = anchor_prices.iter().all(|&price| price > 0.0);
            let log = ui
                .add_enabled(
                    log_allowed,
                    egui::Checkbox::new(&mut payload.log_scale, "log scale"),
                )
                .on_disabled_hover_text("Log needs every anchor price above zero.");
            edited |= log.changed();
            if ui
                .small_button("Invert A \u{2194} B")
                .on_hover_text("Swap the A and B anchors; C never moves")
                .clicked()
            {
                swap_anchors = true;
            }
        });
        DisplayOutcome {
            edited,
            swap_anchors,
        }
    }
}

/// "Save as default" / "Reset to factory" for the tool's new objects.
struct NewObjectDefaults<'a> {
    drawing: &'a Drawing,
    host: &'a mut dyn PresetHost,
}

impl NewObjectDefaults<'_> {
    fn show(self, ui: &mut egui::Ui) {
        let Self { drawing, host } = self;
        ui.horizontal(|ui| {
            // Named, because "Set as default" sits four rows up and means the
            // neighbouring thing: that one promotes a preset the trader
            // *named*, this one takes the setup in front of them as it
            // stands. Without the words in front, two buttons a thumb apart
            // read as the same button.
            ui.label(egui::RichText::new("New objects").small());
            if ui
                .small_button("Save as default")
                .on_hover_text(
                    "New objects of this tool open with these levels, colours and labels",
                )
                .clicked()
            {
                crate::drawings::save_tool_default(host, drawing);
            }
            if crate::drawings::has_saved_default(host, drawing.tool)
                && ui
                    .small_button("Reset to factory")
                    .on_hover_text(concat!(
                        "New objects go back to the built-in levels and ",
                        "colours. Clears the default preset choice too; ",
                        "saved presets are kept"
                    ))
                    .clicked()
            {
                crate::drawings::reset_tool_default(host, drawing.tool);
            }
        });
    }
}

/// The colour the current drawing paints with, for the per-level colour
/// button's initial value. Read from the style the caller stashed in the
/// Ui's data (set by the tool impl before calling the editor).
fn drawing_color_of(ui: &egui::Ui) -> egui::Color32 {
    ui.data(|data| data.get_temp(egui::Id::new("fib-editor-drawing-color")))
        .unwrap_or(crate::drawings::DEFAULT_DRAWING_COLOR)
}

/// Stash the drawing colour for [`drawing_color_of`].
pub(in crate::drawings) fn remember_drawing_color(ui: &egui::Ui, color: egui::Color32) {
    ui.data_mut(|data| data.insert_temp(egui::Id::new("fib-editor-drawing-color"), color));
}
