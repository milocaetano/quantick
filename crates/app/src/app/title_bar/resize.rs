//! Native resize gestures on the undecorated viewport's outer border.
use eframe::egui;

const BORDER: f32 = 4.0;
const CORNER: f32 = 12.0;

pub(super) fn draw(ctx: &egui::Context) {
    if ctx.input(|input| {
        let viewport = input.viewport();
        viewport.maximized.unwrap_or(false)
            || viewport.minimized.unwrap_or(false)
            || viewport.fullscreen.unwrap_or(false)
    }) {
        return;
    }
    let bounds = ctx.screen_rect();
    let x = [
        bounds.left(),
        bounds.left() + CORNER,
        bounds.right() - CORNER,
        bounds.right(),
    ];
    let y = [
        bounds.top(),
        bounds.top() + CORNER,
        bounds.bottom() - CORNER,
        bounds.bottom(),
    ];
    use egui::CursorIcon as C;
    use egui::viewport::ResizeDirection as D;
    // Corners are L-shaped: their longer reach stays inside the outer border,
    // leaving the interiors of title buttons and chart widgets untouched.
    let regions = [
        (
            D::NorthWest,
            C::ResizeNwSe,
            [x[0], y[0], x[1], y[0] + BORDER],
        ),
        (
            D::North,
            C::ResizeVertical,
            [x[1], y[0], x[2], y[0] + BORDER],
        ),
        (
            D::NorthEast,
            C::ResizeNeSw,
            [x[2], y[0], x[3], y[0] + BORDER],
        ),
        (
            D::NorthWest,
            C::ResizeNwSe,
            [x[0], y[0] + BORDER, x[0] + BORDER, y[1]],
        ),
        (
            D::NorthEast,
            C::ResizeNeSw,
            [x[3] - BORDER, y[0] + BORDER, x[3], y[1]],
        ),
        (
            D::West,
            C::ResizeHorizontal,
            [x[0], y[1], x[0] + BORDER, y[2]],
        ),
        (
            D::East,
            C::ResizeHorizontal,
            [x[3] - BORDER, y[1], x[3], y[2]],
        ),
        (
            D::SouthWest,
            C::ResizeNeSw,
            [x[0], y[2], x[0] + BORDER, y[3] - BORDER],
        ),
        (
            D::SouthEast,
            C::ResizeNwSe,
            [x[3] - BORDER, y[2], x[3], y[3] - BORDER],
        ),
        (
            D::SouthWest,
            C::ResizeNeSw,
            [x[0], y[3] - BORDER, x[1], y[3]],
        ),
        (
            D::South,
            C::ResizeVertical,
            [x[1], y[3] - BORDER, x[2], y[3]],
        ),
        (
            D::SouthEast,
            C::ResizeNwSe,
            [x[2], y[3] - BORDER, x[3], y[3]],
        ),
    ];
    for (index, (direction, cursor, [left, top, right, bottom])) in regions.into_iter().enumerate()
    {
        let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom))
            .intersect(bounds);
        if !rect.is_positive() {
            continue;
        }
        egui::Area::new(egui::Id::new(("native_resize_border", index)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .movable(false)
            .constrain(false)
            .sense(egui::Sense::hover())
            .show(ctx, |ui| {
                ui.set_clip_rect(rect);
                let response = ui.allocate_rect(rect, egui::Sense::drag());
                let response = response.on_hover_cursor(cursor);
                if response.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            });
    }
}
