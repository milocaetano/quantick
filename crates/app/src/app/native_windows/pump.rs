//! Keep immediate viewports scheduled after their root HWND is hidden.
//!
//! eframe 0.29 redirects an immediate child's redraw to its root. Winit's
//! Windows redraw invalidates that HWND, but Windows does not generate paint
//! messages for hidden windows. A UI-thread timer posts the missing paint;
//! the ordinary eframe update still owns all processing and child painting.
//! No extra worker, app callback, visibility change or window subclassing.

#[derive(Default)]
pub(in crate::app) struct HiddenRootPump {
    #[cfg(windows)]
    hwnd: isize,
    #[cfg(windows)]
    timer: usize,
}

impl HiddenRootPump {
    pub(in crate::app) fn attach(&mut self, _handle: &impl raw_window_handle::HasWindowHandle) {
        #[cfg(windows)]
        {
            self.set_active(false);
            self.hwnd = _handle
                .window_handle()
                .ok()
                .map_or(0, |handle| match handle.as_raw() {
                    raw_window_handle::RawWindowHandle::Win32(handle) => handle.hwnd.get(),
                    _ => 0,
                });
        }
    }

    pub(in crate::app) fn set_active(&mut self, _active: bool) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};
            // The timer belongs to the root HWND and runs on its event thread.
            // This private ID is used only by this root's repaint pump.
            const TIMER_ID: usize = 0x514E_544B;
            const FRAME_INTERVAL_MS: u32 = 16;
            // SAFETY: the handle comes from eframe's root surface. Only this
            // owner creates/removes this timer; Drop also removes it before the
            // app releases its surface. The callback retains no app reference.
            unsafe {
                if !_active && self.timer != 0 {
                    KillTimer(self.hwnd as _, self.timer);
                    self.timer = 0;
                } else if _active && self.timer == 0 && self.hwnd != 0 {
                    self.timer = SetTimer(
                        self.hwnd as _,
                        TIMER_ID,
                        FRAME_INTERVAL_MS,
                        Some(wake_hidden_root),
                    );
                    if self.timer == 0 {
                        tracing::error!("could not schedule hidden root viewport repaint");
                    }
                }
            }
        }
    }
}

impl Drop for HiddenRootPump {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

#[cfg(windows)]
unsafe extern "system" fn wake_hidden_root(
    hwnd: windows_sys::Win32::Foundation::HWND,
    _message: u32,
    _timer: usize,
    _time: u32,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, PostMessageW, WM_PAINT};
    // SAFETY: Windows supplies this owner HWND on its UI thread. Posting queues
    // normal winit dispatch; it never re-enters the app from the timer callback.
    // A stale queued timer after teardown cannot target a visible replacement.
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            PostMessageW(hwnd, WM_PAINT, 0, 0);
        }
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW, WM_PAINT,
    };

    #[test]
    fn hidden_root_pump_posts_paint_without_showing_window_and_releases_timer() {
        // A private hidden STATIC window needs no app, GL context or UI input.
        let class: Vec<_> = "STATIC\0".encode_utf16().collect();
        // SAFETY: the built-in class copies the class name; no pointer is kept.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                1,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let mut pump = HiddenRootPump {
            hwnd: hwnd as isize,
            timer: 0,
        };
        pump.set_active(true);
        assert_ne!(pump.timer, 0);
        let timer = pump.timer;
        pump.set_active(true);
        assert_eq!(
            pump.timer, timer,
            "one timer per root, regardless of frame count"
        );
        // Exercise the exact timer callback without a timing-sensitive sleep.
        let mut message = MSG::default();
        // SAFETY: this test owns the HWND and dispatches no foreign messages.
        unsafe {
            wake_hidden_root(hwnd, 0, pump.timer, 0);
            assert_ne!(
                PeekMessageW(&raw mut message, hwnd, WM_PAINT, WM_PAINT, PM_REMOVE),
                0
            );
            assert_eq!(
                windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd),
                0
            );
        }
        pump.set_active(false);
        assert_eq!(pump.timer, 0);
        pump.set_active(true);
        let timer = pump.timer;
        drop(pump);
        // SAFETY: the timer is stopped and the test still owns its window.
        assert_eq!(
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(hwnd, timer) },
            0
        );
        assert_ne!(unsafe { DestroyWindow(hwnd) }, 0);
    }
}
