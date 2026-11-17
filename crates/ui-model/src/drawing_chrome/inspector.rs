//! Inspector preferences and deterministic selection/placement transitions.
use super::{DrawingEdit, InspectorTab, Shared};
use quantick_chart_interaction::panel_placement::{
    Area, NEIGHBOURHOOD_PX, PanelPlacement, PanelSpacing, place_beside,
};

pub const AUTO_PIN_CHART_WIDTH_PX: f32 = 1180.0;
pub const MIN_WIDTH_PX: f32 = 300.0;
pub const OBJECT_GAP_PX: f32 = 12.0;

#[derive(Default)]
pub struct State {
    pub prepared: Option<bool>,
    pub tab: InspectorTab,
    pub moved: bool,
    pub pos: Option<[f32; 2]>,
    pub position_dirty: bool,
    pub pin_touched: bool,
    pub settle_frame: bool,
}

impl State {
    /// A hand's point survives subsequent automatic layout repair.
    pub fn place_by_hand(&mut self, position: [f32; 2]) {
        self.pos = Some(position);
        self.moved = true;
    }
    pub fn remembered_position(&self) -> Option<[f32; 2]> {
        self.pos.filter(|_| self.moved)
    }
    pub fn restore_position(&mut self, remembered: Option<[f32; 2]>) {
        match remembered.filter(|[x, y]| x.is_finite() && y.is_finite()) {
            Some(position) => self.place_by_hand(position),
            None => {
                self.pos = None;
                self.moved = false;
            }
        }
    }
    pub fn take_position_dirty(&mut self) -> bool {
        std::mem::take(&mut self.position_dirty)
    }
    /// Return whether the selection changed, or wait for pinning/layout to settle.
    pub fn prepare_floating(
        &mut self,
        shared: &mut Shared,
        index: usize,
        chart_width: Option<f32>,
    ) -> Option<bool> {
        if !shared.open || std::mem::take(&mut self.settle_frame) {
            return None;
        }
        let changed = shared.last_selection != Some(index);
        if changed
            && !self.pin_touched
            && !self.moved
            && chart_width.is_some_and(|width| width < AUTO_PIN_CHART_WIDTH_PX)
        {
            shared.pinned = true;
            return None;
        }
        shared.last_selection = Some(index);
        Some(changed)
    }
    pub fn reset_automatic(&mut self, placed: Option<[f32; 2]>) {
        self.moved = false;
        self.pos = placed;
        self.position_dirty = true;
    }
    /// Begin at the displayed point, then accumulate against the remembered point.
    pub fn drag(&mut self, drawn_start: Option<[f32; 2]>, delta: [f32; 2]) {
        if let Some(drawn) = drawn_start {
            self.pos = Some(drawn);
        }
        match self.pos {
            Some([x, y]) => self.place_by_hand([x + delta[0], y + delta[1]]),
            None => self.moved = true,
        }
    }
}

/// A lost or changed selection settles the edit against its original owner.
pub fn prologue(shared: &mut Shared, selected: Option<usize>) -> Option<Box<DrawingEdit>> {
    if selected.is_none() {
        shared.delete_confirm = false;
        shared.last_selection = None;
        return shared.edit_baseline.take().map(Box::new);
    }
    if shared
        .edit_baseline
        .as_ref()
        .is_some_and(|edit| Some(edit.index) != selected)
    {
        return shared.edit_baseline.take().map(Box::new);
    }
    None
}

pub fn placement(chart: Area, bbox: Area, size: [f32; 2]) -> PanelPlacement {
    place_beside(
        chart,
        bbox,
        size[0],
        size[1],
        PanelSpacing {
            neighbourhood: NEIGHBOURHOOD_PX,
            gap: OBJECT_GAP_PX,
            min_width: MIN_WIDTH_PX,
        },
    )
}

/// The object's standing, in words, as the inspector lists it: who placed it,
/// and whether it is locked or hidden. Data honesty, where the trader decides
/// what to do with the object: an assistant's mark never passes for their own.
#[must_use]
pub fn object_notes<T>(drawing: &quantick_drawings::Drawing<T>) -> Vec<String> {
    let author = drawing.author.as_ref();
    let placed = author.map(|author| format!("Placed by {} - not by you.", author.label()));
    let locked = drawing.locked.then_some(LOCKED_NOTE);
    let hidden = drawing.hidden.then_some(HIDDEN_NOTE);
    placed
        .into_iter()
        .chain([locked, hidden].into_iter().flatten().map(String::from))
        .collect()
}

const LOCKED_NOTE: &str = "Locked - protected from accidental moves. Style stays editable.";
const HIDDEN_NOTE: &str = "Hidden - Show brings it back.";

#[cfg(test)]
#[path = "inspector_tests.rs"]
mod tests;
