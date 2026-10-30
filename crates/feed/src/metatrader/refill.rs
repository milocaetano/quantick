//! The driver half of a reconnect refill: carries out what [`GapFill`]
//! decides — asks the bridge, holds the live tape, releases the result — and
//! says each step in the log and on the `RefillStatus` event the control
//! plane reads.
//!
//! The refilled prints go out as [`FeedEvent::Refilled`], the chart's history
//! path: they build bars and move the paper mark, and never reach a fill, a
//! strategy, an alarm or the arrival-latency reading. Only the live tape held
//! behind the refill goes out as live, because only it is.

use quantick_engine::Trade;
use tracing::{info, warn};

use super::gap_fill::{self, FillStep, GapFill, Release};
use super::{EventFlow, Mt5Feed};
use crate::config::{BackfillState, BackfillStop};
use crate::{FeedContinuity, FeedEvent, FeedNotice, ResumeSeam};

/// Prints per [`FeedEvent::Refilled`]: the bound on what one channel message
/// hands a frame, the opening slice's order of magnitude.
const RELEASE_BATCH_PRINTS: usize = 8_192;

impl Mt5Feed {
    /// Reconnect history landed on a chart that holds prints: chart its
    /// unseen tail when it already reaches the chart, else refill the hole
    /// between them.
    pub(super) async fn on_resumed(&mut self, block: Vec<Trade>, seam: ResumeSeam) -> EventFlow {
        match gap_fill::closed_hole(block, seam) {
            Ok(tail) => {
                info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_RECONNECT_NO_HOLE",
                    symbol = %self.symbol,
                    seam_ms = seam.last_ms,
                    forwarded = tail.len(),
                    "the bridge came back with history reaching the chart; no refill needed"
                );
                self.send_refilled(vec![tail]).await
            }
            Err(block) => self.begin_refill(block, seam).await,
        }
    }

    /// Open a refill of the hole between the chart's `seam` and `block`.
    async fn begin_refill(&mut self, block: Vec<Trade>, seam: ResumeSeam) -> EventFlow {
        let can_page = self.caps_tx.borrow().history_paging;
        let fill = GapFill::begin(seam, block, can_page);
        let report = fill.report();
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_GAP_REFILL_STARTED",
            symbol = %self.symbol,
            from_ms = report.from_ms,
            to_ms = report.to_ms,
            held_at_last = seam.held_at_last,
            received = report.received,
            can_page,
            hold_limit_ms = u64::try_from(self.refill_hold_limit.as_millis()).unwrap_or(u64::MAX),
            "the bridge came back after a hole; refilling it before the live tape resumes"
        );
        // The refill's notice is its own to clear only while nothing that
        // needs the trader has been said since it went up.
        if !self.latch.terminal_down() {
            self.refill_notice = Some(self.latch.attentions());
            let _ = self
                .notice_tx
                .send(FeedNotice::working(
                    "MetaTrader is back — filling in the prints missed while it was away",
                ))
                .await;
        }
        if let EventFlow::UiGone = self.send(FeedEvent::RefillStatus(report)).await {
            return EventFlow::UiGone;
        }
        self.refill = Some(fill);
        self.refill_deadline = Some(tokio::time::Instant::now() + self.refill_hold_limit);
        self.drive_refill().await
    }

    /// Ask, wait or release, as the refill decides. Called after anything
    /// that can change its answer — a slice, a page for it, and a page for
    /// the chart, which frees a pager the refill was refused.
    pub(super) async fn drive_refill(&mut self) -> EventFlow {
        let Some(fill) = self.refill.as_mut() else {
            return EventFlow::Handled;
        };
        match fill.step() {
            FillStep::Wait => EventFlow::Handled,
            FillStep::Ask {
                count,
                before_utc_ms,
            } => {
                // Not while a page the chart asked for before the refill is
                // still owed: replies carry no request id, so the first one
                // back is the chart's. Settling it drives the refill again.
                let asked =
                    !self.tape.page_owed() && self.history_pager.request(count, before_utc_ms);
                if asked {
                    fill.asked();
                }
                info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_GAP_REFILL_PAGE_ASKED",
                    symbol = %self.symbol,
                    count,
                    before_utc_ms,
                    asked,
                    "asked the bridge for the next page of the hole"
                );
                EventFlow::Handled
            }
            FillStep::Release(release) => {
                self.refill = None;
                self.release_refill(release).await
            }
        }
    }

    /// End a running refill now, for `stop`, with what is in hand.
    pub(super) async fn abort_refill(&mut self, stop: BackfillStop) -> EventFlow {
        match self.refill.take() {
            Some(mut fill) => self.release_refill(fill.stop_with(stop)).await,
            None => EventFlow::Handled,
        }
    }

    /// The hold's deadline passed: the live tape has waited long enough.
    pub(super) async fn on_refill_deadline(&mut self) -> EventFlow {
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_GAP_REFILL_DEADLINE",
            symbol = %self.symbol,
            hold_limit_ms = u64::try_from(self.refill_hold_limit.as_millis()).unwrap_or(u64::MAX),
            action = "release_live_tape",
            "the refill held the live tape too long; charting what it has"
        );
        self.abort_refill(BackfillStop::HeldTooLong).await
    }

    /// Hold one live event behind a running refill, or hand it back when
    /// none runs.
    pub(super) async fn hold_behind_refill(
        &mut self,
        event: FeedEvent,
    ) -> Result<EventFlow, FeedEvent> {
        let Some(fill) = self.refill.as_mut() else {
            return Err(event);
        };
        if fill.hold(event) {
            return Ok(self.abort_refill(BackfillStop::HeldTooLong).await);
        }
        Ok(EventFlow::Handled)
    }

    /// Chart a finished refill: the unfilled part as a gap, the prints in
    /// order, the report, then everything the live tape did meanwhile.
    async fn release_refill(&mut self, release: Release) -> EventFlow {
        let Release {
            gap,
            chunks,
            report,
            held,
            page_owed,
        } = release;
        self.refill_deadline = None;
        self.stray_refill_page = page_owed;
        let (outcome, stop, unfilled_to_ms) = match report.state {
            BackfillState::Partial {
                unfilled_to_ms,
                stop,
            } => ("partial", stop.wire_name(), Some(unfilled_to_ms)),
            state => (state.wire_name(), "none", None),
        };
        let forwarded: usize = chunks.iter().map(Vec::len).sum();
        if gap.is_some() {
            warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_GAP_REFILL_FINISHED",
                symbol = %self.symbol,
                requested_from_ms = report.from_ms,
                requested_to_ms = report.to_ms,
                received = report.received,
                forwarded,
                outcome,
                stop,
                unfilled_to_ms = ?unfilled_to_ms,
                held = held.len(),
                "the refill stopped short; the rest of the hole is marked as a gap"
            );
        } else {
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_GAP_REFILL_FINISHED",
                symbol = %self.symbol,
                requested_from_ms = report.from_ms,
                requested_to_ms = report.to_ms,
                received = report.received,
                forwarded,
                outcome,
                stop,
                held = held.len(),
                "the hole is refilled; releasing the held live tape behind it"
            );
        }
        if let Some(gap) = gap {
            let event = FeedContinuity {
                gap: Some(gap),
                missing_messages: None,
                non_monotonic: false,
            };
            if let EventFlow::UiGone = self.send(FeedEvent::Continuity(event)).await {
                return EventFlow::UiGone;
            }
        }
        if let EventFlow::UiGone = self.send_refilled(chunks).await {
            return EventFlow::UiGone;
        }
        if let EventFlow::UiGone = self.send(FeedEvent::RefillStatus(report)).await {
            return EventFlow::UiGone;
        }
        // Cleared only while it is still the refill's own notice: a closed
        // terminal, a missing book or anything else said meanwhile stays.
        let owned = self.refill_notice.take() == Some(self.latch.attentions());
        if owned && !self.latch.terminal_down() {
            let _ = self.notice_tx.send(FeedNotice::Clear).await;
        }
        for event in held {
            let flow = match event {
                FeedEvent::Live(trade) => self.forward_live(trade).await,
                other => self.send(other).await,
            };
            if let EventFlow::UiGone = flow {
                return EventFlow::UiGone;
            }
        }
        EventFlow::Handled
    }

    /// Hand the chart recovered prints on its history path, in batches of
    /// at most [`RELEASE_BATCH_PRINTS`], ascending — and at least one batch,
    /// even empty: it is what tells the chart the feed owns the seam, so the
    /// live tape behind it meets no resume floor.
    async fn send_refilled(&mut self, mut chunks: Vec<Vec<Trade>>) -> EventFlow {
        if chunks.is_empty() {
            chunks.push(Vec::new());
        }
        for chunk in chunks {
            for trade in &chunk {
                // The refill owns the seam, so nothing is owed here.
                let _ = self.tape.forward(trade);
            }
            if chunk.len() <= RELEASE_BATCH_PRINTS {
                if let EventFlow::UiGone = self.send(FeedEvent::Refilled(chunk)).await {
                    return EventFlow::UiGone;
                }
                continue;
            }
            for batch in chunk.chunks(RELEASE_BATCH_PRINTS) {
                if let EventFlow::UiGone = self.send(FeedEvent::Refilled(batch.to_vec())).await {
                    return EventFlow::UiGone;
                }
            }
        }
        EventFlow::Handled
    }

    /// Whether a page reply belongs to a refill — the running one, or one
    /// that finished before its page landed — rather than to the chart.
    pub(super) fn refill_wants_page(&self) -> bool {
        self.refill.as_ref().is_some_and(GapFill::page_owed) || self.stray_refill_page
    }

    /// Take a page reply [`Self::refill_wants_page`] claimed.
    pub(super) async fn take_refill_page(
        &mut self,
        trades: Vec<Trade>,
        exhausted: bool,
        scanned_to_utc_ms: Option<i64>,
    ) -> EventFlow {
        let count = trades.len();
        let Some(fill) = self.refill.as_mut().filter(|fill| fill.page_owed()) else {
            self.stray_refill_page = false;
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_GAP_REFILL_PAGE",
                symbol = %self.symbol,
                count,
                action = "discard",
                "a page the finished refill asked for landed after it; nobody wants it"
            );
            return EventFlow::Skipped;
        };
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_GAP_REFILL_PAGE",
            symbol = %self.symbol,
            count,
            exhausted,
            scanned_to_utc_ms = ?scanned_to_utc_ms,
            action = "absorb",
            "a page of the hole arrived"
        );
        fill.page(trades, exhausted, scanned_to_utc_ms);
        self.drive_refill().await
    }

    /// Whether a *load older* from the chart must wait: the pager belongs to
    /// the refill until its last page is answered.
    pub(super) fn refill_owns_pager(&self) -> bool {
        self.refill.is_some() || self.stray_refill_page
    }
}
