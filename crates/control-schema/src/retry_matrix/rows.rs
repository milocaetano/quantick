//! The declared list: every family's rows, joined into one table.
//!
//! Adding a capability adds a row to its own family module. The only line
//! that changes here is a new family's, which is why this file is short and
//! stays short.

use crate::readback::{Readback, flatten, total};
use crate::{analysis, attention, feed, indicator_guide, layers, layout, notify, script, trade};

/// Every family that declares readback rows. Order is irrelevant: the
/// document is rendered in the registry's order.
pub const FAMILIES: &[&[Readback]] = &[
    analysis::READBACKS,
    attention::READBACKS,
    feed::READBACKS,
    indicator_guide::READBACKS,
    layers::READBACKS,
    layout::READBACKS,
    notify::READBACKS,
    script::READBACKS,
    trade::READBACKS,
];

const TOTAL: usize = total(FAMILIES);
const JOINED: [Readback; TOTAL] = flatten(FAMILIES);

/// Every mutable capability's readback.
pub const READBACKS: &[Readback] = &JOINED;

/// The row for `capability`, if the matrix has one.
pub fn readback(capability: &str) -> Option<&'static Readback> {
    READBACKS.iter().find(|row| row.capability == capability)
}
