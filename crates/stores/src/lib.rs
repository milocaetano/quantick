//! The cockpit's stores: what the chart remembers between launches, as
//! documents.
//!
//! Every module here is one file the trader's cockpit is made of — the feed
//! and symbol catalogue, the symbols added by hand, the footprint knobs and
//! presets, the aggression-bubble presets, the indicator set, its presets and the script library,
//! the arrangement of the window — parsed, validated, restored against the
//! live configuration and written back through the same discipline: a
//! versioned TOML file, a temp sibling and a rename, a bad file that degrades
//! to defaults and says so.
//!
//! None of it knows where a file lives. The window resolves every path — an
//! environment override, the durable home, the launch directory — and hands
//! it in, so a store can be exercised in a test against a scratch file and
//! reused by a second consumer without inheriting the desktop's environment.
//! The shipped defaults are embedded from `crates/app/config/`, the folder
//! every document names, and the starter scripts from `crates/app/scripts/`;
//! both are read here at compile time only.

pub mod bubble_presets;
pub mod bundle;
pub mod config;
pub mod footprint_config;
pub mod footprint_presets;
pub mod home;
pub mod indicator_presets;
pub mod indicator_state;
#[cfg(test)]
mod scratch;
pub mod script_library;
pub mod symbols_file;
pub mod ui_state;
