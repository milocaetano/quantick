//! Per-egui-context conversion cache. Retain one Arc at each render boundary.
use eframe::egui;
#[cfg(test)]
use quantick_appearance::Token;
use quantick_appearance::{Appearance, FontFamily, FontRole, PremultipliedRgba, ResolvedTheme};
use std::sync::Arc;

/// Install bundled definitions once; either configurable text family retains icons.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("phosphor".into());
    ctx.set_fonts(fonts);
}

pub struct Palette {
    pub resolved: ResolvedTheme,
    colors: quantick_appearance::RenderPalette<egui::Color32>,
    pub footprint_text: quantick_chart::footprint_lod::typography::FootprintText,
    pub loading_logo: [egui::Color32; 256],
    pub heat_buy: [egui::Color32; 6],
    pub heat_sell: [egui::Color32; 6],
    pub heat_ink: [egui::Color32; 6],
}

impl std::ops::Deref for Palette {
    type Target = quantick_appearance::RenderPalette<egui::Color32>;
    fn deref(&self) -> &Self::Target {
        &self.colors
    }
}

impl Palette {
    fn new(resolved: &ResolvedTheme) -> Self {
        Self {
            footprint_text: quantick_chart::footprint_lod::typography::FootprintText::new(
                &resolved.typography,
            ),
            loading_logo: quantick_chart::logo_shatter::palette(resolved).map(rendered),
            resolved: resolved.clone(),
            colors: quantick_appearance::RenderPalette::from_fn(|token| {
                rendered(resolved.color(token))
            }),
            heat_buy: resolved.footprint_heat.buy.map(rendered),
            heat_sell: resolved.footprint_heat.sell.map(rendered),
            heat_ink: resolved.footprint_heat.ink.map(rendered),
        }
    }
    #[cfg(test)]
    pub fn color(&self, token: Token) -> egui::Color32 {
        *self.colors.get(token)
    }
    pub fn footprint_font(&self, row_height: f32, width: f32) -> egui::FontId {
        let mut font = self.font(FontRole::Footprint);
        font.size = self.footprint_text.fitted_size(row_height, width);
        font
    }
    pub fn label_space(&self, size: f32) -> f32 {
        self.resolved.typography.reserve(FontRole::ChartLabel, size)
    }
    pub fn layout(&self, size: f32) -> f32 {
        size * self.resolved.typography.layout_scale()
    }
    pub fn body_font(&self, size: f32) -> egui::FontId {
        self.font_at(FontRole::Body, size)
    }
    pub fn mono_font(&self, size: f32) -> egui::FontId {
        self.font_at(FontRole::Monospace, size)
    }
    pub fn axis_font(&self, size: f32) -> egui::FontId {
        self.font_at(FontRole::ChartAxis, size)
    }
    pub fn label_font(&self, size: f32) -> egui::FontId {
        self.font_at(FontRole::ChartLabel, size)
    }
    pub fn font(&self, role: FontRole) -> egui::FontId {
        rendered_font(self.resolved.typography.get(role))
    }
    pub fn font_at(&self, role: FontRole, reference_size: f32) -> egui::FontId {
        let font = self.resolved.typography.at_size(role, reference_size);
        rendered_font(font)
    }
}

fn rendered_font(font: quantick_appearance::FontSpec) -> egui::FontId {
    egui::FontId::new(
        font.size,
        match font.family {
            FontFamily::Proportional => egui::FontFamily::Proportional,
            FontFamily::Monospace => egui::FontFamily::Monospace,
        },
    )
}

fn rendered(color: PremultipliedRgba) -> egui::Color32 {
    let [r, g, b, a] = color.to_array();
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

#[derive(Clone)]
struct Installed {
    revision: u64,
    palette: Arc<Palette>,
}
fn key() -> egui::Id {
    egui::Id::new("quantick.resolved_appearance")
}

/// One context lock at the render boundary, never inside a cell or token lookup.
pub fn get(ctx: &egui::Context) -> Arc<Palette> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_insert_with(key(), || Installed {
            revision: u64::MAX,
            palette: Arc::new(Palette::new(ResolvedTheme::bundled_dark())),
        })
        .palette
        .clone()
    })
}

/// Called once per frame; conversion and style changes happen only on revision.
/// Font definitions (including Phosphor and fallback registrations) stay intact.
pub fn sync(ctx: &egui::Context, state: &Appearance) {
    let installed = ctx.data(|data| data.get_temp::<Installed>(key()));
    if installed
        .as_ref()
        .is_some_and(|cached| cached.revision == state.revision())
    {
        return;
    }
    let palette = Arc::new(Palette::new(state.resolved()));
    let typography_changed = installed.as_ref().is_none_or(|prior| {
        prior.revision == u64::MAX
            || prior.palette.resolved.typography != palette.resolved.typography
    });
    super::chrome::apply_palette(ctx, &palette, typography_changed);
    ctx.data_mut(|data| {
        data.insert_temp(
            key(),
            Installed {
                revision: state.revision(),
                palette,
            },
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_sync_installs_text_roles_even_after_an_early_palette_read() {
        let ctx = egui::Context::default();
        let early = get(&ctx);
        ctx.style_mut(|style| {
            style
                .text_styles
                .get_mut(&egui::TextStyle::Body)
                .unwrap()
                .size = 77.0;
        });
        sync(&ctx, &Appearance::default());
        assert_eq!(
            ctx.style().text_styles[&egui::TextStyle::Body],
            early.font(FontRole::Body)
        );
    }

    #[test]
    fn configured_painter_fonts_and_axis_reservations_grow_together() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut state = Appearance::default();
        sync(&ctx, &state);
        let dark = get(&ctx);
        let mut original_width = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            original_width = ctx.fonts(|fonts| {
                fonts
                    .layout_no_wrap("12345.67".into(), dark.axis_font(11.0), dark.text_primary())
                    .size()
                    .x
            });
        });
        state
            .apply("version = 1\nname = 'Large'\n[typography.chart_axis]\nsize = 22")
            .unwrap();
        sync(&ctx, &state);
        let large = get(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let width = ctx.fonts(|fonts| {
                fonts
                    .layout_no_wrap(
                        "12345.67".into(),
                        large.axis_font(11.0),
                        large.text_primary(),
                    )
                    .size()
                    .x
            });
            assert!(width > original_width * 1.8);
            assert!(
                width < quantick_appearance::AxisMetrics::new(&large.resolved.typography).gutter
            );
        });
    }

    #[test]
    fn contexts_are_isolated_and_repeated_frames_reuse_the_adapter() {
        let dark_ctx = egui::Context::default();
        let light_ctx = egui::Context::default();
        let mut light = Appearance::default();
        light.apply(quantick_appearance::LIGHT).unwrap();
        sync(&dark_ctx, &Appearance::default());
        sync(&light_ctx, &light);
        assert_ne!(
            get(&dark_ctx).color(Token::Chrome),
            get(&light_ctx).color(Token::Chrome)
        );
        let before = get(&light_ctx);
        sync(&light_ctx, &light);
        assert!(Arc::ptr_eq(&before, &get(&light_ctx)));
        assert!(light.apply("version = 99\nname = 'invalid'").is_err());
        sync(&light_ctx, &light);
        assert!(Arc::ptr_eq(&before, &get(&light_ctx)));
    }
    #[test]
    fn typography_updates_preserve_icon_font_definitions() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut state = Appearance::default();
        sync(&ctx, &state);
        let original = get(&ctx);
        state
            .apply(
                "version = 1\nname = 'Larger'\n[typography.body]\nsize = 18\nfamily = 'monospace'",
            )
            .unwrap();
        sync(&ctx, &state);
        assert!(!Arc::ptr_eq(&original, &get(&ctx)));
        assert_eq!(ctx.style().text_styles[&egui::TextStyle::Body].size, 18.0);
        // No set_fonts call is made by the adapter; this named fallback remains registered.
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(egui_phosphor::regular::GEAR);
                let font = &ctx.style().text_styles[&egui::TextStyle::Body];
                assert_eq!(font.family, egui::FontFamily::Monospace);
                ctx.fonts(|fonts| {
                    assert!(fonts.has_glyphs(font, egui_phosphor::regular::GEAR));
                    assert!(fonts.has_glyphs(font, "\u{0394}\u{00b1}"));
                });
            });
        });
    }
}
