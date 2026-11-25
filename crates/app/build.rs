//! Embeds `assets/icon.ico` as the Windows executable's resource icon (the one
//! Explorer, the taskbar and Alt+Tab show). No-op on other targets.

fn main() {
    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed Windows icon resource");
    }
    // Windows reserves 1 MiB of stack, Linux and macOS 8 MiB; match them. The
    // reserve covers every thread made without a size, as address space only.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
