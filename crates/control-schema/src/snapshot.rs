//! Published wire documents for snapshot.

/// Published schemas, owned beside their wire records.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "observer-snapshot-scope-descriptor-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::catalogue::SnapshotScopeDescriptor,
            >(),
        ),
        (
            "observer-snapshot-capture-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::projection::SerializedSnapshotCapture,
            >(),
        ),
    ]
}
