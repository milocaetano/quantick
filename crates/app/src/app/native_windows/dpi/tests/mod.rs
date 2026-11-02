use super::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetWindowPos,
};

#[link(name = "comctl32")]
unsafe extern "system" {
    fn GetWindowSubclass(hwnd: HWND, callback: Subclass, id: usize, data: *mut usize) -> i32;
}

#[derive(Default)]
struct Backend {
    depth: Cell<u32>,
    events: Cell<u32>,
    nested: Cell<bool>,
    destroy: Cell<bool>,
}

fn hidden(window: WindowId) -> HWND {
    let class: Vec<_> = "STATIC\0".encode_utf16().collect();
    let title: Vec<_> = format!("{}\0", exposed::title(window))
        .encode_utf16()
        .collect();
    // SAFETY: the built-in class copies the names and creates a hidden test window.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            600,
            400,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null());
    hwnd
}

unsafe extern "system" fn backend(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: each test keeps Backend alive until its private HWND is destroyed.
    unsafe {
        let backend = &*(data as *const Backend);
        if message == WM_DPICHANGED {
            backend.events.set(backend.events.get() + 1);
            assert_eq!(wparam, 96 | (96 << 16));
            if backend.destroy.get() {
                DestroyWindow(hwnd);
                return 0;
            }
            let depth = backend.depth.get();
            backend.depth.set(depth + 1);
            if backend.nested.get() && depth == 0 {
                let rect = RECT {
                    left: -1100,
                    top: 200,
                    right: -500,
                    bottom: 600,
                };
                SendMessageW(hwnd, WM_DPICHANGED, wparam, &raw const rect as LPARAM);
                assert_eq!(bounds(hwnd), [-1100, 200, -500, 600]);
            }
            // Model the pinned backend's incorrect rectangle while retaining
            // the actual synchronous Windows resize and subclass chain.
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                500,
                300,
                1500,
                1000,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            backend.depth.set(depth);
            return 0;
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

fn bounds(hwnd: HWND) -> [i32; 4] {
    let mut rect = RECT::default();
    // SAFETY: the test's hidden window is live at each call.
    assert_ne!(unsafe { GetWindowRect(hwnd, &raw mut rect) }, 0);
    [rect.left, rect.top, rect.right, rect.bottom]
}

fn state(hwnd: HWND) -> Rc<State> {
    let mut data = 0;
    // SAFETY: query our own live subclass and retain its typed Rc for inspection.
    unsafe {
        assert_ne!(
            GetWindowSubclass(hwnd, dispatch, SUBCLASS_ID, &raw mut data),
            0
        );
        Rc::increment_strong_count(data as *const State);
        Rc::from_raw(data as *const State)
    }
}

#[test]
fn dpi_nested_resize_uses_os_rect_then_restores_scope_and_releases_on_destroy() {
    if !windows_11() {
        return;
    }
    let window = WindowId(9051);
    let hwnd = hidden(window);
    let backend_state = Backend {
        nested: Cell::new(true),
        ..Backend::default()
    };
    // SAFETY: synchronous callbacks borrow this local until the window is destroyed.
    unsafe {
        assert_ne!(
            SetWindowSubclass(hwnd, backend, 1, &raw const backend_state as usize),
            0
        );
    }
    attach_child(window);
    let retained = state(hwnd);
    attach_child(window);
    assert!(
        Rc::ptr_eq(&retained, &state(hwnd)),
        "one registration per lifecycle"
    );
    let rect = RECT {
        left: -1500,
        top: 150,
        right: -840,
        bottom: 590,
    };
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            96 | (96 << 16),
            &raw const rect as LPARAM,
        );
    }
    assert_eq!(
        backend_state.events.get(),
        2,
        "both scale events reach the backend"
    );
    assert_eq!(
        bounds(hwnd),
        [-1500, 150, -840, 590],
        "outer scope restored after nested DPI"
    );
    assert!(retained.suggested.get().is_none());
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            100,
            100,
            700,
            500,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    assert_eq!(
        bounds(hwnd),
        [100, 100, 800, 600],
        "ordinary resizing is untouched"
    );
    let weak = Rc::downgrade(&retained);
    drop(retained);
    unsafe {
        assert_ne!(DestroyWindow(hwnd), 0);
    }
    assert!(
        weak.upgrade().is_none(),
        "destruction releases subclass ownership"
    );
    assert!(!INSTALLED.with(|map| map.borrow().contains_key(&window)));
    let replacement = hidden(window);
    attach_child(window);
    assert!(INSTALLED.with(|map| map.borrow().contains_key(&window)));
    unsafe {
        DestroyWindow(replacement);
    }
}

#[test]
fn destruction_during_dpi_dispatch_keeps_outer_state_alive_until_return() {
    if !windows_11() {
        return;
    }
    let hwnd = hidden(WindowId(9052));
    let backend_state = Backend {
        destroy: Cell::new(true),
        ..Backend::default()
    };
    unsafe {
        SetWindowSubclass(hwnd, backend, 1, &raw const backend_state as usize);
    }
    install(hwnd, WindowId(9052));
    let weak = Rc::downgrade(&state(hwnd));
    let rect = RECT {
        left: -1000,
        top: 150,
        right: -400,
        bottom: 550,
    };
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            96 | (96 << 16),
            &raw const rect as LPARAM,
        );
    }
    assert_eq!(backend_state.events.get(), 1);
    assert!(weak.upgrade().is_none());
    assert!(!INSTALLED.with(|map| map.borrow().contains_key(&WindowId(9052))));
}

#[test]
fn registration_refuses_another_ui_threads_window() {
    let hwnd = hidden(WindowId(9053));
    let address = hwnd as isize;
    std::thread::spawn(move || {
        install(address as HWND, WindowId(9053));
        assert!(INSTALLED.with(|map| map.borrow().is_empty()));
    })
    .join()
    .unwrap();
    let mut data = 0;
    unsafe {
        assert_eq!(
            GetWindowSubclass(hwnd, dispatch, SUBCLASS_ID, &raw mut data),
            0
        );
        DestroyWindow(hwnd);
    }
}
