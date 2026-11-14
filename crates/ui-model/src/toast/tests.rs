use super::*;
use crate::Model as _;
const MS: i128 = 1_000_000;
fn note(state: &mut State, message: &'static str, now: i128) {
    assert_eq!(
        Model::update(
            state,
            Intent::Note {
                message: message.into(),
                now
            }
        ),
        Effects::default()
    );
}
fn undo_note(state: &mut State, message: &'static str, now: i128) {
    assert_eq!(
        Model::update(
            state,
            Intent::NoteWithUndo {
                message: message.into(),
                now
            }
        ),
        Effects::default()
    );
}
fn tick(state: &mut State, now: i128) {
    assert_eq!(Model::update(state, Intent::Tick(now)), Effects::default());
}

#[test]
fn a_toast_expires_after_the_undo_window() {
    let mut surface = State::default();
    let shown = 0;
    note(&mut surface, "Workspace saved.", shown);
    assert_eq!(surface.message(), Some("Workspace saved."));

    tick(&mut surface, shown + (WINDOW - MS));
    assert_eq!(
        surface.message(),
        Some("Workspace saved."),
        "still inside the window"
    );

    tick(&mut surface, shown + WINDOW);
    assert_eq!(surface.message(), None, "the window closed on the boundary");
}

#[test]
fn a_plain_message_waits_behind_a_live_undo() {
    let mut surface = State::default();
    let shown = 0;
    undo_note(&mut surface, "Trend line deleted.", shown);

    note(&mut surface, "SIM: stop filled.", shown + (2_000 * MS));
    assert_eq!(
        surface.message(),
        Some("Trend line deleted."),
        "the way back stays on screen"
    );
    assert!(surface.offers_undo(), "and so does its button");

    // Once the undo window closes, the held message takes the slot — and
    // starts its own eight seconds from here, not from when it was raised.
    let expired = shown + WINDOW;
    tick(&mut surface, expired);
    assert_eq!(surface.message(), Some("SIM: stop filled."));
    assert!(!surface.offers_undo());

    tick(&mut surface, expired + (WINDOW - MS));
    assert_eq!(
        surface.message(),
        Some("SIM: stop filled."),
        "it gets a full window of its own, not the remainder of another's"
    );
}

#[test]
fn a_spent_undo_defers_nothing() {
    let mut surface = State::default();
    let shown = 0;
    undo_note(&mut surface, "Trend line deleted.", shown);
    note(&mut surface, "Workspace saved.", shown + WINDOW);
    assert_eq!(surface.message(), Some("Workspace saved."));
    assert!(!surface.offers_undo());
}

#[test]
fn a_newer_undo_takes_the_slot_and_the_queue_with_it() {
    let mut surface = State::default();
    let shown = 0;
    undo_note(&mut surface, "Trend line deleted.", shown);
    note(&mut surface, "SIM: stop filled.", shown + (1_000 * MS));
    undo_note(&mut surface, "Rectangle deleted.", shown + (2_000 * MS));
    assert_eq!(surface.message(), Some("Rectangle deleted."));

    tick(&mut surface, shown + (2_000 * MS) + WINDOW);
    assert_eq!(
        surface.message(),
        None,
        "the held message went with the slot it was waiting for"
    );
}

#[test]
fn migration_preserves_state_and_effects() {
    let mut state = State::default();
    undo_note(&mut state, "delete", 0);
    note(&mut state, "old", MS);
    note(&mut state, "latest", 2 * MS);
    assert_eq!(
        Model::update(&mut state, Intent::Undo),
        Effects { undo_drawing: true }
    );
    assert_eq!(state.message(), None, "Undo does not eagerly promote");
    tick(&mut state, WINDOW);
    assert_eq!(state.message(), Some("latest"));
    assert_eq!(state.pending.as_ref().unwrap().shown_at, WINDOW);
    tick(&mut state, 2 * WINDOW - 1);
    assert_eq!(state.message(), Some("latest"));
    tick(&mut state, 2 * WINDOW);
    assert_eq!(state.message(), None);
}

#[test]
fn late_plain_notice_preserves_the_deferred_slot_after_expiry_or_undo() {
    for used in [false, true] {
        let mut state = State::default();
        undo_note(&mut state, "delete", 0);
        note(&mut state, "held", MS);
        if used {
            assert!(Model::update(&mut state, Intent::Undo).undo_drawing);
        }
        note(&mut state, "late", WINDOW);
        tick(&mut state, WINDOW);
        assert_eq!(state.message(), Some("late"));
        tick(&mut state, 2 * WINDOW);
        assert_eq!(state.message(), Some("held"));
        assert_eq!(state.pending.as_ref().unwrap().shown_at, 2 * WINDOW);
    }
}

#[test]
fn earlier_time_has_zero_age_without_rebasing_the_deadline() {
    let mut state = State::default();
    note(&mut state, "plain", 0);
    tick(&mut state, -MS);
    assert_eq!(state.message(), Some("plain"));
    tick(&mut state, WINDOW);
    assert_eq!(state.message(), None);
}
