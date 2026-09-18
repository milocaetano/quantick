//! Placement transitions own the draft gesture, borrowing its longer-lived store.
//! Canvas and context-menu placement share completion; the pane applies effects.
use super::drawing_projection::DrawingProjection;
use super::primary_button::pane_chrome_hit;
use super::{DRAWING_DRAG_COMPLETES_PX, FREEHAND_MAX_POINTS, FREEHAND_MIN_STEP_PX, PaneGestures};
use crate::bands::{self, Band};
use crate::drawings::{self, ChartPoint, DrawingBand, DrawingTool, Drawings, PresetHost};
use crate::plot_area::PlotAreas;
use eframe::egui;

pub(super) struct PlacementFrame<'a> {
    pub(super) ui: &'a egui::Ui,
    pub(super) areas: &'a PlotAreas,
    pub(super) bands: &'a [Band],
    pub(super) id: egui::Id,
    pub(super) history_right: f32,
}

pub(super) struct PlacementOptions {
    pub(super) tool: Option<DrawingTool>,
    pub(super) magnet: bool,
    pub(super) constrain: drawings::Constrain,
    pub(super) parked_position: Option<egui::Pos2>,
}

#[derive(Clone, Copy)]
pub(super) struct PlacementDefaults<'a> {
    pub(super) presets: &'a dyn PresetHost,
    pub(super) repeat: bool,
}

#[derive(Default)]
pub(super) struct PlacementCompletion {
    pub(super) arm_pointer: bool,
    pub(super) begin_text_edit: bool,
}
impl PlacementCompletion {
    fn include(&mut self, next: Self) {
        self.arm_pointer |= next.arm_pointer;
        self.begin_text_edit |= next.begin_text_edit;
    }
}

#[derive(Default)]
pub(super) struct PlacementOutcome {
    pub(super) tool_armed: bool,
    // Inactive placement leaves the pane's existing hover alone.
    pub(super) hover_position: Option<Option<egui::Pos2>>,
    pub(super) completion: PlacementCompletion,
}

struct PlacementRead<'a, 'series> {
    projection: &'a DrawingProjection<'series>,
    areas: &'a PlotAreas,
    bands: &'a [Band],
    history_right: f32,
    magnet: bool,
    constrain: drawings::Constrain,
}

impl PaneGestures {
    pub(super) fn update_placement(
        &mut self,
        drawings: &mut Drawings,
        projection: &DrawingProjection<'_>,
        frame: PlacementFrame<'_>,
        options: PlacementOptions,
        defaults: PlacementDefaults<'_>,
    ) -> PlacementOutcome {
        let Some(tool) = options.tool else {
            self.cancel_placement(drawings);
            return PlacementOutcome::default();
        };
        let read = PlacementRead {
            projection,
            areas: frame.areas,
            bands: frame.bands,
            history_right: frame.history_right,
            magnet: options.magnet,
            constrain: options.constrain,
        };
        let ui = frame.ui;
        let surface = frame
            .bands
            .iter()
            .fold(frame.bands[0].rect, |union, band| union.union(band.rect));
        let response = ui.interact(surface, frame.id, egui::Sense::click_and_drag());
        let mut outcome = PlacementOutcome {
            tool_armed: true,
            hover_position: Some(response.hover_pos()),
            completion: PlacementCompletion::default(),
        };
        self.preview_placement(
            drawings,
            &read,
            ui,
            &response,
            tool,
            ui.input(|input| input.pointer.latest_pos())
                .filter(|position| surface.contains(*position))
                .or(options.parked_position),
        );
        let pressed_position = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if let Some(position) = pressed_position
            .filter(|position| surface.contains(*position) && !over_chrome(ui, *position))
            && let Some((band, point)) = read.shaped_placement(drawings, tool, position)
        {
            let band = band.key.clone();
            self.begin_placement_press(drawings, tool, position);
            outcome
                .completion
                .include(self.place_point(drawings, tool, &band, point, defaults));
        }
        let released_position = ui.input(|input| {
            input
                .pointer
                .primary_released()
                .then(|| input.pointer.latest_pos())
                .flatten()
        });
        // Capture the tool for the whole update, even when a press completes an
        // object and requests Pointer. Existing release behavior still uses it.
        if tool.freehand() {
            outcome
                .completion
                .include(self.advance_freehand(drawings, &read, ui, tool, defaults));
            if released_position.is_some() {
                outcome
                    .completion
                    .include(self.finish_freehand(drawings, defaults.repeat));
            }
            return outcome;
        }
        outcome.completion.include(self.finish_anchor_release(
            drawings,
            &read,
            tool,
            released_position,
            surface,
            defaults,
        ));
        if released_position.is_some() {
            self.press_position = None;
            self.press_started_empty = false;
        }
        outcome
    }

    fn cancel_placement(&mut self, drawings: &mut Drawings) {
        drawings.cancel_draft();
        self.hover = None;
        self.band_hint = None;
        self.press_position = None;
        self.press_started_empty = false;
    }

    fn preview_placement(
        &mut self,
        drawings: &Drawings,
        read: &PlacementRead<'_, '_>,
        ui: &egui::Ui,
        response: &egui::Response,
        tool: DrawingTool,
        preview_position: Option<egui::Pos2>,
    ) {
        // Widget hover serves the hint/cursor; raw pointer serves the preview.
        // A dragged widget is not necessarily hovered.
        let hovered = response
            .hover_pos()
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| bands::band_at(read.bands, position));
        let over_pane_chrome = response
            .hover_pos()
            .is_some_and(|position| pane_chrome_hit(read.areas, position));
        self.band_hint = hovered
            .filter(|band| band.drawable() && !over_pane_chrome)
            .map(|band| band.rect);
        self.hover = preview_position
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| {
                read.shaped_placement(drawings, tool, position)
                    .map(|(_, point)| point)
            });
        if (response.hovered() || response.dragged()) && !over_pane_chrome {
            ui.ctx().set_cursor_icon(match hovered {
                Some(band) if band.drawable() => egui::CursorIcon::Crosshair,
                _ => egui::CursorIcon::NotAllowed,
            });
        }
        if let Some(refusal) = hovered.and_then(|band| band.refusal) {
            response.clone().on_hover_text(refusal);
        }
    }

    fn begin_placement_press(
        &mut self,
        drawings: &Drawings,
        tool: DrawingTool,
        position: egui::Pos2,
    ) {
        self.press_started_empty = drawings.draft_len() == 0;
        self.press_position = Some(position);
        // Seed decimation from the actual press: a stationary held frame must
        // not append a second point on the same pixel.
        if tool.freehand() {
            self.freehand_last_position = Some(position);
        }
    }

    fn advance_freehand(
        &mut self,
        drawings: &mut Drawings,
        read: &PlacementRead<'_, '_>,
        ui: &egui::Ui,
        tool: DrawingTool,
        defaults: PlacementDefaults<'_>,
    ) -> PlacementCompletion {
        if drawings.draft_len() > 0
            && ui.input(|input| input.pointer.primary_down())
            && let Some(position) = ui.input(|input| input.pointer.latest_pos())
            && let Some((band, position)) = read.placement_target(drawings, position)
            && drawings
                .draft()
                .is_some_and(|draft| draft.band == tool.band_for(&band.key))
            && let Some(point) = read.projection.drawing_point_at(
                position,
                read.history_right,
                read.projection.series.slots(),
                read.magnet,
                tool.anchor_snap(),
                band,
            )
            && self
                .freehand_last_position
                .is_none_or(|last| last.distance(position) >= FREEHAND_MIN_STEP_PX)
            && drawings.draft_len() < FREEHAND_MAX_POINTS
        {
            self.freehand_last_position = Some(position);
            let band = band.key.clone();
            return self.place_point(drawings, tool, &band, point, defaults);
        }
        PlacementCompletion::default()
    }

    fn finish_freehand(&mut self, drawings: &mut Drawings, repeat: bool) -> PlacementCompletion {
        self.freehand_last_position = None;
        let mut completion = PlacementCompletion::default();
        if drawings.finish_draft() {
            completion.arm_pointer = !repeat;
            self.hover = None;
        }
        self.press_position = None;
        self.press_started_empty = false;
        completion
    }

    fn finish_anchor_release(
        &mut self,
        drawings: &mut Drawings,
        read: &PlacementRead<'_, '_>,
        tool: DrawingTool,
        released_position: Option<egui::Pos2>,
        surface: egui::Rect,
        defaults: PlacementDefaults<'_>,
    ) -> PlacementCompletion {
        if tool.required_points() > 1
            && self.press_started_empty
            && let Some(start) = self.press_position
            && let Some(position) = released_position
            && surface.contains(position)
            && start.distance(position) >= DRAWING_DRAG_COMPLETES_PX
            && let Some((band, point)) = read.shaped_placement(drawings, tool, position)
        {
            let band = band.key.clone();
            return self.place_point(drawings, tool, &band, point, defaults);
        }
        PlacementCompletion::default()
    }

    pub(super) fn place_point(
        &mut self,
        drawings: &mut Drawings,
        tool: DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        defaults: PlacementDefaults<'_>,
    ) -> PlacementCompletion {
        // Only a fresh draft constructs defaults; existing objects are untouched.
        if !drawings.place_with(tool, band, point, |tool| {
            drawings::new_drawing_from_defaults(defaults.presets, tool)
        }) {
            return PlacementCompletion::default();
        }
        let completion = PlacementCompletion {
            arm_pointer: !defaults.repeat,
            begin_text_edit: tool.holds_text(),
        };
        // Hide the empty note's placeholder in this same input/paint frame.
        if completion.begin_text_edit {
            self.content_editing = drawings.selected();
        }
        self.hover = None;
        completion
    }
}

impl PlacementRead<'_, '_> {
    fn placement_target(
        &self,
        drawings: &Drawings,
        position: egui::Pos2,
    ) -> Option<(&Band, egui::Pos2)> {
        if pane_chrome_hit(self.areas, position) {
            return None;
        }
        let pinned = drawings
            .draft()
            .filter(|draft| draft.band != DrawingBand::AllBands)
            .and_then(|draft| self.bands.iter().find(|band| band.key == draft.band));
        let band = match pinned {
            Some(band) => band,
            None => bands::band_at(self.bands, position)?,
        };
        if !band.drawable() {
            return None;
        }
        Some((
            band,
            egui::pos2(
                position.x,
                position.y.clamp(band.rect.top(), band.rect.bottom()),
            ),
        ))
    }

    fn shaped_placement(
        &self,
        drawings: &Drawings,
        tool: DrawingTool,
        position: egui::Pos2,
    ) -> Option<(&Band, ChartPoint)> {
        let (band, position) = self.placement_target(drawings, position)?;
        let total = self.projection.series.slots();
        // Only bounded non-freehand drafts use shaping. Projecting a whole
        // brush stroke here would allocate and walk it up to three times/frame.
        let shaped = match (drawings.draft(), band.scale.as_ref()) {
            (Some(draft), Some(scale)) if draft.tool == tool && !tool.freehand() => {
                let placed = self.projection.projected_drawing_points(
                    draft,
                    self.history_right,
                    total,
                    scale,
                );
                tool.pending_anchor(&placed, position, self.constrain)
            }
            _ => position,
        };
        // Do not re-clamp shaped output: the tool's pixel floor prevents a
        // degenerate shape and may deliberately extend just outside the band.
        let point = self.projection.drawing_point_at(
            shaped,
            self.history_right,
            total,
            self.magnet,
            tool.anchor_snap(),
            band,
        )?;
        Some((band, point))
    }
}

fn over_chrome(ui: &egui::Ui, position: egui::Pos2) -> bool {
    ui.ctx()
        .layer_id_at(position)
        .is_some_and(|layer| layer != ui.layer_id())
}
