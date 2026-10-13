//! The window's half of the drawing gestures: egui's pointer read into the
//! plain frames `quantick_drawings::gesture` takes, and its answers carried
//! out. Nothing here decides what a gesture does.

use eframe::egui;
use quantick_drawings::gesture::{
    self, Band, GestureCursor, PaneGestures, PlacementDefaults, PlacementOptions, PlacementOutcome,
};

use super::primary_button::pane_chrome_hit;
use crate::drawings::{Drawings, egui_rect, pos, px};
use crate::plot_area::PlotAreas;

/// The egui cursor a gesture's cursor names.
pub(super) fn cursor_icon(cursor: GestureCursor) -> egui::CursorIcon {
    match cursor {
        GestureCursor::Move => egui::CursorIcon::Move,
        GestureCursor::Resize => egui::CursorIcon::ResizeNwSe,
        GestureCursor::ResizeHorizontal => egui::CursorIcon::ResizeHorizontal,
        GestureCursor::NotAllowed => egui::CursorIcon::NotAllowed,
        GestureCursor::Crosshair => egui::CursorIcon::Crosshair,
    }
}

/// Whether floating chrome — the inspector, the manager, a flyout — sits over
/// `position`.
pub(super) fn over_chrome(ui: &egui::Ui, position: egui::Pos2) -> bool {
    ui.ctx()
        .layer_id_at(position)
        .is_some_and(|layer| layer != ui.layer_id())
}

/// What the placement surface reads off egui for one frame.
pub(super) struct PlacementInput<'a> {
    pub(super) ui: &'a egui::Ui,
    pub(super) areas: &'a PlotAreas,
    pub(super) bands: &'a [Band],
    pub(super) id: egui::Id,
    pub(super) history_right: f32,
}

/// One frame of drawing placement: the surface takes the pointer only while a
/// tool is armed, and the cursor and refusal the update asks for are shown.
pub(super) fn update_placement(
    gestures: &mut PaneGestures,
    drawings: &mut Drawings,
    projection: &gesture::DrawingProjection<'_>,
    input: PlacementInput<'_>,
    options: PlacementOptions,
    defaults: PlacementDefaults<'_>,
) -> PlacementOutcome {
    let ui = input.ui;
    let surface = gesture::placement_surface(input.bands);
    let response = options
        .tool
        .map(|_| ui.interact(egui_rect(surface), input.id, egui::Sense::click_and_drag()));
    let (latest, pressed, released, down) = ui.input(|input| {
        let pointer = &input.pointer;
        (
            pointer.latest_pos(),
            pointer
                .primary_pressed()
                .then(|| pointer.interact_pos())
                .flatten(),
            pointer
                .primary_released()
                .then(|| pointer.latest_pos())
                .flatten(),
            pointer.primary_down(),
        )
    });
    let over = |position| over_chrome(ui, pos(position));
    let on_pane_chrome = |position| pane_chrome_hit(input.areas, pos(position));
    let outcome = gestures.update_placement(
        drawings,
        projection,
        gesture::PlacementFrame {
            bands: input.bands,
            surface,
            history_right: input.history_right,
            hover: response
                .as_ref()
                .and_then(egui::Response::hover_pos)
                .map(px),
            surface_active: response
                .as_ref()
                .is_some_and(|response| response.hovered() || response.dragged()),
            latest: latest.map(px),
            pressed: pressed.map(px),
            released: released.map(px),
            down,
            over_chrome: &over,
            on_pane_chrome: &on_pane_chrome,
        },
        options,
        defaults,
    );
    if let Some(cursor) = outcome.cursor {
        ui.ctx().set_cursor_icon(cursor_icon(cursor));
    }
    if let (Some(response), Some(refusal)) = (response, outcome.refusal) {
        response.on_hover_text(refusal);
    }
    outcome
}
