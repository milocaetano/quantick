//! Neutral sound delivery and existing per-frame refusal policy.
use crate::sound::Cue;

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

/// Where a signal alarm is played, and why the last one was not.
///
/// The two travel together because a sink that refused is only ever reported
/// through the failure beside it: an alarm the trader never heard is never
/// assumed heard.
pub struct AlertState {
    /// Where signal alarms are played. The shipped sink is the platform's
    /// own sounds; a test swaps in a recorder, which is how "the alarm
    /// sounded, once, and it was the sound the preset named" is asserted
    /// without a build machine making noise.
    pub alerts: Box<dyn AlertSink>,

    /// The last reason a sound could not be played, shown once in the
    /// dialog. A build with no audio backend, or a platform that refused,
    /// is reported: an alarm the trader never heard is never assumed heard.
    pub alert_failure: Option<String>,
}

impl AlertState {
    /// Play `cues` and record whether they reached the trader. The text to
    /// post on the window's acknowledgement lane, on the first refusal of a
    /// run only.
    pub fn play(&mut self, cues: &[Cue]) -> Option<String> {
        let outcome = self.alerts.play(cues);
        self.report(outcome)
    }

    /// Play the alarm cues every tab's armed instances asked for this
    /// frame, and empty their queues.
    ///
    /// Every tab, not only the active one: a tab the trader is not looking
    /// at keeps its feed running and its instances judging, and an alarm
    /// exists precisely to be heard when the eyes are elsewhere.
    ///
    /// One cue per *distinct* cue per frame, across every tab. The kernel's
    /// repeat rule has already thinned each instance's stream to one per
    /// bar (or one per cooldown); this is the second, blunter guard, for
    /// the frame that ingested a burst of prints and closed several bars at
    /// once — four identical beeps stacked into one instant are one noise,
    /// not four alarms.
    ///
    /// Deduplicating by *cue* rather than collapsing to one is the whole
    /// point of letting a preset choose a sound: a trader who gave two
    /// regions two sounds did it to tell them apart, and swallowing the
    /// second because it shared a frame with the first would hide a signal
    /// and leave no trace that it had. The frame's cues go to the sink as
    /// one batch, which plays them in order — so the second is heard after
    /// the first rather than instead of it. The set of sounds is small and
    /// fixed, so this is bounded by the catalogue however busy the tape.
    ///
    /// Per frame, but cheap: the walk is over a handful of tabs whose
    /// queues are empty on every frame but the one a signal happened on,
    /// and the sink is only asked when something is queued.
    pub fn play_pending(&mut self, cues: impl IntoIterator<Item = Cue>) -> Option<String> {
        // Order of first request, duplicates dropped — `dedup` alone would
        // only collapse neighbours.
        let mut distinct: Vec<Cue> = Vec::new();
        for cue in cues {
            if !distinct.contains(&cue) {
                distinct.push(cue);
            }
        }
        if distinct.is_empty() {
            return None;
        }
        self.play(&distinct)
    }

    /// Record whether a sound actually reached the trader.
    ///
    /// A notification that never arrived is reported, never assumed — so a
    /// first failure raises a toast rather than waiting for the trader to
    /// reopen the arming dialog, which they may never do. Only the *first*
    /// of a run: a build with no audio backend fails on every alarm, and a
    /// toast per bar would be its own noise. A success clears the reason, so
    /// one transient refusal does not leave a permanent red line behind it.
    fn report(&mut self, outcome: Result<(), &'static str>) -> Option<String> {
        match outcome {
            Ok(()) => {
                self.alert_failure = None;
                None
            }
            Err(reason) => {
                let first = self.alert_failure.as_deref() != Some(reason);
                self.alert_failure = Some(reason.to_owned());
                first.then(|| format!("no alarm sound was played: {reason}"))
            }
        }
    }
}
