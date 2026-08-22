//! MCP adapter for the Quantick control plane.
//!
//! `quantick-mcp` is a local STDIO server that an MCP client (Codex, Claude
//! Code, any other) launches as a subprocess. It discovers a Quantick
//! instance that is *already running* with local agent access enabled,
//! authenticates with the descriptor that instance published, and exposes a
//! small, fixed set of tools over the gateway's capabilities. It is an adapter
//! in the plan's sense: no domain code knows it exists, and a future CLI can
//! speak to the same gateway through the same [`quantick_control_local`]
//! client this crate uses.
//!
//! The crate is a leaf. It depends on `quantick-control` (the contract) and
//! `quantick-control-local` (discovery and the loopback client), never on the
//! application or a domain crate. It never starts Quantick: with no running
//! instance, discovery reports none and the tools say so with a next step.
//!
//! Module map:
//!
//! - [`jsonrpc`]: the slice of JSON-RPC 2.0 a line-delimited STDIO server needs;
//! - [`protocol`]: the MCP shapes this server emits (tools, annotations,
//!   results) and the protocol versions it negotiates;
//! - [`link`]: the port to a running instance ([`link::ControlLink`]) and its
//!   real implementation over the local client;
//! - [`tools`]: the fixed observer tool set and how each maps to a capability;
//! - [`server`]: the request loop that turns lines into answers;
//! - [`setup`]: the Codex / Claude Code registration assistant;
//! - [`fake`]: a second implementation of the port, for tests.

pub mod fake;
pub mod jsonrpc;
pub mod link;
pub mod protocol;
pub mod server;
pub mod setup;
pub mod tools;
