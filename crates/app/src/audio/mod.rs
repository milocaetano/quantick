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

use std::time::{Duration, Instant};

pub use library::{Clip, ClipId};

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

/// Where a sound comes from, which is also how the picker groups them and
/// which folder of `assets/alarms/` a clip is filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundCategory {
    /// The operating system's own scheme sounds. Never a clip's category.
    System,
    /// `assets/alarms/standard/`: clips that behave like an alarm — beeps,
    /// phones, a cuckoo.
    Standard,
    /// `assets/alarms/nature/`: ambient clips — rain, surf, a steam train —
    /// mostly long, which is what the cut is for.
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
            Self::Clip(id) => id.clip().category,
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
    /// The one way a stored setting becomes a cue: the sound, cut after
    /// `secs` when there are seconds and the sound can be cut. A platform
    /// beep is whole whatever the setting says — the operating system
    /// plays it in one piece — so two presets naming the same beep with
    /// different cuts compile to the *same* cue, and the per-frame guard
    /// against stacked beeps sees one sound, not two. The preset compiler
    /// and the dialog's Test button both come through here, so what the
    /// trader auditions is what the armed instance will play.
    #[must_use]
    pub fn new(sound: AlertSound, secs: Option<u32>) -> Self {
        match secs {
            Some(secs) if sound.can_be_cut() => Self::cut_after(sound, secs),
            _ => Self::whole(sound),
        }
    }

    /// The sound, whole — what every caller before the length existed
    /// meant.
    #[must_use]
    pub const fn whole(sound: AlertSound) -> Self {
        Self {
            sound,
            length: PlayLength::Whole,
        }
    }

    /// The sound, cut after `secs`. Prefer [`Self::new`], which knows
    /// which sounds a cut applies to.
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
    /// once — and its clips queue behind each other so two regions with
    /// two sounds are both heard. Platform sounds cannot queue: they sound
    /// the moment they are asked for, ahead of the batch's clips.
    ///
    /// Every cue is attempted. `Ok(())` means every one was handed to
    /// something that plays it; `Err` carries the *first* reason one could
    /// not be, in words a client can print — a notification that never
    /// reached the trader is reported, never assumed, and one that did is
    /// not withheld because a neighbour failed.
    fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str>;

    /// Get ready to play `cue` soon, off the path that will ask for it.
    /// A sink that has a device to open opens it here — when an alarm is
    /// armed, not on the frame the signal lands — so the first alarm pays
    /// nothing the second does not. The default does nothing, which is
    /// right for a sink with nothing to prepare.
    fn warm_up(&mut self, cue: Cue) {
        let _ = cue;
    }
}

/// How long the [`Speaker`] leaves a refused output device alone before
/// asking for it again. Long enough that a machine with no output device
/// does not re-enumerate its audio stack on every signal bar, short enough
/// that a headset plugged in mid-session is found within a minute.
pub const DEVICE_RETRY_AFTER: Duration = Duration::from_secs(30);

/// The shipped sink: platform sounds through the operating system, library
/// clips through the default output device.
///
/// The device is opened by [`AlertSink::warm_up`] when an alarm naming a
/// clip is armed, or by the first clip if nothing warmed it — never at
/// start-up, so a chart that never arms an alarm never touches an audio
/// device — and kept open from then on. A device that refused is asked
/// again after [`DEVICE_RETRY_AFTER`]; one that failed after opening is
/// dropped and reopened on the next cue, so a success is never claimed
/// about a device that is no longer there.
#[derive(Default)]
pub struct Speaker {
    clips: Option<player::ClipPlayer>,
    /// When the device last refused to open, if it did.
    refused_at: Option<Instant>,
}

impl Speaker {
    /// The open device, opening or reopening it as needed.
    fn device(&mut self) -> Result<&mut player::ClipPlayer, &'static str> {
        if self
            .clips
            .as_ref()
            .is_some_and(player::ClipPlayer::is_faulted)
        {
            self.clips = None;
        }
        if self.clips.is_none() {
            if let Some(refused_at) = self.refused_at
                && refused_at.elapsed() < DEVICE_RETRY_AFTER
            {
                return Err("no audio output device could be opened");
            }
            match player::ClipPlayer::open() {
                Ok(player) => {
                    self.refused_at = None;
                    self.clips = Some(player);
                }
                Err(reason) => {
                    self.refused_at = Some(Instant::now());
                    return Err(reason);
                }
            }
        }
        Ok(self
            .clips
            .as_mut()
            .expect("the device was opened a moment ago"))
    }
}

impl AlertSink for Speaker {
    fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str> {
        let mut first_failure: Option<&'static str> = None;
        let mut clips: Vec<(&'static Clip, PlayLength)> = Vec::with_capacity(cues.len());
        for cue in cues {
            match cue.sound {
                AlertSound::Clip(id) => clips.push((id.clip(), cue.length)),
                platform_sound => {
                    if let Err(reason) = platform::alert(platform_sound) {
                        first_failure.get_or_insert(reason);
                    }
                }
            }
        }
        if clips.is_empty() {
            // The newest signal is a beep: a clip still running from an
            // older one would bury it, and its news is already old.
            if let Some(player) = self.clips.as_mut() {
                player.stop();
            }
        } else if let Err(reason) = self.device().and_then(|player| player.play(&clips)) {
            first_failure.get_or_insert(reason);
        }
        first_failure.map_or(Ok(()), Err)
    }

    fn warm_up(&mut self, cue: Cue) {
        if cue.sound.can_be_cut() {
            // The refusal, if any, is reported by the cue that needs the
            // device; warming up is silent by design.
            let _ = self.device();
        }
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
    pub warmed: std::rc::Rc<std::cell::RefCell<Vec<Cue>>>,
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

    /// What the app asked to be ready for, in order.
    pub fn warmed_up(&self) -> Vec<Cue> {
        self.warmed.borrow().clone()
    }
}

#[cfg(test)]
impl AlertSink for RecordingAlerts {
    fn play(&mut self, cues: &[Cue]) -> Result<(), &'static str> {
        self.played.borrow_mut().extend_from_slice(cues);
        Ok(())
    }

    fn warm_up(&mut self, cue: Cue) {
        self.warmed.borrow_mut().push(cue);
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
    /// under none — and no clip claims the system heading.
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
            "a platform beep cannot be cut, and no clip is filed under system"
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

    /// A cut applies to a clip and not to a beep: the same beep asked for
    /// with and without seconds is one cue, so two presets that agree on
    /// the sound cannot make the platform play it twice in a frame.
    #[test]
    fn a_cut_only_reaches_a_sound_that_can_be_cut() {
        let clip = AlertSound::in_category(SoundCategory::Standard)
            .next()
            .expect("a standard clip");
        assert_eq!(Cue::new(clip, Some(4)), Cue::cut_after(clip, 4));
        assert_eq!(Cue::new(clip, None), Cue::whole(clip));
        assert_eq!(
            Cue::new(AlertSound::Critical, Some(4)),
            Cue::whole(AlertSound::Critical)
        );
        assert_eq!(
            Cue::new(AlertSound::Critical, Some(4)),
            Cue::new(AlertSound::Critical, None)
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
        installed.warm_up(first);
        assert_eq!(installed.play(&[first]), Ok(()));
        assert_eq!(installed.play(&[second]), Ok(()));
        assert_eq!(sink.cues(), vec![first, second]);
        assert_eq!(sink.sounds(), vec![AlertSound::Critical, AlertSound::Beep]);
        assert_eq!(sink.warmed_up(), vec![first]);
    }
}
