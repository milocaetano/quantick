//! The operating system's own attention sounds.
//!
//! One function, one platform: `MessageBeep` on Windows, which reads the
//! `MB_*` styles as sound-scheme entries and so respects the sounds the
//! user chose and the silence they chose. Every other platform answers
//! honestly that it has none.

use super::AlertSound;

/// Ask the platform for one of its scheme sounds. A [`AlertSound::Clip`]
/// is not the platform's to play and is refused here rather than mapped to
/// a beep it does not resemble.
#[cfg(windows)]
pub(super) fn alert(sound: AlertSound) -> Result<(), &'static str> {
    use windows_sys::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE;

    // The `MB_*` styles, which `MessageBeep` reads as sound-scheme entries
    // rather than as icons. Each already respects the user's chosen scheme
    // and their silence.
    const MB_OK: MESSAGEBOX_STYLE = 0x0000_0000;
    const MB_ICONHAND: MESSAGEBOX_STYLE = 0x0000_0010;
    const MB_ICONQUESTION: MESSAGEBOX_STYLE = 0x0000_0020;
    const MB_ICONEXCLAMATION: MESSAGEBOX_STYLE = 0x0000_0030;
    const MB_ICONASTERISK: MESSAGEBOX_STYLE = 0x0000_0040;

    let style = match sound {
        AlertSound::Information => MB_ICONASTERISK,
        AlertSound::Question => MB_ICONQUESTION,
        AlertSound::Exclamation => MB_ICONEXCLAMATION,
        AlertSound::Critical => MB_ICONHAND,
        AlertSound::Beep => MB_OK,
        AlertSound::Clip(_) => return Err("a library clip is not one of the platform's sounds"),
    };
    // SAFETY: `MessageBeep` takes one integer, touches no memory this
    // process owns, and is callable from any thread.
    let played = unsafe { windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(style) };
    if played == 0 {
        return Err("the operating system refused to play its alert sound");
    }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn alert(sound: AlertSound) -> Result<(), &'static str> {
    let _ = sound;
    Err("this build has no platform alert sound, so an audible alert cannot be produced")
}
