//! What the History button says when it cannot take a press, and how far
//! the candle entry beside it reaches: the words, decided beside the
//! reasons they report.

use crate::OlderCandles;

/// Why the History button is not taking a press.
///
/// One value rather than a pair of bools: a bool is enough to grey a control
/// out and not enough to say *why*, and a control offering two reasons while
/// being off for a third tells the trader something untrue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPagingOff {
    /// This source has no venue to page prints from, but it does hold the
    /// past in the *other* record and the menu entry beside this button can
    /// fetch it. A market replay with a run-up on disk is exactly this.
    CandlesInstead,
    /// The same, except nothing on this chart is cut by time, so the candle
    /// entry cannot be pressed either until the lead-in is switched on.
    LeadInNeeded,
    /// There are no older trades to be had, by any route.
    NothingToPage,
}

impl HistoryPagingOff {
    /// Which reason the button is off for, or `None` while it takes a press.
    ///
    /// `candles` is the candle entry's *own* state, not the raw capability, so
    /// this refusal can never send the trader to a control that is greyed out
    /// for a reason of its own.
    #[must_use]
    pub fn of(feed_can_page: bool, candles: OlderCandles) -> Option<Self> {
        if feed_can_page {
            return None;
        }
        Some(match candles {
            OlderCandles::Available | OlderCandles::Fetching | OlderCandles::NotArrivedYet => {
                Self::CandlesInstead
            }
            OlderCandles::NoChartCutByTime => Self::LeadInNeeded,
            OlderCandles::FeedServesNone | OlderCandles::RecordStartsHere => Self::NothingToPage,
        })
    }

    /// What the trader is told hovering the button in this state.
    #[must_use]
    pub const fn hover(self) -> &'static str {
        match self {
            Self::CandlesInstead => {
                "no older trades to fetch here — this source's past is venue \
                 candles: open the menu beside this button and use \"+ older \
                 candles\""
            }
            Self::LeadInNeeded => {
                "no older trades to fetch here — this source's past is venue \
                 candles, and no chart here is cut by time: switch on the \
                 venue lead-in under View to put them in front of these bars"
            }
            Self::NothingToPage => {
                "no older trades to fetch: this feed only streams forward, or \
                 its history is already all here"
            }
        }
    }
}

/// A candle-history span in the words the menu uses: whole weeks, else days,
/// else hours. Rendered from the constant so the control and the request can
/// never disagree about how far one press reaches.
#[must_use]
pub fn fmt_history_span(span_ms: i64) -> String {
    const HOUR_MS: i64 = 60 * 60 * 1_000;
    const DAY_MS: i64 = 24 * HOUR_MS;
    const WEEK_MS: i64 = 7 * DAY_MS;
    let plural = |count: i64, unit: &str| {
        if count == 1 {
            unit.to_owned()
        } else {
            format!("{count} {unit}s")
        }
    };
    match span_ms {
        span if span >= WEEK_MS && span % WEEK_MS == 0 => plural(span / WEEK_MS, "week"),
        span if span >= DAY_MS => plural(span / DAY_MS, "day"),
        span if span >= HOUR_MS => plural(span / HOUR_MS, "hour"),
        span => format!("{} ms", span.max(0)),
    }
}

#[cfg(test)]
mod button_tests;
