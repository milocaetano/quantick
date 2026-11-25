//! Embeds `assets/icon.ico` as the Windows executable's resource icon (the one
//! Explorer, the taskbar and Alt+Tab show), and gives the Windows main thread
//! the stack reserve the other desktops already give it. No-op on other
//! targets.

/// The executable's default stack reserve on Windows: 8 MiB, what Linux and
/// macOS give a main thread. Windows reserves only 1 MiB, so the UI thread
/// overflowed there first while CI on Linux stayed green. The reserve covers
/// the main thread and any native thread created without a size; Rust's own
/// threads set theirs. It is address space, committed only as it is touched.
const MAIN_STACK_BYTES: u64 = 8 * 1024 * 1024;

fn main() {
    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed Windows icon resource");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:{MAIN_STACK_BYTES}"),
            Ok("gnu") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,{MAIN_STACK_BYTES}"),
            _ => {}
        }
    }
}
