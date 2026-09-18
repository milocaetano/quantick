//! What the control plane says: the wire shapes of every Quantick
//! projection and action, their scope and capability identifiers, and the
//! pure transformations from domain values into them.
//!
//! The window projects its own state through these shapes and binds the
//! handlers; a client, a generator or a test reads them without the window.
//! Nothing here holds application state, reads a clock or names a renderer:
//! every function is a total function of what it is handed.

pub mod analysis;
pub mod attention;
pub mod chart;
pub mod deal_recording;
pub mod evidence;
pub mod feed;
pub mod health;
pub mod indicator_guide;
pub mod layers;
pub mod layout;
pub mod layout_v2;
pub mod notify;
pub mod orderflow;
pub mod recovery;
pub mod retry_matrix;
pub mod scene;
pub mod script;
pub mod session;
pub mod trade;
pub mod workspace;
