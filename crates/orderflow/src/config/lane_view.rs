//! The flow pane's three views, a layer over the lane's two switches.
//!
//! The pane shows the candles and the tape side by side, the candles alone or
//! the tape alone. The screen calls the candles side the Trail. [`LaneView`] is
//! not a third switch: it is the lane's `enabled` and `tape_only` read together,
//! so nothing can disagree with them, and it is the one state: a side closed by
//! dragging the divider to its edge is this same view.
//!
//! Beside the candles has one door, [`LiveLaneStyle::open_beside`], at the default
//! width. Only a hand on the rail or a share set on purpose opens at another
//! ([`LiveLaneStyle::show_by_hand`]).

use super::lane::{DEFAULT_LIVE_LANE_SHARE, LiveLaneStyle};
use super::lane_fold::LaneFold;

/// What the flow pane shows: never stored, always [`LiveLaneStyle::view`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneView {
    /// Candles and tape side by side, split at the lane's share.
    Beside,
    /// The candles alone: the tape is switched off.
    CandlesOnly,
    /// The tape alone: no candle and no candle layer is drawn.
    TapeOnly,
}

/// One of the two sides of the flow pane, as its header's switches name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneSide {
    /// The candles side.
    Trail,
    /// The prints as they arrive.
    Tape,
}

impl LaneView {
    /// Whether this view shows `side`.
    #[must_use]
    pub fn shows(self, side: LaneSide) -> bool {
        match (self, side) {
            (Self::Beside, _)
            | (Self::CandlesOnly, LaneSide::Trail)
            | (Self::TapeOnly, LaneSide::Tape) => true,
            (Self::CandlesOnly, LaneSide::Tape) | (Self::TapeOnly, LaneSide::Trail) => false,
        }
    }

    /// The view with `side` switched the other way, or `None` when it is the
    /// last side showing: the pane is never left with nothing on it.
    #[must_use]
    pub fn toggled(self, side: LaneSide) -> Option<Self> {
        match (self, side) {
            (Self::Beside, LaneSide::Trail) => Some(Self::TapeOnly),
            (Self::Beside, LaneSide::Tape) => Some(Self::CandlesOnly),
            (Self::CandlesOnly, LaneSide::Tape) | (Self::TapeOnly, LaneSide::Trail) => {
                Some(Self::Beside)
            }
            (Self::CandlesOnly, LaneSide::Trail) | (Self::TapeOnly, LaneSide::Tape) => None,
        }
    }

    /// The view the tape key leads to: tape only, and from there back beside
    /// the candles. Nothing remembers the view it left.
    #[must_use]
    pub fn tape_key(self) -> Self {
        if self == Self::TapeOnly {
            Self::Beside
        } else {
            Self::TapeOnly
        }
    }

    /// The tape key's mirror: the trail alone, and from there back beside the
    /// tape.
    #[must_use]
    pub fn trail_key(self) -> Self {
        if self == Self::CandlesOnly {
            Self::Beside
        } else {
            Self::CandlesOnly
        }
    }

    /// This view by the side it closes: the readback of `tape_fold`.
    #[must_use]
    pub fn fold(self) -> LaneFold {
        match self {
            Self::Beside => LaneFold::Open,
            Self::CandlesOnly => LaneFold::Tape,
            Self::TapeOnly => LaneFold::Candles,
        }
    }
}

impl LiveLaneStyle {
    /// What the two switches ask the pane to show. A tape switched off is
    /// candles only whatever `tape_only` says.
    #[must_use]
    pub fn view(&self) -> LaneView {
        match (self.enabled, self.tape_only) {
            (true, false) => LaneView::Beside,
            (true, true) => LaneView::TapeOnly,
            (false, _) => LaneView::CandlesOnly,
        }
    }

    /// The view a key's move leads to, or `None` while the market holds the
    /// tape: both keys obey the switches they speak for, and those refuse.
    #[must_use]
    pub fn keyed_view(&self, key: fn(LaneView) -> LaneView) -> Option<LaneView> {
        (!self.market_held()).then(|| key(self.view()))
    }

    /// The side on screen: [`Self::view`], except that a drag's close and the
    /// market's hold show one side alone over a stored view they never write.
    #[must_use]
    pub fn shown_view(&self) -> LaneView {
        if let Some(close) = self.drag_close {
            close
        } else if self.market_held() {
            LaneView::CandlesOnly
        } else {
            self.view()
        }
    }

    /// Show `view`, the one write every door but a hand's goes through; answers
    /// whether the view changed. A view already showing writes nothing, so a
    /// `tape_only` a preset saved with the tape off stays and the preset reads
    /// unedited. Moving to candles only clears `tape_only`, so the tape's own
    /// switch brings back a tape beside the candles.
    pub fn set_view(&mut self, view: LaneView) -> bool {
        if view == LaneView::Beside {
            self.open_beside()
        } else {
            self.write_view(view)
        }
    }

    /// Move beside the candles at the default width: the one door every way back
    /// but a hand's takes, however the side closed, the market's hold included. A
    /// pane already showing beside the candles stays where it is. It stands the hold
    /// down, or the hold would take straight back what was opened.
    pub fn open_beside(&mut self) -> bool {
        let released = self.market_held();
        if !self.write_view(LaneView::Beside) && !released {
            return false;
        }
        self.width_share = DEFAULT_LIVE_LANE_SHARE;
        self.market_hold.stand_down();
        true
    }

    /// The two switches for `view` and nothing else.
    pub(super) fn write_view(&mut self, view: LaneView) -> bool {
        if self.view() == view {
            return false;
        }
        self.enabled = view != LaneView::CandlesOnly;
        self.tape_only = view == LaneView::TapeOnly;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART: f32 = 1_000.0;
    const VIEWS: [LaneView; 3] = [LaneView::Beside, LaneView::CandlesOnly, LaneView::TapeOnly];

    fn lane(view: LaneView) -> LiveLaneStyle {
        let mut style = LiveLaneStyle::default();
        style.set_view(view);
        style
    }

    /// The view is the two switches read together: no third state to disagree
    /// with them, and a tape switched off is candles only whatever `tape_only`
    /// says.
    #[test]
    fn the_view_is_derived_from_the_two_switches() {
        for (enabled, tape_only, view) in [
            (true, false, LaneView::Beside),
            (true, true, LaneView::TapeOnly),
            (false, false, LaneView::CandlesOnly),
            (false, true, LaneView::CandlesOnly),
        ] {
            let style = LiveLaneStyle {
                enabled,
                tape_only,
                ..LiveLaneStyle::default()
            };
            assert_eq!(style.view(), view, "{enabled} {tape_only}");
        }
        assert_eq!(LiveLaneStyle::default().view(), LaneView::Beside);
    }

    /// Setting a view writes the switches the view reads back from and says
    /// whether anything changed. Only the way back beside moves the share.
    #[test]
    fn setting_a_view_is_idempotent_and_only_beside_resets_the_share() {
        for view in VIEWS {
            for from in VIEWS {
                let mut style = LiveLaneStyle {
                    width_share: 0.42,
                    ..lane(from)
                };
                assert_eq!(style.set_view(view), from != view, "{from:?} to {view:?}");
                assert_eq!(style.view(), view);
                assert!(!style.set_view(view), "the same view twice is no change");
                let share = if view == LaneView::Beside && from != view {
                    crate::config::DEFAULT_LIVE_LANE_SHARE
                } else {
                    0.42
                };
                assert_eq!(style.width_share, share, "{from:?} to {view:?}");
            }
        }
    }

    /// The side on screen is the view, and only the market's hold shows another.
    #[test]
    fn the_shown_view_differs_from_the_view_only_under_a_hold() {
        for view in VIEWS {
            let mut style = lane(view);
            assert_eq!(style.shown_view(), view);
            style.market_hold.held = true;
            let shown = if view == LaneView::Beside {
                LaneView::CandlesOnly
            } else {
                view
            };
            assert_eq!(style.shown_view(), shown, "{view:?}");
            assert_eq!(style.view(), view, "the stored view is not moved");
        }
    }

    /// Candles only leaves no stale `tape_only` behind, or the tape's own switch
    /// would bring back a tape-only pane nobody asked for.
    #[test]
    fn candles_only_clears_tape_only_so_the_tape_switch_returns_beside() {
        let mut style = lane(LaneView::TapeOnly);
        style.set_view(LaneView::CandlesOnly);
        assert!(!style.tape_only);
        assert!(style.set_enabled(true));
        assert_eq!(style.view(), LaneView::Beside);
    }

    /// Both lit is beside, one lit is that side alone, and the last lit side
    /// refuses to go out, whichever it is.
    #[test]
    fn the_last_side_showing_stays() {
        use LaneSide::{Tape, Trail};
        for (view, side, after) in [
            (LaneView::Beside, Trail, Some(LaneView::TapeOnly)),
            (LaneView::Beside, Tape, Some(LaneView::CandlesOnly)),
            (LaneView::CandlesOnly, Tape, Some(LaneView::Beside)),
            (LaneView::TapeOnly, Trail, Some(LaneView::Beside)),
            (LaneView::CandlesOnly, Trail, None),
            (LaneView::TapeOnly, Tape, None),
        ] {
            assert_eq!(view.toggled(side), after, "{view:?} {side:?}");
            if let Some(after) = after {
                assert_ne!(after.shows(side), view.shows(side), "{view:?} {side:?}");
            }
        }
    }

    /// Held by the market, the pane shows the candles alone and neither key moves
    /// it; the stored view and share are untouched, so a file never learns of it,
    /// and the band comes back at the width it had.
    #[test]
    fn a_hold_shows_candles_only_refuses_the_keys_and_writes_nothing() {
        let mut style = LiveLaneStyle {
            width_share: 0.42,
            ..LiveLaneStyle::default()
        };
        let band = style.resolved_width_px(CHART);
        style.market_hold.held = true;
        assert!(style.market_held());
        assert_eq!(style.shown_view(), LaneView::CandlesOnly);
        assert_eq!(style.view(), LaneView::Beside, "the stored view stays");
        assert_eq!(style.width_share, 0.42, "the stored share stays");
        assert!(style.fold_rail(CHART).is_some());
        assert_eq!(style.keyed_view(LaneView::tape_key), None);
        assert_eq!(style.keyed_view(LaneView::trail_key), None);

        let written = toml::to_string(&style).expect("writes");
        assert!(!written.contains("hold"), "{written}");
        assert!(!written.contains("fold ="), "{written}");
        let read: LiveLaneStyle = toml::from_str(&written).expect("reads");
        assert!(!read.market_held());
        assert_eq!(read.view(), LaneView::Beside);
        assert!(read.fold_market_closed);

        style.market_hold.held = false;
        assert_eq!(style.shown_view(), LaneView::Beside);
        assert_eq!(style.resolved_width_px(CHART), band, "its width unchanged");
        assert_eq!(
            style.keyed_view(LaneView::tape_key),
            Some(LaneView::TapeOnly)
        );
    }

    /// Off is the only value a file stores, and it comes back off.
    #[test]
    fn the_market_setting_round_trips_and_defaults_on() {
        let style = LiveLaneStyle {
            fold_market_closed: false,
            ..LiveLaneStyle::default()
        };
        let written = toml::to_string(&style).expect("writes");
        assert!(written.contains("fold_market_closed = false"), "{written}");
        let read: LiveLaneStyle = toml::from_str(&written).expect("reads");
        assert!(!read.fold_market_closed);
    }

    /// An explicit word on a held pane releases the hold and says so, whichever it
    /// asks; asking open is the rail's click, the default width.
    #[test]
    fn an_explicit_word_releases_the_hold() {
        for (fold, view, share) in [
            (
                LaneFold::Open,
                LaneView::Beside,
                crate::config::DEFAULT_LIVE_LANE_SHARE,
            ),
            (LaneFold::Tape, LaneView::CandlesOnly, 0.42),
            (LaneFold::Candles, LaneView::TapeOnly, 0.42),
        ] {
            let mut style = LiveLaneStyle {
                width_share: 0.42,
                ..LiveLaneStyle::default()
            };
            style.market_hold.held = true;
            assert!(style.set_fold(fold), "{fold:?}");
            assert!(!style.market_held());
            assert!(style.market_hold.stood_down);
            assert_eq!(style.view(), view);
            assert_eq!(style.width_share, share);
            assert!(!style.set_fold(fold), "a retry moves nothing");
        }
    }

    /// Opening a closed side inside a closed spell is a word the hold must not
    /// take straight back.
    #[test]
    fn opening_beside_stands_the_hold_down() {
        let mut style = lane(LaneView::CandlesOnly);
        assert!(style.set_view(LaneView::Beside));
        assert!(style.market_hold.stood_down);
    }

    /// Beside on a held pane is the same word as asking the fold open: one door.
    #[test]
    fn beside_on_a_held_pane_releases_it_at_the_default_width() {
        let mut style = LiveLaneStyle {
            width_share: 0.42,
            ..LiveLaneStyle::default()
        };
        style.market_hold.held = true;
        assert!(style.set_view(LaneView::Beside), "the pane moved");
        assert!(!style.market_held());
        assert!(style.market_hold.stood_down);
        assert_eq!(style.width_share, crate::config::DEFAULT_LIVE_LANE_SHARE);
        assert!(!style.set_view(LaneView::Beside), "a retry moves nothing");
    }

    /// The key goes to tape only and back to beside; there is no remembered view,
    /// so candles only returns through tape only.
    #[test]
    fn the_tape_key_goes_to_tape_only_and_from_there_beside() {
        assert_eq!(LaneView::Beside.tape_key(), LaneView::TapeOnly);
        assert_eq!(LaneView::CandlesOnly.tape_key(), LaneView::TapeOnly);
        assert_eq!(LaneView::TapeOnly.tape_key(), LaneView::Beside);
    }

    /// The two keys are mirrors: each leads to its side alone and back beside.
    #[test]
    fn the_trail_key_mirrors_the_tape_key() {
        assert_eq!(LaneView::Beside.trail_key(), LaneView::CandlesOnly);
        assert_eq!(LaneView::TapeOnly.trail_key(), LaneView::CandlesOnly);
        assert_eq!(LaneView::CandlesOnly.trail_key(), LaneView::Beside);
    }

    /// Beside the candles the lane takes its share's band; alone it is the chart,
    /// off it is nothing.
    #[test]
    fn the_band_follows_the_view() {
        let beside = LiveLaneStyle {
            width_share: 0.2,
            ..LiveLaneStyle::default()
        };
        assert!((beside.resolved_width_px(CHART) - CHART * 0.2).abs() < 0.01);
        assert_eq!(lane(LaneView::TapeOnly).resolved_width_px(CHART), CHART);
        assert_eq!(lane(LaneView::CandlesOnly).resolved_width_px(CHART), 0.0);
    }

    /// A tape-only mode a preset saved with the tape off is a real state: it reads
    /// candles only, asking for candles only changes nothing, so the preset stays
    /// unedited, and the tape's switch returns the tape it switched off.
    #[test]
    fn a_saved_tape_only_survives_the_tape_switch_and_a_view_already_shown() {
        let saved = || LiveLaneStyle {
            enabled: false,
            tape_only: true,
            ..LiveLaneStyle::default()
        };
        assert_eq!(saved().view(), LaneView::CandlesOnly);

        let mut asked = saved();
        assert!(!asked.set_view(LaneView::CandlesOnly), "already showing");
        assert_eq!(asked, saved(), "and nothing was written");

        let mut switched = saved();
        assert!(switched.set_enabled(true));
        assert_eq!(switched.view(), LaneView::TapeOnly);
        assert_eq!(switched.width_share, saved().width_share);
    }
}
