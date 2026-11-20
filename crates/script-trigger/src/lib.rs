//! A Pine script as a strategy trigger: its `alertcondition`s pull it, and
//! the kernel's placements decide where it may ring.
//!
//! The division of labour is `quantick-strategy`'s — the trigger answers
//! *when*, a placement answers *where*, and the armed state machine joins
//! them — with the force-bar ruler swapped for a script the trader wrote.
//! [`ScriptTrigger`] runs one compiled `.pine` over every closed bar; an
//! `alertcondition` that held on the close becomes a [`Signal`] whose side is
//! the alert title's first word ([`side_of_title`]). A title that names no
//! side is refused when the script is loaded ([`Script::load`]), never
//! discovered on the first live alarm.
//!
//! An instance ([`ScriptInstance`]) is armed [`Execution::AlarmOnly`]: it
//! judges every bar exactly as a trading instance would and places nothing.
//! A runtime error in its script disarms that instance alone, with the line
//! and the stable error code as the reason ([`ScriptFault`]); its siblings
//! keep ringing.
//!
//! Headless like the kernel it plugs into: no UI, no network, no clock, no
//! file reads — the caller hands in the source.
//!
//! [`Signal`]: quantick_strategy::Signal
//! [`Execution::AlarmOnly`]: quantick_strategy::Execution::AlarmOnly

mod instance;
mod placement;
mod script;
mod trigger;

pub use instance::{ScriptInstance, ScriptState};
pub use placement::{ResolvedPlacement, ScriptPlacement};
pub use script::{ArmRefusal, Script, ScriptFault};
pub use trigger::{SCRIPT_FAULT, ScriptTrigger, WARMUP_BARS};
