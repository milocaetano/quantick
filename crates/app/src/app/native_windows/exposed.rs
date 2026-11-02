//! The exposed native window under a physical desktop pointer, independent of pointer capture.
#[cfg(all(windows, not(test)))]
use super::Point;
use super::WindowId;

pub(super) fn title(window: WindowId) -> String {
    format!("Quantick - Market window {}", window.0)
}

#[cfg(all(windows, not(test)))]
pub(super) fn at(point: Point) -> Option<WindowId> {
    use windows_sys::Win32::{
        Foundation::POINT,
        UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetWindowTextW, GetWindowThreadProcessId, WindowFromPoint,
        },
    };
    if !point.x.is_finite() || !point.y.is_finite() {
        return None;
    }
    // SAFETY: this is a read-only hit test. No returned handle is retained or dereferenced.
    // PID ownership is checked before the bounded title read; a foreign process can never
    // become a transfer target, even if it copied our title. WindowFromPoint observes z-order,
    // including when a different window owns mouse capture during a drag.
    unsafe {
        let hwnd = GetAncestor(
            WindowFromPoint(POINT {
                x: point.x.round() as i32,
                y: point.y.round() as i32,
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
fn parse(value: &str) -> Option<WindowId> {
    value
        .strip_prefix("Quantick - Market window ")?
        .parse()
        .ok()
        .map(WindowId)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_window_titles_map_only_our_bounded_identity_format() {
        assert_eq!(parse(&title(WindowId(17))), Some(WindowId(17)));
        for title in [
            "Quantick",
            "Other app",
            "Quantick - Market window -1",
            "Quantick - Market window 18446744073709551616",
        ] {
            assert!(parse(title).is_none());
        }
    }
}
