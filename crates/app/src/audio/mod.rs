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

pub use quantick_strategy::sound::{AlertSound, ClipId, Cue, PlayLength, SoundCategory};

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
        let mut clips: Vec<(ClipId, PlayLength)> = Vec::with_capacity(cues.len());
        for cue in cues {
            match cue.sound {
                AlertSound::Clip(id) => clips.push((id, cue.length)),
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
        assert_eq!(
            all.len(),
            AlertSound::PLATFORM.len() + quantick_strategy::sound::CLIPS.len()
        );
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
