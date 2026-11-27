//! The published documents whose wire types this crate owns. The window's
//! catalogue adds the few it still owns itself and commits them all under
//! `schemas/control`.

macro_rules! family_documents {
    ($args:tt $($family:ident {
        schemas: $schemas:tt, catalog: [$($catalog:ident),*], readbacks: $readbacks:tt,
        handlers: $handlers:tt, actions: $actions:tt
    })*) => {{
        let mut documents = Vec::new();
        $($(documents.extend(crate::wire::$catalog::documents());)*)*
        documents
    }};
}

/// Every document this crate publishes, as `(file name, schema)`.
#[must_use]
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    quantick_control::control_families!(family_documents)
}
