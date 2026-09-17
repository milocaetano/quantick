//! The independently versioned received-row delivery observation scope.

use super::registry::{ProjectionRegistry, ProjectionRegistryError};
use crate::app::QuantickApp;
use quantick_control::{
    feed::{FeedDeliverySnapshot, ReceivedRowCounts, SourceCounts, TabFeedDeliverySnapshot},
    id::{ModuleId, SnapshotScopeId},
};

pub(super) fn register(
    registry: &mut ProjectionRegistry,
    module: ModuleId,
) -> Result<(), ProjectionRegistryError> {
    registry.register_scope(
        SnapshotScopeId::new("health.feed_delivery").expect("static scope"), module, 1,
        "Feed delivery observations",
        "Separates skipped source IDs and unknown outages from received-row exclusions; zero observations do not certify completeness.",
        &["observe", "observe.health"], |app, _| snapshot(app),
    )
}

pub(super) fn snapshot(app: &QuantickApp) -> FeedDeliverySnapshot {
    FeedDeliverySnapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| {
                let i = tab.feed_integrity;
                let x = tab.feed_delivery.exclusions;
                TabFeedDeliverySnapshot::from_counts(
                    tab.id,
                    SourceCounts {
                        anomalies: i.anomalies,
                        missing_messages: i.missing_messages,
                        unknown_loss: i.unknown_loss,
                        non_monotonic: i.non_monotonic,
                    },
                    tab.feed_delivery.reporting.then_some(ReceivedRowCounts {
                        malformed_rows: x.malformed_rows,
                        stale_rows: x.stale_rows,
                    }),
                )
            })
            .collect(),
    }
}
