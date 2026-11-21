//! Published wire documents, selected by the shared family manifest and
//! emitted under `schemas/control` by the headless generator.

/// Every document this crate publishes, as `(file name, schema)`.
#[must_use]
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    let mut documents = Vec::new();
    macro_rules! schema_families {
    ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { $( $(documents.extend(crate::$schema::documents());)* )* };
}
    quantick_control::control_families!(schema_families);
    documents
}
