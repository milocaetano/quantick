use super::{BarDismissal, PressOn};

#[test]
fn a_press_is_classified_by_bar_then_gesture_pick_then_bounds() {
    let classify = PressOn::classify;
    assert_eq!(classify(true, Some(Some(2)), 2, true), PressOn::Bar);
    assert_eq!(classify(false, Some(Some(2)), 2, false), PressOn::SelectedDrawing);
    assert_eq!(classify(false, Some(Some(1)), 2, true), PressOn::Elsewhere);
    assert_eq!(classify(false, Some(None), 2, true), PressOn::Elsewhere);
    assert_eq!(classify(false, None, 2, true), PressOn::SelectedDrawing);
    assert_eq!(classify(false, None, 2, false), PressOn::Elsewhere);
}

#[test]
fn a_press_elsewhere_dismisses_and_a_press_on_the_object_reopens() {
    let mut bar = BarDismissal::default();
    assert!(!bar.dismissed());
    assert!(bar.note_press(1.0, PressOn::Elsewhere));
    assert!(bar.dismissed());
    assert!(!bar.note_press(2.0, PressOn::SelectedDrawing));
    assert!(!bar.dismissed());
}

#[test]
fn a_press_on_the_bar_changes_nothing() {
    let mut bar = BarDismissal::default();
    bar.note_press(1.0, PressOn::Bar);
    assert!(!bar.dismissed());
    bar.note_press(2.0, PressOn::Elsewhere);
    bar.note_press(3.0, PressOn::Bar);
    assert!(
        bar.dismissed(),
        "a press where the bar was does not reopen it"
    );
}

#[test]
fn a_held_press_is_decided_once() {
    let mut bar = BarDismissal::default();
    bar.note_press(1.0, PressOn::Elsewhere);
    bar.note_selection_changed();
    // The same press, seen again on a later frame of its drag.
    assert!(!bar.note_press(1.0, PressOn::Elsewhere));
    assert!(!bar.dismissed());
}

#[test]
fn a_new_selection_reopens() {
    let mut bar = BarDismissal::default();
    bar.note_press(1.0, PressOn::Elsewhere);
    bar.note_selection_changed();
    assert!(!bar.dismissed());
}

#[test]
fn losing_focus_dismisses_and_regaining_it_does_not_reopen() {
    let mut bar = BarDismissal::default();
    assert!(!bar.note_focus(false), "no focus report before is no loss");
    assert!(!bar.note_focus(true));
    assert!(bar.note_focus(false));
    assert!(bar.dismissed());
    assert!(!bar.note_focus(true));
    assert!(bar.dismissed());
    bar.note_press(1.0, PressOn::SelectedDrawing);
    assert!(!bar.dismissed());
}
