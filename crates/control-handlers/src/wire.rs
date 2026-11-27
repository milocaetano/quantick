//! What the control plane says: the wire shapes of every Quantick
//! projection and action, their scope and capability identifiers, and the
//! pure transformations from domain values into them.
//!
//! The window projects its own state through these shapes and binds the
//! handlers; a client, a generator or a test reads them without the window.
//! Nothing here holds application state, reads a clock or names a renderer:
//! every function is a total function of what it is handed.

macro_rules! schema_modules {
    ($args:tt $($family:ident { wire: $wire:tt, handlers: $handlers:tt, actions: $actions:tt })*) => {
        $(schema_module!($family $wire);)*
    };
}

/// Only the manifest's facets, in its order, pass: a misspelt or reordered
/// one fails the build instead of dropping out of the catalogue or matrix.
macro_rules! schema_module {
    ($family:ident none) => {};
    ($family:ident [$(catalog)?]) => {
        pub mod $family;
    };
    ($family:ident [$(catalog,)? readbacks]) => {
        pub mod $family;
    };
}

quantick_control::control_families!(schema_modules);
