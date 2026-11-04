//! The market-closed hold: a tape with nothing to say goes away by itself and
//! comes back with the first print. Decided here from what the feed reports, so
//! a replay, which has no transport to go quiet, never holds; the lane only
//! holds the answer ([`MarketHold`]) and the window only passes the facts on.
//!
//! The hold is a display over the lane ([`LiveLaneStyle::shown_view`]): it shows
//! the candles alone and writes neither the view nor the share a workspace saves,
//! so the first print brings back what was showing, at the width it had.
//!

use quantick_orderflow::tape_clock::TapeHold;
use quantick_orderflow::{LaneSide, LaneView, LiveLaneStyle, MarketHold};

use quantick_civil::TzOffset;

use crate::connection_state::{Book, ConnectionState, Link};
use crate::geometry::{TimeLabelFormat, time_label};
use crate::state::ChartState;

/// What one step of [`next_hold`] decides from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketFacts {
    /// The stored view: the hold starts only beside the candles.
    pub view: LaneView,
    pub link: Link,
    /// The asset's "Fold the tape while the market is closed".
    pub setting: bool,
    /// When the last print arrived; the clock is the tracker's, compared and
    /// never read.
    pub last_data_at: Option<i64>,
}

fn says_closed(link: Link) -> bool {
    matches!(link, Link::Quiet | Link::Offline)
}

/// The hold after this frame's facts.
///
/// Reads the feed's link and not the spoken one because a blip cannot move it:
/// it is always Attempting, which neither starts a hold nor ends one or a spell.
///
/// It starts beside the candles when the link says Quiet or Offline, the setting
/// is on and no hand opened the tape during this closed spell. It ends at the first
/// print, not at a change of link, so Offline to Attempting does not flicker; also
/// with the setting off, a view change or a source with no transport. A hand's word
/// ([`MarketHold::stand_down`]) holds it off until the next entry: an attempt inside
/// a spell is not one, as the feed's retries would otherwise take away what the
/// trader just opened.
#[must_use]
pub fn next_hold(hold: MarketHold, facts: MarketFacts) -> MarketHold {
    let printed = facts.last_data_at.is_some() && facts.last_data_at != hold.seen_print_at;
    let closed = says_closed(facts.link);
    let entered = closed && !hold.closed_spell;
    let stood_down = hold.stood_down && !entered;
    let may_hold = facts.setting
        && facts.view == LaneView::Beside
        && !printed
        && facts.link != Link::NoTransport;
    MarketHold {
        held: may_hold && (hold.held || (closed && !stood_down)),
        stood_down,
        closed_spell: match facts.link {
            Link::Quiet | Link::Offline => true,
            Link::Attempting => hold.closed_spell && !printed,
            Link::Live | Link::NoTransport => false,
        },
        seen_print_at: facts.last_data_at,
    }
}

/// [`next_hold`] over a lane as it stands, with the feed as the tracker has it.
#[must_use]
pub fn next_for(lane: &LiveLaneStyle, feed: ConnectionState) -> MarketHold {
    next_hold(
        lane.market_hold,
        MarketFacts {
            view: lane.view(),
            link: feed.link,
            setting: lane.fold_market_closed,
            last_data_at: feed.last_data_at,
        },
    )
}

/// When the newest print traded: the tape's own, which is what a clock about
/// prints reads. A bar's close stands in only with no tape retained.
#[must_use]
pub fn last_print_ms(state: &ChartState) -> Option<i64> {
    let tape = state.trades().last().map(|print| print.timestamp_ms);
    tape.or_else(|| {
        let bar = state.partial().or_else(|| state.bars().last());
        bar.map(|bar| bar.close_time)
    })
}

/// Whether nothing is coming to the tape, by the link as it is spoken
/// ([`crate::connection_state::ConnectionTracker::spoken_link`]): every link but
/// one delivering, and a source with no transport, whose clock is its own. A blip
/// over a live tape still says Live, so its window never rewinds. An attempt not
/// yet spoken with no word to keep is nothing coming too: the attempts a dead
/// feed retries must not walk a held window on and back.
fn nothing_coming(spoken: Option<Link>) -> bool {
    spoken.is_none_or(|link| matches!(link, Link::Quiet | Link::Attempting | Link::Offline))
}

/// Whether the tape on screen gets the seam: it shows beside or alone, and the
/// link says nothing is coming. A hold leaves no tape on screen to mark.
#[must_use]
pub fn seam_shown(shown_view: LaneView, spoken: Option<Link>) -> bool {
    shown_view.shows(LaneSide::Tape) && nothing_coming(spoken)
}

/// What holds the tape's window this frame: `None` on a link delivering prints, so
/// a live tape moves exactly as it did, and on a replay. The window is asked for
/// only when it holds: working it out costs a sort, every frame of a live tape.
#[must_use]
pub fn tape_hold(
    spoken: Option<Link>,
    state: &ChartState,
    window_ms: impl FnOnce() -> i64,
) -> Option<TapeHold> {
    if !nothing_coming(spoken) {
        return None;
    }
    Some(TapeHold {
        last_print_ms: last_print_ms(state)?,
        window_ms: window_ms(),
    })
}

/// Where a hold has stopped a book that went on arriving: the cap, when the newest
/// event recorded is past it and the book is synced and drawn. A print-only feed
/// has no event past the cap, as its last print is what the cap is counted from.
#[must_use]
pub fn book_held_at(cap_ms: Option<i64>, newest_event_ms: Option<i64>, book: Book) -> Option<i64> {
    let cap = cap_ms?;
    (book == Book::Synced && newest_event_ms? > cap).then_some(cap)
}

/// The seam's two lines, stacked: its muted line, and under it the one saying so when
/// the heat stops at the cap while the book goes on: a status of "book live" beside
/// heat that has stopped must not read as a book that stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamWords {
    pub line: Option<String>,
    pub book_held: Option<String>,
}

impl SeamWords {
    /// The two lines top to bottom, as [`seam_plates`] places them.
    #[must_use]
    pub fn lines(&self) -> [Option<&str>; 2] {
        [self.line.as_deref(), self.book_held.as_deref()]
    }
}

#[must_use]
pub fn seam_words(line: Option<&str>, book_held_at: Option<i64>, tz: TzOffset) -> SeamWords {
    SeamWords {
        line: line.map(str::to_owned),
        book_held: book_held_at
            .map(|at| format!("book held at {}", time_label(at, tz, TimeLabelFormat::Full))),
    }
}

/// What the seam leaves between its two plates.
const SEAM_PLATE_GAP_PX: f32 = 2.0;

/// Where each plate's corner stands for lines `widths_px` wide, `[line, book_held]`,
/// centred in the empty part right of the seam and stacked on the side's middle. Each
/// line stands by its own fit; where the side's height has room for one, the book
/// held takes it, being the data-honesty label.
#[must_use]
pub fn seam_plates(
    seam_x: f32,
    right: f32,
    side_y: std::ops::RangeInclusive<f32>,
    widths_px: [Option<f32>; 2],
) -> [Option<(f32, f32)>; 2] {
    const PLATE_PX: f32 = crate::pane_header::CAPSULE_HEIGHT_PX;
    let height = side_y.end() - side_y.start();
    let left = |width: Option<f32>| {
        let words = seam_words_x(seam_x, right, width?)?;
        (height >= PLATE_PX).then_some(words - crate::side_label::SIDE_LABEL_PADDING_PX)
    };
    let (mut line, held) = (left(widths_px[0]), left(widths_px[1]));
    let both_px = 2.0 * PLATE_PX + SEAM_PLATE_GAP_PX;
    if held.is_some() && height < both_px {
        line = None;
    }
    let middle = (side_y.start() + side_y.end()) / 2.0;
    if line.is_some() && held.is_some() {
        let top = middle - both_px / 2.0;
        let under = top + PLATE_PX + SEAM_PLATE_GAP_PX;
        return [line.map(|x| (x, top)), held.map(|x| (x, under))];
    }
    [line, held].map(|left| left.map(|x| (x, middle - PLATE_PX / 2.0)))
}

/// Where the seam's line starts when it is `words_px` wide: centred in the empty
/// part right of the seam, and only once that part has room to say it.
#[must_use]
pub fn seam_words_x(seam_x: f32, right: f32, words_px: f32) -> Option<f32> {
    let room = right - seam_x;
    (words_px + 2.0 * crate::side_label::SIDE_LABEL_PADDING_PX <= room)
        .then_some(seam_x + (room - words_px) / 2.0)
}

/// Where the seam stands on a tape whose window runs `inset_px` in from `left_px`
/// and `span_px` on to NOW; the oldest visible time when the print is older.
#[must_use]
pub fn seam_x_px(
    (left_px, inset_px, span_px): (f32, f32, f32),
    now_ms: i64,
    print_ms: i64,
    window_ms: i64,
) -> Option<f32> {
    if window_ms <= 0 {
        return None;
    }
    let age = now_ms.saturating_sub(print_ms).max(0) as f64 / window_ms as f64;
    Some(left_px + inset_px + span_px * (1.0 - age.min(1.0)) as f32)
}

/// Where the seam stands on a tape side at `[left, width, height]`, given
/// `(now_ms, print_ms, window_ms)`: the native tape's window sits inside its
/// insets, any other lane's fills the side.
#[must_use]
pub fn tape_seam_x(
    config: &quantick_orderflow::HeatmapConfig,
    [left, width, height]: [f32; 3],
    (now_ms, print_ms, window_ms): (i64, i64, i64),
) -> Option<f32> {
    let (inset, span) = if config.native_tape() {
        let tape = quantick_orderflow::projection::TapeHorizontalGeometry::resolve(
            width,
            height,
            &config.bubbles,
        );
        (tape.inset_px, tape.span_px)
    } else {
        (0.0, width)
    };
    seam_x_px((left, inset, span), now_ms, print_ms, window_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Option<i64> = Some(5_000);

    fn facts(link: Link) -> MarketFacts {
        MarketFacts {
            view: LaneView::Beside,
            link,
            setting: true,
            last_data_at: AT,
        }
    }

    /// A hold that has seen the print at [`AT`], and whether it is held.
    fn settled(held: bool) -> MarketHold {
        MarketHold {
            held,
            closed_spell: held,
            seen_print_at: AT,
            ..MarketHold::default()
        }
    }

    fn held_after(hold: MarketHold, facts: MarketFacts) -> bool {
        next_hold(hold, facts).held
    }

    /// Each link, each view and each thing that forbids a start.
    #[test]
    fn it_starts_only_beside_on_quiet_or_offline_with_the_setting_on() {
        for (link, starts) in [
            (Link::Quiet, true),
            (Link::Offline, true),
            (Link::Attempting, false),
            (Link::Live, false),
            (Link::NoTransport, false),
        ] {
            assert_eq!(held_after(settled(false), facts(link)), starts, "{link:?}");
        }
        for link in [Link::Quiet, Link::Offline] {
            for view in [LaneView::CandlesOnly, LaneView::TapeOnly] {
                let alone = MarketFacts {
                    view,
                    ..facts(link)
                };
                assert!(!held_after(settled(false), alone), "{view:?}");
            }
            let off = MarketFacts {
                setting: false,
                ..facts(link)
            };
            assert!(!held_after(settled(false), off));
        }
    }

    /// The first print releases, whatever the link; a change of link alone never
    /// does, so Offline to Attempting does not flicker.
    #[test]
    fn the_first_print_releases_and_a_link_change_does_not() {
        for link in [Link::Attempting, Link::Live, Link::Quiet, Link::Offline] {
            assert!(held_after(settled(true), facts(link)), "{link:?}");
        }
        for link in [Link::Attempting, Link::Live, Link::Quiet, Link::Offline] {
            let printed = MarketFacts {
                last_data_at: Some(6_000),
                ..facts(link)
            };
            assert!(!held_after(settled(true), printed), "{link:?}");
        }
    }

    /// A new timeline forgets its dates, which is not a print.
    #[test]
    fn a_timeline_that_forgot_its_prints_does_not_release() {
        let forgot = MarketFacts {
            last_data_at: None,
            ..facts(Link::Attempting)
        };
        assert!(held_after(settled(true), forgot));
    }

    /// Replay has no transport: never a fold, and a source switched under a hold
    /// ends it.
    #[test]
    fn a_source_with_no_transport_never_folds() {
        assert!(!held_after(settled(false), facts(Link::NoTransport)));
        assert!(!held_after(settled(true), facts(Link::NoTransport)));
    }

    #[test]
    fn turning_the_setting_off_releases_and_on_folds_again() {
        let off = MarketFacts {
            setting: false,
            ..facts(Link::Quiet)
        };
        let released = next_hold(settled(true), off);
        assert!(!released.held);
        assert!(next_hold(released, facts(Link::Quiet)).held, "still closed");
    }

    #[test]
    fn a_view_change_releases() {
        for view in [LaneView::CandlesOnly, LaneView::TapeOnly] {
            let moved = MarketFacts {
                view,
                ..facts(Link::Attempting)
            };
            assert!(!held_after(settled(true), moved), "{view:?}");
        }
    }

    /// A hand opened it: it stays open through the rest of the spell, a retry
    /// included, and folds again only at the next entry.
    #[test]
    fn a_hand_holds_it_off_until_the_next_entry() {
        let mut hold = settled(true);
        hold.stand_down();
        assert!(!hold.held && hold.stood_down);
        for link in [
            Link::Offline,
            Link::Attempting,
            Link::Offline,
            Link::Quiet,
            Link::Attempting,
            Link::Quiet,
        ] {
            hold = next_hold(hold, facts(link));
            assert!(!hold.held, "{link:?}: the same spell");
        }
        hold = next_hold(hold, facts(Link::Live));
        assert!(!hold.held && hold.stood_down, "open is not an entry");
        hold = next_hold(hold, facts(Link::Quiet));
        assert!(hold.held && !hold.stood_down, "the next entry");
    }

    /// A print ends a spell whatever the link it arrives under, so what follows
    /// is an entry; and it never starts a hold on its own frame.
    #[test]
    fn a_print_ends_the_spell() {
        let mut hold = settled(true);
        hold.stand_down();
        hold = next_hold(
            hold,
            MarketFacts {
                last_data_at: Some(6_000),
                ..facts(Link::Attempting)
            },
        );
        assert!(!hold.closed_spell && !hold.held);
        let printed_quiet = MarketFacts {
            last_data_at: Some(7_000),
            ..facts(Link::Quiet)
        };
        assert!(!held_after(settled(false), printed_quiet));
        hold = next_hold(
            hold,
            MarketFacts {
                last_data_at: Some(6_000),
                ..facts(Link::Quiet)
            },
        );
        assert!(hold.held);
    }

    /// The hold is a display: through a whole hold the stored view and share stay
    /// what the trader left, a side the trader closed is not taken over, and the
    /// first print shows the band at the width it had.
    #[test]
    fn a_hold_leaves_the_stored_view_and_share_untouched() {
        let quiet = ConnectionState {
            link: Link::Quiet,
            feed: crate::connection_state::Stream::Idle,
            book: crate::connection_state::Book::Off,
            trades: crate::connection_state::Stream::Idle,
            last_connected_at: None,
            last_data_at: None,
            current_job: None,
        };
        let mut lane = LiveLaneStyle {
            width_share: 0.42,
            ..LiveLaneStyle::default()
        };
        let band = lane.resolved_width_px(1_000.0);
        lane.market_hold = next_for(&lane, quiet);
        assert!(lane.market_held());
        assert_eq!(lane.shown_view(), LaneView::CandlesOnly);
        assert_eq!(lane.view(), LaneView::Beside);
        assert_eq!(lane.width_share, 0.42);

        let printed = ConnectionState {
            last_data_at: AT,
            ..quiet
        };
        lane.market_hold = next_for(&lane, printed);
        assert!(!lane.market_held(), "the first print");
        assert_eq!(lane.shown_view(), LaneView::Beside);
        assert_eq!(lane.resolved_width_px(1_000.0), band, "the width it had");

        for view in [LaneView::CandlesOnly, LaneView::TapeOnly] {
            let mut closed = LiveLaneStyle::default();
            closed.set_view(view);
            closed.market_hold = next_for(&closed, quiet);
            assert!(!closed.market_held(), "{view:?}");
            assert_eq!(closed.view(), view);
        }
    }

    #[test]
    fn the_seam_marks_a_closed_tape_that_is_on_screen() {
        for (view, link, shown) in [
            (LaneView::TapeOnly, Some(Link::Quiet), true),
            (LaneView::TapeOnly, Some(Link::Offline), true),
            (LaneView::Beside, Some(Link::Quiet), true),
            (LaneView::CandlesOnly, Some(Link::Quiet), false),
            (LaneView::TapeOnly, Some(Link::Live), false),
            (LaneView::TapeOnly, Some(Link::Attempting), true),
            (LaneView::TapeOnly, Some(Link::NoTransport), false),
            (LaneView::TapeOnly, None, true),
        ] {
            assert_eq!(seam_shown(view, link), shown, "{view:?} {link:?}");
        }
    }

    fn state_printed_at(at: i64) -> ChartState {
        let mut state = ChartState::new(quantick_engine::BarSpec::Tick(100));
        state.ingest_live(&quantick_engine::Trade {
            agg_id: 1,
            timestamp_ms: at,
            price: 100.into(),
            quantity: 1.into(),
            side: quantick_engine::Side::Buy,
        });
        state
    }

    /// A link delivering prints, and a replay, leave the window to the clock; the
    /// other three hold it, on the last print's time.
    #[test]
    fn only_a_link_with_nothing_coming_holds_the_window() {
        let state = state_printed_at(100_000);
        for (link, holds) in [
            (Link::Live, false),
            (Link::NoTransport, false),
            (Link::Quiet, true),
            (Link::Attempting, true),
            (Link::Offline, true),
        ] {
            let hold = tape_hold(Some(link), &state, || 15_000);
            assert_eq!(hold.is_some(), holds, "{link:?}");
            if let Some(hold) = hold {
                assert_eq!(hold.last_print_ms, 100_000);
                assert_eq!(hold.window_end_ms(160_000), 106_750, "{link:?}");
            }
        }
        let empty = ChartState::new(quantick_engine::BarSpec::Tick(100));
        let quiet = Some(Link::Quiet);
        assert_eq!(tape_hold(quiet, &empty, || 15_000), None, "no print yet");
    }

    /// A live tape pays nothing for the question: the window is not worked out.
    #[test]
    fn a_link_delivering_prints_never_asks_for_the_window() {
        let state = state_printed_at(100_000);
        for link in [Link::Live, Link::NoTransport] {
            let asked = || panic!("{link:?} asked for the window");
            assert_eq!(tape_hold(Some(link), &state, asked), None);
        }
    }

    /// A retry of a dead feed, not yet spoken and with no word to keep, stays held
    /// and keeps its seam: neither walks on for two seconds and back.
    #[test]
    fn an_attempt_with_no_word_to_keep_holds_the_window_and_the_seam() {
        let state = state_printed_at(100_000);
        assert!(tape_hold(None, &state, || 15_000).is_some());
        assert!(seam_shown(LaneView::TapeOnly, None));
    }

    /// A transport blip, step by step: from Live it starts nothing, from Quiet it
    /// ends nothing, and a tape a hand opened stays open.
    #[test]
    fn a_blip_cannot_move_the_fold() {
        let mut live = settled(false);
        for link in [Link::Live, Link::Attempting, Link::Live] {
            live = next_hold(live, facts(link));
            assert!(!live.held, "{link:?}");
        }
        let mut quiet = next_hold(settled(false), facts(Link::Quiet));
        let mut opened = quiet;
        opened.stand_down();
        for link in [Link::Attempting, Link::Quiet] {
            quiet = next_hold(quiet, facts(link));
            opened = next_hold(opened, facts(link));
            assert!(quiet.held, "{link:?}");
            assert!(!opened.held, "{link:?}");
        }
    }

    /// Only a synced book with an event past the cap is a book held.
    #[test]
    fn a_book_is_held_only_when_it_went_on_past_the_cap() {
        let cap = Some(10_000);
        assert_eq!(book_held_at(cap, Some(10_001), Book::Synced), Some(10_000));
        assert_eq!(book_held_at(cap, Some(10_000), Book::Synced), None, "at it");
        assert_eq!(book_held_at(cap, None, Book::Synced), None);
        assert_eq!(
            book_held_at(None, Some(10_001), Book::Synced),
            None,
            "no hold"
        );
        for book in [Book::Off, Book::Lost, Book::Connecting, Book::Idle] {
            assert_eq!(book_held_at(cap, Some(10_001), book), None, "{book:?}");
        }
    }

    #[test]
    fn the_seams_two_lines_are_separate() {
        const UTC: TzOffset = TzOffset::new(0);
        let at = Some(3_723_000);
        let words = seam_words(Some("No trades on WIN"), at, UTC);
        assert_eq!(words.line.as_deref(), Some("No trades on WIN"));
        assert_eq!(words.book_held.as_deref(), Some("book held at 01:02:03"));
        let none = seam_words(None, None, UTC);
        assert_eq!((none.line, none.book_held), (None, None));
    }

    /// The seam at 200 of a side 300 wide: 100 of room less the padding fits 86.
    const WIDE: Option<f32> = Some(80.0);
    const TOO_WIDE: Option<f32> = Some(90.0);

    fn plates(height: f32, widths: [Option<f32>; 2]) -> [Option<(f32, f32)>; 2] {
        seam_plates(200.0, 300.0, 0.0..=height, widths)
    }

    #[test]
    fn both_lines_fit_and_stack_the_book_held_underneath() {
        let [line, held] = plates(400.0, [WIDE, WIDE]);
        let ((line_x, line_y), (held_x, held_y)) = (line.unwrap(), held.unwrap());
        assert_eq!(line_x, held_x);
        let plate = crate::pane_header::CAPSULE_HEIGHT_PX;
        assert!(line_y + plate <= held_y, "the book held is underneath");
        assert_eq!(line_y + held_y + plate, 400.0, "centred on the side");
    }

    #[test]
    fn a_line_that_does_not_fit_leaves_the_other_centred() {
        let [line, held] = plates(400.0, [TOO_WIDE, WIDE]);
        assert_eq!(line, None);
        let (_, y) = held.expect("the book held fits on its own");
        let plate = crate::pane_header::CAPSULE_HEIGHT_PX;
        assert_eq!(y, 200.0 - plate / 2.0);
        let [line, held] = plates(400.0, [WIDE, TOO_WIDE]);
        assert!(line.is_some(), "the muted line fits on its own");
        assert_eq!(held, None);
    }

    #[test]
    fn a_side_with_room_for_one_line_gives_it_to_the_book_held() {
        let plate = crate::pane_header::CAPSULE_HEIGHT_PX;
        let [line, held] = plates(plate + 1.0, [WIDE, WIDE]);
        assert_eq!(line, None);
        assert!(held.is_some());
    }

    #[test]
    fn no_line_stands_when_neither_fits() {
        assert_eq!(plates(400.0, [TOO_WIDE, TOO_WIDE]), [None, None]);
        assert_eq!(plates(1.0, [WIDE, WIDE]), [None, None]);
        assert_eq!(plates(400.0, [None, None]), [None, None]);
    }

    /// Held, the seam stands at 55 % of the window and the tape never empties, a
    /// minute on or an hour; the first print puts the window back on the clock.
    #[test]
    fn a_held_seam_parks_at_fifty_five_percent_and_the_first_print_frees_it() {
        let quiet = Some(Link::Quiet);
        let hold = tape_hold(quiet, &state_printed_at(100_000), || 15_000).unwrap();
        let tape = (0.0, 0.0, 1_000.0);
        for now in [106_750, 160_000, 3_700_000] {
            let end = hold.window_end_ms(now);
            let x = seam_x_px(tape, end, 100_000, 15_000).unwrap();
            assert!((x - 550.0).abs() < 0.5, "{now}: {x}");
        }
        let early = seam_x_px(tape, hold.window_end_ms(103_000), 100_000, 15_000).unwrap();
        assert!((early - 800.0).abs() < 0.5, "it walks there first: {early}");
        let freed = tape_hold(quiet, &state_printed_at(160_000), || 15_000).unwrap();
        assert_eq!(freed.window_end_ms(160_000), 160_000);
    }

    /// The line stands in the empty part or nowhere, and a held seam leaves room for
    /// it where the one just after the last print does not.
    #[test]
    fn the_seams_line_shows_once_the_empty_part_has_room() {
        let words = 110.0;
        assert_eq!(seam_words_x(460.0, 500.0, words), None, "at the edge");
        let held = seam_words_x(275.0, 500.0, words).expect("45 % of the side");
        assert!(held > 275.0 && held + words < 500.0);
        assert!((held + words / 2.0 - 387.5).abs() < 0.01, "centred");
    }

    /// The tape's clock is the newest print's time, on every bar rule, whether a
    /// bar is forming, just closed or still at the print before it.
    #[test]
    fn the_tape_clock_reads_the_newest_print_on_every_bar_rule() {
        use quantick_engine::{BarSpec, Side, Trade};
        for spec in [
            BarSpec::Tick(2),
            BarSpec::Time(60_000),
            BarSpec::Volume(2.into()),
        ] {
            let mut state = ChartState::new(spec);
            assert_eq!(last_print_ms(&state), None, "{spec:?}");
            for (id, at) in [(1, 1_000), (2, 2_500), (3, 61_000), (4, 61_700)] {
                state.ingest_live(&Trade {
                    agg_id: id,
                    timestamp_ms: at,
                    price: 100.into(),
                    quantity: 1.into(),
                    side: Side::Buy,
                });
                assert_eq!(last_print_ms(&state), Some(at), "{spec:?}");
                let bar = state.partial().or_else(|| state.bars().last());
                assert_eq!(bar.map(|bar| bar.close_time), Some(at), "{spec:?}");
            }
        }
    }

    /// An ordinary lane's window fills its side.
    #[test]
    fn an_ordinary_lanes_seam_spans_the_whole_side() {
        let config = quantick_orderflow::HeatmapConfig::default();
        let side = [100.0, 200.0, 300.0];
        assert_eq!(tape_seam_x(&config, side, (1_000, 500, 1_000)), Some(200.0));
    }

    #[test]
    fn the_seam_stands_at_the_prints_age_along_the_window() {
        let tape = (100.0, 10.0, 200.0);
        assert_eq!(seam_x_px(tape, 1_000, 1_000, 1_000), Some(310.0), "at NOW");
        assert_eq!(seam_x_px(tape, 1_500, 1_000, 1_000), Some(210.0));
        assert_eq!(seam_x_px(tape, 9_000, 1_000, 1_000), Some(110.0), "older");
        assert_eq!(seam_x_px(tape, 500, 1_000, 1_000), Some(310.0), "no future");
        assert_eq!(seam_x_px(tape, 1_000, 1_000, 0), None);
    }
}
