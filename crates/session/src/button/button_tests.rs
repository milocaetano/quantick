//! The History button's refusal: which reason, from the facts it reads.

mod refusal_tests {
    use super::super::*;

    /// Which of the three reasons the button shows, from the facts it reads.
    ///
    /// A source with no tape to page but a run-up on disk — a market
    /// replay is exactly this — points at the record it *can* serve, because
    /// "no older trades" alone is true and useless to a trader who pressed
    /// precisely because they want the past.
    #[test]
    fn the_refusal_names_the_reason_the_button_is_actually_off() {
        use crate::OlderCandles;
        assert_eq!(
            HistoryPagingOff::of(true, OlderCandles::FeedServesNone),
            None,
            "a feed that pages takes the press"
        );
        assert_eq!(
            HistoryPagingOff::of(false, OlderCandles::Available),
            Some(HistoryPagingOff::CandlesInstead),
            "a recording's past is candles, and the refusal says so"
        );
        assert_eq!(
            HistoryPagingOff::of(false, OlderCandles::FeedServesNone),
            Some(HistoryPagingOff::NothingToPage),
            "and a source with neither gets the plain refusal"
        );
        assert!(
            HistoryPagingOff::CandlesInstead
                .hover()
                .contains("older candles"),
            "the way forward has to be named"
        );
        assert!(
            !HistoryPagingOff::NothingToPage
                .hover()
                .contains("older candles"),
            "and a source without candles must not send the trader to a menu \
             entry that is not there"
        );
    }

    /// The refusal must never end at a control that is itself greyed out.
    ///
    /// A recording with a run-up on disk, opened on the order-flow default —
    /// a tick chart, no time pane — is the case: `+ older` is off because
    /// there are no trades to page, and `+ older candles` is off because
    /// `OlderCandles::NoChartCutByTime`. Pointing the first at the second
    /// would send the trader bouncing between two dead controls, which is a
    /// politer version of the silence this branch is fixing.
    #[test]
    fn a_refusal_never_points_at_a_control_that_is_also_off() {
        use crate::OlderCandles;
        for candles in [
            OlderCandles::NoChartCutByTime,
            OlderCandles::FeedServesNone,
            OlderCandles::RecordStartsHere,
        ] {
            let reason = HistoryPagingOff::of(false, candles).expect("the button is off");
            assert_ne!(
                reason,
                HistoryPagingOff::CandlesInstead,
                "{candles:?} cannot take a press, so it is not a way forward"
            );
            assert!(
                candles.why_not().is_some(),
                "the premise: {candles:?} is a disabled candle entry"
            );
        }
        assert_eq!(
            HistoryPagingOff::of(false, OlderCandles::NoChartCutByTime),
            Some(HistoryPagingOff::LeadInNeeded),
            "and a chart with no time pane is sent to the switch that fixes that"
        );
        assert!(
            HistoryPagingOff::LeadInNeeded.hover().contains("lead-in"),
            "named, so the trader knows what to look for: {}",
            HistoryPagingOff::LeadInNeeded.hover()
        );
    }
}
