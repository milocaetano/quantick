//! The headless host machinery of the Quantick control plane.
//!
//! `quantick-control` is the contract both ends speak; this crate is what a
//! *host* runs on top of it and what used to live in the desktop app although
//! none of it draws: the snapshot projection registry, capability admission,
//! the snapshot-scope catalogue, the idempotency store and the event journal. It has no UI, no network, no
//! async runtime and no clock of its own — time arrives through
//! [`clock::HostClock`] or as an argument — so the headless guard scans it and
//! a change here is tested without building the app.
//!
//! The application keeps its authority table, its capability handlers, the
//! gateway's socket loop and everything that touches a frame, and reaches
//! this crate through the same module paths it used before the move.

pub mod admission;
pub mod catalogue;
pub mod clock;
pub mod idempotency;
pub mod journal;
pub mod projection;
