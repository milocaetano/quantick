//! Where the tape's window ends: pinned to live, or held at a past instant
//! that never leaves the retained tape and never runs past now.
use crate::tape_view::{RETAINED_EDGE_SHARE, TapeEnd};

const LIVE: i64 = 1_000_000;
const WINDOW: i64 = 60_000;

#[test]
fn an_end_at_or_after_the_live_edge_is_live() {
    assert_eq!(TapeEnd::clamped(LIVE, LIVE, WINDOW, None), TapeEnd::Live);
    assert_eq!(TapeEnd::clamped(LIVE + 5, LIVE, WINDOW, None), TapeEnd::Live);
    assert_eq!(
        TapeEnd::clamped(LIVE - 1, LIVE, WINDOW, None),
        TapeEnd::Past { end_ms: LIVE - 1 }
    );
    assert_eq!(TapeEnd::Live.end_ms(LIVE), LIVE);
    assert_eq!(TapeEnd::Past { end_ms: 7 }.end_ms(LIVE), 7);
    assert_eq!(TapeEnd::Past { end_ms: 7 }.past_ms(), Some(7));
    assert!(TapeEnd::Live.is_live() && TapeEnd::Live.past_ms().is_none());
}

/// The clamp keeps the boundary on screen: the window may reach back past the
/// first retained print by at most `RETAINED_EDGE_SHARE` of itself, so the
/// labelled edge sits inside the tape instead of scrolling out of view.
#[test]
fn a_past_end_stops_where_the_retained_tape_begins() {
    let retained = 400_000;
    let earliest = retained + (WINDOW as f64 * RETAINED_EDGE_SHARE) as i64;
    assert_eq!(
        TapeEnd::clamped(0, LIVE, WINDOW, Some(retained)),
        TapeEnd::Past { end_ms: earliest }
    );
    assert_eq!(
        TapeEnd::clamped(earliest + 1, LIVE, WINDOW, Some(retained)),
        TapeEnd::Past {
            end_ms: earliest + 1
        }
    );
    assert_eq!(
        TapeEnd::clamped(i64::MIN, LIVE, WINDOW, None),
        TapeEnd::Past { end_ms: i64::MIN },
        "with nothing known about retention the request stands"
    );
    // History too short to hold a past window at all: there is only live.
    assert_eq!(
        TapeEnd::clamped(0, LIVE, WINDOW, Some(LIVE - 10)),
        TapeEnd::Live
    );
    // A held end re-clamps when retention moves under it.
    assert_eq!(
        TapeEnd::Past { end_ms: 500_000 }.reclamped(LIVE, WINDOW, Some(480_000)),
        TapeEnd::Past { end_ms: 510_000 }
    );
    assert_eq!(
        TapeEnd::Past { end_ms: 500_000 }.reclamped(400_000, WINDOW, None),
        TapeEnd::Live,
        "an end the live edge caught up with is live again"
    );
}

/// A drag the width of the tape moves it back by one window; dragging back
/// the same way re-pins it to live, exactly like the candles' own pan.
#[test]
fn a_horizontal_drag_moves_the_end_by_the_tapes_own_rate() {
    let span = 300.0;
    let back = TapeEnd::Live.panned(span, span, LIVE, WINDOW, None);
    assert_eq!(
        back,
        TapeEnd::Past {
            end_ms: LIVE - WINDOW
        }
    );
    let half = back.panned(-span / 2.0, span, LIVE, WINDOW, None);
    assert_eq!(
        half,
        TapeEnd::Past {
            end_ms: LIVE - WINDOW / 2
        }
    );
    assert_eq!(
        half.panned(-span, span, LIVE, WINDOW, None),
        TapeEnd::Live,
        "past the live edge the tape pins to live"
    );
    assert_eq!(
        TapeEnd::Live.panned(-40.0, span, LIVE, WINDOW, None),
        TapeEnd::Live,
        "dragging toward the future from live stays live"
    );
    for (delta, width) in [(f32::NAN, span), (10.0, 0.0), (10.0, f32::NAN), (0.0, span)] {
        assert_eq!(back.panned(delta, width, LIVE, WINDOW, None), back);
    }
}
