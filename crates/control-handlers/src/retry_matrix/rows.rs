//! Every family's readback rows, selected by the shared control manifest.

use crate::wire::readback::{Readback, flatten, total};

macro_rules! family_readbacks {
    ($args:tt $($family:ident { wire: $wire:tt, handlers: $handlers:tt, actions: $actions:tt })*) => {
        /// Order is irrelevant: the document is rendered in registry order.
        pub const FAMILIES: &[&[Readback]] = &[$(family_readbacks!(@family $family $wire),)*];
    };
    (@family $family:ident [$(catalog,)? readbacks]) => { crate::wire::$family::READBACKS };
    (@family $family:ident [handler_readbacks: $($path:ident)::+]) => {
        crate::$($path)::+::READBACKS
    };
    (@family $family:ident none) => { &[] };
    (@family $family:ident [$(catalog)?]) => { &[] };
}

quantick_control::control_families!(family_readbacks);

/// `flatten` seeds from the first family's first row, so the join starts at
/// the first family declaring any.
const DECLARING: &[&[Readback]] = from_first_declaring(FAMILIES);
const TOTAL: usize = total(DECLARING);
/// A static, not a const: the joined table is past clippy's large-array size.
static JOINED: [Readback; TOTAL] = flatten(DECLARING);

const fn from_first_declaring<'a>(mut families: &'a [&'a [Readback]]) -> &'a [&'a [Readback]] {
    while let [first, rest @ ..] = families {
        if !first.is_empty() {
            break;
        }
        families = rest;
    }
    families
}

/// Every mutable capability's readback.
pub const READBACKS: &[Readback] = &JOINED;

/// The row for `capability`, if the matrix has one.
pub fn readback(capability: &str) -> Option<&'static Readback> {
    READBACKS.iter().find(|row| row.capability == capability)
}
