use super::{BarDismissal, BarTarget, Press, PressOn};

const LINE: Option<BarTarget> = Some(target(1, 10, 100));

const fn target(tab_id: u64, pane_id: u64, drawing_id: u64) -> BarTarget {
    BarTarget {
        tab_id,
        pane_id,
        drawing_id,
    }
}

fn press(started: f64, on: PressOn) -> Option<Press> {
    Some(Press { started, on })
}

#[test]
fn a_press_is_classified_by_bar_then_gesture_pick_then_bounds() {
    let classify = PressOn::classify;
    assert_eq!(classify(true, Some(Some(2)), 2, true), PressOn::Bar);
    assert_eq!(
        classify(false, Some(Some(2)), 2, false),
        PressOn::SelectedDrawing
    );
    assert_eq!(classify(false, Some(Some(1)), 2, true), PressOn::Elsewhere);
    assert_eq!(classify(false, Some(None), 2, true), PressOn::Elsewhere);
    assert_eq!(classify(false, None, 2, true), PressOn::SelectedDrawing);
    assert_eq!(classify(false, None, 2, false), PressOn::Elsewhere);
}

#[test]
fn a_press_elsewhere_dismisses_and_a_press_on_the_object_reopens() {
    let mut bar = BarDismissal::default();
    assert!(!bar.step(LINE, true, None));
    assert!(bar.step(LINE, true, press(1.0, PressOn::Elsewhere)));
    assert!(bar.dismissed());
    assert!(!bar.step(LINE, true, None), "it stays dismissed");
    assert!(bar.dismissed());
    assert!(!bar.step(LINE, true, press(2.0, PressOn::SelectedDrawing)));
    assert!(!bar.dismissed());
}

#[test]
fn a_press_on_the_bar_changes_nothing() {
    let mut bar = BarDismissal::default();
    bar.step(LINE, true, press(1.0, PressOn::Bar));
    assert!(!bar.dismissed());
    bar.step(LINE, true, press(2.0, PressOn::Elsewhere));
    bar.step(LINE, true, press(3.0, PressOn::Bar));
    assert!(
        bar.dismissed(),
        "a press where the bar was does not reopen it"
    );
}

#[test]
fn a_held_press_is_decided_once() {
    let mut bar = BarDismissal::default();
    bar.step(LINE, true, press(1.0, PressOn::Elsewhere));
    // A new target comes up while the same press is still held.
    let other = Some(target(1, 10, 101));
    assert!(!bar.step(other, true, press(1.0, PressOn::Elsewhere)));
    assert!(!bar.dismissed());
}

#[test]
fn another_object_pane_or_tab_is_a_new_bar() {
    for other in [target(1, 10, 101), target(1, 11, 100), target(2, 10, 100)] {
        let mut bar = BarDismissal::default();
        bar.step(LINE, true, press(1.0, PressOn::Elsewhere));
        bar.step(Some(other), true, None);
        assert!(!bar.dismissed(), "{other:?} opens undismissed");
    }
}

#[test]
fn losing_focus_dismisses_and_regaining_it_does_not_reopen() {
    let mut bar = BarDismissal::default();
    assert!(
        !bar.step(LINE, false, None),
        "no focus report before is no loss"
    );
    assert!(!bar.step(LINE, true, None));
    assert!(bar.step(LINE, false, None));
    assert!(!bar.step(LINE, true, None));
    assert!(bar.dismissed());
    bar.step(LINE, true, press(1.0, PressOn::SelectedDrawing));
    assert!(!bar.dismissed());
}

#[test]
fn focus_is_tracked_without_a_selection() {
    let mut bar = BarDismissal::default();
    bar.step(LINE, true, None);
    bar.step(None, true, None);
    // Focus left while nothing was selected; a selection made while
    // unfocused is not a fresh loss.
    bar.step(None, false, None);
    assert!(!bar.step(LINE, false, None));
    assert!(!bar.dismissed());
}
