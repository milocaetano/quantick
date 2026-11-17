use super::*;
use crate::{ChartPoint, DrawingTool};

fn line() -> DrawingTool {
    DrawingTool::by_id("trend-line").expect("registered")
}

/// `count` finished trend lines, none selected.
fn lines(count: usize) -> Drawings<DrawingTool> {
    let mut store = Drawings::default();
    for index in 0..count {
        let bar = index as f32 * 10.0;
        assert!(!store.place(line(), ChartPoint::at(bar, 100.0)));
        assert!(store.place(line(), ChartPoint::at(bar + 5.0, 110.0)));
    }
    store.select(None);
    store
}

/// The inspector's "All windows" flips what the selection shows, one
/// undo step each way; with nothing selected it does nothing.
#[test]
fn toggling_all_windows_flips_the_selection_and_undoes() {
    let mut store = Drawings::default();
    store.set_layout(Some(quantick_workspace::layout_document::LayoutId(1)));
    let time = |bar: i64| Some(1_700_000_000_000 + bar * 60_000);
    assert!(!store.place(line(), ChartPoint::at_time(0.0, 100.0, time(0))));
    assert!(store.place(line(), ChartPoint::at_time(5.0, 110.0, time(5))));
    store.items_mut()[0].scope = crate::DrawingScope::AllCharts;
    store.select(None);
    update(&mut store, DrawingIntent::ToggleAllWindowsSelected);
    assert!(!store.items()[0].all_windows, "nothing selected");
    store.select(Some(0));
    update(&mut store, DrawingIntent::ToggleAllWindowsSelected);
    assert!(store.in_all_windows(0));
    update(&mut store, DrawingIntent::ToggleAllWindowsSelected);
    assert!(!store.in_all_windows(0));
    assert!(store.undo());
    assert!(store.in_all_windows(0));
}

#[test]
fn placing_over_the_traders_draft_is_refused_and_leaves_it_alone() {
    let mut store = Drawings::default();
    assert!(!store.place(line(), ChartPoint::at(0.0, 100.0)));
    let placement = || {
        placement(
            line(),
            vec![ChartPoint::at(1.0, 100.0), ChartPoint::at(2.0, 101.0)],
        )
    };
    let refused = [DrawingEffect::NotPlaced {
        index: 0,
        failure: PlacementFailure::DraftInProgress,
    }];
    assert_eq!(
        update(&mut store, DrawingIntent::Place(placement())),
        refused
    );
    assert_eq!(
        update(&mut store, DrawingIntent::PlaceAll(vec![placement()])),
        refused
    );
    assert!(store.draft().is_some(), "the hand-drawn draft survives");
    assert!(store.items().is_empty());
}

#[test]
fn deleting_a_named_mark_says_its_name_and_an_unnamed_one_its_tool() {
    let mut store = lines(2);
    store.rename_at(0, "congestion 108k");
    store.select(Some(0));
    let id = store.items()[0].id;
    assert_eq!(
        update(&mut store, DrawingIntent::DeleteSelected { force: false }),
        [DrawingEffect::Deleted {
            id,
            label: "congestion 108k".to_owned()
        }]
    );
    store.select(Some(0));
    let id = store.items()[0].id;
    assert_eq!(
        update(&mut store, DrawingIntent::DeleteSelected { force: false }),
        [DrawingEffect::Deleted {
            id,
            label: line().name().to_owned()
        }]
    );
    assert!(store.items().is_empty());
    assert_eq!(
        update(&mut store, DrawingIntent::DeleteSelected { force: false }),
        [],
        "nothing selected, nothing to say"
    );
}

#[test]
fn a_locked_mark_asks_before_it_goes_and_force_takes_it() {
    let mut store = lines(1);
    store.select(Some(0));
    update(&mut store, DrawingIntent::ToggleLockSelected);
    assert!(store.items()[0].locked);
    assert_eq!(
        update(&mut store, DrawingIntent::DeleteSelected { force: false }),
        [DrawingEffect::ConfirmDelete]
    );
    assert_eq!(store.items().len(), 1, "a refused delete removes nothing");
    assert!(matches!(
        update(&mut store, DrawingIntent::DeleteSelected { force: true }).as_slice(),
        [DrawingEffect::Deleted { .. }]
    ));
}

#[test]
fn undo_and_redo_replace_the_marks_and_say_so() {
    let mut store = lines(1);
    store.select(Some(0));
    update(&mut store, DrawingIntent::DeleteSelected { force: false });
    assert_eq!(
        update(&mut store, DrawingIntent::Undo),
        [DrawingEffect::HistoryReplaced]
    );
    assert_eq!(store.items().len(), 1, "undo brings the mark back");
    assert_eq!(
        update(&mut store, DrawingIntent::Redo),
        [DrawingEffect::HistoryReplaced]
    );
    assert!(store.items().is_empty(), "redo takes it again");
}

#[test]
fn the_toggles_flip_the_selected_mark_and_leave_an_unselected_store_alone() {
    let mut store = lines(1);
    update(&mut store, DrawingIntent::ToggleHideSelected);
    assert!(!store.items()[0].hidden, "no selection, no edit");
    store.select(Some(0));
    update(&mut store, DrawingIntent::ToggleHideSelected);
    assert!(store.items()[0].hidden);
    update(&mut store, DrawingIntent::ToggleHideSelected);
    assert!(!store.items()[0].hidden);
}

#[test]
fn stepping_back_a_draft_drops_one_anchor_then_the_draft() {
    let tool = DrawingTool::by_id("parallel-channel").expect("registered");
    let mut store = Drawings::default();
    assert!(!store.place(tool, ChartPoint::at(0.0, 100.0)));
    assert!(!store.place(tool, ChartPoint::at(5.0, 110.0)));
    update(&mut store, DrawingIntent::StepBackDraft);
    assert_eq!(store.draft().map(|draft| draft.points.len()), Some(1));
    update(&mut store, DrawingIntent::StepBackDraft);
    assert!(store.draft().is_none());
}

fn placement(tool: DrawingTool, points: Vec<ChartPoint>) -> Placement {
    Placement {
        tool,
        points,
        band: DrawingBand::Price,
        opening: crate::new_drawing_from_defaults(&crate::NullPresetHost, tool),
        author: Some(DrawingAuthor {
            actor_kind: "agent".to_owned(),
            client_name: "assistant".to_owned(),
        }),
        name: None,
        text: None,
    }
}

#[test]
fn a_placement_on_a_band_its_tool_cannot_hold_is_refused_not_moved() {
    let mut store = Drawings::default();
    let profile = DrawingTool::by_id("fixed-range-profile").expect("registered");
    let cvd = DrawingBand::Indicator(crate::PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    });
    let mut on_cvd = placement(
        profile,
        vec![ChartPoint::at(1.5, -500.0), ChartPoint::at(4.5, 800.0)],
    );
    on_cvd.band = cvd;
    let effects = update(&mut store, DrawingIntent::Place(on_cvd));
    assert!(matches!(
        effects[..],
        [DrawingEffect::NotPlaced {
            failure: PlacementFailure::BandRefused,
            ..
        }]
    ));
    assert!(store.items().is_empty() && store.draft().is_none());
}

#[test]
fn holds_band_keeps_time_only_tools_on_every_band_and_price_tools_on_price() {
    let cvd = DrawingBand::Indicator(crate::PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    });
    let tool = |id| DrawingTool::by_id(id).expect("registered");
    assert!(tool("vertical-line").holds_band(&DrawingBand::Price));
    assert!(tool("vertical-line").holds_band(&cvd));
    assert!(tool("trend-line").holds_band(&cvd));
    assert!(tool("fixed-range-profile").holds_band(&DrawingBand::Price));
    assert!(!tool("fixed-range-profile").holds_band(&cvd));
}

fn level(price: f64) -> Placement {
    let tool = DrawingTool::by_id("horizontal-line").expect("registered");
    placement(tool, vec![ChartPoint::at(1.5, price)])
}

#[test]
fn a_placement_lands_stamped_and_selected_and_says_its_label() {
    let mut store = lines(1);
    let text = DrawingTool::by_id("text").expect("registered");
    let mut note = placement(text, vec![ChartPoint::at(2.5, 105.0)]);
    note.name = Some("absorption".to_owned());
    note.text = Some("watch the bid".to_owned());
    let effects = update(&mut store, DrawingIntent::Place(note));
    let placed = &store.items()[1];
    assert_eq!(
        effects,
        [DrawingEffect::Placed {
            id: placed.id,
            label: "absorption".to_owned()
        }]
    );
    assert_eq!(store.selected(), Some(1));
    assert_eq!(
        placed
            .author
            .as_ref()
            .map(|author| author.client_name.as_str()),
        Some("assistant")
    );
    assert_eq!(
        placed.tool.inline_text(placed.payload.as_ref()),
        Some("watch the bid")
    );
}

#[test]
fn a_placement_lands_on_the_indicator_band_it_names() {
    let cvd = DrawingBand::Indicator(crate::PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    });
    let mut store = Drawings::default();
    let mut leg = placement(
        line(),
        vec![ChartPoint::at(1.5, -1_250.0), ChartPoint::at(4.5, 830.0)],
    );
    leg.band = cvd.clone();
    let effects = update(&mut store, DrawingIntent::Place(leg));
    assert!(matches!(effects.as_slice(), [DrawingEffect::Placed { .. }]));
    assert_eq!(store.items()[0].band, cvd, "a CVD leg stays a CVD leg");
    assert_eq!(store.items()[0].points[0].price, -1_250.0);
}

#[test]
fn a_placement_short_of_anchors_leaves_no_draft_and_says_so() {
    let mut store = lines(1);
    let effects = update(
        &mut store,
        DrawingIntent::Place(placement(line(), vec![ChartPoint::at(1.0, 100.0)])),
    );
    assert_eq!(
        effects,
        [DrawingEffect::NotPlaced {
            index: 0,
            failure: PlacementFailure::Incomplete
        }]
    );
    assert!(
        store.draft().is_none(),
        "a partial draft would block the hand"
    );
    assert_eq!(store.items().len(), 1);
}

#[test]
fn placing_all_is_one_undo_step_and_a_failure_keeps_nothing() {
    let mut store = lines(1);
    let depth = store.undo_depth();
    let effects = update(
        &mut store,
        DrawingIntent::PlaceAll(vec![level(101.0), level(99.0)]),
    );
    assert!(matches!(
        effects.as_slice(),
        [DrawingEffect::Placed { .. }, DrawingEffect::Placed { .. }]
    ));
    assert_eq!(store.undo_depth(), depth + 1, "one step for both");
    let before = store.items().to_vec();
    let short = placement(line(), vec![ChartPoint::at(1.0, 100.0)]);
    let effects = update(
        &mut store,
        DrawingIntent::PlaceAll(vec![level(98.0), short]),
    );
    assert_eq!(
        effects,
        [DrawingEffect::NotPlaced {
            index: 1,
            failure: PlacementFailure::Incomplete
        }]
    );
    assert_eq!(store.items(), &before[..], "no orphan first level");
    assert_eq!(store.undo_depth(), depth + 1, "no step brings it back");
}

#[test]
fn removal_takes_an_operators_mark_and_refuses_the_traders() {
    let mut store = lines(1);
    let hand = store.items()[0].id;
    update(&mut store, DrawingIntent::Place(level(100.0)));
    let authored = store.items()[1].id;
    assert_eq!(
        update(&mut store, DrawingIntent::RemoveAuthored(hand)),
        [DrawingEffect::RemovalRefused { id: hand }]
    );
    assert_eq!(
        update(&mut store, DrawingIntent::RemoveAuthored(authored)),
        [DrawingEffect::Removed { id: authored }]
    );
    assert_eq!(store.items().len(), 1, "the hand-drawn mark stays");
    assert_eq!(
        update(&mut store, DrawingIntent::RemoveAuthored(authored)),
        []
    );
}

/// A projection completes on its two-anchor leg, and one sent in the
/// legacy three-anchor shape is placed whole — origin C kept, no second
/// draft opened. A count the tool never accepted still fails clean.
#[test]
fn a_projection_places_from_two_anchors_or_its_legacy_three() {
    let projection = DrawingTool::by_id("fib-extension").expect("registered");
    let at = |bar: f32, price: f64| ChartPoint::at(bar, price);
    let mut store = Drawings::default();

    let effects = update(
        &mut store,
        DrawingIntent::Place(placement(projection, vec![at(1.0, 100.0), at(2.0, 150.0)])),
    );
    assert!(matches!(effects[..], [DrawingEffect::Placed { .. }]));
    assert_eq!(store.items()[0].points.len(), 2);

    let legacy = vec![at(1.0, 100.0), at(2.0, 150.0), at(3.0, 120.0)];
    let effects = update(
        &mut store,
        DrawingIntent::Place(placement(projection, legacy.clone())),
    );
    assert!(matches!(effects[..], [DrawingEffect::Placed { .. }]));
    assert_eq!(store.items()[1].points, legacy, "the legacy origin is kept");
    assert!(store.draft().is_none(), "no second draft was opened");

    let effects = update(
        &mut store,
        DrawingIntent::Place(placement(
            projection,
            vec![at(1.0, 1.0), at(2.0, 2.0), at(3.0, 3.0), at(4.0, 4.0)],
        )),
    );
    assert!(!matches!(effects[..], [DrawingEffect::Placed { .. }]));
    assert!(store.draft().is_none());
}
