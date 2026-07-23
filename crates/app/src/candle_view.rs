//! Egui adapter for candle appearance.
//!
//! Direction, opacity and input sanitization stay in [`crate::style`], while
//! pixel geometry stays in [`crate::chart`]. This module only turns those pure
//! descriptions into egui shapes and widgets.

use eframe::egui;
use quantick_engine::Bar;

use crate::chart::{PriceScale, VerticalSegment, candle_geometry};
use crate::style::{
    CandleBodyMode, CandlePreset, CandleStyle, ChartStyle, ResolvedCandlePaint, WickColorMode,
};

/// What changed while drawing the appearance window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StylePanelResponse {
    /// At least one appearance value changed.
    pub changed: bool,
    /// A preset button was applied this frame.
    pub applied_preset: Option<CandlePreset>,
}

/// Draw one candle from pure geometry and a resolved paint description.
///
/// The heatmap is painted before this function and aggression bubbles after it.
/// Translucent or absent fills therefore reveal liquidity without allowing
/// candle contours to cover the aggression markers.
pub fn draw_candle(
    painter: &egui::Painter,
    xc: f32,
    half_width: f32,
    scale: &PriceScale,
    bar: &Bar,
    forming: bool,
    style: &CandleStyle,
) {
    let paint = style.resolved(bar.close >= bar.open, forming);
    let geometry = candle_geometry(scale, bar, xc, half_width, paint.min_body_height);
    let body = egui::Rect::from_min_max(
        egui::pos2(geometry.body.left, geometry.body.top),
        egui::pos2(geometry.body.right, geometry.body.bottom),
    );

    if let Some(wick) = paint.wick {
        let stroke = egui::Stroke::new(paint.wick_width, color32(wick));
        if let Some(segment) = geometry.upper_wick {
            draw_vertical_segment(painter, segment, stroke);
        }
        if let Some(segment) = geometry.lower_wick {
            draw_vertical_segment(painter, segment, stroke);
        }
    }

    let body_width = geometry.body.width();
    let body_height = geometry.body.height();
    let radius = paint
        .corner_radius
        .min(body_width / 2.0)
        .min(body_height / 2.0)
        .max(0.0);
    let rounding = egui::Rounding::same(radius);
    if let Some(fill) = paint.fill
        && fill[3] > 0
    {
        painter.rect_filled(body, rounding, color32(fill));
    }

    if paint.outline[3] > 0 {
        // Keep very thick strokes from swallowing candles at the minimum zoom.
        let width = paint
            .outline_width
            .min(body_width.max(0.5))
            .min(body_height.max(0.5));
        painter.rect_stroke(
            body,
            rounding,
            egui::Stroke::new(width, color32(paint.outline)),
        );
    }
}

fn draw_vertical_segment(painter: &egui::Painter, segment: VerticalSegment, stroke: egui::Stroke) {
    if segment.length() > 0.0 {
        painter.line_segment(
            [
                egui::pos2(segment.x, segment.top),
                egui::pos2(segment.x, segment.bottom),
            ],
            stroke,
        );
    }
}

fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Draw the complete candle/canvas appearance window.
pub fn draw_style_window(
    ctx: &egui::Context,
    open: &mut bool,
    style: &mut ChartStyle,
) -> StylePanelResponse {
    if !*open {
        return StylePanelResponse::default();
    }

    let mut response = StylePanelResponse::default();
    let mut window_open = *open;
    egui::Window::new("candle appearance")
        .open(&mut window_open)
        .default_width(390.0)
        .min_width(350.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("candle_style_scroll")
                .max_height(650.0)
                .show(ui, |ui| {
                    draw_presets(ui, style, &mut response);
                    draw_preview(ui, style);
                    ui.separator();
                    draw_body_settings(ui, &mut style.candles, &mut response.changed);
                    ui.separator();
                    draw_outline_settings(ui, &mut style.candles, &mut response.changed);
                    ui.separator();
                    draw_wick_settings(ui, &mut style.candles, &mut response.changed);
                    ui.separator();
                    draw_live_settings(ui, &mut style.candles, &mut response.changed);
                    ui.separator();
                    draw_canvas_settings(ui, style, &mut response.changed);
                    ui.separator();

                    if ui
                        .button("restore Order flow defaults")
                        .on_hover_text("restore candle and canvas appearance only")
                        .clicked()
                    {
                        *style = ChartStyle::default();
                        response.changed = true;
                        response.applied_preset = Some(CandlePreset::OrderFlow);
                    }
                    ui.small("Appearance changes redraw the chart only; feeds and book capture stay untouched.");
                });
        });
    *open = window_open;
    response
}

fn draw_presets(ui: &mut egui::Ui, style: &mut ChartStyle, response: &mut StylePanelResponse) {
    ui.heading("Preset");
    let current = CandlePreset::detect(&style.candles)
        .map(CandlePreset::label)
        .unwrap_or("Custom");
    ui.horizontal_wrapped(|ui| {
        for preset in CandlePreset::ALL {
            let selected = CandlePreset::detect(&style.candles) == Some(preset);
            if ui.selectable_label(selected, preset.label()).clicked() {
                style.candles = preset.style();
                response.changed = true;
                response.applied_preset = Some(preset);
            }
        }
    });
    ui.small(format!("current: {current}"));
}

fn draw_body_settings(ui: &mut egui::Ui, style: &mut CandleStyle, changed: &mut bool) {
    ui.heading("Body");
    let previous_mode = style.body_mode;
    egui::ComboBox::from_id_salt("candle_body_mode")
        .selected_text(match style.body_mode {
            CandleBodyMode::Filled => "Filled",
            CandleBodyMode::OutlineOnly => "Outline only (no fill)",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut style.body_mode, CandleBodyMode::Filled, "Filled");
            ui.selectable_value(
                &mut style.body_mode,
                CandleBodyMode::OutlineOnly,
                "Outline only (no fill)",
            );
        });
    *changed |= style.body_mode != previous_mode;

    egui::Grid::new("candle_direction_colors")
        .num_columns(3)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label("fill");
            ui.label("outline");
            ui.end_row();

            ui.label("Bull");
            *changed |= ui.color_edit_button_srgb(&mut style.bull_fill).changed();
            *changed |= ui.color_edit_button_srgb(&mut style.bull_outline).changed();
            ui.end_row();

            ui.label("Bear");
            *changed |= ui.color_edit_button_srgb(&mut style.bear_fill).changed();
            *changed |= ui.color_edit_button_srgb(&mut style.bear_outline).changed();
            ui.end_row();
        });

    *changed |= ui
        .add_enabled(
            style.body_mode == CandleBodyMode::Filled,
            egui::Slider::new(&mut style.fill_opacity, 0.0..=1.0)
                .text("fill opacity")
                .step_by(0.01),
        )
        .on_hover_text("lower values reveal heatmap liquidity through the candle")
        .changed();
    *changed |= ui
        .add(
            egui::Slider::new(&mut style.body_width_frac, 0.1..=1.0)
                .text("body width")
                .step_by(0.01),
        )
        .changed();
    *changed |= ui
        .add(
            egui::Slider::new(&mut style.corner_radius, 0.0..=8.0)
                .text("corner radius px")
                .step_by(0.25),
        )
        .changed();
    *changed |= ui
        .add(
            egui::Slider::new(&mut style.min_body_height, 1.0..=12.0)
                .text("minimum doji height px")
                .step_by(0.5),
        )
        .changed();
}

fn draw_outline_settings(ui: &mut egui::Ui, style: &mut CandleStyle, changed: &mut bool) {
    ui.heading("Outline");
    *changed |= ui
        .add(
            egui::Slider::new(&mut style.outline_opacity, 0.0..=1.0)
                .text("outline opacity")
                .step_by(0.01),
        )
        .changed();
    *changed |= ui
        .add(
            egui::Slider::new(&mut style.outline_width, 0.5..=4.0)
                .text("outline thickness px")
                .step_by(0.05),
        )
        .changed();
}

fn draw_wick_settings(ui: &mut egui::Ui, style: &mut CandleStyle, changed: &mut bool) {
    ui.heading("Wicks");
    *changed |= ui
        .checkbox(&mut style.show_wicks, "show upper and lower wicks")
        .changed();
    ui.add_enabled_ui(style.show_wicks, |ui| {
        let previous_mode = style.wick_color_mode;
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut style.wick_color_mode,
                WickColorMode::MatchOutline,
                "match bull/bear outline",
            );
            ui.radio_value(&mut style.wick_color_mode, WickColorMode::Custom, "custom");
        });
        *changed |= style.wick_color_mode != previous_mode;
        if style.wick_color_mode == WickColorMode::Custom {
            ui.horizontal(|ui| {
                ui.label("wick colour");
                *changed |= ui.color_edit_button_srgb(&mut style.wick).changed();
            });
        }
        *changed |= ui
            .add(
                egui::Slider::new(&mut style.wick_opacity, 0.0..=1.0)
                    .text("wick opacity")
                    .step_by(0.01),
            )
            .changed();
        *changed |= ui
            .add(
                egui::Slider::new(&mut style.wick_width, 0.5..=4.0)
                    .text("wick thickness px")
                    .step_by(0.05),
            )
            .changed();
    });
}

fn draw_live_settings(ui: &mut egui::Ui, style: &mut CandleStyle, changed: &mut bool) {
    ui.heading("Live candle");
    *changed |= ui
        .add(
            egui::Slider::new(&mut style.forming_opacity, 0.0..=1.0)
                .text("forming-candle opacity")
                .step_by(0.01),
        )
        .on_hover_text("multiplies body, outline and wick opacity while the candle is open")
        .changed();
}

fn draw_canvas_settings(ui: &mut egui::Ui, style: &mut ChartStyle, changed: &mut bool) {
    ui.heading("Canvas");
    *changed |= ui
        .checkbox(
            &mut style.canvas.background_enabled,
            "paint chart background",
        )
        .on_hover_text("disable to leave the plot canvas unpainted")
        .changed();
    ui.add_enabled_ui(style.canvas.background_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("background");
            *changed |= ui
                .color_edit_button_srgb(&mut style.canvas.background)
                .changed();
        });
    });

    *changed |= ui
        .checkbox(&mut style.canvas.grid_enabled, "show price grid")
        .changed();
    ui.add_enabled_ui(style.canvas.grid_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("grid colour");
            *changed |= ui.color_edit_button_srgb(&mut style.canvas.grid).changed();
        });
        *changed |= ui
            .add(
                egui::Slider::new(&mut style.canvas.grid_opacity, 0.0..=1.0)
                    .text("grid opacity")
                    .step_by(0.01),
            )
            .changed();
    });
}

fn draw_preview(ui: &mut egui::Ui, style: &ChartStyle) {
    ui.add_space(4.0);
    let width = ui.available_width().max(260.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 112.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    let background = if style.canvas.background_enabled {
        color32(style.canvas.background_rgba())
    } else {
        egui::Color32::from_rgb(12, 15, 22)
    };
    painter.rect_filled(rect, egui::Rounding::same(4.0), background);

    if let Some(grid) = style.canvas.grid_rgba() {
        for fraction in [0.25_f32, 0.5, 0.75] {
            let y = egui::lerp(rect.y_range(), fraction);
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(1.0_f32, color32(grid)),
            );
        }
    }

    // A few liquidity bands make transparency immediately visible in preview.
    for (index, alpha) in [36_u8, 54, 78, 48].into_iter().enumerate() {
        let y = rect.top() + 22.0 + index as f32 * 19.0;
        let bid = egui::Color32::from_rgba_unmultiplied(20, 185, 230, alpha);
        let ask = egui::Color32::from_rgba_unmultiplied(245, 70, 70, alpha);
        painter.line_segment(
            [
                egui::pos2(rect.left() + 12.0, y),
                egui::pos2(rect.center().x - 6.0, y),
            ],
            egui::Stroke::new(2.0_f32, bid),
        );
        painter.line_segment(
            [
                egui::pos2(rect.center().x + 6.0, y),
                egui::pos2(rect.right() - 12.0, y),
            ],
            egui::Stroke::new(2.0_f32, ask),
        );
    }

    draw_preview_candle(
        &painter,
        rect.left() + rect.width() * 0.36,
        rect.top() + 17.0,
        rect.bottom() - 14.0,
        rect.top() + 41.0,
        rect.bottom() - 35.0,
        style.candles.resolved(true, false),
    );
    draw_preview_candle(
        &painter,
        rect.left() + rect.width() * 0.64,
        rect.top() + 14.0,
        rect.bottom() - 12.0,
        rect.top() + 34.0,
        rect.bottom() - 29.0,
        style.candles.resolved(false, true),
    );

    // Aggression bubbles remain the top layer, mirroring the real chart.
    painter.circle_filled(
        egui::pos2(rect.left() + rect.width() * 0.43, rect.center().y),
        5.0,
        egui::Color32::from_rgba_unmultiplied(55, 226, 176, 210),
    );
    painter.circle_filled(
        egui::pos2(rect.left() + rect.width() * 0.69, rect.center().y + 8.0),
        6.0,
        egui::Color32::from_rgba_unmultiplied(255, 91, 105, 210),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_preview_candle(
    painter: &egui::Painter,
    x: f32,
    high: f32,
    low: f32,
    body_top: f32,
    body_bottom: f32,
    paint: ResolvedCandlePaint,
) {
    if let Some(wick) = paint.wick {
        let stroke = egui::Stroke::new(paint.wick_width, color32(wick));
        painter.line_segment([egui::pos2(x, high), egui::pos2(x, body_top)], stroke);
        painter.line_segment([egui::pos2(x, body_bottom), egui::pos2(x, low)], stroke);
    }
    let body = egui::Rect::from_min_max(
        egui::pos2(x - 15.0, body_top),
        egui::pos2(x + 15.0, body_bottom),
    );
    let radius = paint
        .corner_radius
        .min(body.width() / 2.0)
        .min(body.height() / 2.0);
    let rounding = egui::Rounding::same(radius);
    if let Some(fill) = paint.fill
        && fill[3] > 0
    {
        painter.rect_filled(body, rounding, color32(fill));
    }
    if paint.outline[3] > 0 {
        painter.rect_stroke(
            body,
            rounding,
            egui::Stroke::new(paint.outline_width, color32(paint.outline)),
        );
    }
}
