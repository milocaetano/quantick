use super::*;

#[test]
fn a_gesture_suppresses_the_bar_until_it_is_released() {
    let mut bar = BarState::default();
    assert!(!bar.suppressed(0));
    bar.suppress_gesture();
    assert!(bar.suppressed(0));
    assert!(bar.suppressed(10_000), "a held gesture never times out");
    bar.release_gesture();
    assert!(!bar.suppressed(0), "the bar returns on release");
}

#[test]
fn a_gesture_without_a_release_expires_on_its_own() {
    // Wheel zoom and window resize have no button to let go of.
    let mut bar = BarState::default();
    bar.suppress_transient(1_000);
    assert!(bar.suppressed(1_000));
    assert!(bar.suppressed(1_000 + TRANSIENT_SUPPRESS_MS - 1));
    assert!(!bar.suppressed(1_000 + TRANSIENT_SUPPRESS_MS));
}

#[test]
fn a_renewed_transient_gesture_pushes_its_own_deadline() {
    let mut bar = BarState::default();
    bar.suppress_transient(1_000);
    bar.suppress_transient(1_150);
    assert!(
        bar.suppressed(1_000 + TRANSIENT_SUPPRESS_MS),
        "a continuing zoom keeps the bar down"
    );
    assert!(!bar.suppressed(1_150 + TRANSIENT_SUPPRESS_MS));
}

#[test]
fn a_resize_suppresses_like_a_zoom_but_the_first_screen_does_not() {
    let mut bar = BarState::default();
    let screen = PxRect::from_min_size(Px::new(0.0, 0.0), Px::new(1000.0, 600.0));
    assert!(!bar.note_screen(screen, 1_000), "nothing to compare yet");
    assert!(!bar.note_screen(screen, 1_010));
    assert!(!bar.suppressed(1_010));
    let narrow = PxRect::from_min_size(Px::new(0.0, 0.0), Px::new(800.0, 600.0));
    assert!(bar.note_screen(narrow, 1_020));
    assert!(bar.suppressed(1_020 + TRANSIENT_SUPPRESS_MS - 1));
}

#[test]
fn a_press_is_owned_once_and_never_inherited_by_the_next() {
    let mut bar = BarState::default();
    let rect = PxRect::from_min_size(Px::new(0.0, 0.0), Px::new(100.0, 30.0));
    let moved = rect.translate(Px::new(300.0, 0.0));
    let on_bar = Some((Px::new(10.0, 10.0), 1.0));
    bar.set_rect(rect);
    assert!(bar.owns_press(on_bar, false));
    bar.set_rect(moved);
    assert!(
        bar.owns_press(on_bar, false),
        "a grip drag keeps the press it began with"
    );
    // The release this never saw: the next press is decided afresh.
    assert!(!bar.owns_press(Some((Px::new(10.0, 10.0), 2.0)), false));
    assert!(!bar.owns_press(None, false));

    bar.set_rect(rect);
    let elsewhere = Some((Px::new(500.0, 500.0), 3.0));
    assert!(!bar.owns_press(elsewhere, false));
    assert!(bar.owns_press(Some((Px::new(500.0, 500.0), 4.0)), true));
    assert!(
        bar.owns_press(None, false),
        "the release frame is still owned"
    );
    assert!(bar.press_owned.is_none());
}

#[test]
fn a_new_selection_keeps_the_hand_placed_position() {
    // The bar a trader moved out of the way stays out of the way. It is
    // the same chart behind the next object, so snapping back would hand
    // them the covered price action again, once per click.
    let mut bar = BarState::default();
    bar.note_selection(Some(0));
    bar.set_manual(Px::new(120.0, 40.0));
    assert!(bar.note_selection(Some(1)));
    assert_eq!(bar.manual(), Some(Px::new(120.0, 40.0)));
}

#[test]
fn a_new_selection_drops_the_geometry_of_the_old_object() {
    // The rect is what the wake radius measures against: stale the instant
    // the selection moves.
    let mut bar = BarState::default();
    bar.note_selection(Some(0));
    bar.set_rect(PxRect::from_min_size(Px::new(0.0, 0.0), Px::new(100.0, 30.0)));
    assert!(bar.note_selection(Some(1)));
    assert_eq!(
        bar.last_rect(),
        None,
        "nor can the geometry it was drawn at"
    );
}

#[test]
fn the_double_click_is_still_the_way_back() {
    let mut bar = BarState::default();
    bar.note_selection(Some(0));
    bar.set_manual(Px::new(120.0, 40.0));
    assert!(bar.clear_manual());
    assert_eq!(
        bar.manual(),
        None,
        "the grip's double-click hands placement back to the rule"
    );
    assert!(!bar.clear_manual(), "nothing left to give back");
}

#[test]
fn the_same_selection_keeps_the_hand_placed_position() {
    let mut bar = BarState::default();
    bar.note_selection(Some(0));
    bar.set_manual(Px::new(120.0, 40.0));
    assert!(!bar.note_selection(Some(0)));
    assert_eq!(bar.manual(), Some(Px::new(120.0, 40.0)));
}
