//! The drawings scope lists what each pane shows: a mark on all timeframes
//! that another layout owns is held for merges, never listed there.
use super::*;
use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingScope, DrawingStyle, NewDrawing};
use quantick_workspace::layout_document::LayoutId;

const LEVELS: LayoutId = LayoutId(1);
const FLOWS: LayoutId = LayoutId(2);

struct Pane {
    indicators: IndicatorViews,
    drawings: Drawings<DrawingTool>,
}

impl AnalysisPane for Pane {
    fn pane_id(&self) -> u64 {
        7
    }

    fn indicators(&self) -> &IndicatorViews {
        &self.indicators
    }

    fn drawings(&self) -> &Drawings<DrawingTool> {
        &self.drawings
    }
}

fn pane(drawings: Drawings<DrawingTool>) -> Pane {
    Pane {
        indicators: IndicatorViews::default(),
        drawings,
    }
}

/// A dated level on all timeframes, owned by the store's layout.
fn level(drawings: &mut Drawings<DrawingTool>, price: f64) {
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    let point = ChartPoint::at_time(3.0, price, Some(1_700_000_000_000));
    let fresh = |tool: DrawingTool| NewDrawing {
        style: DrawingStyle::default(),
        payload: tool.default_payload(),
    };
    assert!(drawings.place_with(tool, &DrawingBand::Price, point, fresh));
    let index = drawings.items().len() - 1;
    let before = drawings.items()[index].clone();
    drawings.items_mut()[index].set_scope(DrawingScope::AllCharts);
    drawings.record_edit_of(index, before);
}

/// The same marks as a pane on `layout` holds them.
fn held_on(layout: LayoutId, owner: &Drawings<DrawingTool>) -> Drawings<DrawingTool> {
    let mut held = Drawings::default();
    held.set_layout(Some(layout));
    held.adopt(owner.items().iter().cloned());
    held
}

#[test]
fn a_pane_lists_only_the_marks_its_layout_shows_and_reads_all_windows() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level(&mut drawings, 100.0);
    level(&mut drawings, 90.0);
    assert!(drawings.set_all_windows_at(1, true));
    let mut held = held_on(FLOWS, &drawings);
    held.select(Some(1));

    let owner = pane_drawings(&pane(drawings), PaneSideDto::Time);
    assert_eq!(owner.drawing_count.get(), 2);
    assert!(!owner.drawings[0].all_windows);
    assert!(owner.drawings[1].all_windows);
    assert_eq!(owner.drawings[0].layout_id.map(WireU64::get), Some(LEVELS.0));

    let other = pane_drawings(&pane(held), PaneSideDto::Flow);
    assert_eq!(other.drawing_count.get(), 1, "the other layout's mark is not listed");
    assert_eq!(other.drawings.len(), 1);
    assert!(other.drawings[0].all_windows);
    assert_eq!(
        other.selected_index.map(WireU64::get),
        Some(0),
        "the selection names its row"
    );
}

/// The revision moves when the listed rows do, and only then: a mark the pane
/// holds for merges and does not show moves nothing, and one that comes into
/// the list, or the layout the list is for, moves it.
#[test]
fn the_revision_follows_the_listed_rows_and_not_the_held_ones() {
    let mut owner = Drawings::default();
    owner.set_layout(Some(LEVELS));
    level(&mut owner, 100.0);
    let mut held = held_on(FLOWS, &owner);
    let quiet = pane_revision(&pane(held_on(FLOWS, &owner)));
    assert!(quiet.drawings.is_empty(), "nothing listed");

    held.set_locked_at(0, true);
    assert_eq!(pane_revision(&pane(held)), quiet, "a held mark's lock lists nothing");

    assert!(owner.set_all_windows_at(0, true));
    let mut shown = held_on(FLOWS, &owner);
    let listed = pane_revision(&pane(held_on(FLOWS, &owner)));
    assert_ne!(listed, quiet, "a mark that comes into the list moves it");
    assert_eq!(listed.drawings.len(), 1);

    shown.set_layout(Some(LEVELS));
    assert_ne!(pane_revision(&pane(shown)), listed, "and so does the layout it lists for");
}
