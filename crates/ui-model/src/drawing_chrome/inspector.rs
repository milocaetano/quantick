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
    placed.into_iter().chain([locked, hidden].into_iter().flatten().map(String::from)).collect()
}

const LOCKED_NOTE: &str = "Locked - protected from accidental moves. Style stays editable.";
const HIDDEN_NOTE: &str = "Hidden - Show brings it back.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_s_notes_name_its_author_then_its_lock_then_its_eye() {
        use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingAuthor, DrawingBand, Drawings};
        let mut store = Drawings::default();
        let note = DRAWING_TOOLS.into_iter().find(|tool| tool.holds_text()).unwrap();
        store.place_on(note, &DrawingBand::Price, ChartPoint::at_time(1.0, 10.0, None));
        let mut drawing = store.items()[0].clone();
        assert!(object_notes(&drawing).is_empty(), "the trader's own, unlocked, shown");
        drawing.author = Some(DrawingAuthor {
            actor_kind: "agent".to_owned(),
            client_name: "assistant".to_owned(),
        });
        (drawing.locked, drawing.hidden) = (true, true);
        assert_eq!(
            object_notes(&drawing),
            ["Placed by assistant (agent) - not by you.", LOCKED_NOTE, HIDDEN_NOTE]
        );
    }
    /// Automatic placement is not a preference, so it is not written down.
    /// Only a hand — or the hook standing in for one — makes a position the
    /// file has to remember.
    #[test]
    fn only_a_hand_placed_position_reaches_the_file() {
        let mut inspector = State::default();
        assert_eq!(inspector.remembered_position(), None);
        inspector.pos = Some([10.0, 20.0]);
        assert_eq!(
            inspector.remembered_position(),
            None,
            "a placed position is the app's decision, not the trader's"
        );
        inspector.place_by_hand([30.0, 40.0]);
        assert_eq!(inspector.remembered_position(), Some([30.0, 40.0]));
        assert!(
            !inspector.take_position_dirty(),
            "parking alone owes nothing: the drag calls it on every frame the              hand moves, and the file write is owed when the hand comes off"
        );
        inspector.position_dirty = true;
        assert!(inspector.take_position_dirty(), "and then exactly once");
        assert!(!inspector.take_position_dirty());
    }

    /// A file with a nonsense position leaves automatic placement in charge
    /// rather than parking the panel at an invented pixel: NaN survives
    /// `clamp`, so neither coordinate may slip through, and neither the point
    /// nor the hand's claim may outlive the refusal.
    #[test]
    fn a_nonsense_remembered_position_is_refused() {
        let mut inspector = State::default();
        for pair in [
            [f32::NAN, 40.0],
            [30.0, f32::NAN],
            [f32::INFINITY, 40.0],
            [30.0, f32::INFINITY],
        ] {
            inspector.place_by_hand([30.0, 40.0]);
            inspector.restore_position(Some(pair));
            assert_eq!(inspector.pos, None, "{pair:?} is not a position");
            assert!(!inspector.moved, "{pair:?} hands back automatic placement");
        }
        inspector.restore_position(Some([12.0, 34.0]));
        assert_eq!(inspector.remembered_position(), Some([12.0, 34.0]));
    }
}
