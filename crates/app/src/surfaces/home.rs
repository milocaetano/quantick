//! The Home page, and the logo its tab shares as a plate-less Q.
//!
//! A placeholder until the Market Shelf exists: the feeds and charts keep
//! running behind it.

use eframe::egui;

use crate::theme;
use quantick_ui_model::home::{Effects, Intent, Model, State};

#[derive(Default)]
pub(crate) struct View;

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "home"
    }

    fn paint(
        &mut self,
        _: &State,
        target: &mut super::ViewContext<'_>,
        _: Option<&super::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let ui = target.ui.as_deref_mut()?;
        let palette = theme::get(ui.ctx());
        let mut open = false;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - PAGE_LOGO_PX).max(0.0));
            let (logo, _) =
                ui.allocate_exact_size(egui::Vec2::splat(PAGE_LOGO_PX), egui::Sense::hover());
            paint_logo(ui, logo, Logo::Plated);
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Market Shelf not built yet").color(palette.text_muted()));
            ui.add_space(12.0);
            open = ui.button("Open a market").clicked();
        });
        open.then_some(Intent::OpenMarket)
    }
}

pub(crate) fn effects(effect: Effects) -> super::SurfaceResponse {
    super::SurfaceResponse {
        model_effects: vec![super::HostEffect::Home(effect)],
        ..Default::default()
    }
}

const PAGE_LOGO_PX: f32 = 48.0;

/// A bundled logo raster, only ever scaled. Both themes draw the same Q.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Logo {
    /// The plated logo.
    Plated,
    /// The Home tab's Q without the plate (`q-bare`'s frame).
    Q,
}

/// Each logo's texture name and bundled raster, in [`Logo`]'s order.
const LOGOS: [(&str, &[u8]); 2] = [
    ("Quantick logo", include_bytes!("../../assets/icon.png")),
    ("Quantick Q", include_bytes!("../../assets/logo-q.png")),
];

/// Paint a logo whole into `rect`: untinted, pixel-snapped, mipmapped.
pub(crate) fn paint_logo(ui: &egui::Ui, rect: egui::Rect, logo: Logo) {
    let ctx = ui.ctx();
    let id = egui::Id::new(("logo", logo));
    let cached = ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let mipmaps = Some(egui::TextureFilter::Linear);
        let options = egui::TextureOptions::LINEAR.with_mipmap_mode(mipmaps);
        let texture = ctx.load_texture(LOGOS[logo as usize].0, logo_image(logo), options);
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    let rect = painter.round_rect_to_pixels(rect);
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}

/// A logo as egui reads it: the plated one straight, the Q premultiplied in
/// sRGB, as browsers do.
pub(crate) fn logo_image(logo: Logo) -> egui::ColorImage {
    let png = eframe::icon_data::from_png_bytes(LOGOS[logo as usize].1);
    let mut icon = png.expect("bundled logo is a valid PNG");
    let size = [icon.width as usize, icon.height as usize];
    if logo == Logo::Plated {
        return egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
    }
    quantick_appearance::color::premultiply_in_srgb(&mut icon.rgba);
    egui::ColorImage::from_rgba_premultiplied(size, &icon.rgba)
}
