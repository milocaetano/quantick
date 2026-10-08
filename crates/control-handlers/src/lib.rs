//! The control plane's handler families, over headless ports.
//!
//! A family here is everything a capability decides: how its input is parsed
//! and validated, what it refuses and in which words, what it asks the window
//! to do, and what it answers. What it may *not* do is name the window. Each
//! family declares the narrow port it reads or acts through — a trait that
//! speaks headless types only: the wire shapes in `quantick-control-schema`,
//! the chart model in `quantick-chart`, the history vocabulary in
//! `quantick-sources`, or a plain struct of its own. The application keeps
//! one adapter per port that reads its panes and tabs into those types, and
//! one registration line per family.
//!
//! Register functions are generic over the host and the access type, the
//! registries in `quantick-control-host` already are, so the window docks a
//! family with the same call a test docks it into a fake with.
//!
//! Nothing here reads a clock: a handler that stamps an event or a page is
//! handed the time, or asks its port to stamp it.

pub mod chart;
pub mod dock;
pub mod history;
pub mod notify;
pub mod recovery;
mod tabs;

pub use tabs::{TabDirectory, tab_index};

#[cfg(test)]
mod test_support;
