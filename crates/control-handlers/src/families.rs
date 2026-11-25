//! Host-side registration emitted from the shared control manifest.

/// Register every action that takes only a registry, propagating the first
/// registration error. The registry expression is evaluated once; the caller
/// supplies a host implementing the families' existing ports.
#[macro_export]
macro_rules! register_family_actions {
    ([$registry:expr] $($family:ident {
        schemas: $schemas:tt, catalog: $catalog:tt, readbacks: $readbacks:tt,
        handlers: $handlers:tt, actions: [$($action:ident),*]
    })*) => {{
        let registry = $registry;
        $($($crate::$family::$action(registry)?;)*)*
    }};
}

#[cfg(test)]
use tests::{record, reject};
#[cfg(test)]
mod tests;
