//! The sounds the platform makes.
//!
//! Two kinds of sound, one door. The platform's own attention sounds are
//! what quantick has always used: the operating system owns mixing,
//! devices and the user's sound scheme, and a build whose platform has none
//! says so rather than pretending the alert was heard. The alarm clips are
//! the trader's own library — twenty-seven recordings shipped inside the
//! binary ([`library`]) — for an alarm whose whole job is to be recognised
//! across a room. Those the app plays itself, through the default output
//! device ([`player`]), which is the one thing the system beep could never
//! offer: a sound long enough, and distinct enough, to say *which* region
//! spoke.
//!
//! What plays is a [`Cue`]: which [`AlertSound`], and for how long
//! ([`PlayLength`]). The length is the trader's knife — a rainforest that
//! runs for a minute is an alarm that has outstayed its news, so a preset
//! says "five seconds of it" and the clip is cut there. Where the cue
//! *goes* is a port ([`AlertSink`]). The shipped implementation is the
//! [`Speaker`]; the tests use a recorder, which is how the alarm's
//! decisions are asserted without a build machine making noise.
//!
//! Nothing in here knows what a strategy is. The signal alarm is one
//! consumer, the annotate tier's `notify.sound` is another, and the next
//! thing that wants the trader's ear — a price alert, a feed that dropped —
//! builds a [`Cue`] and hands it to the same sink.

mod library;
mod platform;
mod player;

use std::time::Duration;

pub use library::{Clip, ClipCategory, ClipId};

/// One of the sounds a trader may pick.
///
/// A closed set: the platform's five scheme sounds, and every clip in the
/// shipped [`library`]. Not a path to a file — a preset naming a sound that
/// no build owns would be an alarm that plays on one machine and not on the
/// next. Adding a clip is a file under `assets/alarms/` and a row in
/// [`library::CLIPS`]; nothing else in the app learns about it.
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
    /// One of the shipped alarm clips, played by the app itself.
    Clip(ClipId),
}

/// Where a sound comes from, which is also how the picker groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundCategory {
    /// The operating system's own scheme sounds.
    System,
    /// The library's alarm-like clips: beeps, phones, a cuckoo.
    Standard,
    /// The library's ambient clips: rain, surf, a steam train.
    Nature,
}

impl SoundCategory {
    /// Picker order: the sounds the app has always had first, then the
    /// clips that behave like alarms, then the ones that behave like a
    /// room.
    pub const ALL: [Self; 3] = [Self::System, Self::Standard, Self::Nature];

    /// The heading the picker shows over the group.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Standard => "standard alarms",
            Self::Nature => "nature alarms",
        }
    }
}

impl AlertSound {
    /// The platform's sounds, in the order the dialog lists them: quietest
    /// first, so walking the list escalates.
    pub const PLATFORM: [Self; 5] = [
        Self::Information,
        Self::Question,
        Self::Exclamation,
        Self::Critical,
        Self::Beep,
    ];

    /// Every sound a trader may pick, grouped by [`SoundCategory::ALL`]
    /// and, within a group, in the library's own order.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::PLATFORM
            .into_iter()
            .chain(ClipId::all().map(Self::Clip))
    }

    /// The sounds under one heading of the picker.
    pub fn in_category(category: SoundCategory) -> impl Iterator<Item = Self> {
        Self::all().filter(move |sound| sound.category() == category)
    }

    /// Which heading of the picker this sound sits under.
    #[must_use]
    pub fn category(self) -> SoundCategory {
        match self {
            Self::Clip(id) => match id.clip().category {
                ClipCategory::Standard => SoundCategory::Standard,
                ClipCategory::Nature => SoundCategory::Nature,
            },
            _ => SoundCategory::System,
        }
    }

    /// Whether a [`PlayLength`] can shorten this sound. The library's clips
    /// are cut wherever the cue says; a platform sound is one beep the
    /// operating system plays whole, and there is nothing to cut.
    #[must_use]
    pub fn can_be_cut(self) -> bool {
        matches!(self, Self::Clip(_))
    }

    /// The name shown in the picker.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Question => "question",
            Self::Exclamation => "exclamation",
            Self::Critical => "critical",
            Self::Beep => "default beep",
            Self::Clip(id) => id.clip().label,
        }
    }

    /// The token a preset file stores. Kept separate from [`Self::label`]
    /// so the words on screen can be reworded without silently voiding
    /// every saved preset that named one. A clip's token is its file stem
    /// under `assets/alarms/`, so a hand-edited preset can be checked
    /// against the folder.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Question => "question",
            Self::Exclamation => "exclamation",
            Self::Critical => "critical",
            Self::Beep => "beep",
            Self::Clip(id) => id.clip().token,
        }
    }

    /// Read a stored token. `None` for anything this build does not know —
    /// a preset naming a sound that does not exist is refused whole by its
    /// reader, like every other field it cannot honour.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        Self::PLATFORM
            .into_iter()
            .find(|sound| sound.token() == token)
            .or_else(|| ClipId::from_token(token).map(Self::Clip))
    }
}

/// How much of a sound plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayLength {
    /// The sound as recorded, to its end.
    #[default]
    Whole,
    /// Cut at this point, however long the recording is. A cap longer than
    /// the clip changes nothing; a cap of zero is a cue that says nothing,
    /// which is why the preset that stores one has a floor.
    Capped(Duration),
}

impl PlayLength {
    /// The seconds a preset stores, as a length.
    #[must_use]
    pub const fn seconds(secs: u32) -> Self {
        Self::Capped(Duration::from_secs(secs as u64))
    }
}

/// One request to be heard: which sound, and for how long.
///
/// The unit every consumer speaks in and every sink accepts. Small and
/// `Copy` on purpose — an armed instance keeps one, a tab queues a few per
/// frame, and a preset compiles into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cue {
    pub sound: AlertSound,
    pub length: PlayLength,
}

impl Cue {
    /// The sound, whole — what every caller before the length existed
    /// meant.
    #[must_use]
    pub const fn whole(sound: AlertSound) -> Self {
        Self {
            sound,
            length: PlayLength::Whole,
        }
    }

    /// The sound, cut after `secs`.
    #[must_use]
    pub const fn cut_after(sound: AlertSound, secs: u32) -> Self {
        Self {
            sound,
            length: PlayLength::seconds(secs),
        }
    }
}

/// Where a cue goes.
///
/// A port, so the alarm's *decisions* can be tested without a machine
/// making noise — and so a build with no audio has one honest place to say
/// so rather than every caller guessing.
pub trait AlertSink {
    /// Play these, in order, and let them replace whatever was still
    /// sounding: a clip still running has already turned the head, and the
    /// newest signal is the one the trader has not heard yet. A batch is
    /// one frame's worth — a burst of prints that closed several bars at
    /// once — and its cues queue behind each other so two regions with two
    /// sounds are both heard.
    ///
    /// `Ok(())` means every cue was handed to something that plays it.
    /// `Err` carries the reason one could not be, in words a client can
    /// print: a notification that never reached the trader is reported,
    /// never assumed.
    fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str>;
}

/// The shipped sink: platform sounds through the operating system, library
/// clips through the default output device.
///
/// The device is opened on the first clip, not at start-up — a chart that
/// never arms an alarm never touches an audio device — and kept open from
/// then on, so a second alarm does not pay for another device handshake. A
/// device that refused once is asked again on the next cue: the alarm path
/// is rare, and a headset plugged in mid-session should start working
/// without a restart.
#[derive(Default)]
pub struct Speaker {
    clips: Option<player::ClipPlayer>,
}

impl std::fmt::Debug for Speaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speaker")
            .field("device_open", &self.clips.is_some())
            .finish()
    }
}

impl AlertSink for Speaker {
    fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str> {
        // Platform sounds first and at once: the system plays them on its
        // own and they cannot queue, so they are the "now" of the batch
        // while the clips are its story.
        let mut clips: Vec<(&'static Clip, PlayLength)> = Vec::with_capacity(cues.len());
        for cue in cues {
            match cue.sound {
                AlertSound::Clip(id) => clips.push((id.clip(), cue.length)),
                platform_sound => platform::alert(platform_sound)?,
            }
        }
        if clips.is_empty() {
            return Ok(());
        }
        let player = match self.clips.as_mut() {
            Some(player) => player,
            None => self.clips.insert(player::ClipPlayer::open()?),
        };
        player.play(&clips)
    }
}

/// The second implementation of [`AlertSink`]: one that records instead of
/// playing.
///
/// It exists so the alarm's behaviour — which sound, how long, how often,
/// and on which bars — is asserted by tests rather than by a person
/// listening. Shared so a test can hand the app a sink and still read what
/// the app played through it — the sink itself disappears into a `Box<dyn
/// AlertSink>` the moment it is installed.
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub struct RecordingAlerts {
    pub played: std::rc::Rc<std::cell::RefCell<Vec<Cue>>>,
}

#[cfg(test)]
impl RecordingAlerts {
    /// What has been played so far, in order, batches flattened.
    pub fn cues(&self) -> Vec<Cue> {
        self.played.borrow().clone()
    }

    /// The sounds alone, for a test that does not care how long they ran.
    pub fn sounds(&self) -> Vec<AlertSound> {
        self.cues().into_iter().map(|cue| cue.sound).collect()
    }
}

#[cfg(test)]
impl AlertSink for RecordingAlerts {
    fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str> {
        self.played.borrow_mut().extend_from_slice(cues);
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
        for sound in AlertSound::all() {
            assert_eq!(AlertSound::from_token(sound.token()), Some(sound));
        }
        assert_eq!(AlertSound::from_token("klaxon"), None);
        assert_eq!(AlertSound::from_token(""), None);
    }

    /// Two sounds sharing a token would make one of them unreachable from a
    /// saved preset; two sharing a label would make the picker a guess.
    #[test]
    fn the_sound_set_has_no_duplicates() {
        let mut tokens: Vec<&str> = AlertSound::all().map(|sound| sound.token()).collect();
        tokens.sort_unstable();
        let named = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), named, "two sounds share a stored token");

        let mut labels: Vec<&str> = AlertSound::all().map(|sound| sound.label()).collect();
        labels.sort_unstable();
        let named = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), named, "two sounds share a picker label");
    }

    /// The picker's groups cover the set exactly once, in the order the
    /// headings are listed, so a sound is neither under two headings nor
    /// under none.
    #[test]
    fn the_categories_partition_the_sounds_in_picker_order() {
        let grouped: Vec<AlertSound> = SoundCategory::ALL
            .into_iter()
            .flat_map(AlertSound::in_category)
            .collect();
        let all: Vec<AlertSound> = AlertSound::all().collect();
        assert_eq!(grouped, all);
        assert_eq!(all.len(), AlertSound::PLATFORM.len() + library::CLIPS.len());
        assert!(
            AlertSound::in_category(SoundCategory::System).all(|sound| !sound.can_be_cut()),
            "a platform beep cannot be cut"
        );
        assert!(
            AlertSound::in_category(SoundCategory::Nature).all(AlertSound::can_be_cut),
            "every clip can be cut"
        );
    }

    /// The default is the sound this platform has always made here, so a
    /// preset that never names one behaves as the app did before the choice
    /// existed — and a cue that never names a length plays it whole.
    #[test]
    fn the_defaults_are_what_the_app_already_did() {
        assert_eq!(AlertSound::default(), AlertSound::Information);
        assert_eq!(Cue::default(), Cue::whole(AlertSound::Information));
        assert_eq!(
            Cue::cut_after(AlertSound::Beep, 5).length,
            PlayLength::Capped(Duration::from_secs(5))
        );
    }

    /// The port's other implementation records what it was asked for, in
    /// order — the whole reason the alarm can be tested at all.
    #[test]
    fn the_recording_sink_keeps_what_it_was_asked_to_play() {
        let sink = RecordingAlerts::default();
        // The handle the test keeps and the one the app is given are the
        // same recorder — the whole point of sharing it.
        let mut installed = sink.clone();
        let first = Cue::whole(AlertSound::Critical);
        let second = Cue::cut_after(AlertSound::Beep, 3);
        assert_eq!(installed.play(&[first]), Ok(()));
        assert_eq!(installed.play(&[second]), Ok(()));
        assert_eq!(sink.cues(), vec![first, second]);
        assert_eq!(sink.sounds(), vec![AlertSound::Critical, AlertSound::Beep]);
    }
}
