//! The Home page, and the logo its tab shares as a plate-less Q.
//!
//! A placeholder until the Market Shelf exists: the feeds and charts keep
//! running behind it.

use eframe::egui;

use crate::theme;

const PAGE_LOGO_PX: f32 = 48.0;

/// The plated logo, only ever scaled, and for the Home tab its own Q without the
/// plate (`logo-q.png`, framed as the design's `q-bare` viewBox).
const LOGOS: [&[u8]; 2] = [
    include_bytes!("../../assets/icon.png"),
    include_bytes!("../../assets/logo-q.png"),
];

/// Paint the logo, or with `mark` the Q alone, whole into `rect`, untinted in
/// every state; mipmaps keep the large raster sharp at tab size.
pub(crate) fn paint_logo(ui: &egui::Ui, rect: egui::Rect, mark: bool) {
    let id = egui::Id::new(("logo", mark));
    let texture = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id))
        .unwrap_or_else(|| {
            let icon = eframe::icon_data::from_png_bytes(LOGOS[mark as usize])
                .expect("bundled logo is a valid PNG");
            let size = [icon.width as usize, icon.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
            let options = egui::TextureOptions {
                mipmap_mode: Some(egui::TextureFilter::Linear),
                ..egui::TextureOptions::LINEAR
            };
            let texture = ui.ctx().load_texture("Quantick logo", image, options);
            ui.ctx()
                .data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    ui.painter()
        .image(texture.id(), rect, uv, egui::Color32::WHITE);
}

/// Draw the page. Returns whether "Open a market" was pressed.
pub(crate) fn draw_page(ui: &mut egui::Ui) -> bool {
    let mut open = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - PAGE_LOGO_PX).max(0.0));
        let (logo, _) =
            ui.allocate_exact_size(egui::Vec2::splat(PAGE_LOGO_PX), egui::Sense::hover());
        paint_logo(ui, ui.painter().round_rect_to_pixels(logo), false);
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Market Shelf not built yet").color(theme::TEXT_MUTED));
        ui.add_space(12.0);
        open = ui.button("Open a market").clicked();
    });
    open
}
