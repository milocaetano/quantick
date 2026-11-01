//! The Home page, and the logo its tab shares.
//!
//! A placeholder until the Market Shelf exists: the feeds and charts keep
//! running behind it.

use eframe::egui;

use crate::theme;

const PAGE_LOGO_PX: f32 = 48.0;
/// Physical size of the small raster, drawn pixel for pixel at 100% scale.
const SMALL_LOGO_PX: f32 = 16.0;

/// Paint the solid logo into `rect` at `opacity`. The tint is the only opacity
/// control, so the raster stays sharp on a whole-pixel rectangle.
pub(crate) fn paint_logo(ui: &egui::Ui, rect: egui::Rect, opacity: f32) {
    let small = rect.width() * ui.ctx().pixels_per_point() <= SMALL_LOGO_PX + 0.5;
    let (key, png): (_, &[u8]) = if small {
        (
            "Quantick logo 16",
            include_bytes!("../../assets/icon-solid-16.png"),
        )
    } else {
        (
            "Quantick logo 48",
            include_bytes!("../../assets/icon-solid-48.png"),
        )
    };
    let id = egui::Id::new(key);
    let texture = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id))
        .unwrap_or_else(|| {
            let icon = eframe::icon_data::from_png_bytes(png).expect("bundled logo is a valid PNG");
            let size = [icon.width as usize, icon.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
            let options = egui::TextureOptions::LINEAR;
            let texture = ui.ctx().load_texture(key, image, options);
            ui.ctx()
                .data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    ui.painter().image(
        texture.id(),
        ui.painter().round_rect_to_pixels(rect),
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE.gamma_multiply(opacity),
    );
}

/// Draw the page. Returns whether "Open a market" was pressed.
pub(crate) fn draw_page(ui: &mut egui::Ui) -> bool {
    let mut open = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - PAGE_LOGO_PX).max(0.0));
        let (logo, _) =
            ui.allocate_exact_size(egui::Vec2::splat(PAGE_LOGO_PX), egui::Sense::hover());
        paint_logo(ui, logo, 1.0);
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Market Shelf not built yet").color(theme::TEXT_MUTED));
        ui.add_space(12.0);
        open = ui.button("Open a market").clicked();
    });
    open
}
