//! Per-egui-context conversion cache. Retain one Arc at each render boundary.
use eframe::egui;
#[cfg(test)]
use quantick_appearance::Token;
use quantick_appearance::{
    Appearance, ChartSurface, FontFamily, FontRole, PremultipliedRgba, ResolvedTheme,
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

#[cfg(test)]
impl Palette {
    pub fn color(&self, token: Token) -> egui::Color32 {
        *self.colors.get(token)
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
    lit_flow: Option<Arc<Palette>>,
    title_strip: Arc<Palette>,
}
impl Palettes {
    fn new(theme: &ResolvedTheme) -> Self {
        let ui = Arc::new(Palette::new(theme));
        let surface = |resolved: &ResolvedTheme| {
            if std::ptr::eq(theme, resolved) {
                ui.clone()
            } else {
                Arc::new(Palette::new(resolved))
            }
        };
        let lit = theme.surface(ChartSurface::LitFlow);
        Self {
            bookmap: surface(theme.bookmap()),
            lit_flow: theme.has_lit_flow().then(|| surface(lit)),
            title_strip: surface(theme.title_strip()),
            ui: ui.clone(),
        }
    }
}
type Installed = quantick_appearance::RenderCache<Palettes>;
fn key() -> egui::Id {
    egui::Id::new("quantick.resolved_appearance")
}
fn cached(data: &mut egui::util::IdTypeMap) -> &mut Installed {
    data.get_temp_mut_or_insert_with(key(), || {
        Installed::new(Palettes::new(ResolvedTheme::bundled_dark()))
    })
}

/// One context lock per boundary. Warm frames reuse converted colors and fonts.
pub fn get(ctx: &egui::Context) -> Arc<Palette> {
    ctx.data_mut(|data| {
        data.get_temp::<Arc<Palette>>(surface_key())
            .unwrap_or_else(|| cached(data).value.ui.clone())
    })
}

/// Light axes use interface ink; Dark retains the authored plot palette.
pub fn axes(ctx: &egui::Context) -> Arc<Palette> {
    ctx.data_mut(|data| {
        let root = cached(data).value.ui.clone();
        data.get_temp::<Arc<Palette>>(surface_key())
            .filter(|_| root.resolved.dark)
            .unwrap_or(root)
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
    ctx.data_mut(|data| cached(data).value.bookmap.clone())
}

#[cfg(test)]
pub fn bookmap_scope(ctx: &egui::Context) -> PaintScope {
    scope(ctx, bookmap(ctx))
}

/// The painter colours of a chart pane's surface: paper, the island or lit.
pub fn surface(ctx: &egui::Context, surface: ChartSurface) -> Arc<Palette> {
    ctx.data_mut(|data| {
        let palettes = &cached(data).value;
        let lit = palettes.lit_flow.as_ref();
        match surface {
            ChartSurface::Paper => palettes.ui.clone(),
            ChartSurface::Flow => palettes.bookmap.clone(),
            ChartSurface::LitFlow => lit.unwrap_or(&palettes.bookmap).clone(),
        }
    })
}

/// The title strip's painter colors (logo, tabs, `+`, menu, window buttons).
pub fn title_strip_scope(ctx: &egui::Context) -> PaintScope {
    let palette = ctx.data_mut(|data| cached(data).value.title_strip.clone());
    scope(ctx, palette)
}

pub fn scope(ctx: &egui::Context, palette: Arc<Palette>) -> PaintScope {
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
    let mut installed = ctx.data_mut(|data| cached(data).clone());
    if let Some(typography_changed) = installed.refresh(state, Palettes::new) {
        super::chrome::apply_palette(ctx, &installed.value.ui, typography_changed);
        ctx.data_mut(|data| data.insert_temp(key(), installed));
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod surface_tests;
