//! Published wire documents for system.

/// Published schemas, owned beside their wire records.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![(
        "observer-system-info-v1.schema.json",
        quantick_control::schema::generated_schema::<quantick_control_host::system::SystemSnapshot>(
        ),
    )]
}
