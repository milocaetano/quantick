//! Published wire documents for annotate.

/// Published schemas, owned beside their wire records.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "annotate-object-input-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control::annotation::AnnotationInput,
            >(),
        ),
        (
            "annotate-object-result-v1.schema.json",
            quantick_control::schema::generated_schema::<
                quantick_control::annotation::AnnotationResult,
            >(),
        ),
        (
            "annotate-remove-input-v1.schema.json",
            quantick_control::schema::generated_schema::<quantick_control::annotation::RemoveInput>(
            ),
        ),
        (
            "annotate-remove-result-v1.schema.json",
            quantick_control::schema::generated_schema::<quantick_control::annotation::RemoveResult>(
            ),
        ),
    ]
}
