//! Published wire documents for protocol.

/// Published schemas, owned beside their wire records.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "observer-empty-input-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::authority::EmptyInput,
            >(),
        ),
        (
            "observer-snapshot-read-input-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::authority::SnapshotReadInput,
            >(),
        ),
        (
            "observer-describe-result-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::authority::DescribeResult,
            >(),
        ),
    ]
}
