//! Published wire documents shared by the offline generator and live host.
use serde_json::Value;

/// One file under `schemas/control`.
pub struct SchemaDocument {
    pub file_name: &'static str,
    pub schema: Value,
}

#[must_use]
pub fn documents() -> Vec<SchemaDocument> {
    quantick_control_schema::catalog::documents()
        .into_iter()
        .map(|(file_name, schema)| SchemaDocument { file_name, schema })
        .collect()
}
