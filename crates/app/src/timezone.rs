//! Display timezone: a fixed UTC offset applied only when rendering trade times.
//!
//! The type lives in `quantick_paper::civil`, beside the civil-date law the
//! paper report cuts on, because a crate below `app` needs it and the engine
//! is documented as never seeing a timezone. The name stays here so the chart
//! axis, the workspace file and the control plane keep one address for it.

pub use quantick_paper::civil::TzOffset;
