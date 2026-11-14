//! Paints a chart's ⋯ at its header's right end, the menu it hangs, and the
//! strip a minimized chart folds to. What the rows say and do is
//! `quantick_workspace::canvas::menu`'s and `quantick_control_handlers::canvas`'s;
//! a press is only recorded here, for the window to carry out.

use eframe::egui::{self, Align2, Color32, Painter, Pos2, Rect, Sense, vec2};
use egui_phosphor::regular as icons;
use quantick_control_handlers::canvas::item_capability;
use quantick_workspace::canvas::{CanvasState, Edge, PANE_MENU_BUTTON_PX, PaneMenuItem as Item};
use quantick_workspace::canvas::{PaneSlot, pane_menu};

use crate::theme;
use crate::toolbar::DrawnControl;

/// The menu's width, a row's height and a separator's.
const MENU_PX: f32 = 190.0;
const ROW_PX: f32 = 26.0;
const RULE_PX: f32 = 9.0;
/// How far below its restore a folded chart's name starts.
const NAME_GAP_PX: f32 = 40.0;
/// How far in from its door's right end a ⋯ sits.
const DOOR_INSET_PX: f32 = 4.0;

/// One tab's chart menus: the one open, a press this frame, a ⋯ dragged
/// this frame to lift its chart, the dialog up, and every ⋯, row, restore
/// and dialog button as drawn, for the scene.
#[derive(Default)]
pub(crate) struct PaneMenus {
    pub(crate) open: Option<PaneSlot>,
    pub(crate) picked: Option<(PaneSlot, Item)>,
    pub(crate) lift: Option<PaneSlot>,
    pub(crate) removing: Option<PaneSlot>,
    pub(crate) drawn: Vec<DrawnControl>,
}

impl PaneMenus {
    pub(crate) fn publish(
        &mut self,
        id: String,
        label: &str,
        rect: Rect,
        cap: Option<&'static str>,
    ) {
        let (label, selected, capability) = (label.to_owned(), false, cap);
        let control = DrawnControl {
            id,
            label,
            rect,
            selected,
            capability,
        };
        self.drawn.push(control);
    }

    /// A rail as drawn, named `id`: its restore, as a strip's is.
    pub(crate) fn publish_rail(&mut self, id: String, rect: Rect) {
        self.publish(id, "Restore", rect, Some(item_capability(Item::Restore)));
    }
}

/// `text` centred on `at`, `size` points high, in `ink`.
fn glyph(painter: &Painter, at: Pos2, (text, size): (&str, f32), ink: Color32) {
    let font = egui::FontId::proportional(size);
    painter.text(at, Align2::CENTER_CENTER, text, font, ink);
}

/// The ⋯ at `strip`'s right end, in `ink`, and its menu while open. It is
/// the chart's grip too: dragging it lifts the chart to float and move.
pub(crate) fn button(
    ui: &egui::Ui,
    strip: Rect,
    (slot, state): (PaneSlot, &CanvasState),
    menus: &mut PaneMenus,
    ink: Color32,
) {
    let palette = theme::get(ui.ctx());
    let side = PANE_MENU_BUTTON_PX.min(strip.height());
    let centre = egui::pos2(strip.right() - side / 2.0 - DOOR_INSET_PX, strip.center().y);
    let rect = Rect::from_center_size(centre, vec2(side, side));
    let id = ui.id().with(("pane_menu", slot));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if response.clicked() {
        menus.open = (menus.open != Some(slot)).then_some(slot);
    }
    menus.lift = menus.lift.or(response.dragged().then_some(slot));
    let open = menus.open == Some(slot);
    if response.hovered() || open {
        ui.painter().rect_filled(rect, 4.0, palette.hover_fill());
    }
    glyph(ui.painter(), rect.center(), (icons::DOTS_THREE, 15.0), ink);
    let response = response.on_hover_text("Chart options · drag to move");
    menus.publish(format!("pane_menu.{slot}"), "Chart options", rect, None);
    if open {
        hang(ui.ctx(), rect, (slot, state), menus, &response);
    }
}

/// The menu under the ⋯ at `door`, its right edge on the door's. A press
/// outside it, Esc or a row closes it.
fn hang(
    ctx: &egui::Context,
    door: Rect,
    (slot, state): (PaneSlot, &CanvasState),
    menus: &mut PaneMenus,
    response: &egui::Response,
) {
    let rule = theme::get(ctx).border();
    let shown = egui::Area::new(egui::Id::new(("pane_menu_rows", slot)))
        .order(egui::Order::Foreground)
        .fixed_pos(door.right_bottom() + vec2(0.0, 4.0))
        .pivot(Align2::RIGHT_TOP)
        .show(ctx, |ui| {
            let frame = egui::Frame::popup(ui.style());
            frame
                .inner_margin(egui::Margin::symmetric(4.0, 6.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let rows = pane_menu(state, slot);
                    for item in rows {
                        if item == Item::Remove {
                            let size = vec2(MENU_PX, RULE_PX);
                            let (at, _) = ui.allocate_exact_size(size, Sense::hover());
                            ui.painter().hline(at.x_range(), at.center().y, (0.5, rule));
                        }
                        row(ui, item, (slot, menus));
                    }
                });
        });
    let pointer = ctx.input(|input| input.pointer.any_click());
    let outside = !shown.response.contains_pointer() && !response.contains_pointer();
    let escape = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if (pointer && outside) || escape || menus.picked.is_some() {
        menus.open = None;
    }
}

/// One row: a hover patch, its icon and words, red for removal.
fn row(ui: &mut egui::Ui, item: Item, (slot, menus): (PaneSlot, &mut PaneMenus)) {
    let palette = theme::get(ui.ctx());
    let size = vec2(MENU_PX, ROW_PX);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, palette.hover_fill());
    }
    let (icon_ink, ink) = match item {
        Item::Remove => (danger_ink(ui), danger_ink(ui)),
        _ => (palette.text_muted(), palette.text_primary()),
    };
    let icon = match item {
        Item::Minimize => icons::MINUS,
        Item::Maximize => icons::CORNERS_OUT,
        Item::Restore => icons::CORNERS_IN,
        Item::Float => icons::PICTURE_IN_PICTURE,
        Item::Dock(Edge::Left) => icons::ARROW_LINE_LEFT,
        Item::Dock(Edge::Right) => icons::ARROW_LINE_RIGHT,
        Item::Dock(Edge::Top) => icons::ARROW_LINE_UP,
        Item::Dock(Edge::Bottom) => icons::ARROW_LINE_DOWN,
        Item::OpenInNewWindow => icons::ARROW_SQUARE_OUT,
        Item::ReturnToCanvas => icons::ARROW_SQUARE_IN,
        Item::Remove => icons::X,
    };
    let painter = ui.painter();
    let at = rect.left_center() + vec2(18.0, 0.0);
    glyph(painter, at, (icon, 13.0), icon_ink);
    let font = egui::FontId::proportional(13.0);
    let words = rect.left_center() + vec2(34.0, 0.0);
    painter.text(words, Align2::LEFT_CENTER, item.label(), font, ink);
    let id = format!("pane_menu.{slot}.{}", item.id());
    menus.publish(id, item.label(), rect, Some(item_capability(item)));
    if response.clicked() {
        menus.picked = Some((slot, item));
    }
}

/// The red a destructive word and a doomed chart's outline wear.
pub(crate) fn danger_ink(ui: &egui::Ui) -> Color32 {
    let palette = theme::get(ui.ctx());
    if ui.visuals().dark_mode {
        palette.sell()
    } else {
        palette.widget_error()
    }
}

/// A minimized chart's strip: the whole of it restores, its » on top, and
/// its `name` written along it, upwards in a tall strip, across in a flat
/// one. Answers the door its [`button`] takes, in the far square.
pub(crate) fn strip(
    ui: &egui::Ui,
    rect: Rect,
    name: &str,
    slot: PaneSlot,
    menus: &mut PaneMenus,
) -> Rect {
    let palette = theme::get(ui.ctx());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.chrome());
    let (tall, edge) = (rect.height() >= rect.width(), (1.0, palette.border()));
    if tall {
        painter.vline(rect.right() - 0.5, rect.y_range(), edge);
    } else {
        painter.hline(rect.x_range(), rect.bottom() - 0.5, edge);
    }
    let side = rect.width().min(rect.height());
    let restore = Rect::from_min_size(rect.min, vec2(side, side));
    let id = ui.id().with(("pane_strip", slot));
    let response = ui.interact(rect, id, Sense::click());
    if response.hovered() {
        painter.rect_filled(restore.shrink(2.0), 4.0, palette.hover_fill());
    }
    let (ink, icon) = (palette.text_primary(), (icons::CARET_DOUBLE_RIGHT, 13.0));
    glyph(&painter, restore.center(), icon, ink);
    if response.on_hover_text("Restore").clicked() {
        menus.picked = Some((slot, Item::Restore));
    }
    let restores = Some(item_capability(Item::Restore));
    menus.publish(format!("pane_menu.{slot}.strip"), "Restore", rect, restores);
    let (font, muted) = (egui::FontId::proportional(12.0), palette.text_muted());
    let galley = painter.layout_no_wrap(name.to_owned(), font, muted);
    let (w, h) = (galley.size().x, galley.size().y);
    let shape = if tall {
        let at = egui::pos2(rect.center().x - h / 2.0, rect.top() + NAME_GAP_PX + w);
        let upwards = -std::f32::consts::FRAC_PI_2;
        egui::epaint::TextShape::new(at, galley, muted).with_angle(upwards)
    } else {
        let at = egui::pos2(rect.left() + NAME_GAP_PX, rect.center().y - h / 2.0);
        egui::epaint::TextShape::new(at, galley, muted)
    };
    painter.add(shape);
    // The ⋯ fills the far square: its door reaches past it by the inset.
    let far = rect.max + vec2(DOOR_INSET_PX, 0.0);
    Rect::from_min_max(rect.max - vec2(side, side), far)
}
