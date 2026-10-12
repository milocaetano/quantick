//! Placement transitions own the draft gesture, borrowing its longer-lived store.
//! Canvas and context-menu placement share completion; the pane applies effects.
use super::band::{self, Band};
use super::projection::DrawingProjection;
use super::{
    DRAWING_DRAG_COMPLETES_PX, FREEHAND_MAX_POINTS, FREEHAND_MIN_STEP_PX, GestureCursor, Marks,
    PaneGestures,
};
use crate::{
    ChartPoint, Constrain, DrawingBand, DrawingTool, PresetHost, Px, PxRect,
    new_drawing_from_defaults,
};

/// One frame of the pointer while a drawing tool is armed, as the window read
/// it off the placement surface ([`placement_surface`]).
pub struct PlacementFrame<'a> {
    pub bands: &'a [Band],
    /// [`placement_surface`] of `bands`, as the window took the pointer on it.
    pub surface: PxRect,
    pub history_right: f32,
    /// Where the surface reports the pointer hovering it.
    pub hover: Option<Px>,
    /// Whether the surface is hovered or being dragged.
    pub surface_active: bool,
    /// The pointer's latest position, wherever it is.
    pub latest: Option<Px>,
    /// Where the primary button went down this frame, if it did.
    pub pressed: Option<Px>,
    /// Where the primary button came up this frame, if it did.
    pub released: Option<Px>,
    /// Whether the primary button is held.
    pub down: bool,
    /// Whether floating chrome (a panel, a flyout) sits over a position.
    pub over_chrome: &'a dyn Fn(Px) -> bool,
    /// Whether a position is on the pane's own chrome — an indicator pane's
    /// chevron, header or divider.
    pub on_pane_chrome: &'a dyn Fn(Px) -> bool,
}

pub struct PlacementOptions {
    pub tool: Option<DrawingTool>,
    pub magnet: bool,
    pub constrain: Constrain,
    pub parked_position: Option<Px>,
}

#[derive(Clone, Copy)]
pub struct PlacementDefaults<'a> {
    pub presets: &'a dyn PresetHost,
    pub repeat: bool,
}

#[derive(Default)]
pub struct PlacementCompletion {
    pub arm_pointer: bool,
    pub begin_text_edit: bool,
}
impl PlacementCompletion {
    fn include(&mut self, next: Self) {
        self.arm_pointer |= next.arm_pointer;
        self.begin_text_edit |= next.begin_text_edit;
    }
}

#[derive(Default)]
pub struct PlacementOutcome {
    pub tool_armed: bool,
    // Inactive placement leaves the pane's existing hover alone.
    pub hover_position: Option<Option<Px>>,
    pub completion: PlacementCompletion,
    /// The cursor the surface asks for, when it asks for one.
    pub cursor: Option<GestureCursor>,
    /// Why the band under the pointer refuses an anchor, for the hover text.
    pub refusal: Option<&'static str>,
}

/// The surface an armed tool takes the pointer on: every band, so a tool
/// reaches an indicator pane below the candles.
#[must_use]
pub fn placement_surface(bands: &[Band]) -> PxRect {
    bands
        .iter()
        .fold(PxRect::NOTHING, |union, band| union.union(band.rect))
}

struct PlacementRead<'a, 'series> {
    projection: &'a DrawingProjection<'series>,
    on_pane_chrome: &'a dyn Fn(Px) -> bool,
    bands: &'a [Band],
    history_right: f32,
    magnet: bool,
    constrain: Constrain,
}

impl PaneGestures {
    pub fn update_placement(
        &mut self,
        drawings: &mut Marks,
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
            on_pane_chrome: frame.on_pane_chrome,
            bands: frame.bands,
            history_right: frame.history_right,
            magnet: options.magnet,
            constrain: options.constrain,
        };
        let surface = frame.surface;
        let mut outcome = PlacementOutcome {
            tool_armed: true,
            hover_position: Some(frame.hover),
            ..PlacementOutcome::default()
        };
        self.preview_placement(
            drawings,
            &read,
            &frame,
            &mut outcome,
            tool,
            frame
                .latest
                .filter(|position| surface.contains(*position))
                .or(options.parked_position),
        );
        if let Some(position) = frame
            .pressed
            .filter(|position| surface.contains(*position) && !(frame.over_chrome)(*position))
            && let Some((band, point)) = read.shaped_placement(drawings, tool, position)
        {
            let band = band.key.clone();
            self.begin_placement_press(drawings, tool, position);
            outcome
                .completion
                .include(self.place_point(drawings, tool, &band, point, defaults));
        }
        let released_position = frame.released;
        // Capture the tool for the whole update, even when a press completes an
        // object and requests Pointer. Existing release behavior still uses it.
        if tool.freehand() {
            outcome
                .completion
                .include(self.advance_freehand(drawings, &read, &frame, tool, defaults));
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

    fn cancel_placement(&mut self, drawings: &mut Marks) {
        drawings.cancel_draft();
        self.hover = None;
        self.band_hint = None;
        self.press_position = None;
        self.press_started_empty = false;
    }

    fn preview_placement(
        &mut self,
        drawings: &Marks,
        read: &PlacementRead<'_, '_>,
        frame: &PlacementFrame<'_>,
        outcome: &mut PlacementOutcome,
        tool: DrawingTool,
        preview_position: Option<Px>,
    ) {
        // Widget hover serves the hint/cursor; raw pointer serves the preview.
        // A dragged widget is not necessarily hovered.
        let hovered = frame
            .hover
            .filter(|position| !(frame.over_chrome)(*position))
            .and_then(|position| band::band_at(read.bands, position));
        let over_pane_chrome = frame
            .hover
            .is_some_and(|position| (frame.on_pane_chrome)(position));
        self.band_hint = hovered
            .filter(|band| band.drawable() && !over_pane_chrome)
            .map(|band| band.rect);
        self.hover = preview_position
            .filter(|position| !(frame.over_chrome)(*position))
            .and_then(|position| {
                read.shaped_placement(drawings, tool, position)
                    .map(|(_, point)| point)
            });
        if frame.surface_active && !over_pane_chrome {
            outcome.cursor = Some(match hovered {
                Some(band) if band.drawable() => GestureCursor::Crosshair,
                _ => GestureCursor::NotAllowed,
            });
        }
        outcome.refusal = hovered.and_then(|band| band.refusal);
    }

    fn begin_placement_press(&mut self, drawings: &Marks, tool: DrawingTool, position: Px) {
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
        drawings: &mut Marks,
        read: &PlacementRead<'_, '_>,
        frame: &PlacementFrame<'_>,
        tool: DrawingTool,
        defaults: PlacementDefaults<'_>,
    ) -> PlacementCompletion {
        if drawings.draft_len() > 0
            && frame.down
            && let Some(position) = frame.latest
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

    fn finish_freehand(&mut self, drawings: &mut Marks, repeat: bool) -> PlacementCompletion {
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
        drawings: &mut Marks,
        read: &PlacementRead<'_, '_>,
        tool: DrawingTool,
        released_position: Option<Px>,
        surface: PxRect,
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

    pub fn place_point(
        &mut self,
        drawings: &mut Marks,
        tool: DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        defaults: PlacementDefaults<'_>,
    ) -> PlacementCompletion {
        // Only a fresh draft constructs defaults; existing objects are untouched.
        if !drawings.place_with(tool, band, point, |tool| {
            new_drawing_from_defaults(defaults.presets, tool)
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
    fn placement_target(&self, drawings: &Marks, position: Px) -> Option<(&Band, Px)> {
        if (self.on_pane_chrome)(position) {
            return None;
        }
        let pinned = drawings
            .draft()
            .filter(|draft| draft.band != DrawingBand::AllBands)
            .and_then(|draft| self.bands.iter().find(|band| band.key == draft.band));
        let band = match pinned {
            Some(band) => band,
            None => band::band_at(self.bands, position)?,
        };
        if !band.drawable() {
            return None;
        }
        Some((
            band,
            Px::new(
                position.x,
                position.y.clamp(band.rect.top(), band.rect.bottom()),
            ),
        ))
    }

    fn shaped_placement(
        &self,
        drawings: &Marks,
        tool: DrawingTool,
        position: Px,
    ) -> Option<(&Band, ChartPoint)> {
        let (band, position) = self.placement_target(drawings, position)?;
        let total = self.projection.series.slots();
        // Only bounded non-freehand drafts use shaping. Projecting a whole
        // brush stroke here would allocate and walk it up to three times/frame.
        let (shaped, level_price) = match (drawings.draft(), band.scale.as_ref()) {
            (Some(draft), Some(scale)) if draft.tool == tool && !tool.freehand() => {
                let placed = self.projection.projected_drawing_points(
                    draft,
                    self.history_right,
                    total,
                    scale,
                );
                let shaped = tool.pending_anchor(&placed, position, self.constrain);
                // A levelled far end copies the near price; a screen trip lands ulps off.
                let level = match (self.constrain, placed.as_slice(), draft.points.first()) {
                    (Constrain::Level, [start], Some(near)) if shaped.y == start.y => {
                        Some(near.price)
                    }
                    _ => None,
                };
                (shaped, level)
            }
            _ => (position, None),
        };
        // Do not re-clamp shaped output: the tool's pixel floor prevents a
        // degenerate shape and may deliberately extend just outside the band.
        let mut point = self.projection.drawing_point_at(
            shaped,
            self.history_right,
            total,
            self.magnet,
            tool.anchor_snap(),
            band,
        )?;
        if let Some(price) = level_price {
            point.price = price;
        }
        Some((band, point))
    }
}

#[cfg(test)]
#[path = "placing_tests.rs"]
mod placing_tests;
