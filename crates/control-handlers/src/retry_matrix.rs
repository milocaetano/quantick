//! The retry matrix's judgement over the registry: [`drift`] compares the rows
//! `quantick_control_schema::retry_matrix` declares with the registry a
//! contract serves, and [`render`] turns the two into
//! `docs/control-plane/retry-matrix.md` once they agree.
//!
//! The rows, the reach a grant has and the generated marker are the wire
//! vocabulary and stay in `quantick-control-schema`; what this module adds is
//! the check and the document, run only from `main`'s dump path and from
//! tests.

mod drift;
mod render;

pub use drift::{
    Drift, MAX_SCHEMA_HOPS, branches, drift, read_needs, readable, row_drift, schema_has_path,
};
pub use render::{PREAMBLE, RenderedRow, enforced, policy_name, render, summary};
