//! Provider-neutral feed integrity snapshot contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::wire::WireU64;

/// Counts source messages, never estimates how many executed trades were lost.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedIntegritySnapshot {
    /// Diagnostic events, independent of the number of rejected source rows.
    pub anomalies: WireU64,
    /// Known source messages absent from the usable tape: sequence gaps or
    /// individually rejected rows. Never an estimate of lost executions.
    pub missing_messages: WireU64,
    /// Interruptions whose missing-message count is unknown, never known zero.
    pub unknown_loss: WireU64,
    /// Repeated/backwards ID or stale-row diagnostic categories, not row count.
    pub non_monotonic: WireU64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_contract_matches_both_existing_snapshot_definitions() {
        let mut generated = crate::schema::generated_schema::<FeedIntegritySnapshot>();
        let object = generated.as_object_mut().unwrap();
        object.remove("$schema");
        object.remove("title");
        object.remove("$defs");
        for source in [
            include_str!("../../../schemas/control/observer-feed-status-v1.schema.json"),
            include_str!("../../../schemas/control/observer-health-summary-v1.schema.json"),
        ] {
            let existing: serde_json::Value = serde_json::from_str(source).unwrap();
            assert_eq!(generated, existing["$defs"]["FeedIntegritySnapshot"]);
        }
    }
}
