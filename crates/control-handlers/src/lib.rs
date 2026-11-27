//! The control plane's handler families, over headless ports.
//!
//! A family here is everything a capability decides: how its input is parsed
//! and validated, what it refuses and in which words, what it asks the window
//! to do, and what it answers. What it may *not* do is name the window. Each
//! family declares the narrow port it reads or acts through — a trait that
//! speaks headless types only: the wire shapes in [`wire`],
//! the chart model in `quantick-chart`, the history vocabulary in
//! `quantick-sources`, the marks in `quantick-drawings`, or a plain struct
//! of its own. The application keeps one adapter per port that reads its
//! panes and tabs into those types. The manifest in `quantick-control` lists each family.
//!
//! Register functions are generic over the host and the access type, the
//! registries in `quantick-control-host` already are, so the window docks a
//! family with the same call a test docks it into a fake with.
//!
//! Nothing here reads a clock: a handler that stamps an event or a page is
//! handed the time, or asks its port to stamp it.

macro_rules! handler_modules {
    ($args:tt $($family:ident {
        schemas: $schemas:tt, catalog: $catalog:tt, readbacks: $readbacks:tt,
        handlers: [$($visibility:vis $handler:ident),*], actions: $actions:tt
    })*) => {
        $($($visibility mod $handler;)*)*
    };
}

quantick_control::control_families!(handler_modules);

pub mod call;
pub mod wire;

pub(crate) use tabs::per_open_tab;
pub use tabs::{JournalAccess, OpenTabs, TabDirectory, tab_index};

/// A malformed input or an unencodable answer, as every family refuses it.
pub(crate) fn invalid(error: impl ToString) -> quantick_control::error::ControlError {
    quantick_control::error::ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
mod canvas_tests;
#[cfg(test)]
#[path = "../../stores/src/scratch.rs"]
mod scratch;
#[cfg(test)]
mod test_support;
