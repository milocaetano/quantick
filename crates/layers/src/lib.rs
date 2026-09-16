//! Chart-layer policy and registration, independent of any renderer or window.
//! External feature switches stay with their owners; transitions return typed
//! writes instead of mirroring those owners' state.

mod builtins;
mod descriptor;
mod document;
mod persistence;
mod state;

pub use descriptor::*;
pub use document::*;
pub use persistence::*;
pub use state::*;
