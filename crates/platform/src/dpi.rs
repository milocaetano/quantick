//! Windows 11 supplies the authoritative rectangle for a DPI transition.
//!
//! winit 0.30.13 still applies the older monitor-nudge workaround, which can
//! recursively return a dragged window to its previous DPI and grow it. Keep
//! its scale/event handling, but use the OS rectangle for its nested resize.
//! Upstream fix: https://github.com/rust-windowing/winit/commit/488c036.
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
use windows_sys::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{SystemInformation::OSVERSIONINFOW, Threading::GetCurrentThreadId},
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                EnumThreadWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsZoomed,
                SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_DPICHANGED, WM_NCDESTROY,
                WM_WINDOWPOSCHANGING,
            },
        },
    },
};

const SUBCLASS_ID: usize = 0x5144_5049;
/// Which window a subclass serves: the root, or a market child by number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Root,
    Child(u64),
}
thread_local! {
    static INSTALLED: std::cell::RefCell<BTreeMap<Key, isize>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}
struct State {
    window: Key,
    suggested: Cell<Option<RECT>>,
}

fn windows_11() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: RtlGetVersion writes the ABI-sized, initialized local structure.
        unsafe { RtlGetVersion(&raw mut version) >= 0 && version.dwBuildNumber >= 22000 }
    })
}

pub(crate) fn attach_root(hwnd: isize) {
    install(hwnd as HWND, Key::Root);
}

pub(crate) fn attach_child(number: u64) {
    let window = Key::Child(number);
    if !windows_11() || INSTALLED.with(|map| map.borrow().contains_key(&window)) {
        return;
    }
    // eframe exposes only the root raw handle. Enumerate this UI thread once
    // when a new immediate viewport appears; the exact title identifies it.
    let mut lookup: (String, HWND) = (crate::window_title(number), std::ptr::null_mut());
    // SAFETY: enumeration is synchronous and the callback only borrows lookup.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(find_child),
            &raw mut lookup as LPARAM,
        )
    };
    install(lookup.1, window);
}

unsafe extern "system" fn find_child(hwnd: HWND, data: LPARAM) -> i32 {
    // SAFETY: attach_child supplies a live tuple for this synchronous enumeration.
    let (title, found) = unsafe { &mut *(data as *mut (String, HWND)) };
    let mut text = [0u16; 96];
    // SAFETY: the buffer is writable and bounded; enumeration supplies the HWND.
    let length = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) };
    if length > 0 && String::from_utf16_lossy(&text[..length as usize]) == *title {
        *found = hwnd;
        0
    } else {
        1
    }
}

fn install(hwnd: HWND, window: Key) {
    if hwnd.is_null() || !windows_11() || INSTALLED.with(|map| map.borrow().contains_key(&window)) {
        return;
    }
    let mut pid = 0;
    // SAFETY: query ownership before subclassing. All state stays on the HWND's
    // own UI thread; no runtime, worker, egui context or app reference is kept.
    unsafe {
        if GetWindowThreadProcessId(hwnd, &raw mut pid) != GetCurrentThreadId()
            || pid != std::process::id()
        {
            return;
        }
        let data = Rc::into_raw(Rc::new(State {
            window,
            suggested: Cell::new(None),
        }));
        if SetWindowSubclass(hwnd, Some(dispatch), SUBCLASS_ID, data as usize) == 0 {
            drop(Rc::from_raw(data));
            tracing::error!("could not install native DPI rectangle adapter");
        } else {
            INSTALLED.with(|map| map.borrow_mut().insert(window, hwnd as isize));
        }
    }
}

unsafe extern "system" fn dispatch(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: one Rc is owned by this subclass until WM_NCDESTROY. Each active
    // callback keeps another, so recursive dispatch or destruction cannot free
    // the state while an outer callback restores its scoped rectangle.
    unsafe {
        let pointer = data as *const State;
        Rc::increment_strong_count(pointer);
        let state = Rc::from_raw(pointer);
        if message == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(dispatch), id);
            INSTALLED.with(|map| map.borrow_mut().remove(&state.window));
            drop(Rc::from_raw(pointer));
        } else if message == WM_DPICHANGED
            && lparam != 0
            && IsZoomed(hwnd) == 0
            && IsIconic(hwnd) == 0
        {
            let previous = state.suggested.replace(Some(*(lparam as *const RECT)));
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            state.suggested.set(previous);
            return result;
        } else if message == WM_WINDOWPOSCHANGING
            && lparam != 0
            && let Some(rect) = state.suggested.get()
        {
            let position = &mut *(lparam as *mut WINDOWPOS);
            if position.flags & SWP_NOMOVE == 0 {
                position.x = rect.left;
                position.y = rect.top;
            }
            if position.flags & SWP_NOSIZE == 0 {
                position.cx = rect.right - rect.left;
                position.cy = rect.bottom - rect.top;
            }
        }
        // Preserve winit events, sibling subclasses, and default min/max checks.
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests;
