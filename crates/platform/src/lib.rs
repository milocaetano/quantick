//! The OS-window exception: the native window calls `app` needs and egui
//! does not supply, behind safe functions.
//!
//! `CLAUDE.md` keeps native APIs out of every crate below `app`; this crate is
//! the one place they are bound for it, the way `feed` is the one place that
//! owns a clock. It knows windows by number and title only: no egui, no
//! workspace or domain type. Windows code sits under `cfg(windows)`; on every
//! other target each call is a no-op or answers `None`. All `unsafe` stays in
//! here, beside its SAFETY note, so the caller holds none.

#[cfg(windows)]
mod dpi;
mod exposed;
mod monitors;
mod pump;

pub use exposed::{window_at, window_title};
pub use monitors::{monitor_at, monitors};
pub use pump::HiddenRootPump;

/// Give one market child window the Windows 11 DPI-transition rectangle fix.
///
/// Idempotent per window lifecycle; the root receives it through
/// [`HiddenRootPump::attach`]. A no-op before Windows 11 and on other targets.
pub fn attach_dpi_child(window: u64) {
    #[cfg(windows)]
    dpi::attach_child(window);
    #[cfg(not(windows))]
    let _ = window;
}
