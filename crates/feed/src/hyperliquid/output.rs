//! Two static consumers of the same host, without forwarding channels.

use std::future::Future;

use tokio::sync::mpsc;

use crate::{FeedEvent, FeedExclusion, ObservedFeedEvent};

pub(crate) trait Output: Send + 'static {
    fn send(&self, event: FeedEvent) -> impl Future<Output = Result<(), ()>> + Send;
    fn exclude(&self, event: FeedExclusion) -> impl Future<Output = Result<(), ()>> + Send;
}

pub(crate) struct LegacyOutput(pub(crate) mpsc::Sender<FeedEvent>);
pub(crate) struct ObservedOutput(pub(crate) mpsc::Sender<ObservedFeedEvent>);

impl Output for LegacyOutput {
    async fn send(&self, event: FeedEvent) -> Result<(), ()> {
        self.0.send(event).await.map_err(|_| ())
    }

    async fn exclude(&self, _event: FeedExclusion) -> Result<(), ()> {
        // The old enum has no typed exclusion. Source warnings remain; opt in
        // through spawn_observed for complete typed delivery observations.
        Ok(())
    }
}

impl Output for ObservedOutput {
    async fn send(&self, event: FeedEvent) -> Result<(), ()> {
        self.0
            .send(ObservedFeedEvent::Feed(event))
            .await
            .map_err(|_| ())
    }

    async fn exclude(&self, event: FeedExclusion) -> Result<(), ()> {
        self.0
            .send(ObservedFeedEvent::Excluded(event))
            .await
            .map_err(|_| ())
    }
}
