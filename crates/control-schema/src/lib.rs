//! What the control plane says: the wire shapes of every Quantick
//! projection and action, their scope and capability identifiers, and the
//! pure transformations from domain values into them.
//!
//! The window projects its own state through these shapes and binds the
//! handlers; a client, a generator or a test reads them without the window.
//! Nothing here holds application state, reads a clock or names a renderer:
//! every function is a total function of what it is handed.

pub mod bubble_save;
pub mod catalog;
pub mod deal_recording;
pub mod drawing_text;
pub mod launch_calls;
pub mod launch_values;
pub mod layout;
pub mod pnl_display;
pub mod readback;
pub mod recovery;
pub mod retry_matrix;
pub mod settings_sections;
pub mod tape_fold;
pub mod tape_market_fold;
pub mod tape_view_set;
pub mod tick_bubbles;
pub mod trace_replay;
pub mod trade;
pub mod trade_account;
pub mod window_menu;

macro_rules! family_modules {
    ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => { $($(pub mod $schema;)*)* };
}
quantick_control::control_families!(family_modules);
