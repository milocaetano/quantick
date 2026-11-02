//! The flow pane's three views, a layer over the lane's two switches.
//!
//! The pane shows the candles and the tape side by side, the candles alone or
//! the tape alone. The screen calls the candles side the Trail. [`LaneView`] is
//! not a third switch: it is the lane's `enabled` and `tape_only` read together,
//! so nothing can disagree with them.
//!
//! A view is not a fold. Moving between views writes only those two switches,
//! so a side the divider folded to its rail is still folded on the way back
//! beside the candles.

use super::lane::LiveLaneStyle;
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

    /// The side on screen: [`Self::view`], except that beside the candles a
    /// fold leaves only the other side showing.
    #[must_use]
    pub fn shown_view(&self) -> LaneView {
        match (self.view(), self.shown_fold()) {
            (LaneView::Beside, LaneFold::Candles) => LaneView::TapeOnly,
            (LaneView::Beside, LaneFold::Tape) => LaneView::CandlesOnly,
            (view, _) => view,
        }
    }

    /// Show `view`, the one write every view goes through; answers whether the
    /// view changed. A view already showing writes nothing, so a `tape_only` a
    /// preset saved with the tape off stays and the preset reads unedited. Only
    /// the two switches move, written directly because
    /// [`Self::set_enabled`] would open a fold. Moving to candles only clears
    /// `tape_only`, so the tape's own switch brings back a tape beside the
    /// candles.
    pub fn set_view(&mut self, view: LaneView) -> bool {
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

    /// Setting a view writes the switches the view reads back from, says whether
    /// anything changed, and leaves the share alone.
    #[test]
    fn setting_a_view_is_idempotent_and_keeps_the_share() {
        for view in VIEWS {
            for from in VIEWS {
                let mut style = LiveLaneStyle {
                    width_share: 0.42,
                    ..lane(from)
                };
                assert_eq!(style.set_view(view), from != view, "{from:?} to {view:?}");
                assert_eq!(style.view(), view);
                assert!(!style.set_view(view), "the same view twice is no change");
                assert_eq!(style.width_share, 0.42);
            }
        }
    }

    /// A fold outlives every move between views, whichever side it holds, and is
    /// there again on the way back beside the candles.
    #[test]
    fn a_fold_survives_every_transition_and_returns_beside_the_candles() {
        for fold in [LaneFold::Candles, LaneFold::Tape] {
            for from in VIEWS {
                for to in VIEWS {
                    let mut style = LiveLaneStyle { fold, ..lane(from) };
                    style.set_view(to);
                    assert_eq!(style.fold, fold, "{fold:?}: {from:?} to {to:?}");
                }
            }
            let mut style = LiveLaneStyle {
                fold,
                ..LiveLaneStyle::default()
            };
            let rail = style.fold_rail(CHART);
            assert!(rail.is_some(), "{fold:?}");
            for next in [LaneView::CandlesOnly, LaneView::TapeOnly, LaneView::Beside] {
                assert!(style.set_view(next));
                assert_eq!(style.fold, fold, "{fold:?} at {next:?}");
            }
            assert_eq!(style.view(), LaneView::Beside);
            assert_eq!(style.fold_rail(CHART), rail, "{fold:?}: the rail is back");
        }
    }

    /// The side on screen: the view, and a fold beside the candles names the
    /// other side; off or alone on the pane the fold shows nothing.
    #[test]
    fn the_shown_view_reads_a_fold_as_the_side_left_on_screen() {
        for (view, fold, shown) in [
            (LaneView::Beside, LaneFold::Open, LaneView::Beside),
            (LaneView::Beside, LaneFold::Candles, LaneView::TapeOnly),
            (LaneView::Beside, LaneFold::Tape, LaneView::CandlesOnly),
            (
                LaneView::CandlesOnly,
                LaneFold::Candles,
                LaneView::CandlesOnly,
            ),
            (LaneView::CandlesOnly, LaneFold::Tape, LaneView::CandlesOnly),
            (LaneView::TapeOnly, LaneFold::Candles, LaneView::TapeOnly),
            (LaneView::TapeOnly, LaneFold::Tape, LaneView::TapeOnly),
        ] {
            let style = LiveLaneStyle { fold, ..lane(view) };
            assert_eq!(style.shown_view(), shown, "{view:?} {fold:?}");
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

    /// Held by the market, a hold shows as the tape's fold and neither key moves
    /// the pane; the saved fold is untouched, so a file never learns of it.
    #[test]
    fn a_hold_folds_the_tape_refuses_the_keys_and_saves_nothing() {
        let mut style = LiveLaneStyle::default();
        style.market_hold.held = true;
        assert!(style.market_held());
        assert_eq!(style.shown_fold(), LaneFold::Tape);
        assert_eq!(style.shown_view(), LaneView::CandlesOnly);
        assert_eq!(style.view(), LaneView::Beside);
        assert!(style.fold_rail(CHART).is_some());
        assert_eq!(style.fold, LaneFold::Open, "the saved fold stays");
        assert_eq!(style.keyed_view(LaneView::tape_key), None);
        assert_eq!(style.keyed_view(LaneView::trail_key), None);

        let written = toml::to_string(&style).expect("writes");
        assert!(!written.contains("hold"), "{written}");
        assert!(!written.contains("fold"), "{written}");
        let read: LiveLaneStyle = toml::from_str(&written).expect("reads");
        assert!(!read.market_held());
        assert!(read.fold_market_closed);

        style.market_hold.held = false;
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

    /// A hand on the fold releases the hold and says so, whichever fold it asks.
    #[test]
    fn a_hand_on_the_fold_releases_the_hold() {
        for fold in [LaneFold::Open, LaneFold::Tape, LaneFold::Candles] {
            let mut style = LiveLaneStyle::default();
            style.market_hold.held = true;
            assert!(style.set_fold(fold), "{fold:?}");
            assert!(!style.market_held());
            assert!(style.market_hold.stood_down);
            assert_eq!(style.fold, fold);
            assert!(!style.set_fold(fold), "a retry moves nothing");
        }
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
    }
}
