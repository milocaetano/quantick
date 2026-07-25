//! quantick-replay — recorded sessions, played back like a live feed.
//!
//! Market replay is how a trader rehearses: take a day that already happened and
//! run it forward at a chosen speed, with the chart forming bars print by print
//! exactly as it did live. This crate owns the three pieces that need no screen
//! and no network:
//!
//! - [`format`] — the tick file: `Date,Time,Price,Bid,Ask,Volume,Side` rows with
//!   a `#` metadata block. Reading, writing, and telling a person precisely why
//!   a file was rejected.
//! - [`library`] — scanning a chosen folder for playable sessions, and reporting
//!   everything that is not one instead of silently skipping it.
//! - [`clock`] — the deterministic playhead: given how much real time passed, it
//!   says which trades fall due.
//!
//! Like [`quantick_engine`], this crate is headless and deterministic: no UI, no
//! network, no async, and no wall-clock read anywhere. The playhead is *told*
//! how much time passed. That is what lets the same session drive the chart, a
//! backtest and a bot through one code path, and be unit-tested without sleeping.
//!
//! # Why this file format
//!
//! Two shapes are established for replay data. Platforms with a proprietary
//! store keep a binary file per instrument-day in a folder tree — NinjaTrader's
//! `db/replay/<Instrument>/<yyyyMMdd>.nrd`, Sierra Chart's `.scid`. Everyone who
//! *sells* tick data ships CSV with a row per print. quantick takes the folder
//! layout from the first and the row format from the second:
//!
//! ```text
//! <replay folder>/
//!     WINJ26/
//!         20260316.csv
//!         20260317.csv
//! ```
//!
//! A recorded session is then something a person can open, diff, hand-edit and
//! generate from any language, while the folder still says at a glance which
//! instrument and which day. Loading is a single pass with the columns resolved
//! once from the header, and playback runs entirely from memory, so the text
//! format costs a load and never a frame.
//!
//! # Extending it
//!
//! Columns are read by name, and unknown ones are carried as notes rather than
//! rejected — a recorder may add its own without breaking older sessions, and a
//! future quantick may read a new one without invalidating recorded ones. Depth
//! (the L2 book behind the heatmap) is deliberately absent from version 1: it
//! belongs in a sibling file per session, not smuggled into the trade rows.

pub mod clock;
pub mod format;
pub mod library;
pub mod session;

pub use clock::{Batch, PlaybackConfig, Playhead, SPEEDS};
pub use format::{
    FORMAT_HELP, FORMAT_NAME, FORMAT_VERSION, FormatError, FormatErrorKind, ParseOptions, Quote,
    UtcOffset,
};
pub use library::{Library, Problem, ProblemKind, SessionEntry, scan};
pub use session::{Session, SessionDate, SessionError};
