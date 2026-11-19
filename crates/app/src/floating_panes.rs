//! Paints a chart floating over the canvas or standing in its own OS window:
//! its shadow and outline, the edges that resize it, the slot a drop
//! would dock it into, and the window it pops out to.
//!
//! Where it goes is `quantick_workspace::canvas`'s: the held gesture, the
//! one drop rule and the dock preview are the model's, fed the pointer here.
//! What letting go asks for is recorded, and the window carries it out
//! through the `canvas.*` capabilities.

use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use quantick_workspace::canvas::{
    CanvasPoint, CanvasRect, CanvasState, DOCK_HINT, DropOutcome, FloatGesture, MIN_FLOAT_SIZE,
    PaneSlot, RESIZE_GRIP_PX, ResizeEdges,
};

use crate::theme;

/// The outline a floating chart wears, and Light's blue for it.
const OUTLINE_PX: f32 = 1.5;
const LIGHT_OUTLINE: Color32 = Color32::from_rgb(0x7C, 0x9C, 0xD6);
/// How strongly a dock slot is tinted with the accent, Light then Dark.
const TINT: (f32, f32) = (0.30, 0.45);

/// What a chart's own window did this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct WindowEvents {
    /// It settled at this rectangle, in desktop points, on this monitor.
    pub(crate) moved: Option<(CanvasRect, Option<String>)>,
    /// Its close button was pressed: it returns to the canvas.
    pub(crate) closed: bool,
}

/// One tab's floating charts: the one held, what letting go asked for, and
/// what their windows did, until the window carries them out.
#[derive(Default)]
pub(crate) struct FloatingPanes {
    pub(crate) held: Option<FloatGesture>,
    pub(crate) dropped: Option<(PaneSlot, DropOutcome)>,
    pub(crate) windows: Vec<(PaneSlot, WindowEvents)>,
    /// Drop targets for the scene: identity, label, geometry, capability, selection.
    pub(crate) drawn: Vec<(String, String, CanvasRect, &'static str, bool)>,
    /// The window's desktop origin, in points, and its scale, as last read.
    pub(crate) origin: (f32, f32),
    pub(crate) scale: f32,
    /// Each chart window's place when it opened, which its builder keeps so
    /// egui never moves it back, and where it was last seen.
    pub(crate) placed: std::collections::BTreeMap<PaneSlot, (CanvasRect, CanvasRect)>,
    /// A pointer a capture holds the gesture at, in place of the hand's.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) pinned: Option<egui::Pos2>,
}

impl FloatingPanes {
    fn pointer(&self, ctx: &egui::Context) -> Option<egui::Pos2> {
        #[cfg(any(feature = "scenario-harness", test))]
        if self.pinned.is_some() {
            return self.pinned;
        }
        ctx.input(|input| input.pointer.latest_pos())
    }
}

/// The monitor holding `rect` (desktop points at `scale`), by device name.
pub(crate) fn monitor_of(rect: CanvasRect, scale: f32) -> Option<String> {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let (x, y) = (rect.x + rect.w / 2.0, rect.y + 10.0);
    quantick_platform::monitor_at(x * scale, y * scale)
}

pub(crate) fn model_rect(rect: Rect) -> CanvasRect {
    CanvasRect::new(rect.left(), rect.top(), rect.width(), rect.height())
}

pub(crate) fn screen_rect(rect: CanvasRect) -> Rect {
    Rect::from_min_size(pos2(rect.x, rect.y), vec2(rect.w, rect.h))
}

fn model_point(point: egui::Pos2) -> CanvasPoint {
    CanvasPoint::new(point.x, point.y)
}

/// Where `slot` draws this frame: where the hand holds it, else `saved`.
pub(crate) fn placed(
    floats: &FloatingPanes,
    slot: PaneSlot,
    saved: CanvasRect,
    ctx: &egui::Context,
) -> Rect {
    let held = floats.held.filter(|held| held.slot() == slot);
    let at = held
        .zip(floats.pointer(ctx))
        .map(|(held, pointer)| held.rect(model_point(pointer)));
    screen_rect(at.unwrap_or(saved))
}

/// A Ui on the layer `slot` floats on, registered for this frame at `rect`
/// so it stands above the canvas and takes the pointer there.
pub(crate) fn layer(ctx: &egui::Context, (tab, slot): (u64, PaneSlot), rect: Rect) -> egui::Ui {
    let id = egui::Id::new(("floating_pane", tab, slot));
    let shown = egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            _ = ui.allocate_exact_size(rect.size(), Sense::hover())
        });
    let builder = egui::UiBuilder::new().max_rect(rect);
    let mut ui = egui::Ui::new(
        ctx.clone(),
        shown.response.layer_id,
        id.with("body"),
        builder,
    );
    ui.set_clip_rect(rect.expand(RESIZE_GRIP_PX + 8.0));
    ui
}

/// A floating chart's frame and resize edges; returns its content rectangle.
pub(crate) fn frame(
    ui: &mut egui::Ui,
    rect: Rect,
    slot: PaneSlot,
    floats: &mut FloatingPanes,
) -> Rect {
    let palette = theme::get(ui.ctx());
    let outline = [LIGHT_OUTLINE, palette.accent()][usize::from(ui.visuals().dark_mode)];
    let shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 6.0),
        blur: 18.0,
        spread: 0.0,
        color: Color32::from_black_alpha(90),
    };
    ui.painter().add(shadow.as_shape(rect, 4.0));
    ui.painter().rect_filled(rect, 0.0, palette.chrome());
    let content = rect.shrink(OUTLINE_PX);
    let pointer = floats.pointer(ui.ctx()).map(model_point);
    let from = model_rect(rect);
    let ring = rect.expand(RESIZE_GRIP_PX);
    let edge = ui.interact(ring, ui.id().with("edges"), Sense::drag());
    let edges = pointer.and_then(|pointer| ResizeEdges::at(from, pointer).zip(Some(pointer)));
    if let (true, Some((edges, start))) = (edge.drag_started(), edges) {
        floats.held = Some(FloatGesture::Resize {
            slot,
            edges,
            from,
            start,
        });
    }
    if let Some((edges, _)) = edges.filter(|_| floats.held.is_none()) {
        edge.on_hover_cursor(resize_cursor(edges));
    }
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(OUTLINE_PX, outline));
    content
}

fn resize_cursor(edges: ResizeEdges) -> egui::CursorIcon {
    match (edges.left || edges.right, edges.top || edges.bottom) {
        (true, true) if edges.left == edges.top => egui::CursorIcon::ResizeNwSe,
        (true, true) => egui::CursorIcon::ResizeNeSw,
        (true, false) => egui::CursorIcon::ResizeHorizontal,
        _ => egui::CursorIcon::ResizeVertical,
    }
}

/// While a chart is held: the slot a drop would dock it into, lit with its
/// pill, and the hint; on letting go, what the drop does. The canvas's edges
/// and corners and each docked chart's sides and centre are published as the
/// drop targets they are.
pub(crate) fn preview(
    ctx: &egui::Context,
    floats: &mut FloatingPanes,
    state: &CanvasState,
    canvas: Rect,
) {
    let held = floats
        .held
        .filter(|held| matches!(held, FloatGesture::Move { .. }))
        .map(|held| held.slot());
    let pointer = floats.pointer(ctx).map(model_point);
    let targets = quantick_control_handlers::canvas::drop_targets(
        state,
        model_rect(canvas),
        held,
        pointer.filter(|_| held.is_some()),
    );
    let mut selected = false;
    floats
        .drawn
        .extend(targets.into_iter().map(|(id, label, rect, capability)| {
            let hot = !selected && held.is_some() && pointer.is_some_and(|at| rect.contains(at));
            selected |= hot;
            (id, label, rect, capability, hot)
        }));
    let (Some(held), Some(pointer)) = (floats.held, floats.pointer(ctx)) else {
        return;
    };
    let (area, at) = (model_rect(canvas), model_point(pointer));
    let window = model_rect(ctx.screen_rect());
    let released = ctx.input(|input| !input.pointer.primary_down());
    #[cfg(any(feature = "scenario-harness", test))]
    let released = released && floats.pinned.is_none();
    if released {
        floats.dropped = Some((held.slot(), held.release(state, area, window, at)));
        floats.held = None;
        return;
    }
    if !matches!(held, FloatGesture::Move { .. }) {
        return;
    }
    let palette = theme::get(ctx);
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("canvas_drop"));
    let painter = ctx.layer_painter(layer);
    if let Some(slot) = held.dock_preview(state, area, window, at) {
        let slot = screen_rect(slot);
        let tint = [TINT.0, TINT.1][usize::from(ctx.style().visuals.dark_mode)];
        painter.rect_filled(slot, 0.0, palette.accent().gamma_multiply(tint));
        let plate = (palette.accent(), Color32::WHITE);
        let swap = matches!(
            held.release(state, area, window, at),
            DropOutcome::Dock(quantick_workspace::canvas::DockTarget::Swap(_))
        );
        let label = if swap { "Swap" } else { "Dock here" };
        pill(&painter, slot.center_top() + vec2(0.0, 24.0), label, plate);
    }
    guides(&painter, state, area, held.slot(), at);
    let words = (palette.chrome(), palette.text_primary());
    let hint = pos2(canvas.center().x, canvas.bottom() - 80.0);
    pill(&painter, hint, DOCK_HINT, words);
}

fn guides(
    painter: &egui::Painter,
    state: &CanvasState,
    area: CanvasRect,
    held: PaneSlot,
    at: CanvasPoint,
) {
    let palette = theme::get(painter.ctx());
    for guide in quantick_ui_model::canvas_guides::guides(state, area, held, at) {
        let (rect, badge, hot) = (
            screen_rect(guide.zone),
            screen_rect(guide.badge),
            guide.selected,
        );
        let (fill, ink) = if hot {
            (palette.accent(), Color32::WHITE)
        } else {
            (palette.chrome(), palette.text_primary())
        };
        painter.rect_filled(
            rect,
            4.0,
            palette
                .accent()
                .gamma_multiply(if hot { 0.22 } else { 0.06 }),
        );
        painter.rect_filled(badge, 5.0, fill);
        painter.rect_stroke(
            badge,
            5.0,
            Stroke::new(1.5, if hot { ink } else { palette.border() }),
        );
        let font = egui::FontId::proportional(guide.text_size);
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            guide.label,
            font,
            ink,
        );
    }
}

/// `text` on a rounded plate centred on `at`.
fn pill(painter: &egui::Painter, at: egui::Pos2, text: &str, (fill, ink): (Color32, Color32)) {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(12.0), ink);
    let plate = Rect::from_center_size(at, galley.size() + vec2(20.0, 10.0));
    painter.rect_filled(plate, plate.height() / 2.0, fill);
    painter.galley(plate.center() - galley.size() / 2.0, galley, ink);
}

/// A chart in its own OS window at `rect` (desktop points), drawn by
/// `draw`; answers where the window went and whether it was closed.
pub(crate) fn os_window(
    ctx: &egui::Context,
    (tab, slot): (u64, PaneSlot),
    (saved, (opened, seen)): (CanvasRect, &mut (CanvasRect, CanvasRect)),
    mut draw: impl FnMut(&mut egui::Ui),
) -> WindowEvents {
    let id = egui::ViewportId::from_hash_of(("canvas_pane", tab, slot));
    let rect = *opened;
    let builder = egui::ViewportBuilder::default()
        .with_title("Quantick chart")
        .with_position(pos2(rect.x, rect.y))
        .with_inner_size([rect.w, rect.h])
        .with_min_inner_size([MIN_FLOAT_SIZE.0, MIN_FLOAT_SIZE.1]);
    ctx.show_viewport_immediate(id, builder, |child, class| {
        if class == egui::ViewportClass::Embedded {
            egui::Window::new("Quantick chart")
                .id(egui::Id::new(("embedded_pane", tab, slot)))
                .default_rect(screen_rect(rect))
                .show(child, |ui| draw(ui));
            return WindowEvents::default();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::get(child).chrome()))
            .show(child, |ui| draw(ui));
        let (outer, inner, closed) = child.input(|input| {
            let viewport = input.viewport();
            (
                viewport.outer_rect,
                viewport.inner_rect,
                viewport.close_requested(),
            )
        });
        let at = outer
            .zip(inner)
            .map(|(outer, inner)| (outer.min, inner.size()));
        let at = at.map(|(min, size)| CanvasRect::new(min.x, min.y, size.x, size.y));
        let settled = at.and_then(|at| quantick_control_handlers::canvas::settled(at, seen, saved));
        let scale = child.pixels_per_point();
        let moved = settled.map(|at| (at, monitor_of(at, scale)));
        WindowEvents { moved, closed }
    })
}
