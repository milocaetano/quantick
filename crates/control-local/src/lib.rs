//! Local transport of the Quantick control plane (ADR 0001).
//!
//! Two things live here, and they live here because two processes need the
//! same implementation of each:
//!
//! - [`discovery`]: the private per-user directory in which a running Quantick
//!   instance publishes its descriptor, and from which a client discovers live
//!   instances. Publication (the application) and discovery (an adapter such
//!   as `quantick-mcp`, or a later CLI) verify ownership and permissions with
//!   one body of code, so the security-critical part is never written twice.
//! - [`client`]: the blocking loopback client that authenticates against one
//!   gateway and exchanges framed envelopes with it. It is the client side of
//!   the handshake and framing `quantick-control` defines; the server side is
//!   the gateway inside the application.
//!
//! The crate depends only on `quantick-control`. It never starts the
//! application, never binds a listener, and knows nothing about MCP, egui or
//! the domain crates.

pub mod client;
pub mod discovery;
#[cfg(test)]
mod scratch;
