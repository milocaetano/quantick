//! The Home page, and the logo its tab shares as a plate-less Q.
//!
//! A placeholder until the Market Shelf exists: the feeds and charts keep
//! running behind it.

use eframe::egui;

use crate::theme;

const PAGE_LOGO_PX: f32 = 48.0;

/// A bundled logo raster, only ever scaled, or the Light Q drawn per scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Logo {
    /// The plated logo.
    Plated,
    /// The Home tab's Q without the plate (`q-bare`'s frame).
    Q,
    /// The Light theme's Q (`quantick_appearance::mark`): white faces, black
    /// edges, rasterised at the exact physical size it is drawn at.
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
            Self::QLight => unreachable!("the Light Q is rasterised per scale"),
        }
    }
}

/// Paint a logo whole into `rect`: untinted, pixel-snapped, mipmapped; the
/// Light Q rasterised for this scale instead, one texel per pixel.
pub(crate) fn paint_logo(ui: &egui::Ui, rect: egui::Rect, logo: Logo) {
    use quantick_appearance::mark;
    let (ctx, ppp) = (ui.ctx(), ui.ctx().pixels_per_point());
    let light = logo == Logo::QLight;
    let side = mark::side_px(ppp);
    let id = egui::Id::new(("logo", logo, light.then_some(side)));
    let cached = ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let (name, image, options) = if light {
            let texels: Vec<u8> = mark::light_q(side).into_iter().flatten().collect();
            let image = egui::ColorImage::from_rgba_premultiplied([side as usize; 2], &texels);
            ("Quantick Q light", image, egui::TextureOptions::NEAREST)
        } else {
            let mipmaps = Some(egui::TextureFilter::Linear);
            let options = egui::TextureOptions::LINEAR.with_mipmap_mode(mipmaps);
            (logo.source().0, logo_image(logo), options)
        };
        let texture = ctx.load_texture(name, image, options);
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    let rect = if light {
        let [left, top, size] = mark::light_q_box(rect.center().into(), rect.width(), ppp);
        egui::Rect::from_min_size(egui::pos2(left, top), egui::Vec2::splat(size))
    } else {
        painter.round_rect_to_pixels(rect)
    };
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
