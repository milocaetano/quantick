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
        rendered_font(self.resolved.typography.at_size(role, reference_size))
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

pub(super) fn rendered(color: PremultipliedRgba) -> egui::Color32 {
    let [r, g, b, a] = color.to_array();
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

#[derive(Clone)]
struct Palettes {
    ui: Arc<Palette>,
    bookmap: Arc<Palette>,
}
impl Palettes {
    fn new(theme: &ResolvedTheme) -> Self {
        let ui = Arc::new(Palette::new(theme));
        let bookmap = if std::ptr::eq(theme, theme.bookmap()) {
            ui.clone()
        } else {
            Arc::new(Palette::new(theme.bookmap()))
        };
        Self { ui, bookmap }
    }
}
type Installed = quantick_appearance::RenderCache<Palettes>;
fn key() -> egui::Id {
    egui::Id::new("quantick.resolved_appearance")
}
fn cached(ctx: &egui::Context) -> Installed {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_insert_with(key(), || {
            Installed::new(Palettes::new(ResolvedTheme::bundled_dark()))
        })
        .clone()
    })
}

/// One context lock per boundary. Warm frames reuse converted colors and fonts.
pub fn get(ctx: &egui::Context) -> Arc<Palette> {
    ctx.data_mut(|data| {
        data.get_temp::<Arc<Palette>>(surface_key())
            .unwrap_or_else(|| {
                data.get_temp_mut_or_insert_with(key(), || {
                    Installed::new(Palettes::new(ResolvedTheme::bundled_dark()))
                })
                .value
                .ui
                .clone()
            })
    })
}

fn surface_key() -> egui::Id {
    egui::Id::new("quantick.paint_surface")
}

/// Scoped painter colors only: never changes widget visuals, fonts or revisions.
/// The guard restores the previous scope even on an empty pane's early return.
pub struct PaintScope {
    ctx: egui::Context,
    previous: Option<Arc<Palette>>,
}
impl Drop for PaintScope {
    fn drop(&mut self) {
        self.ctx.data_mut(|data| {
            if let Some(previous) = self.previous.take() {
                data.insert_temp(surface_key(), previous);
            } else {
                data.remove::<Arc<Palette>>(surface_key());
            }
        });
    }
}

pub fn bookmap(ctx: &egui::Context) -> Arc<Palette> {
    cached(ctx).value.bookmap
}

pub fn bookmap_scope(ctx: &egui::Context) -> PaintScope {
    let palette = bookmap(ctx);
    let previous = ctx.data_mut(|data| {
        let previous = data.get_temp::<Arc<Palette>>(surface_key());
        data.insert_temp(surface_key(), palette);
        previous
    });
    PaintScope {
        ctx: ctx.clone(),
        previous,
    }
}

pub fn sync(ctx: &egui::Context, state: &Appearance) {
    let mut installed = cached(ctx);
    if let Some(typography_changed) = installed.refresh(state, Palettes::new) {
        super::chrome::apply_palette(ctx, &installed.value.ui, typography_changed);
        ctx.data_mut(|data| data.insert_temp(key(), installed));
    }
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
                "version = 1\nname = 'Modest'\n[typography.body]\nsize = 13.5\nfamily = 'monospace'",
            )
            .unwrap();
        sync(&ctx, &state);
        assert!(!Arc::ptr_eq(&original, &get(&ctx)));
        assert_eq!(ctx.style().text_styles[&egui::TextStyle::Body].size, 13.5);
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

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[test]
    fn nested_paint_scopes_restore_ui_and_do_not_leak_between_contexts() {
        let ctx = egui::Context::default();
        let other = egui::Context::default();
        let mut state = Appearance::default();
        state.apply(quantick_appearance::LIGHT).unwrap();
        sync(&ctx, &state);
        sync(&other, &state);
        let root = get(&ctx);
        let surface = bookmap(&ctx);
        let visuals = ctx.style().visuals.clone();
        {
            let _outer = bookmap_scope(&ctx);
            assert!(Arc::ptr_eq(&get(&ctx), &surface));
            assert_eq!(get(&other).canvas(), root.canvas());
            {
                let _inner = bookmap_scope(&ctx);
            }
            assert!(Arc::ptr_eq(&get(&ctx), &surface));
            assert_eq!(ctx.style().visuals, visuals);
        }
        assert!(Arc::ptr_eq(&get(&ctx), &root));
        sync(&ctx, &state);
        assert!(Arc::ptr_eq(&bookmap(&ctx), &surface));
        state.apply(quantick_appearance::DARK).unwrap();
        sync(&ctx, &state);
        assert!(!Arc::ptr_eq(&bookmap(&ctx), &surface));
        assert_eq!(
            get(&ctx).canvas(),
            egui::Color32::from_rgb(0x13, 0x17, 0x22)
        );
        assert_ne!(get(&ctx).canvas(), surface.canvas());
        assert_eq!(get(&other).canvas(), root.canvas());
    }
}
