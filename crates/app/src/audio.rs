//! The one sound the platform makes.
//!
//! quantick has no audio engine and does not want one: a chart that plays
//! sounds is a chart that has to own mixing, devices and the failures of
//! both. What the annotate tier needs is narrower — an attention sound the
//! operating system already owns — so this module is the whole audio surface,
//! and a build whose platform has none says so rather than pretending the
//! alert was heard.

/// Ask the platform for its attention sound.
///
/// `Ok(())` means the sound was handed to the operating system. `Err` carries
/// the reason it could not be, in words a client can print: a notification
/// that never reached the trader is reported, never assumed.
pub fn alert() -> Result<(), &'static str> {
    platform_alert()
}

#[cfg(windows)]
fn platform_alert() -> Result<(), &'static str> {
    // MB_ICONASTERISK is the system's "information" sound: the one Windows
    // itself uses to say "look here", already respecting the user's sound
    // scheme and their silence.
    const MB_ICONASTERISK: windows_sys::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE =
        0x0000_0040;
    // SAFETY: `MessageBeep` takes one integer, touches no memory this process
    // owns, and is callable from any thread.
    let played =
        unsafe { windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONASTERISK) };
    if played == 0 {
        return Err("the operating system refused to play its alert sound");
    }
    Ok(())
}

#[cfg(not(windows))]
fn platform_alert() -> Result<(), &'static str> {
    Err("this build has no audio backend, so an audible alert cannot be produced")
}
