//! The declared list: every family's rows, joined into one table.
//!
//! Each family owns its rows. The shared control manifest selects them;
//! this consumer only joins their slices.

use crate::readback::{Readback, flatten, total};

macro_rules! readback_families {
    ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { pub const FAMILIES: &[&[Readback]] = &[$($(crate::$readback::READBACKS,)*)*]; };
}
quantick_control::control_families!(readback_families);

const TOTAL: usize = total(FAMILIES);
/// A static, not a const: the joined table is past clippy's large-array size.
static JOINED: [Readback; TOTAL] = flatten(FAMILIES);

/// Every mutable capability's readback.
pub const READBACKS: &[Readback] = &JOINED;

/// The row for `capability`, if the matrix has one.
pub fn readback(capability: &str) -> Option<&'static Readback> {
    READBACKS.iter().find(|row| row.capability == capability)
}
