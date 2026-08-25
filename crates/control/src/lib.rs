//! Transport-neutral contracts for observing and controlling Quantick.
//!
//! This crate deliberately has no dependency on the application or trading
//! domain crates. Hosts and adapters meet at these owned DTOs and registries.

pub mod canonical;
pub mod codec;
pub mod cursor;
pub mod descriptor;
pub mod error;
pub mod fake;
pub mod handshake;
pub mod id;
pub mod limits;
pub mod registry;
pub mod schema;
pub mod schema_catalog;
pub mod wire;
