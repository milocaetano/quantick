//! The desktop's monitors by device name: which are attached, and which one
//! holds a physical desktop point. A chart popped out to its own window is
//! saved with its monitor's name and falls back to the canvas when that
//! monitor is gone.

/// The device name of every attached monitor (`\\.\DISPLAY1`), in the
/// order the OS lists them. Empty on every target but Windows.
pub fn monitors() -> Vec<String> {
    #[cfg(windows)]
    {
        attached()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The device name of the monitor holding physical desktop pixel (`x`,
/// `y`); `None` off every monitor, for a non-finite point, and on every
/// target but Windows.
pub fn monitor_at(x: f32, y: f32) -> Option<String> {
    #[cfg(windows)]
    {
        holding(x, y)
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y);
        None
    }
}

#[cfg(windows)]
fn device_name(monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> Option<String> {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` is a live, correctly sized MONITORINFOEXW the call fills;
    // the handle came from the OS this frame and is only read.
    let filled = unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) };
    if filled == 0 {
        return None;
    }
    let length = info.szDevice.iter().position(|unit| *unit == 0)?;
    String::from_utf16(&info.szDevice[..length]).ok()
}

#[cfg(windows)]
fn attached() -> Vec<String> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    unsafe extern "system" fn each(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        names: LPARAM,
    ) -> windows_sys::core::BOOL {
        // SAFETY: `names` is the `Vec` `attached` lent for the duration of
        // the enumeration below, and nothing else touches it meanwhile.
        let names = unsafe { &mut *(names as *mut Vec<String>) };
        names.extend(device_name(monitor));
        1
    }
    let mut names: Vec<String> = Vec::new();
    // SAFETY: a synchronous enumeration; the callback writes only into
    // `names`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            (&raw mut names) as LPARAM,
        );
    }
    names
}

#[cfg(windows)]
fn holding(x: f32, y: f32) -> Option<String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let point = POINT {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    // SAFETY: a read-only lookup; the returned handle is only read below.
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };
    if monitor.is_null() {
        return None;
    }
    device_name(monitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_finite_point_is_on_no_monitor() {
        assert_eq!(monitor_at(f32::NAN, 0.0), None);
        assert_eq!(monitor_at(0.0, f32::INFINITY), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_monitor_holding_a_point_is_one_of_the_attached() {
        let attached = monitors();
        if let Some(name) = monitor_at(1.0, 1.0) {
            assert!(attached.contains(&name), "{name} not in {attached:?}");
        }
    }
}
