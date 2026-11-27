//! Every family's readback rows, selected by the shared control manifest.

use crate::wire::readback::{Readback, flatten, total};

macro_rules! family_readbacks {
    ($args:tt $($family:ident {
        schemas: $schemas:tt, catalog: $catalog:tt, readbacks: [$($($readback:ident)::+),*],
        handlers: $handlers:tt, actions: $actions:tt
    })*) => {
        /// Order is irrelevant: the document is rendered in registry order.
        pub const FAMILIES: &[&[Readback]] = &[$($(crate::$($readback)::+::READBACKS,)*)*];
    };
}

quantick_control::control_families!(family_readbacks);

const TOTAL: usize = total(FAMILIES);
/// A static, not a const: the joined table is past clippy's large-array size.
static JOINED: [Readback; TOTAL] = flatten(FAMILIES);

/// Every mutable capability's readback.
pub const READBACKS: &[Readback] = &JOINED;

/// The row for `capability`, if the matrix has one.
pub fn readback(capability: &str) -> Option<&'static Readback> {
    READBACKS.iter().find(|row| row.capability == capability)
}
