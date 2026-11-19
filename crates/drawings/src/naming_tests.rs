use crate::{ChartPoint, DRAWING_TOOLS, DrawingTool, Drawings};
use quantick_workspace::drawing_names::{LINE_PREFIX, LINE_TOOL_IDS, NameFloors};

fn tool(id: &str) -> DrawingTool {
    DrawingTool::by_id(id).expect("a registered tool")
}

fn at(bar: f32, price: f64) -> ChartPoint {
    ChartPoint::at_time(bar, price, Some(1_000 + bar as i64))
}

fn place(drawings: &mut Drawings<DrawingTool>, tool: DrawingTool) {
    for bar in 0..tool.required_points() {
        drawings.place(tool, at(bar as f32, 100.0 + bar as f64));
    }
}

fn names(drawings: &Drawings<DrawingTool>) -> Vec<&str> {
    let items = drawings.items().iter();
    items.map(|mark| mark.name.as_deref().unwrap()).collect()
}

#[test]
fn a_placed_mark_is_named_by_its_kind_and_count() {
    let mut drawings = Drawings::default();
    place(&mut drawings, tool("rectangle"));
    place(&mut drawings, tool("trend-line"));
    place(&mut drawings, tool("horizontal-line"));
    place(&mut drawings, tool("rectangle"));
    assert_eq!(names(&drawings), ["zone-1", "line-1", "line-2", "zone-2"]);
}

#[test]
fn a_deleted_name_is_not_reborn_and_a_reserved_floor_is_skipped() {
    let mut drawings = Drawings::default();
    place(&mut drawings, tool("rectangle"));
    place(&mut drawings, tool("rectangle"));
    drawings.remove_by_id(drawings.items()[1].id);
    place(&mut drawings, tool("rectangle"));
    assert_eq!(names(&drawings), ["zone-1", "zone-3"]);
    drawings.reserve_names(&NameFloors::from([("zone".to_owned(), 7)]));
    place(&mut drawings, tool("rectangle"));
    assert_eq!(names(&drawings)[2], "zone-8");
}

#[test]
fn the_traders_name_is_kept_and_clearing_it_gives_a_generated_one() {
    let mut drawings = Drawings::default();
    place(&mut drawings, tool("rectangle"));
    drawings.rename_at(0, "  congestion 108k ");
    place(&mut drawings, tool("rectangle"));
    assert_eq!(names(&drawings), ["congestion 108k", "zone-2"]);
    drawings.rename_at(0, "   ");
    assert_eq!(names(&drawings), ["zone-3", "zone-2"]);
}

#[test]
fn a_pasted_copy_takes_a_name_of_its_own() {
    let mut drawings = Drawings::default();
    place(&mut drawings, tool("trend-line"));
    let source = drawings.items()[0].clone();
    let _ = drawings.paste(&source, 2.0);
    assert_eq!(names(&drawings), ["line-1", "line-2"]);
}

#[test]
fn the_line_prefix_covers_exactly_the_lines_family() {
    let lines = crate::tools::LINES_FAMILY.id;
    let mut family: Vec<&str> = DRAWING_TOOLS
        .into_iter()
        .filter(|tool| tool.family().is_some_and(|family| family.id == lines))
        .map(DrawingTool::id)
        .collect();
    family.sort_unstable();
    assert_eq!(family, LINE_TOOL_IDS, "every {LINE_PREFIX} tool, no other");
}
