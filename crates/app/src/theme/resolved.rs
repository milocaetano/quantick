//! Per-egui-context conversion cache. Retain one Arc at each render boundary.
use eframe::egui;
use quantick_appearance::{
    Appearance, FontFamily, FontRole, PremultipliedRgba, ResolvedTheme, Token,
};
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
            resolved: resolved.clone(),
            colors: quantick_appearance::RenderPalette::from_fn(|token| {
                rendered(resolved.color(token))
            }),
            heat_buy: resolved.footprint_heat.buy.map(rendered),
            heat_sell: resolved.footprint_heat.sell.map(rendered),
            heat_ink: resolved.footprint_heat.ink.map(rendered),
        }
    }
    pub fn color(&self, token: Token) -> egui::Color32 {
        *self.colors.get(token)
    }
    pub fn font(&self, role: FontRole) -> egui::FontId {
        let font = self.resolved.typography.get(role);
        egui::FontId::new(
            font.size,
            match font.family {
                FontFamily::Proportional => egui::FontFamily::Proportional,
                FontFamily::Monospace => egui::FontFamily::Monospace,
            },
        )
    }
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
    super::chrome::apply_palette(ctx, &palette);
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
