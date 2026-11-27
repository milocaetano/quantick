//! The published documents whose wire types this module owns. The window's
//! catalogue adds the few it still owns itself and commits them all under
//! `schemas/control`.

macro_rules! family_documents {
    ($args:tt $($family:ident { wire: $wire:tt, handlers: $handlers:tt, actions: $actions:tt })*) => {{
        let mut documents = Vec::new();
        $(family_documents!(@family documents $family $wire);)*
        documents
    }};
    (@family $documents:ident $family:ident [catalog $(, readbacks)?]) => {
        $documents.extend(crate::wire::$family::documents());
    };
    (@family $documents:ident $family:ident none) => {};
    (@family $documents:ident $family:ident [$(readbacks)?]) => {};
}

/// Every document this module publishes, as `(file name, schema)`.
#[must_use]
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    quantick_control::control_families!(family_documents)
}
