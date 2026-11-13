//! Paints the toolbar's + and the ADD TO CANVAS menu it hangs: one row per
//! `quantick_workspace::canvas::WIDGETS` entry, its miniature, title and
//! line. What a row adds and whether it can is the canvas model's
//! (`add_menu`); a press is only recorded here, for the window to carry out
//! through `canvas.widget.add`.

use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, vec2};
use egui_phosphor::regular as icons;
use quantick_control_handlers::canvas::add_act;
use quantick_workspace::canvas::WidgetSpec;

use crate::theme;
use crate::toolbar::DrawnControl;

/// The + button's side, and the air between it and the capsule.
pub(crate) const BUTTON_PX: f32 = 30.0;
pub(crate) const GAP_PX: f32 = 8.0;
/// The menu's width, a row's height and a miniature's size.
const MENU_PX: f32 = 280.0;
const ROW_PX: f32 = 44.0;
const MINIATURE: egui::Vec2 = vec2(40.0, 28.0);
/// The + in Light: an outlined white button.
const LIGHT_EDGE: Color32 = Color32::from_rgb(0x7C, 0x7C, 0x7C);
const HEADING: &str = "ADD TO CANVAS";
const HOVER: &str = "Add to canvas";

/// The + and its menu: open or not, where the + stands, a row pressed this
/// frame, and every row as drawn, for the scene.
#[derive(Default)]
pub(crate) struct AddMenu {
    pub(crate) door: Option<Rect>,
    pub(crate) drawn: Vec<DrawnControl>,
    /// The launch hooks were read.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) hooked: bool,
}

/// The + itself: accent-filled with a dark + in Dark, white with a grey edge
/// and a black + in Light, lit while its menu is `open`.
pub(crate) fn button(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_PX, BUTTON_PX), Sense::click());
    let dark = ui.visuals().dark_mode;
    let (fill, edge, ink) = if dark {
        (palette.accent(), Stroke::NONE, palette.canvas())
    } else {
        (
            Color32::WHITE,
            Stroke::new(1.0_f32, LIGHT_EDGE),
            Color32::BLACK,
        )
    };
    let lit = response.hovered() || open;
    let fill = if lit { fill.gamma_multiply(0.85) } else { fill };
    ui.painter().rect(rect, 4.0_f32, fill, edge);
    let font = egui::FontId::proportional(17.0);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icons::PLUS, font, ink);
    response.on_hover_text(HOVER)
}

/// The menu under the + at `door`, its right edge on the door's; `rows` say
/// what each entry is and whether it can be added now. A press outside, Esc
/// or a row closes it.
pub(crate) fn hang(
    ctx: &egui::Context,
    rows: &[(&'static WidgetSpec, bool)],
    menu: &mut AddMenu,
    session: &mut quantick_ui_model::UpdateSession<'_, quantick_ui_model::canvas_popovers::Model>,
) {
    menu.drawn.clear();
    let Some(door) = menu.door.filter(|_| session.state().projection().add_open) else {
        return;
    };
    let muted = theme::get(ctx).text_muted();
    let shown = egui::Area::new(egui::Id::new("add_widget_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(door.right_bottom() + vec2(0.0, 4.0))
        .pivot(Align2::RIGHT_TOP)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(6.0, 8.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let font = egui::FontId::proportional(11.0);
                    ui.label(egui::RichText::new(HEADING).font(font).color(muted));
                    for (widget, ready) in rows {
                        if row(ui, widget, *ready, menu) {
                            session.update(quantick_ui_model::canvas_popovers::Intent::Pick {
                                widget,
                                available: *ready,
                            });
                        }
                    }
                });
        });
    let pressed = ctx.input(|input| input.pointer.any_click());
    let pointer = ctx.pointer_latest_pos().unwrap_or_default();
    let outside = !shown.response.contains_pointer() && !door.contains(pointer);
    let escape = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    session.update(quantick_ui_model::canvas_popovers::Intent::DismissAdd {
        outside: pressed && outside,
        escaped: escape,
    });
}

/// One row: its miniature, title and line; greyed and inert when it cannot
/// be added now.
fn row(ui: &mut egui::Ui, widget: &'static WidgetSpec, ready: bool, menu: &mut AddMenu) -> bool {
    let palette = theme::get(ui.ctx());
    let sense = Sense {
        click: ready,
        ..Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(MENU_PX, ROW_PX), sense);
    if ready && response.hovered() {
        ui.painter().rect_filled(rect, 4.0, palette.hover_fill());
    }
    let fade = |ink: Color32| if ready { ink } else { ink.gamma_multiply(0.45) };
    let mini = Rect::from_min_size(
        rect.min + vec2(6.0, (ROW_PX - MINIATURE.y) / 2.0),
        MINIATURE,
    );
    let painter = ui.painter();
    painter.rect(
        mini,
        3.0_f32,
        palette.chrome(),
        Stroke::new(1.0_f32, palette.border()),
    );
    let glyph = egui::FontId::proportional(16.0);
    painter.text(
        mini.center(),
        Align2::CENTER_CENTER,
        icon(widget),
        glyph,
        fade(palette.accent()),
    );
    let left = mini.right() + 10.0;
    let title = egui::pos2(left, rect.center().y - 1.0);
    let font = egui::FontId::proportional(13.0);
    painter.text(
        title,
        Align2::LEFT_BOTTOM,
        widget.title,
        font,
        fade(palette.text_primary()),
    );
    let line = egui::pos2(left, rect.center().y + 1.0);
    let font = egui::FontId::proportional(11.0);
    painter.text(
        line,
        Align2::LEFT_TOP,
        widget.line,
        font,
        fade(palette.text_muted()),
    );
    let (capability, _) = add_act(widget);
    menu.drawn.push(DrawnControl {
        id: format!("add_widget.{}", widget.id),
        label: widget.title.to_owned(),
        rect,
        selected: false,
        capability: Some(capability),
    });
    ready && response.clicked()
}

/// The glyph a widget's miniature wears.
fn icon(widget: &WidgetSpec) -> &'static str {
    match widget.id {
        "timeframe-chart" => icons::CHART_BAR,
        "order-flow" => icons::WAVEFORM,
        "depth" => icons::STACK,
        "big_trades" => icons::CIRCLES_THREE,
        "liquidations" => icons::LIGHTNING,
        "open_interest" => icons::CHART_LINE,
        _ => icons::ARROWS_LEFT_RIGHT,
    }
}
