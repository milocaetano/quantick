//! The interface's half of the operability contract.
//!
//! The table of behaviours, the matrix renderer and the drift comparison are
//! `quantick-operability`, a headless crate: they are data and a pure
//! function over it. What only this crate can supply is the walk of its own
//! registries — the toolbar actions, the drawing tools, the hotkeys, the menu
//! entries — and that is [`sources`], test-only because reading
//! `crates/app/src` at runtime is meaningless in a shipped binary. The tests
//! below run the shared comparison against that walk, so the guard proves the
//! table against the interface it describes rather than against itself.

#[cfg(test)]
#[path = "tests/sources.rs"]
pub(crate) mod sources;

pub(crate) use quantick_operability::matrix;
#[cfg(test)]
pub(crate) use quantick_operability::{Registered, Source};

#[cfg(test)]
mod tests;
