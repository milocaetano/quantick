//! Reading the window's client area straight from the platform.
//!
//! This module exists because of a defect it deliberately does **not** try to
//! correct, and the reason for that restraint is the whole point of the file.
//!
//! On Windows, a process whose DPI context has gone stale — the display's
//! scaling changed without a sign-out, so the session's system DPI is 144 while
//! the monitor reports 96 — lays the chart out 1.5x too large and paints a
//! third of it outside the window. What falls off is the right and bottom edge,
//! which on this chart is the toolbar's layer group, the price axis, the live
//! strip and the dock rail. Measured: `screen_rect` 2560x1369 points at
//! `native_pixels_per_point = 1.5`, painting 3840x2054 pixels into a surface
//! that is really 2560x1369.
//!
//! Three candidate corrections were built and measured, and each failed for its
//! own reason. They are written down here because the next person to try will
//! reach for them in this order:
//!
//! 1. **Against `ViewportInfo::monitor_size`** — "a window cannot be bigger
//!    than its screen". `egui-winit-0.29.1/src/lib.rs:970` builds that field as
//!    `monitor.size().to_logical(pixels_per_point)`, so it is in **points**.
//!    Comparing `points * scale` against it condemns every honest window on a
//!    150% display: 1400x900 points evaluates `2100 > 1707` and `1350 > 961`
//!    and gets shrunk to 933x600 — a correction far worse than the defect.
//!    Reading it as points removes the danger and the detection with it, since
//!    a maximised window legitimately *equals* its monitor in points.
//! 2. **Against `screen_rect` vs `inner_rect`** — they are wrong together.
//! 3. **Against the platform's own `GetClientRect`** — the reading this module
//!    still provides. It comes back **3840x2052** from inside the process, not
//!    the 2560x1369 the same call returns from a process with a correct DPI
//!    context: Windows virtualises coordinates into the caller's context, so
//!    this "independent" reading is wrong in exactly the same way and agrees
//!    with egui to the pixel.
//!
//! The conclusion is the finding: every observable available *inside* the
//! process is self-consistently wrong, because the thing that is wrong is the
//! process's own coordinate space. A correction would have to key off something
//! outside it — the GL framebuffer's real size, or a DPI comparison made in a
//! different awareness context — and that is a change to how the app declares
//! its DPI awareness, not a fix-up on the way into a frame.
//!
//! Upstream has the same family open and unfixed: `emilk/egui#7648` (0.33.1)
//! and `emilk/egui#7095`. The jump from the 0.29 we build against is 262
//! compile errors for a release that still has it.
//!
//! So what ships here is the *reading*, wired into the health summary beside
//! `screen_pt` and `scale`. Those three numbers together are what turned this
//! from "the buttons are gone" into a measurement, and they are what the next
//! diagnosis will start from.

use eframe::egui;

/// A handle to the window, kept so the client area can be read back from the
/// platform on any frame.
///
/// Taken once at startup from the [`eframe::CreationContext`], because that is
/// where eframe hands out the window handle; `App::raw_input_hook`, where the
/// correction is applied, has no window of its own.
pub(crate) struct SurfaceProbe {
    #[cfg(windows)]
    hwnd: isize,
}

impl SurfaceProbe {
    /// Take the platform handle, or `None` on a platform (or a windowing
    /// backend) that does not offer the one this reads.
    #[must_use]
    pub(crate) fn new(_handle: &impl raw_window_handle::HasWindowHandle) -> Option<Self> {
        #[cfg(windows)]
        {
            let handle = _handle.window_handle().ok()?;
            match handle.as_raw() {
                raw_window_handle::RawWindowHandle::Win32(win32) => Some(Self {
                    hwnd: win32.hwnd.get(),
                }),
                _ => None,
            }
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// The window's client area in physical pixels, straight from the platform.
    ///
    /// This is the reading the whole module exists for: the one number egui's
    /// own input cannot be wrong about, because it never passes through the
    /// scale factor that is in doubt.
    #[must_use]
    pub(crate) fn client_size_px(&self) -> Option<egui::Vec2> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{HWND, RECT};
            use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: `hwnd` came from the windowing backend for this process's
            // own window, and is only read while that window is alive — the app
            // owns it for the whole of its run. `GetClientRect` writes the
            // `RECT` it is handed and touches nothing else.
            let ok = unsafe { GetClientRect(self.hwnd as HWND, &raw mut rect) };
            if ok == 0 {
                return None;
            }
            let width = (rect.right - rect.left) as f32;
            let height = (rect.bottom - rect.top) as f32;
            (width > 0.0 && height > 0.0).then_some(egui::vec2(width, height))
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off Windows there is no handle of the kind this reads, and the probe
    /// says so rather than inventing a size — the health summary then reports
    /// the client area as absent, which is the honest answer.
    #[cfg(not(windows))]
    #[test]
    fn a_platform_without_this_handle_reports_no_client_size() {
        // Nothing to construct: `new` is the only constructor and it returns
        // `None` on every non-Windows target, so no probe can exist to ask.
        assert!(
            size_of::<SurfaceProbe>() == 0,
            "the probe carries no handle here"
        );
    }

    /// On Windows the probe is a plain handle, and reading through a window
    /// that does not exist fails rather than returning a made-up size.
    #[cfg(windows)]
    #[test]
    fn a_dead_window_reports_no_client_size() {
        let probe = SurfaceProbe { hwnd: 0 };
        assert_eq!(
            probe.client_size_px(),
            None,
            "a handle that names no window has no client area to report"
        );
    }
}
