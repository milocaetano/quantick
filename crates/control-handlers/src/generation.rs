//! Headless generation entry points. Callers choose the output destination;
//! this module preserves document order and leaves serialization to them.

pub use crate::schema_documents::SchemaDocument;
pub use quantick_control_host::system::BuildIdentity;

/// Existing published schemas, with their original names, order and values.
#[must_use]
pub fn documents() -> Vec<SchemaDocument> {
    crate::schema_documents::documents()
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
