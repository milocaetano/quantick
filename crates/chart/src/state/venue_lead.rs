//! The venue's history of the first bar's bucket, merged into that bar.
//!
//! A time chart's first bar opens on the first print the app retained, inside
//! its bucket rather than on its edge, and the venue candles covering the
//! same bucket are trimmed off the prefix because they overlap it. The lead
//! (`quantick_feed::candles::seam_lead`) is the venue's record of the stretch
//! between the bucket's edge and that print, and this module puts it in front
//! of the first bar — forming or closed — so a daily bar opened at 06:14 holds
//! the whole day, and a weekly one the days before it.
//!
//! The merge is the engine's one summary of a run of bars ([`Bar::absorb`]):
//! open and stamp from the lead, close from the bar, extremes and volumes of
//! both. It applies only where it belongs — a lead of the first bar's own
//! bucket that ended before its first print — so a stale lead after a page of
//! older prints moved the first bar is left out rather than merged into the
//! wrong one. The builder's own bar is kept beside the merged one, so a new
//! lead replaces the old instead of piling onto it, and every rebuild starts
//! from what the prints alone say. Deterministic: same prints and lead in,
//! same bars out.

use quantick_engine::Bar;
use quantick_engine::time_bucket::time_bucket_start;

use super::ChartState;

/// The lead a chart holds, and where it stands against the first bar.
#[derive(Debug, Clone, Default)]
pub(super) struct VenueLead {
    bar: Option<Bar>,
    /// The first closed bar as the builder cut it, while the lead is merged
    /// into it.
    seated_on: Option<Bar>,
    /// Whether the first closed bar was judged against the lead already.
    judged: bool,
}

impl VenueLead {
    /// The same lead, judged afresh against a series about to be rebuilt.
    pub(super) fn carried(&self) -> Self {
        Self {
            bar: self.bar.clone(),
            ..Self::default()
        }
    }

    /// The bars were cut again: nothing is merged into them yet.
    pub(super) fn rebuilt(&mut self) {
        self.seated_on = None;
        self.judged = false;
    }

    /// Hold `lead` instead, putting the first closed bar back as the prints
    /// cut it; whether the lead changed. The caller seats the new one.
    pub(super) fn replace(&mut self, lead: Option<Bar>, bars: &mut [Bar]) -> bool {
        if self.bar == lead {
            return false;
        }
        if let (Some(raw), Some(first)) = (self.seated_on.take(), bars.first_mut()) {
            *first = raw;
        }
        self.judged = false;
        self.bar = lead;
        true
    }

    /// Merge the lead into the first closed bar, once per cut of the series,
    /// or into `partial` — just handed over by the builder — while no bar
    /// has closed. Cheap when there is no lead.
    pub(super) fn seat(
        &mut self,
        interval_ms: Option<i64>,
        bars: &mut [Bar],
        partial: &mut Option<Bar>,
    ) {
        let Some(lead) = self.bar.as_ref() else {
            return;
        };
        if let Some(first) = bars.first_mut() {
            if self.judged {
                return;
            }
            self.judged = true;
            if fits(interval_ms, lead, first) {
                let merged = merged(lead, first);
                self.seated_on = Some(std::mem::replace(first, merged));
            }
        } else if let Some(forming) = partial.as_mut()
            && fits(interval_ms, lead, forming)
        {
            *forming = merged(lead, forming);
        }
    }

    /// The lead, when it is merged into the first bar: `forming` is the
    /// builder's own forming bar.
    pub(super) fn merged_lead(
        &self,
        interval_ms: Option<i64>,
        bars: &[Bar],
        forming: Option<&Bar>,
    ) -> Option<&Bar> {
        let lead = self.bar.as_ref()?;
        if self.seated_on.is_some() {
            return Some(lead);
        }
        let forming = forming.filter(|_| bars.is_empty())?;
        fits(interval_ms, lead, forming).then_some(lead)
    }

    /// The first bar's open as the prints alone cut it.
    pub(super) fn first_print_open_ms(&self, bars: &[Bar], forming: Option<&Bar>) -> Option<i64> {
        self.seated_on
            .as_ref()
            .or_else(|| bars.first())
            .or(forming)
            .map(|bar| bar.open_time)
    }
}

impl ChartState {
    /// Merge `lead` into the first bar where it fits (see [`self`]); `None`
    /// takes it away. Whether the lead changed.
    pub fn set_venue_lead(&mut self, lead: Option<Bar>) -> bool {
        let changed = self.venue_lead.replace(lead, &mut self.bars);
        if changed {
            self.refresh_partial();
            self.bump_series_revision();
        }
        changed
    }

    /// The lead merged into the first bar, when it is: that bar summarises
    /// venue candles *and* prints.
    #[must_use]
    pub fn venue_lead(&self) -> Option<&Bar> {
        let (interval, forming) = (self.spec.time_interval_ms(), self.builder.partial());
        self.venue_lead.merged_lead(interval, &self.bars, forming)
    }

    /// The first bar's open as its prints alone cut it: what a lead is cut
    /// against.
    #[must_use]
    pub fn first_print_open_ms(&self) -> Option<i64> {
        let forming = self.builder.partial();
        self.venue_lead.first_print_open_ms(&self.bars, forming)
    }
}

/// Whether `lead` belongs in front of `first`, a bar as the prints cut it:
/// a time chart's, the same bucket, and over before the first print.
fn fits(interval_ms: Option<i64>, lead: &Bar, first: &Bar) -> bool {
    interval_ms.is_some_and(|interval| {
        lead.close_time < first.open_time
            && time_bucket_start(lead.open_time, interval)
                == time_bucket_start(first.open_time, interval)
    })
}

fn merged(lead: &Bar, first: &Bar) -> Bar {
    let mut bar = lead.clone();
    bar.absorb(first);
    bar
}

#[cfg(test)]
mod venue_lead_tests {
    use super::*;
    use quantick_engine::time_bucket::DAY_MS;
    use quantick_engine::{BarSpec, Side, Trade};
    use rust_decimal::Decimal;

    const HOUR: i64 = 3_600_000;
    /// 06:14:03 on day 9 after the epoch.
    const FIRST_PRINT: i64 = 9 * DAY_MS + 6 * HOUR + 14 * 60_000 + 3_000;

    fn print(at: i64, price: i64) -> Trade {
        Trade {
            agg_id: u64::try_from(at).unwrap(),
            timestamp_ms: at,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    /// The venue's 00:00 to 06:13 of day 9.
    fn lead() -> Bar {
        Bar {
            open_time: 9 * DAY_MS,
            close_time: FIRST_PRINT - 4_000,
            open: Decimal::from(90),
            high: Decimal::from(130),
            low: Decimal::from(80),
            close: Decimal::from(99),
            buy_volume: Decimal::from(40),
            sell_volume: Decimal::from(60),
            trade_count: 500,
        }
    }

    fn chart() -> ChartState {
        let mut chart = ChartState::new(BarSpec::Time(DAY_MS));
        chart.ingest_backfill(&[print(FIRST_PRINT, 100), print(FIRST_PRINT + HOUR, 101)]);
        chart
    }

    /// The forming day opens at 00:00 on the venue's open, and holds the
    /// venue's part and the prints' part, each once — and keeps doing so as
    /// prints land.
    #[test]
    fn the_forming_first_bar_carries_the_lead_through_every_print() {
        let mut chart = chart();
        let raw = chart.partial().cloned().unwrap();
        assert!(chart.set_venue_lead(Some(lead())));
        let expected = merged(&lead(), &raw);
        assert_eq!(chart.partial(), Some(&expected));
        assert_eq!(chart.partial().unwrap().open_time, 9 * DAY_MS);
        assert_eq!(chart.partial().unwrap().trade_count, 502);
        assert_eq!(chart.venue_lead(), Some(&lead()));
        assert_eq!(chart.first_print_open_ms(), Some(FIRST_PRINT));

        chart.ingest_live(&print(FIRST_PRINT + 2 * HOUR, 140));
        let forming = chart.partial().unwrap();
        assert_eq!(forming.open, Decimal::from(90));
        assert_eq!(forming.high, Decimal::from(140));
        assert_eq!(forming.low, Decimal::from(80));
        assert_eq!(forming.close, Decimal::from(140));
        assert_eq!(forming.trade_count, 503, "nothing counted twice");
        assert!(
            !chart.set_venue_lead(Some(lead())),
            "the same lead is no news"
        );
    }

    /// Once the first day closes, the closed bar keeps the lead and the next
    /// day is the prints' alone; a rebuild cuts the same bars again.
    #[test]
    fn the_closed_first_bar_keeps_the_lead_and_a_rebuild_reapplies_it() {
        let mut chart = chart();
        chart.set_venue_lead(Some(lead()));
        chart.ingest_live(&print(10 * DAY_MS + HOUR, 102));
        assert_eq!(chart.bars().len(), 1);
        assert_eq!(chart.bars()[0].open_time, 9 * DAY_MS);
        assert_eq!(chart.bars()[0].trade_count, 502);
        assert_eq!(chart.partial().unwrap().trade_count, 1, "day 10 is its own");
        assert_eq!(chart.venue_lead(), Some(&lead()));

        let shown = chart.bars().to_vec();
        chart.rebuild_bars();
        assert_eq!(chart.bars(), shown.as_slice(), "deterministic");

        // A new lead replaces the old one rather than piling onto it.
        let shorter = Bar {
            close_time: 9 * DAY_MS + HOUR,
            trade_count: 10,
            ..lead()
        };
        chart.set_venue_lead(Some(shorter));
        assert_eq!(chart.bars()[0].trade_count, 12);
        chart.set_venue_lead(None);
        assert_eq!(
            chart.bars()[0].open_time,
            FIRST_PRINT,
            "the prints' own bar"
        );
        assert_eq!(chart.bars()[0].trade_count, 2);
        assert_eq!(chart.venue_lead(), None);
    }

    /// A lead of another bucket, or one reaching the first print, is not
    /// merged — the first bar moved, or the lead would count prints twice.
    #[test]
    fn a_lead_that_does_not_fit_the_first_bar_is_left_out() {
        let mut chart = chart();
        let raw = chart.partial().cloned();
        let yesterday = Bar {
            open_time: 8 * DAY_MS,
            close_time: 8 * DAY_MS + HOUR,
            ..lead()
        };
        chart.set_venue_lead(Some(yesterday));
        assert_eq!(chart.partial().cloned(), raw);
        assert_eq!(chart.venue_lead(), None);
        let overlapping = Bar {
            close_time: FIRST_PRINT,
            ..lead()
        };
        chart.set_venue_lead(Some(overlapping));
        assert_eq!(chart.partial().cloned(), raw);

        // Older prints move the first bar back a day: the lead cut for the
        // old one no longer fits, and the prints' own answer stands.
        chart.set_venue_lead(Some(lead()));
        chart.prepend_history(&[print(8 * DAY_MS + HOUR, 95)]);
        assert_eq!(chart.bars()[0].open_time, 8 * DAY_MS + HOUR);
        assert_eq!(chart.venue_lead(), None);
        assert_eq!(chart.partial().unwrap().open_time, FIRST_PRINT);
    }

    /// A tick chart has no bucket to lead into; a new spec drops the lead.
    #[test]
    fn only_a_time_chart_takes_a_lead_and_a_new_spec_drops_it() {
        let mut ticks = ChartState::new(BarSpec::Tick(10));
        ticks.ingest_backfill(&[print(FIRST_PRINT, 100)]);
        ticks.set_venue_lead(Some(lead()));
        assert_eq!(ticks.venue_lead(), None);
        assert_eq!(ticks.partial().unwrap().trade_count, 1);

        let mut chart = chart();
        chart.set_venue_lead(Some(lead()));
        chart.set_spec(BarSpec::Time(2 * DAY_MS));
        assert_eq!(chart.venue_lead(), None);
        assert_eq!(chart.partial().unwrap().open_time, FIRST_PRINT);
    }
}
