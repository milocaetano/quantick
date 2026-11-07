//! Venue candles on a feed that serves every interval a pane is cut at: one
//! series per interval, asked for at the pane's own interval, held across
//! interval switches, and paged back as the pane scrolls toward its oldest
//! bar.
//!
//! The other path, [`crate::venue`], folds one base of minutes or days for
//! every pane; it stays the answer for a feed without
//! `FeedCapabilities::native_candle_intervals`. Both meet the tape at the same
//! seam: venue candles before the bucket of the first trade, built bars from
//! it on.

use std::collections::BTreeMap;

use quantick_engine::Bar;
use quantick_engine::time_bucket::DAY_MS;
use quantick_sources::candle_base::CandleAsk;
use quantick_sources::candle_series::seam::{SeamMinutes, reads_seam_minutes, seam_stretch};
use quantick_sources::candle_series::{CandleSeries, SeriesStatus, native_series_for};
use quantick_sources::candles::{
    OlderCandles, fold, stamp_on_buckets, starts_inside_bucket, trim_borrowed_to_seam, trim_to_seam,
};
use quantick_sources::config::FeedCapabilities;
use quantick_sources::ohlcv::{OHLCV_BASE_INTERVAL_MS, OhlcvSlice};

use crate::view::PaneView;
use crate::{
    CandlePurpose, Delivery, Effect, FeedRequest, Refusal, SeamTrace, SessionEvent, Step, Wait,
    WaitChange,
};

/// Every series a tab holds, by the interval it was asked for at.
#[derive(Debug, Default)]
pub(crate) struct NativeCandles {
    series: BTreeMap<i64, CandleSeries>,
    /// The series whose request is out. One per tab, not one per series: the
    /// feed serves one candle fetch at a time and refuses a second, so a
    /// second series asks once the first is answered.
    in_flight: Option<i64>,
    /// The minutes the seam bars' leads read, and whether their request is
    /// the one out.
    seam: SeamMinutes,
    seam_out: bool,
}

/// The series a pane reads: its interval's, or the venue's minutes for a
/// pane cut by trades under the lead-in; `None` for none.
fn series_key(interval_ms: Option<i64>, lead_in: bool) -> Option<i64> {
    match interval_ms {
        Some(interval_ms) => native_series_for(interval_ms),
        None => lead_in.then_some(OHLCV_BASE_INTERVAL_MS),
    }
}

impl NativeCandles {
    /// Ask for what the panes lack, one request at a time: every pane's
    /// opening page first, then the page behind a series whose pane scrolled
    /// near its oldest bar.
    ///
    /// Rate: **per-frame**; a walk over the panes and a map lookup each.
    pub(crate) fn poll(&mut self, step: &mut Step<'_>, tab_id: u64, lead_in: bool) {
        self.series.values_mut().for_each(CandleSeries::tick);
        if self.pending() || !step.tab.capabilities.ohlcv_history {
            return;
        }
        if let Some((ask, purpose)) = self.next_request(step.tab.panes, lead_in) {
            step.feed(FeedRequest::Candles {
                tab_id,
                ask,
                purpose,
            });
        }
    }

    fn next_request(
        &self,
        panes: &[PaneView<'_>],
        lead_in: bool,
    ) -> Option<(CandleAsk, CandlePurpose)> {
        let wanted = || {
            panes.iter().filter_map(move |pane| {
                Some((
                    series_key(pane.interval_ms, lead_in)?,
                    pane.first_visible_slot,
                ))
            })
        };
        let opening = wanted().find(|&(key, _)| {
            self.series
                .get(&key)
                .is_none_or(|series| !series.is_answered() && series.next_ask(key, None).is_some())
        });
        if let Some((key, _)) = opening {
            return Some((CandleSeries::opening_ask(key), CandlePurpose::Opening));
        }
        let seams = panes
            .iter()
            .filter_map(|pane| Some((pane.first_print_open_ms?, pane.interval_ms?)));
        if let Some(ask) = self.seam.ask(seam_stretch(seams)) {
            return Some((ask, CandlePurpose::SeamMinutes));
        }
        wanted().find_map(|(key, first_visible)| {
            let series = self.series.get(&key)?;
            let ask = series.next_ask(key, first_visible)?;
            let oldest_ms = series.oldest_ms()?;
            Some((ask, CandlePurpose::Older { oldest_ms }))
        })
    }

    /// What became of a request: only one the feed took is out.
    pub(crate) fn acknowledge(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        ask: CandleAsk,
        purpose: CandlePurpose,
        delivery: Delivery,
    ) {
        let older = matches!(purpose, CandlePurpose::Older { .. });
        match delivery {
            Delivery::Sent if purpose == CandlePurpose::SeamMinutes => {
                self.seam.sent(ask);
                self.seam_out = true;
                step.wait(Wait::VenueCandles, WaitChange::Begin);
            }
            Delivery::Sent => {
                self.in_flight = Some(ask.interval_ms);
                self.series.entry(ask.interval_ms).or_default().sent(ask);
                step.wait(Wait::VenueCandles, WaitChange::Begin);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_SERIES_REQUESTED",
                    tab = tab_id,
                    symbol = %step.tab.symbol,
                    interval_ms = ask.interval_ms,
                    span_ms = ask.span_ms,
                    before_ms = ask.before_ms.unwrap_or(0),
                    action = if older { "await_prepend" } else { "await_opening_page" },
                    "asked the venue for a page of candles at the pane's interval"
                );
            }
            // A busy channel is a busy frame: the next poll asks again.
            Delivery::Busy => tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_REQUEST_BACKPRESSURE",
                tab = tab_id,
                action = "retry_next_frame",
                "candle-series request not queued; channel full"
            ),
            Delivery::Gone => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_REQUEST_CHANNEL_CLOSED",
                tab = tab_id,
                symbol = %step.tab.symbol,
                action = "no_history_until_feed_restart",
                "candle-series request cannot be sent; the feed is gone"
            ),
        }
    }

    /// Take a reply into the series it answers, and rebuild the prefixes
    /// when it brought anything.
    pub(crate) fn take_reply(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        interval_ms: i64,
        bars: Vec<Bar>,
        slice: OhlcvSlice,
        lead_in: bool,
    ) {
        step.event(SessionEvent::CandleReply {
            interval_ms,
            bars: bars.len(),
            slice,
            discarded: !self.pending(),
        });
        if self.seam_out {
            // The seam's minutes; any other interval is not minutes.
            let minutes = if interval_ms == OHLCV_BASE_INTERVAL_MS {
                bars
            } else {
                Vec::new()
            };
            if self.seam.take_reply(minutes, slice) {
                self.seam_out = false;
                step.wait(Wait::VenueCandles, WaitChange::End);
                self.install(step, lead_in);
            }
            return;
        }
        let Some(key) = self.in_flight else {
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_SLICE_DISCARDED",
                tab = tab_id,
                interval_ms,
                bars = bars.len(),
                action = "await_fresh_request",
                "dropped a candle reply no request is waiting for"
            );
            return;
        };
        // Candles at another interval than asked would be drawn at the wrong
        // width: the request is answered with nothing, and cut short.
        let (bars, slice) = if interval_ms == key || bars.is_empty() {
            (bars, slice)
        } else {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_UNEXPECTED_INTERVAL",
                tab = tab_id,
                interval_ms,
                expected_ms = key,
                bars = bars.len(),
                action = "refuse_candles",
                "candles arrived at an interval the series was not asked for"
            );
            let short = slice
                .is_last()
                .then_some(OhlcvSlice::Last { complete: false });
            (Vec::new(), short.unwrap_or(slice))
        };
        let received = bars.len();
        let series = self.series.entry(key).or_default();
        let reaching_back = !series.bars().is_empty();
        let settled = series.take_reply(bars, slice);
        if slice.is_last() {
            self.in_flight = None;
            step.wait(Wait::VenueCandles, WaitChange::End);
        }
        let grew = slice == OhlcvSlice::More && received > 0;
        if let Some(settled) = settled {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_SERIES_SETTLED",
                tab = tab_id,
                symbol = %step.tab.symbol,
                interval_ms = key,
                bars = received,
                added = settled.added,
                held = series.bars().len(),
                exhausted = settled.exhausted,
                action = if settled.added > 0 { "install_prefix" } else { "no_prefix_change" },
                "a page of candles at the pane's interval settled"
            );
            if reaching_back {
                step.event(SessionEvent::OlderCandlesSettled {
                    moved: settled.added > 0,
                    exhausted: settled.exhausted,
                });
            }
            if settled.added > 0 {
                self.install(step, lead_in);
            }
        } else if grew {
            self.install(step, lead_in);
        }
    }

    /// A reply handed in directly, as a scripted hook does: what a request
    /// for that series having gone out looks like is raised first.
    pub(crate) fn deliver(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        interval_ms: i64,
        bars: Vec<Bar>,
        slice: OhlcvSlice,
        lead_in: bool,
    ) {
        if !self.pending() {
            self.in_flight = Some(interval_ms);
            self.series
                .entry(interval_ms)
                .or_default()
                .sent(CandleSeries::opening_ask(interval_ms));
            step.wait(Wait::VenueCandles, WaitChange::Begin);
        }
        self.take_reply(step, tab_id, interval_ms, bars, slice, lead_in);
    }

    /// Rebuild every pane's prefix from the series it reads. Free of the
    /// venue: an interval switch to a series held lands here and nowhere
    /// else.
    pub(crate) fn install(&self, step: &mut Step<'_>, lead_in: bool) {
        for (index, pane) in step.tab.panes.iter().enumerate() {
            let bars = self.prefix_for(pane, lead_in);
            // The bucket of the first trade, before it: the seam's minutes.
            let seam = pane
                .interval_ms
                .zip(pane.first_print_open_ms)
                .filter(|&(interval_ms, _)| reads_seam_minutes(interval_ms));
            let effect = match seam {
                Some((interval_ms, first_print_ms)) => {
                    let outcome = self.seam.lead(first_print_ms, interval_ms);
                    let covers_seam = outcome.covers_seam();
                    let token = outcome.token();
                    Effect::InstallPrefix {
                        pane: index,
                        bars,
                        lead: outcome.into_bar(),
                        covers_seam,
                        seam: Some(SeamTrace {
                            first_print_ms,
                            interval_ms,
                            token,
                        }),
                    }
                }
                None => Effect::InstallPrefix {
                    pane: index,
                    bars,
                    lead: None,
                    covers_seam: false,
                    seam: None,
                },
            };
            step.effects.push(effect);
        }
    }

    fn prefix_for(&self, pane: &PaneView<'_>, lead_in: bool) -> Vec<Bar> {
        let Some(key) = series_key(pane.interval_ms, lead_in) else {
            return Vec::new();
        };
        let Some(series) = self.series.get(&key) else {
            return Vec::new();
        };
        // A pane cut by trades takes the minutes unfolded, cut at its seam.
        let interval_ms = pane.interval_ms.unwrap_or(key);
        if key == interval_ms && key < DAY_MS {
            return trim_borrowed_to_seam(series.bars(), pane.first_bar, pane.partial, interval_ms);
        }
        let folded = if key == interval_ms {
            // Days and longer are stamped on the engine's buckets, as a fold
            // from days stamps them: a month candle names no length.
            let mut bars = series.bars().to_vec();
            stamp_on_buckets(&mut bars, interval_ms);
            bars
        } else {
            let mut folded = fold(series.bars(), key, interval_ms);
            // A page that ends inside a bucket would draw that bucket short
            // and unlabelled; it waits for the page behind. At the record's
            // start it stays, labelled by `record_starts_inside`.
            let cut =
                !series.is_exhausted() && starts_inside_bucket(series.bars(), key, interval_ms);
            folded.drain(..usize::from(cut).min(folded.len()));
            folded
        };
        trim_to_seam(folded, pane.first_bar, pane.partial, interval_ms)
    }

    /// A pane was built: it reads a series already held, or asks.
    pub(crate) fn pane_added(&mut self, step: &mut Step<'_>, tab_id: u64, lead_in: bool) {
        self.install(step, lead_in);
        self.poll(step, tab_id, lead_in);
    }

    /// Take over a fresh feed session. `keep_timeline` keeps every series;
    /// anything in flight goes either way, with the channel carrying it.
    pub(crate) fn attach(&mut self, step: &mut Step<'_>, keep_timeline: bool) {
        if keep_timeline {
            self.series.values_mut().for_each(CandleSeries::abandon);
            self.seam.abandon();
        } else if !self.series.is_empty() {
            self.series.clear();
            step.clear_prefixes();
        }
        if !keep_timeline {
            self.seam = SeamMinutes::default();
        }
        self.in_flight = None;
        self.seam_out = false;
    }

    /// The series a pane cut at `interval_ms` reads, and where it stands.
    pub(crate) fn status(&self, interval_ms: Option<i64>, lead_in: bool) -> Option<SeriesStatus> {
        let key = series_key(interval_ms, lead_in)?;
        Some(self.series.get(&key).map_or_else(
            || CandleSeries::default().status(key),
            |series| series.status(key),
        ))
    }

    /// Whether asking for older candles could get the first pane that reads
    /// candles anything, and when not, why.
    pub(crate) fn older_candles(
        &self,
        capabilities: FeedCapabilities,
        panes: &[PaneView<'_>],
        lead_in: bool,
    ) -> OlderCandles {
        let key = panes
            .iter()
            .find_map(|pane| series_key(pane.interval_ms, lead_in));
        let series = key.and_then(|key| self.series.get(&key));
        if !capabilities.ohlcv_history {
            OlderCandles::FeedServesNone
        } else if key.is_none() {
            OlderCandles::NoChartCutByTime
        } else if self.pending() {
            OlderCandles::Fetching
        } else if series.is_some_and(CandleSeries::is_exhausted) {
            // Before the oldest candle: a series answered empty has none,
            // and the status calls it exhausted too.
            OlderCandles::RecordStartsHere
        } else if series.and_then(CandleSeries::oldest_ms).is_none() {
            OlderCandles::NotArrivedYet
        } else {
            OlderCandles::Available
        }
    }

    /// *Load older* on this path: the page behind the first pane's series,
    /// as scrolling to its left edge would ask for.
    pub(crate) fn request_older(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        lead_in: bool,
    ) -> Result<(), Refusal> {
        let reason = self.older_candles(step.tab.capabilities, step.tab.panes, lead_in);
        let older = step
            .tab
            .panes
            .iter()
            .find_map(|pane| series_key(pane.interval_ms, lead_in))
            .and_then(|key| {
                let series = self.series.get(&key)?;
                Some((series.older_ask(key)?, series.oldest_ms()?))
            });
        let (OlderCandles::Available, Some((ask, oldest_ms))) = (reason, older) else {
            return Err(Refusal::OlderCandles(reason));
        };
        step.feed(FeedRequest::Candles {
            tab_id,
            ask,
            purpose: CandlePurpose::Older { oldest_ms },
        });
        Ok(())
    }

    /// Candles held across every series.
    pub(crate) fn candles_held(&self) -> usize {
        self.series.values().map(|series| series.bars().len()).sum()
    }

    /// Whether any series has been answered, even with nothing.
    pub(crate) fn holds_any(&self) -> bool {
        self.series.values().any(CandleSeries::is_answered)
    }

    pub(crate) const fn pending(&self) -> bool {
        self.in_flight.is_some() || self.seam_out
    }

    /// Whether every series held has reached the venue's record start.
    pub(crate) fn exhausted(&self) -> bool {
        !self.series.is_empty() && self.series.values().all(CandleSeries::is_exhausted)
    }

    /// Whether the venue's record starts inside the `interval_ms` bucket the
    /// oldest candle of that pane's series falls in.
    pub(crate) fn record_starts_inside(&self, interval_ms: i64) -> bool {
        native_series_for(interval_ms)
            .and_then(|key| Some((key, self.series.get(&key)?)))
            .is_some_and(|(key, series)| {
                series.is_exhausted() && starts_inside_bucket(series.bars(), key, interval_ms)
            })
    }
}
