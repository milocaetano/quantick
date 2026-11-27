//! Readback bounds of the orderflow snapshots. One place for later config
//! wiring; a truncated list always says so beside it.

// FLOW execution readback (`flow_execution.rs`).

/// Most painted FLOW marks one snapshot lists; `marks_truncated` reports more.
pub(crate) const MAX_FLOW_READBACK_MARKS: usize = 256;

/// Most member executions one FLOW mark lists; `members_truncated` reports more.
pub(crate) const MAX_FLOW_READBACK_MEMBERS: usize = 128;
