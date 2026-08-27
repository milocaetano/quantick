//! The signal alarm: telling the trader a setup happened, in time to act on
//! it somewhere else.
//!
//! The alarm exists because the trader's hands are not always on this
//! platform. They watch the region here and execute on their broker's
//! terminal, so what they need is not an order — it is *notice*, early
//! enough to reach the other screen. That single fact shapes every rule in
//! this module:
//!
//! - **It fires on the signal, never on the order.** An instance whose
//!   account is busy, whose one shot is spent, or which places no orders at
//!   all still alarms: those gates silence an *order*, and the opportunity
//!   they silence is exactly the one the trader wants to take elsewhere.
//! - **It may fire before the bar closes.** [`AlarmWhen::AtShare`] judges
//!   the bar still forming, once it has run through a share of its closing
//!   measure — "on a 2000-tick chart, start looking past tick 1400". That
//!   head start is the whole point, and it is why the alarm is a separate
//!   surface from the strategy: the [`Trigger`] port judges closed bars
//!   only, and nothing here changes that.
//! - **A mid-bar reading is provisional and says so.** A bar that qualifies
//!   at 70% can stop qualifying by its close. The alarm reports that
//!   outcome ([`AlarmEvent::Faded`]) instead of letting the trader believe
//!   a signal held. Inferred data is labelled, never silently patched.
//! - **It never repeats itself into noise.** One of the two
//!   [`RepeatPolicy`] rules is always in force, so a mid-bar alarm cannot
//!   sound once per print.
//!
//! Like the rest of the kernel this module reads no clock: the caller
//! passes its own milliseconds in, exactly as `quantick-replay` is *told*
//! how much time passed. Same inputs in → same events out.
//!
//! [`Trigger`]: crate::Trigger

use quantick_engine::BarProgress;
use rust_decimal::Decimal;

/// When the alarm is allowed to sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmWhen {
    /// Only on a closed bar — the same instant the strategy itself judges.
    /// No head start, and no provisional readings to correct later.
    OnClose,
    /// From the moment the forming bar has run through `share` of its
    /// closing measure, and again at its close. The bar is judged as it
    /// stands, so the reading is provisional until the bar closes.
    AtShare {
        /// Fraction of the bar's closing measure, in `0.0..=1.0`. Build it
        /// through [`AlarmWhen::at_share`], which clamps: a hand-edited
        /// `3.5` must not quietly mean "never".
        share: Decimal,
    },
}

impl AlarmWhen {
    /// A share gate with the fraction clamped into `0.0..=1.0`.
    #[must_use]
    pub fn at_share(share: Decimal) -> Self {
        Self::AtShare {
            share: share.clamp(Decimal::ZERO, Decimal::ONE),
        }
    }
}

/// How often the alarm may sound. One of the two is always in force: an
/// alarm judged mid-bar is judged on every print, and an alarm with no
/// repeat rule would sound on every one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatPolicy {
    /// At most one sound per bar. The quiet default.
    OncePerBar,
    /// At most one sound per `millis`, counted across bars.
    Cooldown { millis: u64 },
}

/// What the alarm decided about one bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmEvent {
    /// The forming bar qualifies, and the alarm sounded. **Provisional**:
    /// this bar has not closed and may yet stop qualifying.
    Preview,
    /// A closed bar qualifies, and the alarm sounded.
    Confirmed,
    /// A closed bar qualifies, but the repeat policy had already spent this
    /// bar's sound on its preview. Nothing is heard; the preview held, and
    /// the chart drops the provisional label.
    ConfirmedQuietly,
    /// A bar that had raised a preview closed **without** qualifying. Also
    /// silent — the trader already heard that alarm, and a second sound
    /// would announce a new opportunity, which this is the opposite of. It
    /// is reported so the chart can withdraw what it showed.
    Faded,
}

impl AlarmEvent {
    /// Whether this event is one the trader hears. Only a fresh
    /// opportunity makes a sound; corrections, and confirmations of an
    /// alarm already heard, are shown rather than played.
    #[must_use]
    pub fn sounds(self) -> bool {
        matches!(self, Self::Preview | Self::Confirmed)
    }

    /// Whether this event leaves a **provisional** judgement standing on
    /// the chart — a signal announced from a bar that has not closed.
    #[must_use]
    pub fn is_provisional(self) -> bool {
        matches!(self, Self::Preview)
    }
}

/// The alarm's configuration, as a preset stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlarmParams {
    pub when: AlarmWhen,
    pub repeat: RepeatPolicy,
}

impl Default for AlarmParams {
    /// The quiet reading of every option: no head start, one sound per bar.
    fn default() -> Self {
        Self {
            when: AlarmWhen::OnClose,
            repeat: RepeatPolicy::OncePerBar,
        }
    }
}

/// The alarm's running state for one armed instance.
#[derive(Debug, Clone)]
pub struct SignalAlarm {
    params: AlarmParams,
    /// Whether this bar's sound is spent (the [`RepeatPolicy::OncePerBar`]
    /// budget).
    sounded_this_bar: bool,
    /// The caller's clock reading at the last sound, for the cooldown.
    last_sound_ms: Option<u64>,
    /// Whether a preview alarm on the bar now forming is still outstanding.
    previewed_this_bar: bool,
}

impl SignalAlarm {
    #[must_use]
    pub fn new(params: AlarmParams) -> Self {
        Self {
            params,
            sounded_this_bar: false,
            last_sound_ms: None,
            previewed_this_bar: false,
        }
    }

    #[must_use]
    pub fn params(&self) -> &AlarmParams {
        &self.params
    }

    /// Whether the forming bar is worth judging *at all* right now.
    ///
    /// The caller asks this before doing any trigger or region work,
    /// because the mid-bar path runs once per print and the work it guards
    /// is not free. It is the same predicate [`Self::on_forming`] applies,
    /// so skipping the check can only cost time, never change an outcome —
    /// the two cannot drift, because there is one of them.
    #[must_use]
    pub fn wants_forming_check(&self, progress: Option<BarProgress>, now_ms: u64) -> bool {
        self.share_reached(progress) && self.may_sound(now_ms)
    }

    /// Judge the bar currently forming. `qualifies` is the shared
    /// opportunity test's answer for that bar as it stands.
    pub fn on_forming(
        &mut self,
        qualifies: bool,
        progress: Option<BarProgress>,
        now_ms: u64,
    ) -> Option<AlarmEvent> {
        if !qualifies || !self.wants_forming_check(progress, now_ms) {
            return None;
        }
        self.mark_sounded(now_ms);
        self.previewed_this_bar = true;
        Some(AlarmEvent::Preview)
    }

    /// Judge a bar that has closed. Call for **every** closed bar, not only
    /// the qualifying ones: a preview that never confirmed has to be
    /// reported, and this bar's repeat budget has to be handed on.
    pub fn on_closed(&mut self, qualifies: bool, now_ms: u64) -> Option<AlarmEvent> {
        let event = if qualifies {
            if self.may_sound(now_ms) {
                self.mark_sounded(now_ms);
                Some(AlarmEvent::Confirmed)
            } else {
                Some(AlarmEvent::ConfirmedQuietly)
            }
        } else if self.previewed_this_bar {
            Some(AlarmEvent::Faded)
        } else {
            None
        };
        self.sounded_this_bar = false;
        self.previewed_this_bar = false;
        event
    }

    /// Forget everything: the series this alarm was judging no longer
    /// exists. Follows the trigger's own [`reset`](crate::Trigger::reset) —
    /// a cooldown counted against a tape that has been rebuilt is not a
    /// cooldown about anything.
    pub fn reset(&mut self) {
        self.sounded_this_bar = false;
        self.last_sound_ms = None;
        self.previewed_this_bar = false;
    }

    /// Whether a provisional alarm is standing on the bar now forming.
    #[must_use]
    pub fn preview_outstanding(&self) -> bool {
        self.previewed_this_bar
    }

    /// Has the forming bar run far enough into its measure to be judged?
    ///
    /// `None` progress is a bar rule running toward no fixed threshold — an
    /// adaptive rule. There is no honest percentage of a target that does
    /// not exist, so the share gate never opens and the alarm falls back to
    /// closed bars. Reporting a share of an invented target would be the
    /// countdown lie [`BarProgress`] exists to avoid.
    fn share_reached(&self, progress: Option<BarProgress>) -> bool {
        let AlarmWhen::AtShare { share } = self.params.when else {
            return false;
        };
        let Some(progress) = progress else {
            return false;
        };
        if progress.target <= Decimal::ZERO {
            return false;
        }
        progress.done / progress.target >= share
    }

    fn may_sound(&self, now_ms: u64) -> bool {
        match self.params.repeat {
            RepeatPolicy::OncePerBar => !self.sounded_this_bar,
            RepeatPolicy::Cooldown { millis } => match self.last_sound_ms {
                None => true,
                Some(last) => now_ms.saturating_sub(last) >= millis,
            },
        }
    }

    fn mark_sounded(&mut self, now_ms: u64) {
        self.sounded_this_bar = true;
        self.last_sound_ms = Some(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(done: i64, target: i64) -> Option<BarProgress> {
        Some(BarProgress {
            done: Decimal::from(done),
            target: Decimal::from(target),
        })
    }

    fn share(percent: i64) -> AlarmWhen {
        AlarmWhen::at_share(Decimal::new(percent, 2))
    }

    fn at_share_once_per_bar(percent: i64) -> SignalAlarm {
        SignalAlarm::new(AlarmParams {
            when: share(percent),
            repeat: RepeatPolicy::OncePerBar,
        })
    }

    /// The trader's own example: a 2000-tick chart set to 70% starts
    /// judging past tick 1400, and not one print earlier.
    #[test]
    fn a_seventy_percent_gate_opens_at_tick_1400_of_2000() {
        let mut alarm = at_share_once_per_bar(70);
        assert_eq!(alarm.on_forming(true, progress(1399, 2000), 0), None);
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 0),
            Some(AlarmEvent::Preview)
        );
    }

    /// The cheap gate the per-print path consults and the judgement itself
    /// answer the same question — they are one predicate, so a caller that
    /// skips the check cannot get a different outcome, only a slower one.
    #[test]
    fn the_cheap_gate_and_the_judgement_never_disagree() {
        for done in [0_i64, 699, 700, 1399, 1400, 2000] {
            let checked = at_share_once_per_bar(70);
            let mut unchecked = at_share_once_per_bar(70);
            let wants = checked.wants_forming_check(progress(done, 2000), 0);
            let judged = unchecked.on_forming(true, progress(done, 2000), 0);
            assert_eq!(
                wants,
                judged.is_some(),
                "the gate said {wants} and the judgement said {judged:?} at {done}/2000"
            );
        }
    }

    /// A bar rule running toward no fixed threshold has no honest share, so
    /// the gate stays shut and the alarm waits for the close.
    #[test]
    fn an_adaptive_bar_rule_never_opens_the_share_gate() {
        let mut alarm = at_share_once_per_bar(70);
        assert!(!alarm.wants_forming_check(None, 0));
        assert_eq!(alarm.on_forming(true, None, 0), None);
        // Nor does a rule reporting a zero target — a share of nothing.
        assert_eq!(alarm.on_forming(true, progress(10, 0), 0), None);
        // The close still speaks.
        assert_eq!(alarm.on_closed(true, 0), Some(AlarmEvent::Confirmed));
    }

    /// An on-close alarm never judges a forming bar, however far along it
    /// is: no head start was asked for, so none is taken.
    #[test]
    fn an_on_close_alarm_ignores_the_forming_bar() {
        let mut alarm = SignalAlarm::new(AlarmParams::default());
        assert!(!alarm.wants_forming_check(progress(1999, 2000), 0));
        assert_eq!(alarm.on_forming(true, progress(1999, 2000), 0), None);
        assert_eq!(alarm.on_closed(true, 0), Some(AlarmEvent::Confirmed));
    }

    /// The noise guard: a mid-bar alarm is judged on every print, and one
    /// sound per bar means one sound, however many prints agree.
    #[test]
    fn once_per_bar_sounds_once_however_many_prints_qualify() {
        let mut alarm = at_share_once_per_bar(70);
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 0),
            Some(AlarmEvent::Preview)
        );
        for print in 1401..1500 {
            assert_eq!(alarm.on_forming(true, progress(print, 2000), 0), None);
        }
        // The close confirms it, silently: this bar's sound is spent.
        assert_eq!(alarm.on_closed(true, 0), Some(AlarmEvent::ConfirmedQuietly));
        // The next bar starts with a fresh budget.
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 0),
            Some(AlarmEvent::Preview)
        );
    }

    /// The other repeat rule: seconds, counted across bars, on the caller's
    /// own clock — the kernel reads none.
    #[test]
    fn a_cooldown_counts_the_callers_milliseconds_across_bars() {
        let mut alarm = SignalAlarm::new(AlarmParams {
            when: share(70),
            repeat: RepeatPolicy::Cooldown { millis: 30_000 },
        });
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 1_000),
            Some(AlarmEvent::Preview)
        );
        assert_eq!(alarm.on_forming(true, progress(1500, 2000), 20_000), None);
        // The bar closing does not reset a cooldown; that is the point of
        // having one that is not `OncePerBar`.
        assert_eq!(
            alarm.on_closed(true, 25_000),
            Some(AlarmEvent::ConfirmedQuietly)
        );
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 31_000),
            Some(AlarmEvent::Preview)
        );
    }

    /// The honesty rule: a bar that qualified at 70% and stopped qualifying
    /// by its close is reported, and reported *silently* — the trader heard
    /// the alarm already, and a second sound would announce an opportunity
    /// rather than withdraw one.
    #[test]
    fn a_preview_that_does_not_hold_is_reported_and_makes_no_sound() {
        let mut alarm = at_share_once_per_bar(70);
        let preview = alarm
            .on_forming(true, progress(1400, 2000), 0)
            .expect("the gate is open and the bar qualifies");
        assert!(preview.sounds());
        assert!(preview.is_provisional());
        assert!(alarm.preview_outstanding());

        let faded = alarm
            .on_closed(false, 100)
            .expect("a preview that did not hold is reported");
        assert_eq!(faded, AlarmEvent::Faded);
        assert!(!faded.sounds());
        assert!(!faded.is_provisional());
        assert!(!alarm.preview_outstanding());
    }

    /// A bar nobody previewed and which does not qualify is simply not an
    /// event: silence is not a report.
    #[test]
    fn an_ordinary_bar_reports_nothing() {
        let mut alarm = at_share_once_per_bar(70);
        assert_eq!(alarm.on_forming(false, progress(1800, 2000), 0), None);
        assert_eq!(alarm.on_closed(false, 0), None);
    }

    /// Only a fresh opportunity is heard; a confirmation of one already
    /// heard, and a withdrawal, are shown.
    #[test]
    fn only_fresh_opportunities_make_a_sound() {
        assert!(AlarmEvent::Preview.sounds());
        assert!(AlarmEvent::Confirmed.sounds());
        assert!(!AlarmEvent::ConfirmedQuietly.sounds());
        assert!(!AlarmEvent::Faded.sounds());
        assert!(AlarmEvent::Preview.is_provisional());
        assert!(!AlarmEvent::Confirmed.is_provisional());
    }

    /// A hand-edited share outside `0..=1` is clamped rather than read as
    /// "never" (or as "every print").
    #[test]
    fn an_out_of_range_share_is_clamped_not_obeyed() {
        assert_eq!(
            AlarmWhen::at_share(Decimal::new(35, 1)),
            AlarmWhen::AtShare {
                share: Decimal::ONE
            }
        );
        assert_eq!(
            AlarmWhen::at_share(Decimal::new(-5, 1)),
            AlarmWhen::AtShare {
                share: Decimal::ZERO
            }
        );
    }

    /// A rebuilt series takes the cooldown with it: time counted against a
    /// tape that no longer exists is not time this alarm should wait out.
    #[test]
    fn a_reset_forgets_the_cooldown_and_the_outstanding_preview() {
        let mut alarm = SignalAlarm::new(AlarmParams {
            when: share(70),
            repeat: RepeatPolicy::Cooldown { millis: 30_000 },
        });
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 1_000),
            Some(AlarmEvent::Preview)
        );
        assert!(alarm.preview_outstanding());
        alarm.reset();
        assert!(!alarm.preview_outstanding());
        assert_eq!(
            alarm.on_forming(true, progress(1400, 2000), 2_000),
            Some(AlarmEvent::Preview),
            "the cooldown went with the series it was counted against"
        );
    }
}
