//! The Home page, and the logo its tab shares as a plate-less Q.
//!
//! A placeholder until the Market Shelf exists: the feeds and charts keep
//! running behind it.

use eframe::egui;

use crate::theme;

const PAGE_LOGO_PX: f32 = 48.0;

/// A bundled logo raster, only ever scaled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Logo {
    /// The plated logo.
    Plated,
    /// The Home tab's Q without the plate (`q-bare`'s frame).
    Q,
    /// The Light theme's Q, same frame: white faces and black edges
    /// (`tools/logo/light_mark.py`).
    QLight,
}

impl Logo {
    /// The Home tab's Q for a theme: the edged one on a light strip.
    pub(crate) fn mark(dark: bool) -> Self {
        if dark { Self::Q } else { Self::QLight }
    }

    fn source(self) -> (&'static str, &'static [u8]) {
        match self {
            Self::Plated => ("Quantick logo", include_bytes!("../../assets/icon.png")),
            Self::Q => ("Quantick Q", include_bytes!("../../assets/logo-q.png")),
            Self::QLight => (
                "Quantick Q light",
                include_bytes!("../../assets/logo-q-light.png"),
            ),
        }
    }
}

/// Paint a logo whole into `rect`: untinted, pixel-snapped, mipmapped.
pub(crate) fn paint_logo(ui: &egui::Ui, rect: egui::Rect, logo: Logo) {
    let (ctx, id) = (ui.ctx(), egui::Id::new(("logo", logo)));
    let cached = ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let mipmaps = Some(egui::TextureFilter::Linear);
        let options = egui::TextureOptions::LINEAR.with_mipmap_mode(mipmaps);
        let texture = ctx.load_texture(logo.source().0, logo_image(logo), options);
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    let rect = painter.round_rect_to_pixels(rect);
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}

/// The plated logo as egui reads it; a Q premultiplied in sRGB, as browsers do.
pub(crate) fn logo_image(logo: Logo) -> egui::ColorImage {
    let png = eframe::icon_data::from_png_bytes(logo.source().1);
    let mut icon = png.expect("bundled logo is a valid PNG");
    let size = [icon.width as usize, icon.height as usize];
    if logo == Logo::Plated {
        return egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
    }
    for texel in icon.rgba.as_chunks_mut::<4>().0 {
        let [r, g, b, a] = texel.map(u16::from);
        *texel = [r * a, g * a, b * a, a * 255].map(|c| ((c + 127) / 255) as u8);
    }
    egui::ColorImage::from_rgba_premultiplied(size, &icon.rgba)
}

/// Draw the page. Returns whether "Open a market" was pressed.
pub(crate) fn draw_page(ui: &mut egui::Ui) -> bool {
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
    open
}
