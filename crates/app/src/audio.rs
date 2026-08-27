//! The sounds the platform makes.
//!
//! quantick has no audio engine and does not want one: a chart that plays
//! sounds is a chart that has to own mixing, devices and the failures of
//! both. What it needs is narrower — attention sounds the operating system
//! already owns — so this module is the whole audio surface, and a build
//! whose platform has none says so rather than pretending the alert was
//! heard.
//!
//! Two consumers, one surface: the annotate tier's notification, which
//! always wants "look here", and the [signal
//! alarm](quantick_strategy::SignalAlarm), whose whole job is to be
//! recognised across a room — so the trader picks which of the platform's
//! sounds it uses. Choosing among sounds the system already defines keeps
//! the user's own sound scheme, and their silence, in charge.
//!
//! Where the sound *goes* is a port ([`AlertSink`]). The platform is one
//! implementation; the tests use a recorder, which is how the alarm's
//! decisions are asserted without a build machine making noise. A future
//! sink playing a file the trader chose docks here, and nothing that raises
//! an alarm has to change.

/// One of the platform's own attention sounds.
///
/// Deliberately a closed set of the system's sounds rather than a path to a
/// file: each of these is already mapped to something the user chose (or
/// silenced) in their sound scheme, and none of them requires this process
/// to own an audio device. Adding a sound is a variant here and a row in
/// [`AlertSound::ALL`]; nothing else in the app learns about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlertSound {
    /// The system's "look here" sound. The quiet one, and the default the
    /// annotate tier has always used.
    #[default]
    Information,
    Question,
    /// The system's warning sound — more insistent than information, which
    /// is the point of offering it to an alarm.
    Exclamation,
    /// The system's error sound: the most attention-getting of the set.
    Critical,
    /// The plain default beep.
    Beep,
}

impl AlertSound {
    /// Every sound a trader may pick, in the order the dialog lists them:
    /// quietest first, so walking the list escalates.
    pub const ALL: [Self; 5] = [
        Self::Information,
        Self::Question,
        Self::Exclamation,
        Self::Critical,
        Self::Beep,
    ];

    /// The name shown in the picker.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Question => "question",
            Self::Exclamation => "exclamation",
            Self::Critical => "critical",
            Self::Beep => "default beep",
        }
    }

    /// The token a preset file stores. Kept separate from [`Self::label`]
    /// so the words on screen can be reworded without silently voiding
    /// every saved preset that named one.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Question => "question",
            Self::Exclamation => "exclamation",
            Self::Critical => "critical",
            Self::Beep => "beep",
        }
    }

    /// Read a stored token. `None` for anything this build does not know —
    /// a preset naming a sound that does not exist is refused whole by its
    /// reader, like every other field it cannot honour.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sound| sound.token() == token)
    }
}

/// Where an alarm's sound goes.
///
/// A port, so the alarm's *decisions* can be tested without a machine
/// making noise — and so a build with no audio has one honest place to say
/// so rather than every caller guessing.
pub trait AlertSink {
    /// Play it. `Ok(())` means the sound was handed to the platform. `Err`
    /// carries the reason it could not be, in words a client can print: a
    /// notification that never reached the trader is reported, never
    /// assumed.
    fn play(&mut self, sound: AlertSound) -> Result<(), &'static str>;
}

/// The shipped sink: the operating system's own sounds.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlatformAlerts;

impl AlertSink for PlatformAlerts {
    fn play(&mut self, sound: AlertSound) -> Result<(), &'static str> {
        platform_alert(sound)
    }
}

/// Ask the platform for its attention sound.
///
/// The no-argument door the annotate tier has always used, kept as it was:
/// "look here", with no choice to make.
pub fn alert() -> Result<(), &'static str> {
    platform_alert(AlertSound::Information)
}

#[cfg(windows)]
fn platform_alert(sound: AlertSound) -> Result<(), &'static str> {
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
fn platform_alert(sound: AlertSound) -> Result<(), &'static str> {
    let _ = sound;
    Err("this build has no audio backend, so an audible alert cannot be produced")
}

/// The second implementation of [`AlertSink`]: one that records instead of
/// playing.
///
/// It exists so the alarm's behaviour — which sound, how often, and on
/// which bars — is asserted by tests rather than by a person listening.
/// Shared so a test can hand the app a sink and still read what the app
/// played through it — the sink itself disappears into a `Box<dyn
/// AlertSink>` the moment it is installed.
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub struct RecordingAlerts {
    pub played: std::rc::Rc<std::cell::RefCell<Vec<AlertSound>>>,
}

#[cfg(test)]
impl RecordingAlerts {
    /// What has been played so far, in order.
    pub fn sounds(&self) -> Vec<AlertSound> {
        self.played.borrow().clone()
    }
}

#[cfg(test)]
impl AlertSink for RecordingAlerts {
    fn play(&mut self, sound: AlertSound) -> Result<(), &'static str> {
        self.played.borrow_mut().push(sound);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens are the preset file's vocabulary: every sound round-trips,
    /// and an unknown one is refused rather than quietly becoming the
    /// default. A preset that says "critical" and plays "information" is an
    /// alarm the trader cannot trust.
    #[test]
    fn every_sound_round_trips_through_its_stored_token() {
        for sound in AlertSound::ALL {
            assert_eq!(AlertSound::from_token(sound.token()), Some(sound));
        }
        assert_eq!(AlertSound::from_token("foghorn"), None);
        assert_eq!(AlertSound::from_token(""), None);
    }

    /// Two sounds sharing a token would make one of them unreachable from a
    /// saved preset; two sharing a label would make the picker a guess.
    #[test]
    fn the_sound_set_has_no_duplicates() {
        let mut tokens: Vec<&str> = AlertSound::ALL.iter().map(|sound| sound.token()).collect();
        tokens.sort_unstable();
        let named = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), named, "two sounds share a stored token");

        let mut labels: Vec<&str> = AlertSound::ALL.iter().map(|sound| sound.label()).collect();
        labels.sort_unstable();
        let named = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), named, "two sounds share a picker label");
    }

    /// The default is the sound this platform has always made here, so a
    /// preset that never names one behaves as the app did before the choice
    /// existed.
    #[test]
    fn the_default_sound_is_the_one_the_app_already_made() {
        assert_eq!(AlertSound::default(), AlertSound::Information);
    }

    /// The port's other implementation records what it was asked for, in
    /// order — the whole reason the alarm can be tested at all.
    #[test]
    fn the_recording_sink_keeps_what_it_was_asked_to_play() {
        let sink = RecordingAlerts::default();
        // The handle the test keeps and the one the app is given are the
        // same recorder — the whole point of sharing it.
        let mut installed = sink.clone();
        assert_eq!(installed.play(AlertSound::Critical), Ok(()));
        assert_eq!(installed.play(AlertSound::Beep), Ok(()));
        assert_eq!(sink.sounds(), vec![AlertSound::Critical, AlertSound::Beep]);
    }
}
