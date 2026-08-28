//! Playing the library's clips through the default output device.
//!
//! The one place the app touches an audio device, and it does so late: the
//! device is opened by the first clip that needs it — or, better, when an
//! alarm that names a clip is armed ([`super::AlertSink::warm_up`]) — never
//! at start-up. Decoding is lazy too — [`rodio`] pulls samples out of the
//! AAC decoder from its own mixer thread as the device asks for them — so a
//! cue costs this thread a header probe and a queue push, whatever the
//! clip's length. Nothing here runs per frame.
//!
//! Windows only, like the platform sound: the audio dependency is linked
//! there alone, and every other platform gets the stub at the bottom of
//! this file, which answers every clip with the same honest refusal the
//! platform sound already gives.

#[cfg(windows)]
mod windows_player {
    use std::io::Cursor;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use rodio::source::Source as _;
    use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

    use super::super::{Clip, PlayLength};

    /// The open device and whatever it is currently playing.
    pub(in crate::audio) struct ClipPlayer {
        device: MixerDeviceSink,
        /// Raised by the stream's own error callback — the device went away,
        /// the driver refused a buffer — from rodio's thread. Read before every
        /// cue: a device that has failed since it was opened is dropped and
        /// reopened rather than trusted, so "the alarm was handed to the
        /// device" is never claimed about a device that is no longer there.
        faulted: Arc<AtomicBool>,
        /// The queue of the newest batch. Replacing it drops the old
        /// [`Player`], and a dropped player stops — which is how a fresh cue
        /// silences a clip still running from the last one.
        current: Option<Player>,
    }

    impl ClipPlayer {
        /// Open the default output device. The reason a device could not be
        /// opened goes to the log in full; the sink's caller gets the sentence
        /// a toast can show.
        pub(in crate::audio) fn open() -> Result<Self, &'static str> {
            let faulted = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&faulted);
            let opened = DeviceSinkBuilder::from_default_device().and_then(|builder| {
                builder
                .with_error_callback(move |error| {
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "AUDIO_DEVICE_FAULTED",
                        error = %error,
                        "the alarm output device reported an error; it is reopened on the next cue"
                    );
                    flag.store(true, Ordering::Relaxed);
                })
                .open_sink_or_fallback()
            });
            let mut device = opened.map_err(|error| {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "AUDIO_DEVICE_UNAVAILABLE",
                    error = %error,
                    "no audio output device could be opened for the alarm clips"
                );
                "no audio output device could be opened"
            })?;
            // The device lives as long as the app; the goodbye rodio prints on
            // drop is noise in a log that already says the app is closing.
            device.log_on_drop(false);
            Ok(Self {
                device,
                faulted,
                current: None,
            })
        }

        /// Whether the device has reported an error since it was opened.
        pub(in crate::audio) fn is_faulted(&self) -> bool {
            self.faulted.load(Ordering::Relaxed)
        }

        /// Silence whatever is still sounding. What a batch with no clip of its
        /// own asks for: a platform beep is the newest signal, and it is not
        /// heard under a rainforest.
        pub(in crate::audio) fn stop(&mut self) {
            self.current = None;
        }

        /// Queue these clips, in order, replacing whatever is still sounding.
        ///
        /// Every clip is decoded (its header, that is) before any is played,
        /// so a batch either plays whole or, on the one clip that will not
        /// decode, leaves the previous batch alone and reports.
        pub(in crate::audio) fn play(
            &mut self,
            clips: &[(&'static Clip, PlayLength)],
        ) -> Result<(), &'static str> {
            let sources = clips
                .iter()
                .map(|(clip, length)| cue_source(clip, *length))
                .collect::<Result<Vec<_>, _>>()?;
            let player = Player::connect_new(self.device.mixer());
            for source in sources {
                player.append(source);
            }
            self.current = Some(player);
            Ok(())
        }
    }

    /// A clip as something the mixer can pull samples from, cut where the
    /// length says.
    ///
    /// Its own function so the cut can be asserted by decoding — the rodio
    /// `Source` is the same object the device plays, with no device in sight.
    pub(in crate::audio) fn cue_source(
        clip: &'static Clip,
        length: PlayLength,
    ) -> Result<Box<dyn rodio::Source + Send>, &'static str> {
        let decoder = rodio::Decoder::new(Cursor::new(clip.bytes)).map_err(|error| {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "AUDIO_CLIP_UNDECODABLE",
                clip = clip.token,
                error = %error,
                "an alarm clip shipped in this build does not decode"
            );
            "the alarm clip could not be decoded"
        })?;
        Ok(match length {
            PlayLength::Whole => Box::new(decoder),
            PlayLength::Capped(duration) => Box::new(decoder.take_duration(duration)),
        })
    }

    #[cfg(test)]
    mod tests {
        use std::time::Duration;

        use super::*;
        use crate::audio::library::CLIPS;
        use crate::audio::{AlertSound, SoundCategory};

        /// How long a source runs, measured by pulling every sample out of it
        /// — the ground truth a `total_duration` header can only estimate.
        fn measured_length(mut source: Box<dyn rodio::Source + Send>) -> Duration {
            let rate = u64::from(source.sample_rate().get());
            let channels = u64::from(source.channels().get());
            let mut samples: u64 = 0;
            while source.next().is_some() {
                samples += 1;
            }
            Duration::from_secs_f64(samples as f64 / (rate * channels) as f64)
        }

        /// How far a measured length may sit from the one asked for: the
        /// decoder hands out whole frames, and a frame is a few milliseconds.
        const SLACK: Duration = Duration::from_millis(50);

        /// The cap is a cut, not a hint: a clip asked for two seconds runs two
        /// seconds of samples, and one asked for whole runs longer than that.
        /// Measured by decoding, on the same `Source` the device would play —
        /// the proof that "how many seconds the alarm plays" means what it
        /// says, without anyone listening.
        #[test]
        fn a_capped_cue_is_cut_where_the_length_says() {
            // The first standard clip, whatever its name: this test is about
            // the cut, not about which clip is on the shelf.
            let AlertSound::Clip(id) = AlertSound::in_category(SoundCategory::Standard)
                .next()
                .expect("the standard folder has a clip")
            else {
                panic!("a standard-category sound is a clip");
            };
            let clip = id.clip();
            let whole = measured_length(cue_source(clip, PlayLength::Whole).expect("decodes"));
            assert!(
                whole > Duration::from_secs(3),
                "the whole clip is longer than the cap under test: {whole:?}"
            );

            let capped =
                measured_length(cue_source(clip, PlayLength::seconds(2)).expect("decodes"));
            let target = Duration::from_secs(2);
            assert!(
                capped.abs_diff(target) <= SLACK,
                "cut at two seconds, measured {capped:?}"
            );

            // A cap past the end changes nothing.
            let generous =
                measured_length(cue_source(clip, PlayLength::seconds(3_600)).expect("decodes"));
            assert!(
                generous.abs_diff(whole) <= SLACK,
                "a cap longer than the clip plays it whole: {generous:?} vs {whole:?}"
            );
        }

        /// Every clip in the shipped library decodes with the decoder this
        /// build carries: an alarm that cannot decode is one the trader would
        /// discover on the bar that mattered. Probing the header and the first
        /// samples is enough to catch a wrong codec or a file that is not what
        /// its name says; the full decode of one clip is the test above.
        #[test]
        fn every_shipped_clip_decodes() {
            for clip in CLIPS {
                let mut source = cue_source(clip, PlayLength::Whole)
                    .unwrap_or_else(|reason| panic!("{}: {reason}", clip.token));
                assert!(source.sample_rate().get() > 0, "{}", clip.token);
                assert!(source.channels().get() > 0, "{}", clip.token);
                assert_eq!(
                    source.by_ref().take(1_000).count(),
                    1_000,
                    "{} yields fewer than a thousand samples",
                    clip.token
                );
            }
        }
    }
}

#[cfg(windows)]
pub(in crate::audio) use windows_player::ClipPlayer;

/// The stub every platform but Windows compiles: no device, no decoder,
/// and an honest refusal for every clip.
#[cfg(not(windows))]
use super::{Clip, PlayLength};

#[cfg(not(windows))]
pub(in crate::audio) struct ClipPlayer;

#[cfg(not(windows))]
impl ClipPlayer {
    pub(in crate::audio) fn open() -> Result<Self, &'static str> {
        Err("this build has no audio backend, so an alarm clip cannot be played")
    }

    pub(in crate::audio) fn is_faulted(&self) -> bool {
        false
    }

    pub(in crate::audio) fn stop(&mut self) {}

    pub(in crate::audio) fn play(
        &mut self,
        clips: &[(&'static Clip, PlayLength)],
    ) -> Result<(), &'static str> {
        let _ = clips;
        Err("this build has no audio backend, so an alarm clip cannot be played")
    }
}
