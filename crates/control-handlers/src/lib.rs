//! The control plane's handler families, over headless ports.
//!
//! A family here is everything a capability decides: how its input is parsed
//! and validated, what it refuses and in which words, what it asks the window
//! to do, and what it answers. What it may *not* do is name the window. Each
//! family declares the narrow port it reads or acts through — a trait that
//! speaks headless types only: the wire shapes in `quantick-control-schema`,
//! the chart model in `quantick-chart`, the history vocabulary in
//! `quantick-sources`, the marks in `quantick-drawings`, or a plain struct
//! of its own. The application keeps one adapter per port that reads its
//! panes and tabs into those types. The shared manifest selects each family.
//!
//! Register functions are generic over the host and the access type, the
//! registries in `quantick-control-host` already are, so the window docks a
//! family with the same call a test docks it into a fake with.
//!
//! Nothing here reads a clock: a handler that stamps an event or a page is
//! handed the time, or asks its port to stamp it.

pub mod addressed_pane;
pub mod axis_pane;
pub mod bubble_pane;
pub mod chart_layer;
pub mod connection;
pub mod deal_recorder;
pub mod dock;
pub mod drawing_scope;
pub mod feed_status;
pub mod generation;
pub mod indicator_host;
pub mod market_switch;
pub mod market_tab;
pub mod observer;
pub mod pane_marks;
pub mod retry_matrix;
pub mod schema_documents;
pub mod semantic;
pub mod settings;
mod tabs;
pub mod tape_market_fold;
pub mod ticket_money;
pub mod trade_account;

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

// These projections use app-local adapters; their descriptor owners live in generation.
macro_rules! projection_module {
    (feed) => {};
    (interaction) => {};
    (scene) => {};
    (system) => {};
    (workspace) => {};
    ($module:ident) => {
        pub mod $module;
    };
}
macro_rules! family_module {
    ([]; [$($projection:ident),*]) => { $(projection_module!($projection);)* };
    ([$($action:ident),+]; [$($projection:ident),*]) => { $(pub mod $action;)* };
}
macro_rules! family_modules {
    ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => { $(family_module!([$($action),*]; [$($projection),*]);)* };
}
quantick_control::control_families!(family_modules);
