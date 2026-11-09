//! Local treatments for enabled supporting text and editable preset names.
use eframe::egui;

/// Preserve Dark's authored weak ink; Light's enabled hints need opaque ink.
pub fn supporting_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    let text = egui::RichText::new(text);
    let palette = super::get(ui.ctx());
    if palette.resolved.dark {
        text.weak()
    } else {
        text.color(palette.text_support())
    }
}

/// Consequential prose uses the body role and wraps even inside horizontal rows.
pub fn supporting_label(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let palette = super::get(ui.ctx());
    let ink = if palette.resolved.dark {
        palette.text_muted()
    } else {
        palette.text_support()
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .color(ink)
                .text_style(egui::TextStyle::Body),
        )
        .wrap(),
    )
}

/// A local border advertises editing without changing any disabled-state opacity.
pub fn preset_input(ui: &mut egui::Ui, edit: egui::TextEdit<'_>) -> egui::Response {
    let palette = super::get(ui.ctx());
    ui.scope(|ui| {
        if !palette.resolved.dark {
            let border = egui::Stroke::new(1.0_f32, palette.text_support());
            ui.visuals_mut().widgets.inactive.bg_stroke = border;
            ui.visuals_mut().widgets.hovered.bg_stroke = border;
        }
        ui.add(edit)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_appearance::{Appearance, PremultipliedRgba, color::RgbaExt, contrast};

    #[test]
    fn light_supporting_text_preserves_state_hierarchy() {
        for (source, width, scale) in [quantick_appearance::LIGHT, quantick_appearance::DARK]
            .into_iter()
            .flat_map(|source| {
                [1000.0, 1280.0].into_iter().flat_map(move |width| {
                    [1.0, 1.25, 1.5, 2.0].map(|scale| (source, width, scale))
                })
            })
        {
            let ctx = egui::Context::default();
            let mut appearance = Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let palette = crate::theme::get(&ctx);
            let original = ctx.style().visuals.clone();
            let mut name = String::new();
            for (frame, selected) in [false, true, true, false].into_iter().enumerate() {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 780.0),
                    )),
                    events: if frame == 3 {
                        vec![egui::Event::Text("Readable preset".into())]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                };
                input
                    .viewports
                    .entry(egui::ViewportId::ROOT)
                    .or_default()
                    .native_pixels_per_point = Some(scale);
                let output = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let text = supporting_text(ui, "Ctrl+S");
                        ui.add(
                            egui::Button::new("Save")
                                .shortcut_text(text)
                                .selected(selected),
                        );
                        let hint = supporting_text(ui, "Preset name");
                        let response =
                            preset_input(ui, egui::TextEdit::singleline(&mut name).hint_text(hint));
                        assert!(response.enabled());
                        if selected {
                            response.request_focus();
                        }
                        let text = supporting_text(ui, "Unavailable");
                        assert!(!ui.add_enabled(false, egui::Button::new(text)).enabled());
                    });
                });
                assert_eq!(
                    ctx.style().visuals,
                    original,
                    "local treatments must not leak"
                );
                if frame == 3 {
                    assert_eq!(
                        name, "Readable preset",
                        "a focused preset field remains editable"
                    );
                }
                for text in ["Ctrl+S", "Preset name"]
                    .into_iter()
                    .filter(|text| *text != "Preset name" || name.is_empty())
                {
                    let painted = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(painted) if painted.galley.job.text == text => {
                                Some(painted)
                            }
                            _ => None,
                        })
                        .expect("the active supporting text is painted");
                    let ink = painted
                        .override_text_color
                        .unwrap_or(painted.galley.rows[0].visuals.mesh.vertices[0].color);
                    assert_eq!(painted.opacity_factor, 1.0, "active guidance is not faded");
                    if palette.resolved.dark {
                        assert_eq!(ink, original.weak_text_color());
                    } else {
                        assert_eq!(ink.a(), 255);
                        let bytes = |color: egui::Color32| {
                            PremultipliedRgba::from_premultiplied(color.to_array())
                        };
                        let background = if text == "Preset name" {
                            palette.inset()
                        } else if selected {
                            let composite = contrast::over(
                                bytes(original.selection.bg_fill),
                                bytes(palette.chrome()),
                            );
                            egui::Color32::from_rgb(composite.r(), composite.g(), composite.b())
                        } else {
                            palette.control()
                        };
                        assert!(
                            contrast::contrast_ratio(bytes(ink), bytes(background)) >= 4.5,
                            "{text} on {background:?}"
                        );
                    }
                }
                let disabled = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == "Unavailable" => {
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("disabled action remains visible");
                assert!(
                    disabled.galley.rows[0].visuals.mesh.vertices[0].color
                        != disabled.galley.job.sections[0].format.color,
                    "disabled ink retains the toolkit fade"
                );
                if !palette.resolved.dark {
                    assert!(
                        output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Rect(rect) if rect.stroke.width > 0.0 && [palette.text_support(), palette.accent()].contains(&rect.stroke.color))),
                        "the editable field has a visible boundary"
                    );
                }
            }
        }
    }
}
