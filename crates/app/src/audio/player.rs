//! Playing the library's clips through the default output device.
//!
//! The one place the app touches an audio device, and it does so late: the
//! device is opened by the first clip that needs it, never at start-up.
//! Decoding is lazy too — [`rodio`] pulls samples out of the AAC decoder
//! from its own mixer thread as the device asks for them — so a cue costs
//! this thread a header probe and a queue push, whatever the clip's
//! length. Nothing here runs per frame.

use std::io::Cursor;

use rodio::source::Source as _;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

use super::{Clip, PlayLength};

/// The open device and whatever it is currently playing.
pub(super) struct ClipPlayer {
    device: MixerDeviceSink,
    /// The queue of the newest batch. Replacing it drops the old
    /// [`Player`], and a dropped player stops — which is how a fresh cue
    /// silences a clip still running from the last one.
    current: Option<Player>,
}

impl ClipPlayer {
    /// Open the default output device. The reason a device could not be
    /// opened goes to the log in full; the sink's caller gets the sentence
    /// a toast can show.
    pub(super) fn open() -> Result<Self, &'static str> {
        let mut device = DeviceSinkBuilder::open_default_sink().map_err(|error| {
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
            current: None,
        })
    }

    /// Queue these clips, in order, replacing whatever is still sounding.
    ///
    /// Every clip is decoded (its header, that is) before any is played,
    /// so a batch either plays whole or, on the one clip that will not
    /// decode, leaves the previous batch alone and reports.
    pub(super) fn play(
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
pub(super) fn cue_source(
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
    use crate::audio::ClipId;
    use crate::audio::library::CLIPS;

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

    /// The cap is a cut, not a hint: a clip asked for two seconds runs two
    /// seconds of samples, and one asked for whole runs longer than that.
    /// Measured by decoding, on the same `Source` the device would play —
    /// the proof that "how many seconds the alarm plays" means what it
    /// says, without anyone listening.
    #[test]
    fn a_capped_cue_is_cut_where_the_length_says() {
        let clip = ClipId::from_token("cuckoo")
            .expect("the cuckoo is in the library")
            .clip();
        let whole = measured_length(cue_source(clip, PlayLength::Whole).expect("decodes"));
        assert!(
            whole > Duration::from_secs(3),
            "the whole clip is longer than the cap under test: {whole:?}"
        );

        let capped = measured_length(cue_source(clip, PlayLength::seconds(2)).expect("decodes"));
        let target = Duration::from_secs(2);
        let slack = Duration::from_millis(50);
        assert!(
            capped >= target - slack && capped <= target + slack,
            "cut at two seconds, measured {capped:?}"
        );

        // A cap past the end changes nothing.
        let generous =
            measured_length(cue_source(clip, PlayLength::seconds(3_600)).expect("decodes"));
        assert!(
            generous.abs_diff(whole) <= slack,
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
