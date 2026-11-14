use super::*;
#[test]
fn only_the_exposed_same_process_window_can_receive_a_drop_even_when_strips_overlap() {
    use quantick_workspace::arrangement::{TabId, windows::TabWindows};
    use quantick_workspace::lifetime::WorkspaceId;
    let model = TabWindows::new(WorkspaceId(1), TabId(1));
    let child = WindowId(17);
    let bounds = tab_drag::Rect {
        min: Point { x: 0.0, y: 0.0 },
        max: Point { x: 200.0, y: 52.0 },
    };
    let mut native = NativeWindows::default();
    for (window, tab) in [(WindowId::ROOT, TabId(1)), (child, TabId(2))] {
        native.strips.insert(
            window,
            Strip {
                window,
                bounds,
                chrome: bounds,
                chips: vec![(tab, 0, bounds)],
                exclusions: vec![],
                order: vec![tab],
            },
        );
    }
    native.exposed_for_test = Some(child);
    let point = Point { x: 100.0, y: 10.0 };
    let candidates = eligible_strips(&native, point).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].window, child,
        "window order and pointer capture cannot choose an occluded sibling"
    );
    let drop = tab_drag::destination(model.begin_drag(TabId(1)).unwrap(), point, &candidates);
    assert_eq!(drop.target, TransferTarget::Window(child));
    let all: Vec<_> = native.strips.values().cloned().collect();
    assert!(
        tab_drag::eligible_strips(
            model.begin_drag(TabId(1)).ok(),
            point,
            &all,
            tab_drag::Exposure::Unavailable
        )
        .is_none(),
        "an unsupported native target cannot silently detach or choose an overlapping sibling"
    );
    let only_source = &all[..1];
    let fallback = tab_drag::eligible_strips(
        model.begin_drag(TabId(1)).ok(),
        point,
        only_source,
        tab_drag::Exposure::Unavailable,
    )
    .unwrap();
    assert_eq!(
        fallback.len(),
        1,
        "same-window reorder remains available without an OS z-order API"
    );
    assert_eq!(
        tab_drag::destination(model.begin_drag(TabId(1)).unwrap(), point, &fallback).target,
        TransferTarget::Window(WindowId::ROOT)
    );
    native.exposed_for_test = None;
    assert!(
        eligible_strips(&native, point).unwrap().is_empty(),
        "foreign windows and desktop are never attach targets"
    );
}
