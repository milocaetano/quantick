//! Headless alert definitions and injected observation ports.
//! No scaffold command reports success before its implementation is installed.
pub mod book;
pub mod input;
pub mod observer;
pub mod operators;
pub mod sound;
pub mod spec;
pub use book::*;
pub use input::*;
pub use spec::*;
pub mod delivery;
