//! The tool-owned inspector tabs: egui widgets over a tool's headless
//! payload. A tool declares that it brings a tab ([`DrawingTool::extra_tab`]);
//! the widgets that edit its payload live here, beside the inspector that
//! mounts them, because they are window code and the tool is not.

use eframe::egui;
use quantick_drawings::{
    AVWAP_SOURCES, AvwapPayload, ChannelPayload, DEFAULT_VALUE_AREA_PCT, FrvpPayload, MAX_TEXT_PX,
    MAX_WIDTH_FRAC, MIN_TEXT_PX, MIN_WIDTH_FRAC, RectanglePayload, TEXT_PLACEHOLDER, TextPayload,
};

use super::{Drawing, DrawingPayload, DrawingTool, PresetHost, StyleColor32, fib};

/// One tool's inspector tab: draws it, answering whether anything changed.
type TabDrawer = fn(&mut egui::Ui, &mut Drawing, &mut dyn PresetHost) -> bool;

/// The widgets for the tab a tool declares, by the tool's id.
fn tab_drawer(tool_id: &str) -> Option<TabDrawer> {
    Some(match tool_id {
        "anchored-vwap" => draw_vwap_tab,
        "fixed-range-profile" => draw_profile_tab,
        "parallel-channel" => |ui, drawing, _| draw_channel_tab(ui, drawing),
        "rectangle" => |ui, drawing, _| draw_rectangle_tab(ui, drawing),
        "text" => |ui, drawing, _| draw_text_tab(ui, drawing),
        "fib-retracement" | "fib-extension" => |ui, drawing, host| {
            fib::remember_drawing_color(ui, drawing.style.color32());
            fib::draw_levels_tab(ui, drawing, host)
        },
        _ => return None,
    })
}

/// Draw `tool`'s own inspector tab; returns whether anything was edited (the
/// caller folds it into the shared undo coalescing). A tool with no tab
/// edits nothing.
pub fn draw_extra_tab(
    tool: DrawingTool,
    ui: &mut egui::Ui,
    drawing: &mut Drawing,
    host: &mut dyn PresetHost,
) -> bool {
    tab_drawer(tool.id()).is_some_and(|draw| draw(ui, drawing, host))
}

/// The VWAP tab in the inspector: source, the three band pairs, and named
/// presets through the host — the FRVP tab's contract.
fn draw_vwap_tab(ui: &mut egui::Ui, drawing: &mut Drawing, host: &mut dyn PresetHost) -> bool {
    let tool_id = drawing.tool.id();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<AvwapPayload>() else {
        return false;
    };
    let mut edited = false;

    ui.horizontal(|ui| {
        ui.label("source");
        egui::ComboBox::from_id_salt("avwap-source")
            .selected_text(payload.source.as_str())
            .show_ui(ui, |ui| {
                for source in AVWAP_SOURCES {
                    if ui
                        .selectable_value(&mut payload.source, source, source.as_str())
                        .changed()
                    {
                        edited = true;
                    }
                }
            });
    });
    for (pair, band) in payload.bands.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            edited |= ui
                .checkbox(&mut band.on, format!("{}σ band", pair + 1))
                .on_hover_text(
                    "standard-deviation band around the anchored VWAP; \
                     the multiplier scales how far from the average it sits",
                )
                .changed();
            let drag = egui::DragValue::new(&mut band.mult)
                .range(0.0..=f64::MAX)
                .speed(0.1)
                .suffix(" σ");
            edited |= ui.add_enabled(band.on, drag).changed();
        });
    }

    ui.separator();
    edited | draw_preset_rows(ui, host, (tool_id, "avwap"), payload)
}

/// The Profile tab in the inspector: value-area fraction, histogram width,
/// the display toggles, and named presets through the host.
fn draw_profile_tab(ui: &mut egui::Ui, drawing: &mut Drawing, host: &mut dyn PresetHost) -> bool {
    let tool_id = drawing.tool.id();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FrvpPayload>() else {
        return false;
    };
    let mut edited = false;

    let mut pct = i32::from(payload.value_area_pct);
    if ui
        .add(egui::Slider::new(&mut pct, 1..=100).text("value area %"))
        .changed()
    {
        payload.value_area_pct = u8::try_from(pct).unwrap_or(DEFAULT_VALUE_AREA_PCT);
        edited = true;
    }
    if ui
        .add(
            egui::Slider::new(&mut payload.width_frac, MIN_WIDTH_FRAC..=MAX_WIDTH_FRAC)
                .text("histogram width"),
        )
        .changed()
    {
        edited = true;
    }
    edited |= ui.checkbox(&mut payload.show_poc, "POC line").changed();
    edited |= ui
        .checkbox(&mut payload.show_value_area, "value area")
        .changed();
    edited |= ui
        .checkbox(&mut payload.delta_coloring, "split rows by buy/sell")
        .changed();
    edited |= ui.checkbox(&mut payload.show_labels, "labels").changed();
    edited |= ui
        .checkbox(&mut payload.extend_right, "extend to newest bar")
        .on_hover_text(
            "The developing profile: the right edge follows the tape and \
             every new bar joins the fold. The anchors stay where you put \
             them, so switching this off restores the drawn range.",
        )
        .changed();
    edited |= ui
        .checkbox(
            &mut payload.outline_over_heatmap,
            "outline over liquidity map",
        )
        .on_hover_text(
            "Over the liquidity map the profile draws as an outline, so the \
             map is never painted over. Off = always fill, and the two \
             layers compose.",
        )
        .changed();
    edited |= ui
        .checkbox(&mut payload.approximate_history, "approximate from candles")
        .on_hover_text(
            "Venue-history candles carry no tape; with this on their volume              is spread over each candle's high–low and the status line says              'approximated from OHLC'. Off = those bars contribute nothing.",
        )
        .changed();

    ui.separator();
    edited | draw_preset_rows(ui, host, (tool_id, "frvp"), payload)
}

/// Named presets via the host: pick one to apply or delete, or save the
/// payload under a new name. `salt` keeps each tab's egui ids its own:
/// `<salt>-preset` and `<salt>-preset-name`.
fn draw_preset_rows(
    ui: &mut egui::Ui,
    host: &mut dyn PresetHost,
    (tool_id, salt): (&str, &str),
    payload: &mut dyn DrawingPayload,
) -> bool {
    let mut edited = false;
    let customs = host.custom_preset_names(tool_id);
    ui.horizontal(|ui| {
        let selected_id = ui.id().with(format!("{salt}-preset"));
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
    });
    ui.horizontal(|ui| {
        let name_id = ui.id().with(format!("{salt}-preset-name"));
        let mut name: String = ui.data_mut(|data| data.get_temp(name_id).unwrap_or_default());
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("Preset name")
                .desired_width(120.0),
        );
        let trimmed = name.trim();
        if ui
            .add_enabled(
                !trimmed.is_empty(),
                egui::Button::new("Save preset").small(),
            )
            .clicked()
            && let Some(value) = payload.export_preset()
        {
            host.save_custom_preset(tool_id, trimmed, value, true);
        }
        ui.data_mut(|data| data.insert_temp(name_id, name));
    });
    edited
}

/// The Channel tab: the middle line and the two extensions.
fn draw_channel_tab(ui: &mut egui::Ui, drawing: &mut Drawing) -> bool {
    let Some(payload) = drawing
        .payload
        .as_any_mut()
        .downcast_mut::<ChannelPayload>()
    else {
        return false;
    };
    let mut changed = ui.checkbox(&mut payload.midline, "Middle line").changed();
    ui.add_space(4.0);
    changed |= ui
        .checkbox(&mut payload.extend_left, "Extend left")
        .changed();
    changed |= ui
        .checkbox(&mut payload.extend_right, "Extend right")
        .changed();
    changed
}

/// The Rectangle tab: the two extensions.
fn draw_rectangle_tab(ui: &mut egui::Ui, drawing: &mut Drawing) -> bool {
    let payload = drawing
        .payload
        .as_any_mut()
        .downcast_mut::<RectanglePayload>()
        .expect("a rectangle always carries a rectangle payload");
    let left = ui
        .checkbox(&mut payload.extend_left, "extend left")
        .on_hover_text(
            "run the band back to the chart's left edge - one click just inside the \
             left side does the same",
        )
        .changed();
    let right = ui
        .checkbox(&mut payload.extend_right, "extend right")
        .on_hover_text(
            "run the band to the chart's right edge until further notice — an armed \
             strategy then keeps watching past the drawn end instead of expiring there",
        )
        .changed();
    left || right
}

/// The Text tab: the words and their size.
fn draw_text_tab(ui: &mut egui::Ui, drawing: &mut Drawing) -> bool {
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<TextPayload>() else {
        return false;
    };
    let mut changed = ui
        .add(
            egui::TextEdit::multiline(&mut payload.text)
                .desired_rows(3)
                .hint_text(TEXT_PLACEHOLDER)
                .desired_width(f32::INFINITY),
        )
        .changed();
    ui.add_space(4.0);
    changed |= ui
        .add(
            egui::Slider::new(&mut payload.size_px, MIN_TEXT_PX..=MAX_TEXT_PX)
                .text("Size")
                .suffix(" px"),
        )
        .changed();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool that declares a tab gets one, and a tab here belongs to a
    /// tool that declares it: the two halves live in two crates now.
    #[test]
    fn every_declared_tab_has_its_widgets_and_no_widgets_lack_a_tool() {
        for tool in super::super::DRAWING_TOOLS {
            assert_eq!(
                tool.extra_tab().is_some(),
                tab_drawer(tool.id()).is_some(),
                "{}: the declared tab and the window's widgets disagree",
                tool.id()
            );
        }
    }
}
