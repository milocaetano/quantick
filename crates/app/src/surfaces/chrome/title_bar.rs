//! Title-control painting over cached state; viewport commands are host effects.
use crate::surfaces::{HostEffect, SurfaceEnv, SurfaceResponse, ViewContext};
use crate::theme;
use eframe::egui;
use quantick_control_schema::window_menu::{AGENT_PILL_CONTROL, AGENT_PILL_LABEL, KEBAB_CONTROL};
use quantick_ui_model::chrome_title_bar::{Action, Effects, Intent, Model, State};
pub(crate) const HEIGHT: f32 = 36.0;
pub(crate) const WINDOW_BUTTON_WIDTH: f32 = 46.0;
const DRAG_WIDTH: f32 = 32.0;
/// The window buttons' strokes, at the size the Windows Terminal draws its own.
const WINDOW_GLYPH_PX: f32 = 10.0;
/// Keeps the drag region's edge off the `+`, so a press on one never reads as
/// the other.
const DRAG_GAP: f32 = 1.0;
/// The `⋮` button: wide enough to find, as tall as a tab.
pub(crate) const KEBAB_WIDTH: f32 = 36.0;
const KEBAB_GLYPH_PX: f32 = 18.0;
const AGENT_PILL_WIDTH: f32 = 72.0;
/// The outline's gap to the pill's hit area, so it reads as a pill on the strip.
const AGENT_OUTLINE_PX: f32 = 1.0;
const AGENT_OUTLINE_INSET: egui::Vec2 = egui::vec2(3.0, 5.0);
const AGENT_TEXT_PX: f32 = 11.0;

#[derive(Default)]
pub(crate) struct View {
    pub controls: Vec<(&'static str, egui::Rect)>,
    pub kebab_pressed: bool,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "title_bar"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let state = state.snapshot.as_ref()?;
        let ui = target.ui.as_deref_mut()?;
        self.controls.clear();
        let mut actions = Vec::new();
        let pill_width = if state.agent_on {
            AGENT_PILL_WIDTH
        } else {
            0.0
        };
        ui.add_space(DRAG_GAP);
        let drag_width =
            (ui.available_width() - 3.0 * WINDOW_BUTTON_WIDTH - KEBAB_WIDTH - pill_width).max(0.0);
        let (_, drag) = ui.allocate_exact_size(
            egui::vec2(drag_width, HEIGHT),
            egui::Sense::click_and_drag(),
        );
        let bounds = drag.rect.intersect(ui.clip_rect());
        if bounds.is_positive() {
            self.controls.push(("drag", bounds));
        }
        if drag.double_clicked() {
            actions.push(Action::ToggleMaximize);
        } else if drag.drag_started() {
            actions.push(Action::StartDrag);
        }
        if state.agent_on && agent_pill(ui, self) {
            actions.push(Action::Agent);
        }
        if kebab(ui, self, state) {
            actions.push(Action::Menu);
        }
        for (id, hint, action) in [
            ("minimize", "Minimize window", Action::Minimize),
            (
                "maximize",
                if state.maximized {
                    "Restore window"
                } else {
                    "Maximize window"
                },
                Action::ToggleMaximize,
            ),
            ("close", "Close window", Action::Close),
        ] {
            let button = window_button(ui, id, hint, state.maximized).on_hover_text(hint);
            let bounds = button.rect.intersect(ui.clip_rect());
            if bounds.is_positive() {
                self.controls.push((id, bounds));
            }
            if button.clicked() {
                actions.push(action);
            }
        }
        (!actions.is_empty()).then_some(Intent::Actions(actions))
    }
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    if effects.0.is_empty() {
        return SurfaceResponse::default();
    }
    SurfaceResponse {
        model_effects: vec![HostEffect::ChromeTitleBar(effects)],
        ..Default::default()
    }
}
pub(crate) fn tab_width(width: f32, agent: bool) -> f32 {
    (width
        - 3.0 * WINDOW_BUTTON_WIDTH
        - KEBAB_WIDTH
        - if agent { AGENT_PILL_WIDTH } else { 0.0 }
        - DRAG_GAP
        - if width >= 600.0 { DRAG_WIDTH } else { 0.0 })
    .max(0.0)
}
fn kebab(
    ui: &mut egui::Ui,
    title: &mut View,
    state: &quantick_ui_model::chrome_title_bar::Snapshot,
) -> bool {
    let palette = theme::get(ui.ctx());
    let glyph =
        egui::RichText::new(egui_phosphor::regular::DOTS_THREE_VERTICAL).size(KEBAB_GLYPH_PX);
    let surface = ui.painter().add(egui::Shape::Noop);
    let button = ui
        .add_sized(
            [KEBAB_WIDTH, crate::tabstrip::CHIP_HEIGHT],
            egui::Button::new(glyph).frame(false),
        )
        .on_hover_text(&state.menu_hover);
    title.kebab_pressed = state.menu_open || button.is_pointer_button_down_on();
    if title.kebab_pressed {
        let fill = egui::Shape::rect_filled(button.rect, 0.0, palette.control());
        ui.painter().set(surface, fill);
    }
    let bounds = button.rect.intersect(ui.clip_rect());
    if bounds.is_positive() {
        title.controls.push((KEBAB_CONTROL, bounds));
    }
    button.clicked()
}

/// The pill naming local agent access, shown only while it is on.
fn agent_pill(ui: &mut egui::Ui, title: &mut View) -> bool {
    let palette = theme::get(ui.ctx());
    let face = egui::RichText::new(AGENT_PILL_LABEL)
        .size(AGENT_TEXT_PX)
        .color(palette.amber());
    let pill = ui
        .add_sized(
            [AGENT_PILL_WIDTH, crate::tabstrip::CHIP_HEIGHT],
            egui::Button::new(face).frame(false),
        )
        .on_hover_text("Local agent access is on. Click to manage it.");
    ui.painter().rect_stroke(
        pill.rect.shrink2(AGENT_OUTLINE_INSET),
        pill.rect.height(),
        egui::Stroke::new(AGENT_OUTLINE_PX, palette.amber()),
    );
    let bounds = pill.rect.intersect(ui.clip_rect());
    if bounds.is_positive() {
        title.controls.push((AGENT_PILL_CONTROL, bounds));
    }
    pill.clicked()
}

/// Vector strokes keep native controls crisp and equally legible at every DPI.
fn window_button(ui: &mut egui::Ui, id: &str, hint: &str, maximized: bool) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let response = ui.add_sized(
        [WINDOW_BUTTON_WIDTH, HEIGHT],
        egui::Button::new("").frame(false),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), hint));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            response.rect,
            0.0,
            if id == "close" {
                palette.warn()
            } else {
                palette.bare_hover()
            },
        );
    }
    let ink = if id == "close" && (response.hovered() || response.has_focus()) {
        palette.ink_on(palette.warn())
    } else if response.hovered() || response.has_focus() {
        palette.text_primary()
    } else {
        palette.text_muted()
    };
    let stroke = egui::Stroke::new(1.0_f32, ink);
    let icon =
        egui::Rect::from_center_size(response.rect.center(), egui::Vec2::splat(WINDOW_GLYPH_PX));
    match id {
        "minimize" => {
            ui.painter()
                .line_segment([icon.left_center(), icon.right_center()], stroke);
        }
        "maximize" if maximized => {
            let back = icon.translate(egui::vec2(2.0, -2.0)).shrink(2.0);
            ui.painter()
                .line_segment([back.left_top(), back.right_top()], stroke);
            ui.painter()
                .line_segment([back.right_top(), back.right_bottom()], stroke);
            ui.painter().rect_stroke(
                icon.translate(egui::vec2(-2.0, 2.0)).shrink(2.0),
                0.0,
                stroke,
            );
        }
        "maximize" => {
            ui.painter().rect_stroke(icon, 0.0, stroke);
        }
        _ => {
            ui.painter()
                .line_segment([icon.left_top(), icon.right_bottom()], stroke);
            ui.painter()
                .line_segment([icon.right_top(), icon.left_bottom()], stroke);
        }
    }
    response
}
