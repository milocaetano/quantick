//! Deterministic interaction owners, driven by supplied chart facts.
//! Rendering, transport admission and drawing-store mutation belong to callers.

pub mod annotation;
pub mod live_trade_plan;
pub mod quick_range;

#[cfg(test)]
mod tests;

pub mod frame_tail_plan;

pub mod source_drain_plan;
