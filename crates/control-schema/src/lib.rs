//! What the control plane says: the wire shapes of every Quantick
//! projection and action, their scope and capability identifiers, and the
//! pure transformations from domain values into them.
//!
//! The window projects its own state through these shapes and binds the
//! handlers; a client, a generator or a test reads them without the window.
//! Nothing here holds application state, reads a clock or names a renderer:
//! every function is a total function of what it is handed.

macro_rules! schema_modules {
    ($args:tt $($family:ident {
        schemas: [$($schema:ident),*], catalog: $catalog:tt, readbacks: $readbacks:tt,
        handlers: $handlers:tt, actions: $actions:tt
    })*) => {
        $($(pub mod $schema;)*)*
    };
}

quantick_control::control_families!(schema_modules);
