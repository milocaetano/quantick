use super::*;
use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, NewDrawing};
use quantick_workspace::layout_document::LayoutId;

const LEVELS: LayoutId = LayoutId(1);
const FLOWS: LayoutId = LayoutId(2);
const T0: i64 = 1_700_000_000_000;

fn level(drawings: &mut Drawings<DrawingTool>, time_ms: Option<i64>) -> DrawingId {
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    let point = ChartPoint::at_time(3.0, 100.0, time_ms);
    let fresh = |tool: DrawingTool| NewDrawing {
        style: DrawingStyle::default(),
        payload: tool.default_payload(),
    };
    assert!(drawings.place_with(tool, &DrawingBand::Price, point, fresh));
    drawings.items().last().unwrap().id
}

fn input(id: DrawingId, scope: ScopeName) -> DrawingScopeInput {
    DrawingScopeInput {
        tab_id: WireU64::new(1),
        pane_id: WireU64::new(2),
        drawing_id: WireU64::new(id.0),
        scope,
        all_windows: None,
    }
}

fn windows(id: DrawingId, scope: ScopeName, on: bool) -> DrawingScopeInput {
    DrawingScopeInput {
        all_windows: Some(on),
        ..input(id, scope)
    }
}

#[test]
fn this_chart_pins_to_the_bars_and_a_repeat_changes_nothing() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, Some(T0));
    let wide = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "time:15m").unwrap();
    assert!(wide.changed && wide.pinned_bars.is_none());
    let ask = input(id, ScopeName::ThisChart);

    let set = set_scope(&mut drawings, &ask, "time:15m").unwrap();
    assert!(set.changed);
    assert_eq!(set.pinned_bars.as_deref(), Some("time:15m"));
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);

    let revision = drawings.revision();
    let again = set_scope(&mut drawings, &ask, "time:15m").unwrap();
    assert!(!again.changed, "naming the state it stands in is a no-op");
    assert_eq!(again.pinned_bars.as_deref(), Some("time:15m"));
    assert_eq!(drawings.revision(), revision, "nothing to write");

    let back = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "time:15m").unwrap();
    assert!(back.changed && back.pinned_bars.is_none());
    assert!(drawings.undo(), "one undo step per change");
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
}

#[test]
fn all_charts_on_a_drawing_without_market_time_is_refused_with_a_reason() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, None);
    let error = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "tick:100").unwrap_err();
    assert_eq!(
        error.code,
        quantick_control::error::ControlError::invalid_request("").code
    );
    let details = error.context.details.unwrap();
    assert_eq!(details["reason"], "anchor_without_market_time");
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
}

#[test]
fn an_id_the_pane_does_not_hold_is_refused() {
    let mut drawings = Drawings::default();
    let error = set_scope(
        &mut drawings,
        &input(DrawingId(9), ScopeName::ThisChart),
        "tick:100",
    )
    .unwrap_err();
    assert!(error.message.contains("holds no drawing with id 9"));
}

/// A copy of a mark on all timeframes waits on its chart until dated; the call
/// keeping it there is a decision that holds once it is dated.
#[test]
fn this_chart_on_an_undated_copy_holds_once_it_is_dated() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, Some(T0));
    let _ = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "tick:100").unwrap();
    let source = drawings.items()[0].clone();
    let copy = drawings.paste(&source, 2.0);
    let ask = input(copy, ScopeName::ThisChart);
    let set = set_scope(&mut drawings, &ask, "tick:100").unwrap();
    assert!(set.changed, "the call decided where the copy shows");
    assert_eq!(set.pinned_bars.as_deref(), Some("tick:100"));

    let index = drawings.index_of(copy).unwrap();
    drawings.set_times(index, &[Some(1_700_000_060_000)]);
    assert!(!drawings.items()[index].all_timeframes());
}

#[test]
fn a_new_drawing_reads_back_kept_to_its_chart() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, Some(T0));
    let set = set_scope(&mut drawings, &input(id, ScopeName::ThisChart), "tick:100").unwrap();
    assert!(!set.changed, "kept to its chart from the start");
    assert!(!set.all_windows);
}

#[test]
fn all_windows_is_set_read_back_and_undone_as_one_step() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    let id = level(&mut drawings, Some(T0));
    let set = set_scope(
        &mut drawings,
        &windows(id, ScopeName::AllCharts, true),
        "tick:100",
    )
    .unwrap();
    assert!(set.changed);
    assert!(set.all_windows);
    assert_eq!(set.layout_id.map(WireU64::get), Some(LEVELS.0));
    assert!(drawings.items()[0].in_all_windows());

    let again = set_scope(
        &mut drawings,
        &windows(id, ScopeName::AllCharts, true),
        "tick:100",
    )
    .unwrap();
    assert!(!again.changed, "a repeat is a no-op");
    let kept = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "tick:100").unwrap();
    assert!(kept.all_windows, "absent leaves it as it stands");

    assert!(drawings.undo(), "scope and all windows in one step");
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
    assert!(!drawings.items()[0].all_windows);
}

#[test]
fn all_windows_beside_this_chart_is_refused_with_a_reason() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, Some(T0));
    let error = set_scope(
        &mut drawings,
        &windows(id, ScopeName::ThisChart, true),
        "tick:100",
    )
    .unwrap_err();
    let details = error.context.details.unwrap();
    assert_eq!(details["reason"], "all_windows_needs_all_charts");
    assert!(!drawings.items()[0].all_windows, "nothing moved");
}

/// A pane on another layout holds the mark for merges and does not show it:
/// the call refuses it there, as its list does not name it.
#[test]
fn a_drawing_the_panes_layout_does_not_show_is_refused() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    let id = level(&mut drawings, Some(T0));
    let _ = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "tick:100").unwrap();
    drawings.set_layout(Some(FLOWS));
    let error = set_scope(
        &mut drawings,
        &windows(id, ScopeName::AllCharts, true),
        "tick:100",
    )
    .unwrap_err();
    assert!(error.message.contains("holds no drawing with id"));
}
