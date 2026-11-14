//! Real inspector hosts must stay narrow even when the chart note is wide.
#![cfg(test)]
use super::*;
use crate::drawings::{ChartPoint, DrawingTool, Drawings, StyleColor32};
use crate::pane::PaneSide;
use crate::surfaces::drawing_chrome::SelectedDrawing;
use crate::toolrail::ToolboxDock;
use quantick_drawings::{FontFamily, NullPresetHost, TextPayload, text::MAX_WRAP_PX};
use quantick_ui_model::drawing_chrome::InspectorTab;

#[test]
fn floating_text_inspector_bounds_long_content_and_keeps_font_style() {
    inspect_long_text(false);
}

#[test]
fn pinned_text_inspector_bounds_long_content_and_keeps_font_style() {
    inspect_long_text(true);
}

fn inspect_long_text(pinned: bool) {
    for wrap in [None, Some(MAX_WRAP_PX)] {
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let ctx = egui::Context::default();
                let mut fonts = egui::FontDefinitions::default();
                crate::drawings::text::install_fonts(&mut fonts);
                ctx.set_fonts(fonts);
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0));
                let mut drawings = Drawings::default();
                drawings.place(
                    DrawingTool::by_id("text").unwrap(),
                    ChartPoint::at(1.0, 100.0),
                );
                let drawing = drawings.selected_mut().unwrap();
                let payload = TextPayload {
                    text: "W".repeat(200),
                    family,
                    appearance: quantick_drawings::text::TextAppearance {
                        bold,
                        italic,
                        wrap_width_px: wrap,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let font = crate::drawings::text::font_id(payload.format());
                let words = payload.text.clone();
                drawing.payload = Box::new(payload);
                let color = egui::Color32::from_rgb(170, 80, 140);
                drawing.style.set_color32(color);
                let env = DrawingEnv {
                    linked_alerts: 0,
                    pane_id: 1,
                    selected: Some(SelectedDrawing { index: 0, drawing }),
                    chart_area: Some(screen),
                    automatic_bar_area: None,
                    focused_chart_area: Some(screen),
                    candles_right: None,
                    legends: None,
                    auto_range: None,
                    selected_bbox: None,
                    selected_band: None,
                    tab: 1,
                    side: PaneSide::Flow,
                    drawing_tool_armed: false,
                    toolbox_dock: ToolboxDock::default(),
                    authored_objects: 0,
                    manager_rows: &[],
                    presets: &NullPresetHost,
                };
                let mut owner = crate::surfaces::drawing_chrome::new_owner();
                let mut chrome = crate::surfaces::drawing_chrome::borrow(&mut owner);
                chrome.set_inspector_open(true);
                chrome.set_inspector_pinned(pinned);
                chrome.set_inspector_tab(InspectorTab::Extra);
                for frame in 0..3 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ctx| {
                            let width = if pinned {
                                draw_pinned(&mut chrome, ctx, &env);
                                screen.width() - ctx.available_rect().width()
                            } else {
                                draw_floating(&mut chrome, ctx, &env);
                                chrome.view.inspector.size.unwrap().x
                            };
                            assert!(
                                width > 0.0 && width <= INSPECTOR_MAX_WIDTH_PX + 1.0,
                                "host expanded to {width} px: pinned={pinned}, wrap={wrap:?}"
                            );
                        },
                    );
                    // egui's first floating frame is an invisible sizing pass.
                    if frame == 0 {
                        continue;
                    }
                    let galley = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(shape) if shape.galley.job.text == words => {
                                Some(&shape.galley)
                            }
                            _ => None,
                        })
                        .expect("the inspector paints the note's editor");
                    assert!(
                        galley.rows.len() > 1,
                        "the inspector wraps long content locally"
                    );
                    assert!(galley.size().x <= INSPECTOR_MAX_WIDTH_PX);
                    let section = &galley.job.sections[0].format;
                    assert_eq!(section.font_id, font);
                    assert_eq!(section.color, color);
                }
                assert_eq!(
                    drawing
                        .payload
                        .as_any()
                        .downcast_ref::<TextPayload>()
                        .unwrap()
                        .appearance
                        .wrap_width_px,
                    wrap,
                    "inspector layout never rewrites chart wrap"
                );
            }
        }
    }
}
