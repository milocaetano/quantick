//! quantick-pine — the "Quantick Pine" language frontend.
//!
//! A Pine v5 **subset dialect** for scripted indicators on activity-sampled
//! charts: lexer, parser, compile passes and (from M2 PR 7 on) the
//! tree-walking interpreter that turns a script into an implementation of
//! the [`quantick_indicators::Indicator`] trait.
//!
//! The enabling insight (plan §0): Pine's x-axis is `bar_index`, not time,
//! which maps 1:1 onto alternative bars. What is meaningless here —
//! `request.security`, `timeframe.*`, sessions — is exactly what the dialect
//! cuts, each rejection a load-time error with a stable code and a span,
//! never a silently wrong plot (data honesty).
//!
//! A pure domain crate: hand-rolled everything, zero dependencies beyond the
//! indicator runtime, no UI, no clock, no I/O — the app owns files and
//! folders; this crate only ever sees source text.

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;

pub use error::{ErrorCode, PineError, Span};
