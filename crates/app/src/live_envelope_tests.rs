//! The live envelope exercised through the real workers: the burst test
//! ([`burst`]) and the measurement harness ([`measure`]).
//!
//! A sibling of [`crate::live_envelope`] rather than its child: the workers
//! read the envelope's caps, and these tests drive the workers, so living
//! inside the envelope module would weld the three into one module cycle.

mod burst;
mod measure;
