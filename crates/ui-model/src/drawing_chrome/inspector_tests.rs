use super::*;

#[test]
fn an_object_s_notes_name_its_author_then_its_lock_then_its_eye() {
    use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingAuthor, DrawingBand, Drawings};
    let mut store = Drawings::default();
    let note = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.holds_text())
        .unwrap();
    store.place_on(
        note,
        &DrawingBand::Price,
        ChartPoint::at_time(1.0, 10.0, None),
    );
    let mut drawing = store.items()[0].clone();
    assert!(
        object_notes(&drawing).is_empty(),
        "the trader's own, unlocked, shown"
    );
    drawing.author = Some(DrawingAuthor {
        actor_kind: "agent".to_owned(),
        client_name: "assistant".to_owned(),
    });
    (drawing.locked, drawing.hidden) = (true, true);
    assert_eq!(
        object_notes(&drawing),
        [
            "Placed by assistant (agent) - not by you.",
            LOCKED_NOTE,
            HIDDEN_NOTE
        ]
    );
}
/// Automatic placement is not a preference, so it is not written down.
/// Only a hand — or the hook standing in for one — makes a position the
/// file has to remember.
#[test]
fn only_a_hand_placed_position_reaches_the_file() {
    let mut inspector = State::default();
    assert_eq!(inspector.remembered_position(), None);
    inspector.pos = Some([10.0, 20.0]);
    assert_eq!(
        inspector.remembered_position(),
        None,
        "a placed position is the app's decision, not the trader's"
    );
    inspector.place_by_hand([30.0, 40.0]);
    assert_eq!(inspector.remembered_position(), Some([30.0, 40.0]));
    assert!(
        !inspector.take_position_dirty(),
        "parking alone owes nothing: the drag calls it on every frame the              hand moves, and the file write is owed when the hand comes off"
    );
    inspector.position_dirty = true;
    assert!(inspector.take_position_dirty(), "and then exactly once");
    assert!(!inspector.take_position_dirty());
}

/// A file with a nonsense position leaves automatic placement in charge
/// rather than parking the panel at an invented pixel: NaN survives
/// `clamp`, so neither coordinate may slip through, and neither the point
/// nor the hand's claim may outlive the refusal.
#[test]
fn a_nonsense_remembered_position_is_refused() {
    let mut inspector = State::default();
    for pair in [
        [f32::NAN, 40.0],
        [30.0, f32::NAN],
        [f32::INFINITY, 40.0],
        [30.0, f32::INFINITY],
    ] {
        inspector.place_by_hand([30.0, 40.0]);
        inspector.restore_position(Some(pair));
        assert_eq!(inspector.pos, None, "{pair:?} is not a position");
        assert!(!inspector.moved, "{pair:?} hands back automatic placement");
    }
    inspector.restore_position(Some([12.0, 34.0]));
    assert_eq!(inspector.remembered_position(), Some([12.0, 34.0]));
}
