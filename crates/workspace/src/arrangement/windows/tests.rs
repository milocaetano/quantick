use super::*;

fn three() -> TabWindows {
    let mut windows = TabWindows::new(crate::lifetime::WorkspaceId(0), TabId(4));
    windows.append(TabId(5));
    windows.append(TabId(6));
    windows
}

#[test]
fn reordering_honors_final_insertion_and_keeps_selection() {
    let mut windows = three();
    windows.select(TabId(5)).unwrap();
    windows
        .transfer(TabId(4), TransferTarget::Window(WindowId::ROOT), 2)
        .unwrap();
    let root = windows.window(WindowId::ROOT).unwrap();
    assert_eq!(root.tabs(), [TabId(5), TabId(6), TabId(4)]);
    assert_eq!(root.selected(), TabId(5));
    windows
        .transfer(TabId(4), TransferTarget::Window(WindowId::ROOT), 0)
        .unwrap();
    assert_eq!(
        windows.window(WindowId::ROOT).unwrap().tabs(),
        [TabId(4), TabId(5), TabId(6)]
    );
}

#[test]
fn detach_and_reattach_preserve_identity_and_independent_selection() {
    let mut windows = three();
    windows.select(TabId(5)).unwrap();
    let child = windows
        .transfer(TabId(5), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    assert_eq!(windows.window(WindowId::ROOT).unwrap().selected(), TabId(6));
    windows.select(TabId(4)).unwrap();
    assert_eq!(windows.window(child).unwrap().selected(), TabId(5));
    windows.present(child).unwrap();
    windows.append(TabId(7));
    windows.select(TabId(5)).unwrap();
    windows
        .transfer(TabId(7), TransferTarget::Window(WindowId::ROOT), 1)
        .unwrap();
    assert_eq!(
        windows.window(WindowId::ROOT).unwrap().tabs(),
        [TabId(4), TabId(7), TabId(6)]
    );
    assert_eq!(windows.window(child).unwrap().selected(), TabId(5));
}

#[test]
fn cancelling_or_invalid_drop_changes_nothing() {
    let mut windows = three();
    let before = windows.clone();
    let _cancelled = windows.begin_drag(TabId(5)).unwrap();
    assert_eq!(windows, before);
    for (target, position, expected) in [
        (
            TransferTarget::Window(WindowId(88)),
            0,
            WindowError::MissingWindow,
        ),
        (
            TransferTarget::Window(WindowId::ROOT),
            3,
            WindowError::InvalidPosition,
        ),
        (TransferTarget::NewWindow, 1, WindowError::InvalidPosition),
    ] {
        assert_eq!(windows.transfer(TabId(5), target, position), Err(expected));
        assert_eq!(windows, before);
    }
}

#[test]
fn stale_drag_cannot_move_a_tab_after_topology_changed() {
    let mut windows = three();
    let drag = windows.begin_drag(TabId(5)).unwrap();
    windows
        .transfer(TabId(4), TransferTarget::NewWindow, 0)
        .unwrap();
    let before = windows.clone();
    assert_eq!(
        windows.commit_drag(drag, TransferTarget::NewWindow, 0),
        Err(WindowError::StaleDrag)
    );
    assert_eq!(windows, before);
}

#[test]
fn last_tab_detaches_without_leaving_a_dead_selection() {
    let mut windows = TabWindows::new(crate::lifetime::WorkspaceId(0), TabId(42));
    let moved = windows
        .transfer(TabId(42), TransferTarget::NewWindow, 0)
        .unwrap();
    assert_eq!(moved.closed_window, Some(WindowId::ROOT));
    assert_eq!(windows.windows().len(), 1);
    assert_eq!(windows.focused(), moved.destination);
    assert_eq!(windows.present(moved.destination), Ok(TabId(42)));
}

#[test]
fn closing_either_window_preserves_all_tabs_and_final_close_preserves_save_input() {
    for close_root in [true, false] {
        let mut windows = three();
        let child = windows
            .transfer(TabId(5), TransferTarget::NewWindow, 0)
            .unwrap()
            .destination;
        let close = if close_root { WindowId::ROOT } else { child };
        let survivor = windows.close_window(close).unwrap().unwrap();
        let mut ids = windows.window(survivor).unwrap().tabs().to_vec();
        ids.sort();
        assert_eq!(ids, [TabId(4), TabId(5), TabId(6)]);
        let before = windows.clone();
        assert_eq!(windows.close_window(survivor), Ok(None));
        assert_eq!(windows, before);
    }
}

#[test]
fn repeated_absolute_reorder_is_a_noop_and_window_ids_are_not_reused() {
    let mut windows = three();
    let original = windows.clone();
    let result = windows
        .transfer(TabId(4), TransferTarget::Window(WindowId::ROOT), 0)
        .unwrap();
    assert!(!result.changed);
    assert_eq!(windows, original);
    let first = windows
        .transfer(TabId(4), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    windows.close_window(first).unwrap();
    let second = windows
        .transfer(TabId(4), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    assert_ne!(first, second);
}

#[test]
fn colliding_tab_and_window_ids_cannot_accept_a_foreign_workspace_drag() {
    let original = three();
    let token = original.begin_drag(TabId(4)).unwrap();
    let mut other = TabWindows::new(crate::lifetime::WorkspaceId(8), TabId(4));
    let before = other.clone();
    assert_eq!(
        other.commit_drag(token, TransferTarget::NewWindow, 0),
        Err(WindowError::StaleDrag)
    );
    assert_eq!(other, before);
}

#[test]
fn closing_keeps_the_existing_positional_selection_policy() {
    let mut windows = three();
    windows.select(TabId(5)).unwrap();
    windows.remove(TabId(4));
    assert_eq!(windows.window(WindowId::ROOT).unwrap().selected(), TabId(6));
}
