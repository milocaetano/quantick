//! The market-closed fold: a tape with nothing to say folds away by itself and
//! comes back with the first print. Decided here from what the feed reports, so
//! a replay, which has no transport to go quiet, never folds; the lane only
//! holds the answer ([`MarketHold`]) and the window only passes the facts on.
//!
//! The hold goes through the lane's fold ([`LiveLaneStyle::shown_fold`]) without
//! ever writing the fold a workspace saves.
//!

use quantick_orderflow::{LaneSide, LaneView, LiveLaneStyle, MarketHold};

use crate::connection_state::{ConnectionState, Link};
use crate::state::ChartState;

/// What one step of [`next_hold`] decides from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketFacts {
    /// The stored view: a fold is not one.
    pub view: LaneView,
    pub link: Link,
    /// The asset's "Fold the tape while the market is closed".
    pub setting: bool,
    /// A fold the trader left is standing.
    pub trader_fold: bool,
    /// When the last print arrived; the clock is the tracker's, compared and
    /// never read.
    pub last_data_at: Option<i64>,
}

fn says_closed(link: Link) -> bool {
    matches!(link, Link::Quiet | Link::Offline)
}

/// The hold after this frame's facts.
///
/// It starts beside the candles when the link says Quiet or Offline, the setting
/// is on, no trader's fold stands, and no hand opened the tape during this closed
/// spell. It ends at the first print, not at a change of link, so Offline to
/// Attempting does not flicker; also with the setting off, a view change or a
/// source with no transport. A hand's fold ([`MarketHold::stand_down`]) holds it
/// off until the next entry: an attempt inside a spell is not one, as the feed's
/// retries would otherwise fold what the trader just opened.
#[must_use]
pub fn next_hold(hold: MarketHold, facts: MarketFacts) -> MarketHold {
    let printed = facts.last_data_at.is_some() && facts.last_data_at != hold.seen_print_at;
    let closed = says_closed(facts.link);
    let entered = closed && !hold.closed_spell;
    let stood_down = hold.stood_down && !entered;
    let may_hold = facts.setting
        && facts.view == LaneView::Beside
        && !facts.trader_fold
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
            trader_fold: !lane.fold.is_open(),
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

/// Whether the tape on screen gets the seam: it shows beside or alone, and the
/// link says nothing is coming. A hold leaves no tape on screen to mark.
#[must_use]
pub fn seam_shown(shown_view: LaneView, link: Option<Link>) -> bool {
    shown_view.shows(LaneSide::Tape) && link.is_some_and(says_closed)
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
            trader_fold: false,
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

    /// The trader's own fold is not overruled, nor taken over.
    #[test]
    fn a_standing_trader_fold_is_left_alone() {
        let folded = MarketFacts {
            trader_fold: true,
            ..facts(Link::Quiet)
        };
        assert!(!held_after(settled(false), folded));
        assert!(!held_after(settled(true), folded), "and it ends a hold");
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

    /// A saved fold is the lane's own, and a hold never writes it: through a
    /// whole hold it stays what the trader left, and the trader's stays standing.
    #[test]
    fn a_saved_fold_is_untouched_by_a_hold() {
        let quiet = ConnectionState {
            link: Link::Quiet,
            feed: crate::connection_state::Stream::Idle,
            book: crate::connection_state::Book::Off,
            trades: crate::connection_state::Stream::Idle,
            last_connected_at: None,
            last_data_at: None,
            current_job: None,
        };
        let mut lane = LiveLaneStyle::default();
        lane.market_hold = next_for(&lane, quiet);
        assert!(lane.market_held());
        assert_eq!(lane.fold, quantick_orderflow::LaneFold::Open);

        let mut trader = LiveLaneStyle {
            fold: quantick_orderflow::LaneFold::Candles,
            ..LiveLaneStyle::default()
        };
        trader.market_hold = next_for(&trader, quiet);
        assert!(!trader.market_held());
        assert_eq!(trader.fold, quantick_orderflow::LaneFold::Candles);
    }

    #[test]
    fn the_seam_marks_a_closed_tape_that_is_on_screen() {
        for (view, link, shown) in [
            (LaneView::TapeOnly, Some(Link::Quiet), true),
            (LaneView::TapeOnly, Some(Link::Offline), true),
            (LaneView::Beside, Some(Link::Quiet), true),
            (LaneView::CandlesOnly, Some(Link::Quiet), false),
            (LaneView::TapeOnly, Some(Link::Live), false),
            (LaneView::TapeOnly, Some(Link::Attempting), false),
            (LaneView::TapeOnly, Some(Link::NoTransport), false),
            (LaneView::TapeOnly, None, false),
        ] {
            assert_eq!(seam_shown(view, link), shown, "{view:?} {link:?}");
        }
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
