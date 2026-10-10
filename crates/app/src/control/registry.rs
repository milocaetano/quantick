//! The application's snapshot projection registry.
//!
//! The registry itself — scope validation, revision tracking, the capture
//! budget — is headless and lives in `quantick_control_host::projection`. What
//! stays here is what only the application can supply: the host state its
//! projectors read (the window's [`ControlWindow`] port) and the clock
//! ([`SystemClock`]).

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use quantick_control_host::{clock::HostClock, projection};

use crate::{app::ControlWindow, metrics};

pub(crate) use quantick_control_host::projection::{
    CaptureContext, ProjectionRegistryError, SerializedSnapshotCapture, SnapshotCapture,
};

/// The projection registry over the running application: the generic
/// registry with the window's port as its host. Families dock through
/// `quantick_control_handlers::dock::ProjectionDock`, which it implements.
pub(crate) type ProjectionRegistry = projection::ProjectionRegistry<ControlWindow>;

/// The clock a capture is stamped and timed by: the process wall clock, and a
/// monotonic clock measured from its first reading.
pub(crate) struct SystemClock;

impl HostClock for SystemClock {
    fn unix_ms(&self) -> i64 {
        metrics::wall_clock_ms()
    }

    fn monotonic(&self) -> Duration {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN.get_or_init(Instant::now).elapsed()
    }
}
