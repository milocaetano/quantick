//! A floating pane held by its grip or by an edge, as plain data: where it
//! draws while held, the slot a dock would light, and what letting go does.
//!
//! The window feeds pointer positions in and carries the answer out; what a
//! drop inside the canvas does is [`drop_target`]'s alone, so the corner rule
//! has one home. A drop past the window's edge leaves it for an OS window.

use super::drop::{Corner, DockTarget, DropTarget, drop_target, kept_inside};
use super::state::{CanvasState, PaneMode};
use super::tree::{CanvasPoint, CanvasRect, PaneSlot};

/// The hint the canvas shows while a pane is dragged.
pub const DOCK_HINT: &str = "Drop on an edge or a chart to dock · anywhere else to float";
/// The pill on the slot a drop would dock into.
pub const DOCK_PILL: &str = "Dock here";
/// How far in from a floating pane's edge a press resizes it.
pub const RESIZE_GRIP_PX: f32 = 6.0;
/// The smallest a resize leaves a floating pane, in points.
pub const MIN_FLOAT_SIZE: (f32, f32) = (240.0, 160.0);

/// What letting go of a held pane does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropOutcome {
    /// Insert it into the docked tree there.
    Dock(DockTarget),
    /// Snap it floating into this corner.
    Park(Corner),
    /// Leave it floating at this rectangle.
    Float(CanvasRect),
    /// Open it in its own OS window at this rectangle, in window points.
    PopOut(CanvasRect),
}

/// The docked panes of `state` a drop can dock beside or swap with, where
/// they stand on `canvas`: every one the canvas lays out but a folded strip.
#[must_use]
pub fn open_panes(state: &CanvasState, canvas: CanvasRect) -> Vec<(PaneSlot, CanvasRect)> {
    let mut panes = state.rects(canvas);
    panes.retain(|(slot, _)| state.mode(*slot) != Some(PaneMode::Minimized));
    panes
}

/// Which edges of a floating pane a press holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResizeEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl ResizeEdges {
    /// The edges of `rect` within [`RESIZE_GRIP_PX`] of `pointer`, if any.
    #[must_use]
    pub fn at(rect: CanvasRect, pointer: CanvasPoint) -> Option<Self> {
        let grip = RESIZE_GRIP_PX;
        let outer = CanvasRect::new(
            rect.x - grip,
            rect.y - grip,
            rect.w + 2.0 * grip,
            rect.h + 2.0 * grip,
        );
        if !outer.contains(pointer) {
            return None;
        }
        let near = |value: f32, edge: f32| (value - edge).abs() <= grip;
        let edges = Self {
            left: near(pointer.x, rect.x),
            right: near(pointer.x, rect.right()),
            top: near(pointer.y, rect.y),
            bottom: near(pointer.y, rect.bottom()),
        };
        (edges != Self::default()).then_some(edges)
    }

    /// `from` with its held edges moved by `(dx, dy)`, never smaller than
    /// [`MIN_FLOAT_SIZE`]: a held edge stops where the opposite one would
    /// leave too little.
    #[must_use]
    pub fn resized(self, from: CanvasRect, (dx, dy): (f32, f32)) -> CanvasRect {
        let (min_w, min_h) = MIN_FLOAT_SIZE;
        let (mut left, mut top, mut right, mut bottom) =
            (from.x, from.y, from.right(), from.bottom());
        if self.left {
            left = (left + dx).min(right - min_w);
        }
        if self.right {
            right = (right + dx).max(left + min_w);
        }
        if self.top {
            top = (top + dy).min(bottom - min_h);
        }
        if self.bottom {
            bottom = (bottom + dy).max(top + min_h);
        }
        CanvasRect::new(left, top, right - left, bottom - top)
    }
}

/// A floating pane held by the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatGesture {
    /// Held by its grip: it moves with the pointer.
    Move {
        slot: PaneSlot,
        /// The pane as it was when grabbed.
        from: CanvasRect,
        /// Where the grab started.
        start: CanvasPoint,
    },
    /// Held by its `edges`: they follow the pointer.
    Resize {
        slot: PaneSlot,
        edges: ResizeEdges,
        from: CanvasRect,
        start: CanvasPoint,
    },
}

impl FloatGesture {
    /// The pane it holds.
    #[must_use]
    pub const fn slot(&self) -> PaneSlot {
        match self {
            Self::Move { slot, .. } | Self::Resize { slot, .. } => *slot,
        }
    }

    /// Where the pane draws with the pointer at `pointer`.
    #[must_use]
    pub fn rect(&self, pointer: CanvasPoint) -> CanvasRect {
        match *self {
            Self::Move { from, start, .. } => CanvasRect::new(
                from.x + pointer.x - start.x,
                from.y + pointer.y - start.y,
                from.w,
                from.h,
            ),
            Self::Resize {
                edges, from, start, ..
            } => edges.resized(from, (pointer.x - start.x, pointer.y - start.y)),
        }
    }

    /// What letting go at `pointer` over `state`'s `canvas` does: a resize
    /// keeps the new size; a move past `window` pops the pane out, and inside
    /// it the canvas's one drop rule decides, over the panes it lays out. A
    /// pane let go over the window outside the canvas (the toolbar) floats
    /// kept inside the canvas.
    #[must_use]
    pub fn release(
        &self,
        state: &CanvasState,
        canvas: CanvasRect,
        window: CanvasRect,
        pointer: CanvasPoint,
    ) -> DropOutcome {
        let rect = self.rect(pointer);
        if matches!(self, Self::Resize { .. }) {
            return DropOutcome::Float(rect);
        }
        if !window.contains(pointer) {
            return DropOutcome::PopOut(rect);
        }
        if !canvas.contains(pointer) {
            return DropOutcome::Float(kept_inside(rect, canvas));
        }
        match drop_target(canvas, &open_panes(state, canvas), pointer) {
            DropTarget::Dock(target) => DropOutcome::Dock(target),
            DropTarget::Park(corner) => DropOutcome::Park(corner),
            DropTarget::Float => DropOutcome::Float(rect),
        }
    }

    /// The slot a drop at `pointer` would dock the pane into, as the canvas
    /// would then lay it out; `None` when the drop would not dock.
    #[must_use]
    pub fn dock_preview(
        &self,
        state: &CanvasState,
        canvas: CanvasRect,
        window: CanvasRect,
        pointer: CanvasPoint,
    ) -> Option<CanvasRect> {
        let DropOutcome::Dock(target) = self.release(state, canvas, window, pointer) else {
            return None;
        };
        let slot = self.slot();
        let mut docked = state.clone();
        docked.dock(slot, target).ok()?;
        let rects = docked.rects(canvas);
        rects
            .into_iter()
            .find(|(held, _)| *held == slot)
            .map(|(_, rect)| rect)
    }
}
