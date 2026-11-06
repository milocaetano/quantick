//! The Home page, and the logo its tab shares as a plate-less Q.
//!
//! A placeholder until the Market Shelf exists: the feeds and charts keep
//! running behind it.

use eframe::egui;

use crate::theme;

const PAGE_LOGO_PX: f32 = 48.0;

/// The plated logo, only ever scaled, and the Home tab's Q without it (`q-bare`'s frame).
const LOGOS: [(&str, &[u8]); 2] = [
    ("Quantick logo", include_bytes!("../../assets/icon.png")),
    ("Quantick Q", include_bytes!("../../assets/logo-q.png")),
];

/// Paint a logo, or with `mark` the Q, whole into `rect`: untinted, pixel-snapped, mipmapped.
pub(crate) fn paint_logo(ui: &egui::Ui, rect: egui::Rect, mark: bool) {
    let (ctx, id) = (ui.ctx(), egui::Id::new(("logo", mark)));
    let cached = ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let mipmaps = Some(egui::TextureFilter::Linear);
        let options = egui::TextureOptions::LINEAR.with_mipmap_mode(mipmaps);
        let texture = ctx.load_texture(LOGOS[mark as usize].0, logo_image(mark), options);
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let painter = ui.painter();
    let rect = painter.round_rect_to_pixels(rect);
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}

/// The plated logo as egui reads it; the Q premultiplied in sRGB, as browsers do.
pub(crate) fn logo_image(mark: bool) -> egui::ColorImage {
    let png = eframe::icon_data::from_png_bytes(LOGOS[mark as usize].1);
    let mut icon = png.expect("bundled logo is a valid PNG");
    let size = [icon.width as usize, icon.height as usize];
    if !mark {
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
        paint_logo(ui, logo, false);
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Market Shelf not built yet").color(palette.text_muted()));
        ui.add_space(12.0);
        open = ui.button("Open a market").clicked();
    });
    open
}
