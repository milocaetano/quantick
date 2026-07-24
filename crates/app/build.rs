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
}
