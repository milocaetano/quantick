//! Additive delivery observations without changing the legacy feed contract.

use std::{num::NonZeroU64, path::PathBuf};

use tokio::sync::{mpsc, watch};

use crate::{
    DepthEvent, FeedCommand, FeedEvent, FeedHandle, FeedLatency, FeedNotice, FeedSource,
    ReplayLink,
    config::{FeedCapabilities, MetaTraderSettings, ProviderKind},
};

/// Why a received row did not enter the usable tape. Neither reason proves a
/// missing source ID, an execution lost, or an interval with known endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExclusionReason {
    /// The received row could not be mapped into a valid trade.
    MalformedRow,
    /// A non-overlap row arrived behind the accepted market-time watermark.
    StaleTimestamp,
}

/// A known nonzero number of received rows excluded for one reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedExclusion {
    /// The measured rejection category.
    pub reason: ExclusionReason,
    /// Received rows, never missing source IDs or estimated executions.
    pub rows: NonZeroU64,
}

/// Independent cumulative exclusions. A maxed counter is a lower bound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedExclusions {
    /// Received malformed rows excluded from the usable tape.
    pub malformed_rows: u64,
    /// Received stale rows excluded after overlap deduplication.
    pub stale_rows: u64,
}

impl FeedExclusions {
    /// Retain an observation without modifying legacy continuity counters.
    pub fn observe(&mut self, event: FeedExclusion) {
        let count = match event.reason {
            ExclusionReason::MalformedRow => &mut self.malformed_rows,
            ExclusionReason::StaleTimestamp => &mut self.stale_rows,
        };
        *count = count.saturating_add(event.rows.get());
    }
}

/// Ordered v1 delivery stream: original data/continuity and distinct exclusions.
/// Existing exhaustive matches on [`FeedEvent`] remain valid.
pub enum ObservedFeedEvent {
    /// An unchanged legacy event with its original meaning.
    Feed(FeedEvent),
    /// Received rows rejected before the later usable data they qualify.
    Excluded(FeedExclusion),
}

enum Receiver {
    Legacy(mpsc::Receiver<FeedEvent>),
    Observed(mpsc::Receiver<ObservedFeedEvent>),
}

/// A direct receiver adapter, without a forwarding task or a second queue.
pub struct ObservedReceiver(
    Receiver,
    #[cfg(any(test, feature = "test-support"))] Option<crate::test_support::live::FixtureLifetime>,
);

impl From<mpsc::Receiver<FeedEvent>> for ObservedReceiver {
    fn from(receiver: mpsc::Receiver<FeedEvent>) -> Self {
        Self(
            Receiver::Legacy(receiver),
            #[cfg(any(test, feature = "test-support"))]
            None,
        )
    }
}

impl From<mpsc::Receiver<ObservedFeedEvent>> for ObservedReceiver {
    fn from(receiver: mpsc::Receiver<ObservedFeedEvent>) -> Self {
        Self(
            Receiver::Observed(receiver),
            #[cfg(any(test, feature = "test-support"))]
            None,
        )
    }
}

impl ObservedReceiver {
    /// Persistent identity for an explicitly enabled local synthetic fixture.
    #[must_use]
    pub fn is_local_fixture(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.1.is_some()
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            false
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_fixture(
        mut self,
        owner: crate::test_support::live::FixtureLifetime,
    ) -> Self {
        self.1 = Some(owner);
        self
    }

    /// Whether this producer reports typed row exclusions. False is unavailable,
    /// not a claim that no rows were excluded by an older producer.
    #[must_use]
    pub fn reports_exclusions(&self) -> bool {
        matches!(self.0, Receiver::Observed(_))
    }

    /// Receive directly from the producer, retaining channel cancellation.
    pub async fn recv(&mut self) -> Option<ObservedFeedEvent> {
        match &mut self.0 {
            Receiver::Legacy(receiver) => receiver.recv().await.map(ObservedFeedEvent::Feed),
            Receiver::Observed(receiver) => receiver.recv().await,
        }
    }

    /// Drain one event without conflating an empty channel with a closed one.
    pub fn try_recv(&mut self) -> Result<ObservedFeedEvent, mpsc::error::TryRecvError> {
        match &mut self.0 {
            Receiver::Legacy(receiver) => receiver.try_recv().map(ObservedFeedEvent::Feed),
            Receiver::Observed(receiver) => receiver.try_recv(),
        }
    }
}

/// Opt-in observed feed handle. All other channels move once from their owner.
/// Dropping it closes the same producer receivers/command sender as the legacy
/// handle; no hidden clones keep a background feed alive.
pub struct ObservedFeedHandle {
    /// Ordered data, continuity and supported typed exclusions.
    pub events: ObservedReceiver,
    /// Independent bounded depth stream.
    pub book_events: mpsc::Receiver<DepthEvent>,
    /// Independent transport notices.
    pub notices: mpsc::Receiver<FeedNotice>,
    /// Current source capabilities.
    pub capabilities: watch::Receiver<FeedCapabilities>,
    /// Latest measured source latency, if available.
    pub latency: watch::Receiver<Option<FeedLatency>>,
    /// History and lifecycle commands to the running feed.
    pub commands: mpsc::Sender<FeedCommand>,
    /// Replay control only when a recorded source is active.
    pub replay: Option<ReplayLink>,
}

impl From<FeedHandle> for ObservedFeedHandle {
    fn from(handle: FeedHandle) -> Self {
        Self {
            events: handle.events.into(),
            book_events: handle.book_events,
            notices: handle.notices,
            capabilities: handle.capabilities,
            latency: handle.latency,
            commands: handle.commands,
            replay: handle.replay,
        }
    }
}

/// Start the app-facing observed port. Older providers adapt in place; only
/// providers measuring exclusions supply the new observation variant.
#[must_use]
pub fn spawn_observed(
    source: FeedSource,
    mt5_settings: &MetaTraderSettings,
    clock_cache_dir: Option<PathBuf>,
) -> ObservedFeedHandle {
    #[cfg(feature = "live-fixture")]
    if std::env::var("QUANTICK_FEED_INTEGRITY_FIXTURE").as_deref() == Ok("hl-local") {
        return crate::test_support::spawn_local_fixture();
    }
    match source {
        FeedSource::Live {
            provider: ProviderKind::Hyperliquid,
            symbol,
        } => crate::hyperliquid::spawn_observed(&symbol),
        source => crate::spawn(source, mt5_settings, clock_cache_dir).into(),
    }
}

/// Start a live observed source without changing the legacy spawn API.
#[must_use]
pub fn spawn_live_observed(
    provider: ProviderKind,
    symbol: &str,
    mt5_settings: &MetaTraderSettings,
    clock_cache_dir: Option<PathBuf>,
) -> ObservedFeedHandle {
    spawn_observed(
        FeedSource::Live {
            provider,
            symbol: symbol.to_owned(),
        },
        mt5_settings,
        clock_cache_dir,
    )
}

crate::hooks::declare_hooks!["QUANTICK_FEED_INTEGRITY_FIXTURE"];

#[cfg(test)]
mod tests;
