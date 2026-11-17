use super::*;
#[test]
fn requested_default_is_distinct_from_no_request() {
    let mut pane = PaneLayout::<()>::default();
    pane.request_opening(Some(LayoutId(9)));
    assert_eq!(pane.layout_id(), Some(LayoutId(9)));
    pane.request_opening(None);
    assert_eq!(pane.opening, Some(None));
    assert_eq!(pane.layout_id(), None);
    assert!(!pane.seeded());
    assert!(!pane.drawings_dirty(3));
}

#[test]
fn a_pane_holds_the_marks_of_its_feed_at_either_symbol() {
    let mut pane = PaneLayout::<()>::default();
    assert!(!pane.holds_marks_of("f", ["a", "b"]), "nothing held");
    pane.drawings_source = Some(("f".to_owned(), "b".to_owned()));
    assert!(pane.holds_marks_of("f", ["a", "b"]));
    assert!(pane.holds_marks_of("f", ["b", "c"]));
    assert!(!pane.holds_marks_of("f", ["a", "c"]), "another symbol");
    assert!(!pane.holds_marks_of("g", ["a", "b"]), "another feed");
}

#[test]
fn only_the_flow_chart_moves_its_bars_in_place_and_only_when_alike() {
    let keyed = |role| PaneLayout::<u32> {
        drawings_key: Some(DrawingKey {
            market: crate::market_settings::MarketKey {
                venue: "v".to_owned(),
                asset: "a".to_owned(),
            },
            role,
        }),
        drawings_spec: Some(1),
        ..PaneLayout::default()
    };
    let mut flow = keyed(PaneRole::Flow);
    assert!(!flow.recut_in_place(2, 0, |_, _, _| false), "a pinned mark");
    assert_eq!(flow.drawings_spec, Some(1));
    assert!(flow.recut_in_place(3, 0, |_, held, now| (*held, *now) == (1, 3)));
    assert_eq!(flow.drawings_spec, Some(3));
    assert!(!flow.recut_in_place(3, 0, |_, _, _| true), "nothing moved");

    let bars = "time:1m".to_owned();
    let mut context = keyed(PaneRole::Context { bars });
    assert!(
        !context.recut_in_place(2, 0, |_, _, _| true),
        "a context chart's key names its bars"
    );
}

/// A chart held at its old bars (a strategy keeps it) asks the store once
/// per cut and store revision, not every frame: the answer costs a
/// formatted rule, and only a write to the store can change it.
#[test]
fn a_recut_that_cannot_move_in_place_is_asked_once() {
    let mut flow = PaneLayout::<u32> {
        drawings_key: Some(DrawingKey::of_pane("v", "a".to_owned(), None)),
        drawings_spec: Some(1),
        ..PaneLayout::default()
    };
    let mut asked = 0;
    for _ in 0..3 {
        assert!(!flow.recut_in_place(2, 7, |_, _, _| {
            asked += 1;
            false
        }));
    }
    assert_eq!(asked, 1);
    assert!(
        flow.recut_in_place(2, 8, |_, _, _| true),
        "the store moved: asked again"
    );
    assert!(
        flow.recut_in_place(3, 8, |_, _, _| true),
        "a new cut is asked"
    );
}

#[test]
fn an_unkeyed_pane_never_asks_for_a_new_key() {
    let pane = PaneLayout::<u32> {
        drawings_source: Some(("f".to_owned(), "s".to_owned())),
        drawings_spec: Some(1),
        ..PaneLayout::default()
    };
    assert!(
        !pane.needs_rekey("other", "s", Some(&2)),
        "not seeded, no key"
    );
}
