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
pub mod bubble_preset;
pub mod bubble_save;
pub mod bubble_scale;
pub mod chart;
pub mod chart_settings;
pub mod deal_recording;
pub mod drawing_magnet;
pub mod evidence;
pub mod feed;
pub mod feed_source;
pub mod health;
pub mod history;
pub mod home_tab;
pub mod indicator_guide;
pub mod interaction;
pub mod journal_sessions;
pub mod layers;
pub mod layout;
pub mod layout_v2;
pub mod native_window;
pub mod notify;
pub mod opening_scale;
pub mod orderflow;
pub mod pnl_display;
pub mod price_axis;
pub mod readback;
pub mod recovery;
pub mod retry_matrix;
pub mod scene;
pub mod script;
pub mod session;
pub mod tape_fold;
pub mod tape_view;
pub mod tape_view_set;
pub mod tick_bubbles;
pub mod time_axis;
pub mod trace_replay;
pub mod trade;
pub mod workspace;

pub mod indicator_picker;
