//! Published wire documents for events.

/// Published schemas, owned beside their wire records.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "observer-events-read-input-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::events::EventsReadInput,
            >(),
        ),
        (
            "observer-events-wait-input-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control_host::events::EventsWaitInput,
            >(),
        ),
        (
            "observer-event-page-v1.schema.json",
            quantick_control::schema::generated_schema::<quantick_control_host::journal::EventPage>(
            ),
        ),
    ]
}
