//! The driver half of a reconnect refill: carries out what [`GapFill`]
//! decides — asks the bridge, holds the live tape, releases the result — and
//! says each step in the log and on the `Backfill` event the control plane
//! reads.

use quantick_engine::Trade;
use tracing::{info, warn};

use super::gap_fill::{FillStep, GapFill, Release};
use super::{EventFlow, Mt5Feed};
use crate::config::{BackfillState, BackfillStop};
use crate::{FeedContinuity, FeedEvent, FeedNotice, ResumeSeam};

/// Prints per `LiveBatch` when a refill is released: the bound on what one
/// channel message hands a frame, the opening slice's order of magnitude.
const RELEASE_BATCH_PRINTS: usize = 8_192;

impl Mt5Feed {
    /// Reconnect history landed on a chart that holds prints: open a refill
    /// of the hole between them.
    pub(super) async fn begin_refill(&mut self, block: Vec<Trade>, seam: ResumeSeam) -> EventFlow {
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
            "the bridge came back after a hole; refilling it before the live tape resumes"
        );
        let _ = self
            .notice_tx
            .send(FeedNotice::working(
                "MetaTrader is back — filling in the prints missed while it was away",
            ))
            .await;
        if let EventFlow::UiGone = self.send(FeedEvent::Backfill(report)).await {
            return EventFlow::UiGone;
        }
        self.refill = Some(fill);
        self.drive_refill().await
    }

    /// Ask, wait or release, as the refill decides.
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
                // Refused only when a request from before the refill is still
                // in flight; its reply goes to the chart, and the next refill
                // event asks again.
                let asked = self.history_pager.request(count, before_utc_ms);
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
            trades,
            report,
            held,
            page_owed,
        } = release;
        self.stray_refill_page = page_owed;
        let (outcome, stop, unfilled_to_ms) = match report.state {
            BackfillState::Partial {
                unfilled_to_ms,
                stop,
            } => ("partial", stop.wire_name(), Some(unfilled_to_ms)),
            state => (state.wire_name(), "none", None),
        };
        let log_line = |forwarded: usize| {
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
        };
        log_line(trades.len());
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
        let mut trades = trades.into_iter().peekable();
        while trades.peek().is_some() {
            let batch: Vec<Trade> = trades.by_ref().take(RELEASE_BATCH_PRINTS).collect();
            for trade in &batch {
                // The refill owns the seam, so nothing is owed here.
                let _ = self.tape.forward(trade);
            }
            if let EventFlow::UiGone = self.send(FeedEvent::LiveBatch(batch)).await {
                return EventFlow::UiGone;
            }
        }
        if let EventFlow::UiGone = self.send(FeedEvent::Backfill(report)).await {
            return EventFlow::UiGone;
        }
        let _ = self.notice_tx.send(FeedNotice::Clear).await;
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
