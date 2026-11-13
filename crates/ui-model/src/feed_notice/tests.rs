use super::*;
use crate::Model as _;
#[test]
fn every_way_the_popup_closes_and_the_one_way_it_opens() {
    let shut = |chip, offline, dismissed, action| {
        popup_still_open(false, chip, offline, dismissed, action)
    };
    let open =
        |chip, offline, dismissed, action| popup_still_open(true, chip, offline, dismissed, action);

    assert!(shut(true, true, false, false), "the chip opens it");
    assert!(!shut(false, true, false, false), "and nothing else does");
    assert!(!open(true, true, false, false), "the chip closes it again");
    assert!(
        open(false, true, false, false),
        "an untouched frame leaves it up"
    );
    assert!(
        !open(false, true, true, false),
        "a click somewhere else puts it away"
    );
    assert!(
        !open(false, false, false, false),
        "so does a feed that came back on its own"
    );
    for (name, action) in [("Reconnect", true), ("Reload", true)] {
        assert!(
            !open(false, true, false, action),
            "a control that was pressed is under way, not still being read: {name}"
        );
    }
    // No chip, no popup, whatever else is true on the frame. A click on a
    // chip that was never drawn cannot reach this today; the rule holds it
    // anyway rather than resting on that.
    assert!(!shut(true, false, false, false));
    assert!(!open(true, false, false, false));
}

#[test]
fn feed_notice_settle_owns_the_stable_tab_latch() {
    let mut state = State::default();
    Model::update(&mut state, Intent::InitializeOpen(7));
    assert_eq!(state.open(), Some(7));
    let settle = |state: &mut State, tab, chip_clicked, offline, dismissed, action_taken| {
        Model::update(
            state,
            Intent::Settle {
                tab,
                chip_clicked,
                offline,
                dismissed,
                action_taken,
            },
        );
    };
    settle(&mut state, 8, false, true, false, false);
    assert_eq!(state.open(), None, "another tab does not inherit the latch");
    settle(&mut state, 8, true, true, true, true);
    assert_eq!(
        state.open(),
        Some(8),
        "chip wins over coincident dismissal and action"
    );
    settle(&mut state, 8, true, true, true, true);
    assert_eq!(state.open(), None, "the same chip closes the open latch");
    settle(&mut state, 7, true, false, true, true);
    assert_eq!(
        state.open(),
        None,
        "no door cannot open even on a chip event"
    );
}
