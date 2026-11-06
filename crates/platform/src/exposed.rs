//! The exposed native window under a physical desktop pointer, independent of pointer capture.

const TITLE_PREFIX: &str = "Quantick - Market window ";

/// The native title of market window `window`: the identity the exposed-window
/// lookup and the DPI adapter read back from the OS.
pub fn window_title(window: u64) -> String {
    format!("{TITLE_PREFIX}{window}")
}

/// The number of this process's market window exposed at physical desktop
/// pixel (`x`, `y`), by z-order, even while another window holds the mouse
/// capture. `None` for a foreign or untitled window, a non-finite point, and
/// on every target but Windows.
pub fn window_at(x: f32, y: f32) -> Option<u64> {
    #[cfg(windows)]
    {
        exposed_at(x, y)
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y);
        None
    }
}

#[cfg(windows)]
fn exposed_at(x: f32, y: f32) -> Option<u64> {
    use windows_sys::Win32::{
        Foundation::POINT,
        UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetWindowTextW, GetWindowThreadProcessId, WindowFromPoint,
        },
    };
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    // SAFETY: this is a read-only hit test. No returned handle is retained or dereferenced.
    // PID ownership is checked before the bounded title read; a foreign process can never
    // become a transfer target, even if it copied our title. WindowFromPoint observes z-order,
    // including when a different window owns mouse capture during a drag.
    unsafe {
        let hwnd = GetAncestor(
            WindowFromPoint(POINT {
                x: x.round() as i32,
                y: y.round() as i32,
            }),
            GA_ROOT,
        );
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if pid != std::process::id() {
            return None;
        }
        let mut text = [0u16; 96];
        let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        let text = String::from_utf16(text.get(..usize::try_from(length).ok()?)?).ok()?;
        parse(&text)
    }
}

#[cfg(any(windows, test))]
fn parse(value: &str) -> Option<u64> {
    value.strip_prefix(TITLE_PREFIX)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_window_titles_map_only_our_bounded_identity_format() {
        assert_eq!(parse(&window_title(17)), Some(17));
        for title in [
            "Quantick",
            "Other app",
            "Quantick - Market window -1",
            "Quantick - Market window 18446744073709551616",
        ] {
            assert!(parse(title).is_none());
        }
    }

    #[test]
    fn a_non_finite_point_exposes_no_window() {
        assert_eq!(window_at(f32::NAN, 0.0), None);
        assert_eq!(window_at(0.0, f32::INFINITY), None);
    }
}
