//! Armed strategy instances anchored to drawings.
//!
//! The kernel owns the judgement (trigger, region test, state machine);
//! this module owns the *attachment*: which drawing carries which armed
//! instance, how simulator events fan out to them, and when an instance
//! dies with its drawing. A drawing may carry several instances: each
//! steps independently, in creation order, and the drawing's one badge
//! speaks for the one with the most urgent news and counts the rest, so a
//! rectangle never hides a stack of bots behind a badge that names one.
//!
//! Generic over the anchor `K`: the window anchors to its drawing id, and
//! nothing here needs to know what a drawing is beyond telling two apart.

use quantick_engine::{Bar, BarProgress};
use quantick_sim::Command;

use crate::presets::StoredPreset;
use crate::sound::Cue;
use crate::{
    AlarmEvent, ArmedState, ArmedStrategy, DisarmReason, Execution, Placement, SignalAlarm,
};

/// What the last alarm judgement left standing over the drawing.
///
/// The chart shows this beside the badge, because a mid-bar alarm is a
/// claim about a bar that has not finished: the trader who heard it is owed
/// both the word "preview" while it stands and the correction when it turns
/// out not to have held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlarmMark {
    /// Nothing to say.
    #[default]
    Quiet,
    /// A provisional signal, announced from the bar still forming.
    Preview,
    /// The bar closed and the signal held.
    Confirmed,
    /// The bar closed and the previewed signal did **not** hold. Shown
    /// until the next judgement replaces it: a trader who left the desk on
    /// the sound comes back to the reason it meant nothing.
    Faded,
}

impl AlarmMark {
    /// The words the chart shows, or `None` when there is nothing to add.
    #[must_use]
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Quiet => None,
            Self::Preview => Some("signal (preview)"),
            Self::Confirmed => Some("signal"),
            Self::Faded => Some("preview did not hold"),
        }
    }
}

/// One armed strategy riding one drawing.
pub struct AnchoredInstance<K> {
    /// The drawing this instance rides.
    pub drawing: K,
    /// The preset name the badge and tooltip show ("BF compra 1x1").
    pub preset: String,
    /// The stored form this instance was compiled from.
    ///
    /// Kept so a *copy* of the drawing can be armed through the same door
    /// the dialog uses — `StoredPreset::to_kernel` — rather than through a
    /// second construction path that would drift from it. The compiled
    /// halves (`params`, the trigger, the alarm) cannot be read back out of
    /// a running instance: `ArmedStrategy` hands out `&dyn Trigger`, which
    /// deliberately has no way to surrender its own parameters.
    pub spec: StoredPreset,
    pub armed: ArmedStrategy,
    /// The signal alarm, when the preset asked for one. `None` is the
    /// silent instance every preset written before the alarm existed
    /// compiles to.
    pub alarm: Option<SignalAlarm>,
    /// What this instance's alarm plays, and for how long.
    pub cue: Cue,
    /// What the last alarm judgement left on the chart.
    pub mark: AlarmMark,
}

impl<K> AnchoredInstance<K> {
    /// Whether this instance places orders at all.
    #[must_use]
    pub fn alarm_only(&self) -> bool {
        self.armed.params().execution == Execution::AlarmOnly
    }

    /// Whether this instance is still *listening*.
    ///
    /// The alarm deliberately outlives the states that only mean the
    /// instance cannot trade right now — a busy account, a pending entry, a
    /// spent one shot — because the setup still happened and the trader is
    /// acting on it elsewhere. It does **not** outlive a disarm. Every
    /// [`DisarmReason`] is either the trader saying "stop watching" or the
    /// series being rebuilt underneath the ruler, and a badge that reads
    /// `disarmed` over a chart that keeps beeping is the button lying.
    fn listening(&self) -> bool {
        !matches!(self.armed.state(), ArmedState::Disarmed { .. })
    }

    /// Whether this instance is armed or mid-operation — what the drawing's
    /// Disarm reaches. An ended instance is left alone: it keeps the reason
    /// it ended with, which a re-arm reads to know whether the series
    /// changed under the ruler.
    #[must_use]
    pub fn watching(&self) -> bool {
        matches!(
            self.armed.state(),
            ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition
        )
    }

    /// Whether the drawing's Re-arm reaches this instance.
    #[must_use]
    pub fn rearmable(&self) -> bool {
        matches!(
            self.armed.state(),
            ArmedState::Done | ArmedState::Disarmed { .. }
        )
    }

    /// How loudly this instance claims the drawing's one badge: an open
    /// position, then an order in flight, then a signal, then a stop the
    /// trader did not ask for (a rejected entry, a dropped bracket — the
    /// reason is the lesson), then plain watching, then an ended instance
    /// still showing a mark.
    fn urgency(&self) -> u8 {
        let marked = self.mark != AlarmMark::Quiet;
        match self.armed.state() {
            ArmedState::InPosition => 6,
            ArmedState::Fired { .. } => 5,
            ArmedState::Armed if marked => 4,
            ArmedState::Disarmed { reason } if *reason != DisarmReason::User => 3,
            ArmedState::Armed => 2,
            ArmedState::Done | ArmedState::Disarmed { .. } => u8::from(marked),
        }
    }

    /// Judge the bar that just closed, and answer with the sound to play.
    ///
    /// The opportunity comes from the kernel's own reading of that bar
    /// ([`ArmedStrategy::last_close_opportunity`]) rather than from whether
    /// an order went out, which is the whole distinction the alarm exists
    /// to draw: the trader is told their setup happened, not that this
    /// simulator acted on it.
    pub fn alarm_on_closed_bar(&mut self, now_ms: u64) -> Option<Cue> {
        if !self.listening() {
            // Silence *and* forget: a preview raised before the disarm has
            // no close coming that could withdraw it, so leaving the mark
            // standing would strand "signal (preview)" on the badge for the
            // rest of the session.
            self.reset_alarm();
            return None;
        }
        let qualifies = self.armed.last_close_opportunity().is_some();
        let event = self.alarm.as_mut()?.on_closed(qualifies, now_ms);
        self.apply(event)
    }

    /// Judge the bar still forming, and answer with the sound to play.
    ///
    /// Cheap first: the alarm's own gate is asked before the ruler and the
    /// region are, because this runs once per print. Everything past
    /// `wants_forming_check` is skipped on the overwhelming majority of
    /// prints — the bar is not far enough along, or the repeat rule has
    /// this bar's sound already spent.
    pub fn alarm_on_forming_bar(
        &mut self,
        bar: &Bar,
        placement: &dyn Placement,
        region_active: bool,
        progress: Option<BarProgress>,
        now_ms: u64,
    ) -> Option<Cue> {
        if !self.listening() {
            return None;
        }
        if !self.alarm.as_ref()?.wants_forming_check(progress, now_ms) {
            return None;
        }
        let qualifies = self
            .armed
            .preview_opportunity(bar, placement, region_active)
            .is_some();
        let event = self
            .alarm
            .as_mut()
            .expect("the alarm answered its own gate a moment ago")
            .on_forming(qualifies, progress, now_ms);
        self.apply(event)
    }

    /// Record what the chart should show, and hand back the sound — if any.
    /// A silent event still moves the mark: withdrawing a preview that did
    /// not hold is exactly the case where nothing is heard and something
    /// must be seen.
    fn apply(&mut self, event: Option<AlarmEvent>) -> Option<Cue> {
        let event = event?;
        self.mark = match event {
            AlarmEvent::Preview => AlarmMark::Preview,
            AlarmEvent::Confirmed | AlarmEvent::ConfirmedQuietly => AlarmMark::Confirmed,
            AlarmEvent::Faded => AlarmMark::Faded,
        };
        event.sounds().then_some(self.cue)
    }

    /// The series this instance was judging no longer exists.
    pub fn reset_alarm(&mut self) {
        if let Some(alarm) = self.alarm.as_mut() {
            alarm.reset();
        }
        self.mark = AlarmMark::Quiet;
    }
}

/// The instances riding one drawing, as its one badge and its menu read
/// them.
pub struct Stack<'a, K> {
    /// The instance the badge speaks for (see [`StrategyAnchors::for_drawing`]).
    pub speaker: &'a AnchoredInstance<K>,
    /// How many instances ride the drawing.
    pub riders: usize,
    /// Whether any of them the drawing's Disarm would reach.
    pub disarmable: bool,
    /// Whether any of them the drawing's Re-arm would reach.
    pub rearmable: bool,
    /// Whether any of them has a retest limit resting.
    pub retest_resting: bool,
}

impl<K> Stack<'_, K> {
    /// The words the chart prints beside the drawing.
    #[must_use]
    pub fn badge(&self) -> String {
        badge_text(self.speaker, self.riders)
    }
}

/// The pane's armed instances. A plain `Vec`: evaluation order is creation
/// order, stable and deterministic.
pub struct StrategyAnchors<K> {
    /// The armed instances, in creation order.
    pub instances: Vec<AnchoredInstance<K>>,
}

impl<K> Default for StrategyAnchors<K> {
    fn default() -> Self {
        Self {
            instances: Vec::new(),
        }
    }
}

impl<K: Copy + PartialEq> StrategyAnchors<K> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// The instance that speaks for `drawing` on its badge and its menu:
    /// the one with the most urgent news — an open position, an order in
    /// flight, a signal, plain watching, an ended one — the newest among
    /// equals, so a stacked bot's position or alarm is never hidden behind
    /// a quieter sibling's word.
    #[must_use]
    pub fn for_drawing(&self, drawing: K) -> Option<&AnchoredInstance<K>> {
        self.speaker(drawing).map(|index| &self.instances[index])
    }

    #[must_use]
    pub fn for_drawing_mut(&mut self, drawing: K) -> Option<&mut AnchoredInstance<K>> {
        self.speaker(drawing)
            .map(|index| &mut self.instances[index])
    }

    fn speaker(&self, drawing: K) -> Option<usize> {
        (self.instances.iter().enumerate())
            .filter(|(_, instance)| instance.drawing == drawing)
            .max_by_key(|(index, instance)| (instance.urgency(), *index))
            .map(|(index, _)| index)
    }

    /// The stack riding `drawing`, if any instance does.
    #[must_use]
    pub fn stack(&self, drawing: K) -> Option<Stack<'_, K>> {
        let speaker = self.for_drawing(drawing)?;
        let mut stack = Stack {
            speaker,
            riders: 0,
            disarmable: false,
            rearmable: false,
            retest_resting: false,
        };
        for instance in self.on_drawing(drawing) {
            stack.riders += 1;
            stack.disarmable |= instance.watching();
            stack.rearmable |= instance.rearmable();
            stack.retest_resting |= matches!(
                instance.armed.state(),
                ArmedState::Fired { retest: true, .. }
            );
        }
        Some(stack)
    }

    /// Every instance riding `drawing`, in creation order.
    pub fn on_drawing(&self, drawing: K) -> impl Iterator<Item = &AnchoredInstance<K>> {
        (self.instances.iter()).filter(move |instance| instance.drawing == drawing)
    }

    /// Every instance riding `drawing`, in creation order, mutably.
    pub fn on_drawing_mut(&mut self, drawing: K) -> impl Iterator<Item = &mut AnchoredInstance<K>> {
        (self.instances.iter_mut()).filter(move |instance| instance.drawing == drawing)
    }

    /// One stack per drawing, ordered by each drawing's first instance —
    /// what the chart paints one badge for.
    #[must_use]
    pub fn stacks(&self) -> Vec<Stack<'_, K>> {
        let mut drawings: Vec<K> = Vec::new();
        for instance in &self.instances {
            if !drawings.contains(&instance.drawing) {
                drawings.push(instance.drawing);
            }
        }
        (drawings.into_iter())
            .filter_map(|drawing| self.stack(drawing))
            .collect()
    }

    /// Attach `instance` beside any already riding its drawing. Nothing is
    /// replaced, so nothing is owed: each instance steps on its own and
    /// leaves only with its drawing or when the trader removes it.
    pub fn arm(&mut self, instance: AnchoredInstance<K>) {
        self.instances.push(instance);
    }

    /// Remove every instance riding `drawing`, whatever its state. The
    /// caller deleted the drawing or dismissed its strategies: a live
    /// operation's position and bracket stay in the simulator, now the
    /// human's to manage — the honest reading of "I deleted the bot". A
    /// *pending* entry is different — nothing filled, nobody owns it — so
    /// the returned commands sweep each one, in creation order; apply them
    /// to the simulator.
    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn remove_for_drawing(&mut self, drawing: K) -> Vec<Command> {
        let mut cleanup = Vec::new();
        self.instances.retain(|instance| {
            if instance.drawing != drawing {
                return true;
            }
            cleanup.extend(instance.armed.pending_entry_cancel());
            false
        });
        cleanup
    }

    /// Disarm every instance with one shared reason — the timeline reset /
    /// spec change / market switch sweeps. The returned commands sweep any
    /// pending entries (the kernel returns none under a timeline reset,
    /// where the simulator's own reset already cancels them by name).
    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn disarm_all(&mut self, reason: DisarmReason) -> Vec<Command> {
        let mut cleanup = Vec::new();
        for instance in &mut self.instances {
            cleanup.extend(instance.armed.disarm(reason));
            // A disarm that rebuilt the series takes the alarm's cooldown
            // and its outstanding preview with it: both were counted
            // against a tape that no longer exists.
            if reason.resets_series() {
                instance.reset_alarm();
            }
        }
        cleanup
    }

    /// Drop instances whose drawing no longer exists (deleted from any
    /// surface). Run once per evaluation sweep, never per frame. Sweeps
    /// pending entries like [`Self::remove_for_drawing`].
    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn drop_orphans(&mut self, exists: impl Fn(K) -> bool) -> Vec<Command> {
        let mut cleanup = Vec::new();
        self.instances.retain(|instance| {
            if exists(instance.drawing) {
                return true;
            }
            cleanup.extend(instance.armed.pending_entry_cancel());
            false
        });
        cleanup
    }

    /// How many instances are actively watching (armed or mid-operation) —
    /// what decides whether the paper host buffers print events.
    #[must_use]
    pub fn watching(&self) -> usize {
        self.instances
            .iter()
            .filter(|instance| instance.watching())
            .count()
    }
}

/// Badge text for the instance speaking for a drawing `riders` instances
/// ride — the on-chart label next to the drawing. More than one rider
/// puts the count on the bolt, so a second bot is never hidden behind
/// the first one's name. Colors are the paint site's business; words are
/// decided here so every surface says the same thing.
#[must_use]
pub fn badge_text<K>(instance: &AnchoredInstance<K>, riders: usize) -> String {
    // An alarm-only instance says so, because an idle `Armed` badge reads as
    // an ordinary bot the trader is still waiting on an order from. It is a
    // clause *beside* the state, never instead of it: such an instance is
    // not pinned to `Armed` — a hand on the Disarm entry, a timeline reset,
    // a bar-spec change or a market switch all move it — and the badge is
    // the one surface that would tell the trader their watcher is dead.
    let mode = if instance.alarm_only() {
        "alarm only"
    } else {
        ""
    };
    let state = match instance.armed.state() {
        ArmedState::Armed => "",
        ArmedState::Fired { retest: false, .. } => "fired",
        ArmedState::Fired { retest: true, .. } => "retest resting",
        ArmedState::InPosition => "in position",
        ArmedState::Done => "done",
        ArmedState::Disarmed { reason } => reason.label(),
    };
    // The alarm's own word comes last, where the eye lands after the name:
    // it is the most recent thing that happened, and the one a trader who
    // heard a sound is looking for.
    // The refusal is deliberately *not* joined here. Only the pane knows
    // whether the region itself is what is refusing, and its word replaces
    // the kernel's rather than sitting beside it — the pane's own badge
    // text, which is what the chart paints.
    let parts = [mode, state, instance.mark.label().unwrap_or_default()];
    let mut badge = if riders > 1 {
        format!("⚡×{riders} {}", instance.preset)
    } else {
        format!("⚡ {}", instance.preset)
    };
    for part in parts.into_iter().filter(|part| !part.is_empty()) {
        badge.push_str(" · ");
        badge.push_str(part);
    }
    badge
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::AlertSound;
    use crate::{ForceParams, ForceTrigger, Rearm, Region, StrategyParams};
    use quantick_engine::Side;
    use rust_decimal::Decimal;

    /// The window anchors to its drawing id; any copyable key will do here.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DrawingId(u64);

    type AnchoredInstance = super::AnchoredInstance<DrawingId>;
    type StrategyAnchors = super::StrategyAnchors<DrawingId>;

    fn instance(drawing: DrawingId) -> AnchoredInstance {
        AnchoredInstance {
            drawing,
            preset: "test".to_owned(),
            spec: StoredPreset::starting_point(Side::Buy),
            armed: ArmedStrategy::new(
                StrategyParams {
                    side: Side::Buy,
                    quantity: Decimal::ONE,
                    tp_mult: Decimal::ONE,
                    sl_mult: Decimal::ONE,
                    rearm: Rearm::OneShot,
                    on_break: crate::BreakPolicy::Ignore,
                    execution: Execution::Paper,
                },
                Box::new(ForceTrigger::new(ForceParams::default_band())),
            ),
            alarm: None,
            cue: Cue::default(),
            mark: AlarmMark::Quiet,
        }
    }

    #[test]
    fn instances_stack_on_a_drawing_and_orphans_die_with_their_drawing() {
        let mut anchors = StrategyAnchors::default();
        anchors.arm(instance(DrawingId(1)));
        anchors.arm(instance(DrawingId(2)));
        anchors.arm(instance(DrawingId(1)));
        assert_eq!(
            anchors.instances.len(),
            3,
            "re-arming stacked, not replaced"
        );
        assert_eq!(anchors.on_drawing(DrawingId(1)).count(), 2);

        let cleanup = anchors.drop_orphans(|id| id == DrawingId(2));
        assert_eq!(anchors.instances.len(), 1);
        assert_eq!(anchors.instances[0].drawing, DrawingId(2));
        assert!(cleanup.is_empty(), "armed instances have nothing pending");

        let cleanup = anchors.remove_for_drawing(DrawingId(2));
        assert!(anchors.is_empty());
        assert!(cleanup.is_empty());
    }

    /// An instance on `drawing` whose entry `order` is still pending: a
    /// 3-bar ruler that fired on the third bar (body 4 over average 2).
    fn pending(drawing: DrawingId, order: u64) -> AnchoredInstance {
        use quantick_sim::{Bracket, EntryKind, Order, OrderId, VenueEvent};

        let mut riding = instance(drawing);
        riding.armed = ArmedStrategy::new(
            riding.armed.params().clone(),
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: "1.5".parse().expect("fixture"),
                max_factor: "2.5".parse().expect("fixture"),
                min_range: Decimal::ZERO,
            })),
        );
        let region = Region::new(Decimal::from(100), Decimal::from(110));
        let _ = (riding.armed).on_closed_bar(&test_bar(100, 101), &region, true, true);
        let _ = (riding.armed).on_closed_bar(&test_bar(101, 102), &region, true, true);
        let commands = (riding.armed).on_closed_bar(&test_bar(102, 106), &region, true, true);
        assert_eq!(commands.len(), 1, "the fixture fires: {commands:?}");
        let _ = riding.armed.on_sim_events(&[VenueEvent::Placed(Order {
            id: OrderId(order),
            side: Side::Buy,
            kind: EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
            placed_ms: 0,
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        })]);
        riding
    }

    /// An instance whose entry is still pending leaves a cancel behind when
    /// its drawing (and with it the badge) goes away — a resting bot order
    /// with no bot is the one thing removal must not orphan.
    #[test]
    fn removal_sweeps_the_pending_entry() {
        use quantick_sim::{Command, OrderId};

        let mut anchors = StrategyAnchors::default();
        anchors.arm(pending(DrawingId(3), 11));
        let cleanup = anchors.remove_for_drawing(DrawingId(3));
        assert_eq!(
            cleanup,
            vec![Command::CancelOrder { id: OrderId(11) }],
            "removing the bot sweeps its pending entry"
        );
        assert!(anchors.is_empty());
    }

    /// Deleting a drawing takes every instance riding it, and sweeps each
    /// one's pending entry in creation order — never only the first bot's,
    /// and never a bot on another drawing.
    #[test]
    fn deleting_a_drawing_disarms_every_instance_on_it() {
        use quantick_sim::{Command, OrderId};

        let mut anchors = StrategyAnchors::default();
        anchors.arm(pending(DrawingId(5), 21));
        anchors.arm(pending(DrawingId(6), 31));
        anchors.arm(pending(DrawingId(5), 22));
        let cleanup = anchors.drop_orphans(|id| id != DrawingId(5));
        assert_eq!(
            cleanup,
            vec![
                Command::CancelOrder { id: OrderId(21) },
                Command::CancelOrder { id: OrderId(22) },
            ],
        );
        assert_eq!(anchors.instances.len(), 1, "the other drawing's bot stays");
        assert_eq!(anchors.instances[0].drawing, DrawingId(6));

        anchors.arm(pending(DrawingId(6), 32));
        let cleanup = anchors.remove_for_drawing(DrawingId(6));
        assert_eq!(
            cleanup,
            vec![
                Command::CancelOrder { id: OrderId(31) },
                Command::CancelOrder { id: OrderId(32) },
            ],
        );
        assert!(anchors.is_empty());
    }

    /// The drawing's one badge speaks for the instance with the most urgent
    /// news — newest among equals — and counts every rider; the stack says
    /// which of the drawing's verbs reach anyone.
    #[test]
    fn the_badge_speaks_for_the_most_urgent_instance_and_counts_the_stack() {
        let mut anchors = StrategyAnchors::default();
        let mut older = pending(DrawingId(1), 7);
        older.preset = "older".to_owned();
        anchors.arm(older);
        anchors.arm(instance(DrawingId(2)));
        let mut newer = instance(DrawingId(1));
        newer.preset = "newer".to_owned();
        anchors.arm(newer);
        let badges: Vec<_> = anchors.stacks().iter().map(Stack::badge).collect();
        assert_eq!(
            badges,
            ["⚡×2 older · fired", "⚡ test"],
            "an order in flight outranks a newer plain watcher"
        );

        // A signal on the plain watcher still loses to the order in
        // flight; once that ends, the signal speaks.
        anchors.instances[2].mark = AlarmMark::Confirmed;
        let stack = anchors.stack(DrawingId(1)).expect("two ride it");
        assert_eq!(stack.badge(), "⚡×2 older · fired");
        assert!(stack.disarmable, "the plain watcher can be disarmed");
        assert!(!stack.rearmable);
        let _ = anchors.instances[0].armed.disarm(DisarmReason::User);
        assert_eq!(
            anchors.stack(DrawingId(1)).expect("two ride it").badge(),
            "⚡×2 newer · signal"
        );

        // With none watching, the newest ended one speaks.
        let _ = anchors.instances[2].armed.disarm(DisarmReason::User);
        let stack = anchors.stack(DrawingId(1)).expect("two ride it");
        assert_eq!(stack.badge(), "⚡×2 newer · disarmed · signal");
        assert!(!stack.disarmable);
        assert!(stack.rearmable);
    }

    /// A sibling stopped by something the trader did not ask for keeps the
    /// badge over a plain watcher: its reason is the lesson, and a badge
    /// naming only the watcher would leave the Re-arm entry restarting it
    /// blind.
    #[test]
    fn an_unasked_stop_outranks_a_plain_watcher_on_the_badge() {
        let mut anchors = StrategyAnchors::default();
        anchors.arm(instance(DrawingId(1)));
        let mut newer = instance(DrawingId(1));
        newer.preset = "newer".to_owned();
        anchors.arm(newer);
        let _ = (anchors.instances[0].armed).disarm(DisarmReason::EntryCancelled);
        let stack = anchors.stack(DrawingId(1)).expect("two ride it");
        assert_eq!(stack.badge(), "⚡×2 test · entry cancelled");
        assert!(stack.disarmable && stack.rearmable);

        // The trader's own Disarm is no news: the watcher speaks again.
        let _ = (anchors.instances[0].armed).disarm(DisarmReason::User);
        let stack = anchors.stack(DrawingId(1)).expect("two ride it");
        assert_eq!(stack.badge(), "⚡×2 newer");
    }

    #[test]
    fn disarm_all_names_the_reason_and_watching_counts_live_states() {
        let mut anchors = StrategyAnchors::default();
        anchors.arm(instance(DrawingId(1)));
        anchors.arm(instance(DrawingId(2)));
        assert_eq!(anchors.watching(), 2);

        let cleanup = anchors.disarm_all(DisarmReason::TimelineReset);
        assert!(
            cleanup.is_empty(),
            "the simulator's reset owns those cancellations"
        );
        assert_eq!(anchors.watching(), 0);
        for i in &anchors.instances {
            assert_eq!(
                i.armed.state(),
                &ArmedState::Disarmed {
                    reason: DisarmReason::TimelineReset
                }
            );
        }
    }

    /// A 3-bar force ruler on a buy region, with the alarm the test asks
    /// for. The tape below is the one every other fixture here uses:
    /// bodies 1, 1, then 4 — ratio 2, closing inside the region.
    fn alarming_instance(
        when: crate::AlarmWhen,
        repeat: crate::RepeatPolicy,
        cue: Cue,
    ) -> AnchoredInstance {
        let mut instance = instance(DrawingId(9));
        instance.armed = ArmedStrategy::new(
            StrategyParams {
                side: Side::Buy,
                quantity: Decimal::ONE,
                tp_mult: Decimal::ONE,
                sl_mult: Decimal::ONE,
                rearm: Rearm::Auto,
                on_break: crate::BreakPolicy::Ignore,
                execution: Execution::Paper,
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: "1.5".parse().expect("fixture"),
                max_factor: "2.5".parse().expect("fixture"),
                min_range: Decimal::ZERO,
            })),
        );
        instance.alarm = Some(SignalAlarm::new(crate::AlarmParams { when, repeat }));
        instance.cue = cue;
        instance
    }

    fn test_bar(open: i64, close: i64) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: Decimal::from(open),
            high: Decimal::from(open.max(close) + 1),
            low: Decimal::from(open.min(close) - 1),
            close: Decimal::from(close),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    fn test_progress(done: i64, target: i64) -> Option<BarProgress> {
        Some(BarProgress {
            done: Decimal::from(done),
            target: Decimal::from(target),
        })
    }

    /// The whole chain, end to end: a closed force bar inside the region
    /// asks for the sound the preset named, exactly once, and the chart is
    /// told the signal is a confirmed one.
    #[test]
    fn a_closed_signal_bar_asks_for_the_preset_s_own_sound() {
        let region = Region::new(Decimal::from(100), Decimal::from(110));
        let mut instance = alarming_instance(
            crate::AlarmWhen::OnClose,
            crate::RepeatPolicy::OncePerBar,
            Cue::whole(AlertSound::Critical),
        );
        for warm in [test_bar(100, 101), test_bar(101, 102)] {
            instance.armed.on_closed_bar(&warm, &region, true, true);
            assert_eq!(
                instance.alarm_on_closed_bar(0),
                None,
                "a quiet bar is quiet"
            );
            assert_eq!(instance.mark, AlarmMark::Quiet);
        }
        instance
            .armed
            .on_closed_bar(&test_bar(102, 106), &region, true, true);
        assert_eq!(
            instance.alarm_on_closed_bar(0),
            Some(Cue::whole(AlertSound::Critical)),
            "the signal bar plays the sound the preset named"
        );
        assert_eq!(instance.mark, AlarmMark::Confirmed);
        // A trading instance shows both halves: the order it placed and the
        // signal that placed it. They are separate facts, and the badge
        // does not collapse one into the other.
        assert_eq!(badge_text(&instance, 1), "⚡ test · fired · signal");
    }

    /// The head start, and its honest label: the alarm sounds on a bar that
    /// has not closed, the badge says "preview", and when the bar closes
    /// without the signal the chart withdraws it — silently, because the
    /// trader already heard that alarm.
    #[test]
    fn a_previewed_signal_that_does_not_hold_is_withdrawn_without_a_second_sound() {
        let region = Region::new(Decimal::from(100), Decimal::from(110));
        let mut instance = alarming_instance(
            crate::AlarmWhen::at_share(Decimal::new(70, 2)),
            crate::RepeatPolicy::OncePerBar,
            Cue::whole(AlertSound::Exclamation),
        );
        for warm in [test_bar(100, 101), test_bar(101, 102)] {
            instance.armed.on_closed_bar(&warm, &region, true, true);
            let _ = instance.alarm_on_closed_bar(0);
        }

        // The bar forming toward a force close: at 60% nothing is judged,
        // at 70% the alarm speaks.
        let forming = test_bar(102, 106);
        assert_eq!(
            instance.alarm_on_forming_bar(&forming, &region, true, test_progress(1200, 2000), 0),
            None,
            "before the share, not a sound and not a judgement"
        );
        assert_eq!(instance.mark, AlarmMark::Quiet);
        assert_eq!(
            instance.alarm_on_forming_bar(&forming, &region, true, test_progress(1400, 2000), 0),
            Some(Cue::whole(AlertSound::Exclamation))
        );
        assert_eq!(instance.mark, AlarmMark::Preview);
        assert_eq!(badge_text(&instance, 1), "⚡ test · signal (preview)");

        // The bar keeps forming and gives the force back: it closes as an
        // ordinary small body.
        let closed = test_bar(102, 103);
        instance.armed.on_closed_bar(&closed, &region, true, true);
        assert_eq!(
            instance.alarm_on_closed_bar(0),
            None,
            "withdrawing a signal is shown, never played"
        );
        assert_eq!(instance.mark, AlarmMark::Faded);
        assert_eq!(badge_text(&instance, 1), "⚡ test · preview did not hold");
    }

    /// An alarm-only instance says so on its badge. Its state never leaves
    /// `Armed`, so the state alone would read as an ordinary bot the trader
    /// is still waiting on an order from.
    #[test]
    fn an_alarm_only_badge_says_it_will_never_place_an_order() {
        let mut instance = alarming_instance(
            crate::AlarmWhen::OnClose,
            crate::RepeatPolicy::OncePerBar,
            Cue::whole(AlertSound::default()),
        );
        instance.armed = ArmedStrategy::new(
            StrategyParams {
                execution: Execution::AlarmOnly,
                ..instance.armed.params().clone()
            },
            Box::new(ForceTrigger::new(ForceParams::default_band())),
        );
        assert!(instance.alarm_only());
        assert_eq!(badge_text(&instance, 1), "⚡ test · alarm only");
        instance.mark = AlarmMark::Confirmed;
        assert_eq!(badge_text(&instance, 1), "⚡ test · alarm only · signal");

        // "alarm only" is a clause beside the state, never instead of it:
        // such an instance is not pinned to `Armed`, and the badge is the
        // one surface that would tell the trader their watcher is dead.
        let _ = instance.armed.disarm(DisarmReason::User);
        assert_eq!(
            badge_text(&instance, 1),
            "⚡ test · alarm only · disarmed · signal"
        );
    }

    /// The Disarm entry says "stop watching", and the alarm is bound by
    /// that word. Every other silence in this feature is about capacity —
    /// a busy account, a spent one shot — and deliberately does not
    /// silence the alarm; a disarm is the trader saying stop, and a chart
    /// that keeps beeping under a badge reading `disarmed` is the button
    /// lying to them.
    #[test]
    fn a_disarmed_instance_stops_alarming_on_every_path() {
        let region = Region::new(Decimal::from(100), Decimal::from(110));
        let mut instance = alarming_instance(
            crate::AlarmWhen::at_share(Decimal::new(70, 2)),
            crate::RepeatPolicy::OncePerBar,
            Cue::whole(AlertSound::Critical),
        );
        for warm in [test_bar(100, 101), test_bar(101, 102)] {
            instance.armed.on_closed_bar(&warm, &region, true, true);
            let _ = instance.alarm_on_closed_bar(0);
        }
        // It speaks while it is armed — so the silence below is the disarm
        // and not a fixture that never qualified.
        let forming = test_bar(102, 106);
        assert_eq!(
            instance.alarm_on_forming_bar(&forming, &region, true, test_progress(1400, 2000), 0),
            Some(Cue::whole(AlertSound::Critical))
        );
        assert_eq!(instance.mark, AlarmMark::Preview);

        let _ = instance.armed.disarm(DisarmReason::User);
        // The standing preview goes with it: no close is coming that could
        // withdraw it, so leaving the word on the badge would strand it.
        assert_eq!(instance.alarm_on_closed_bar(1), None);
        assert_eq!(instance.mark, AlarmMark::Quiet);

        // And it stays silent on both paths, however well the tape qualifies.
        instance
            .armed
            .on_closed_bar(&test_bar(106, 107), &region, true, true);
        assert_eq!(instance.alarm_on_closed_bar(2), None);
        let second = test_bar(107, 111);
        assert_eq!(
            instance.alarm_on_forming_bar(&second, &region, true, test_progress(1900, 2000), 3),
            None
        );
        assert_eq!(instance.mark, AlarmMark::Quiet);
    }

    /// A rebuilt series takes the alarm's cooldown and its outstanding
    /// preview with it — both were counted against a tape that is gone.
    #[test]
    fn a_series_changing_disarm_resets_the_alarms_too() {
        let mut anchors = StrategyAnchors::default();
        let mut instance = alarming_instance(
            crate::AlarmWhen::at_share(Decimal::new(70, 2)),
            crate::RepeatPolicy::Cooldown { millis: 30_000 },
            Cue::whole(AlertSound::default()),
        );
        instance.mark = AlarmMark::Preview;
        anchors.arm(instance);

        let _ = anchors.disarm_all(DisarmReason::TimelineReset);
        assert_eq!(anchors.instances[0].mark, AlarmMark::Quiet);
        assert!(
            !anchors.instances[0]
                .alarm
                .as_ref()
                .expect("the alarm is still attached")
                .preview_outstanding()
        );

        // A disarm that did *not* rebuild the series leaves the cooldown
        // alone: the tape it was counted against is still the tape.
        anchors.instances[0].mark = AlarmMark::Preview;
        let _ = anchors.disarm_all(DisarmReason::User);
        assert_eq!(anchors.instances[0].mark, AlarmMark::Preview);
    }
}
