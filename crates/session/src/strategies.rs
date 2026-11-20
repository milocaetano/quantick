//! The strategies armed on a tab's panes: which instance rides which
//! drawing, the closed bars queued for them, the cleanup they owe the paper
//! account, and the sweep that runs them against a print.
//!
//! A pane anchors and paints; the kernel (`quantick-strategy`) judges; this
//! module holds the state between the two, keyed by the pane's id. That is
//! why three of [`PaneStrategies`]' four fields are queues rather than
//! state: a pane sees the closed bar and the click, but the simulator and
//! the arming dialog are reached later in the frame, so what it saw is
//! parked here and drained on the same frame.
//!
//! Nothing here reaches into a pane. What the chart knows — whether a
//! drawing still exists, the region it marks at a slot, the bar still
//! forming, the closed bars a ruler warms on — arrives as a closure, so the
//! same transitions serve the menu, the keyboard, the tab's ingestion sweep
//! and a test that never lays a pane out. The account is reached only
//! through [`StrategyPort`], and the alarms a sweep raises come back as
//! data for the tab to play. Told the time, never reading it.

use std::collections::BTreeMap;

use quantick_engine::{Bar, BarProgress};
use quantick_script_trigger::ScriptInstance;
use quantick_sim::{Command, VenueEvent};
use quantick_strategy::anchors::{AlarmMark, AnchoredInstance, StrategyAnchors};
use quantick_strategy::presets::{CompiledPreset, StoredPreset};
use quantick_strategy::runner::{self, StrategyPort};
use quantick_strategy::sound::Cue;
use quantick_strategy::{ArmedState, ArmedStrategy, DisarmReason, ForceTrigger, Region};
use rust_decimal::Decimal;

use crate::strategy_desk::{DeskBook, InstanceRef};

/// A bar one print closed on one pane, at the composed slot it closed at:
/// what the pane's strategies are queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveClose {
    pub bar: Bar,
    /// The point of control of the bar's footprint, when the pane kept one:
    /// what a script strategy reads as `poc`.
    pub poc: Option<Decimal>,
    pub slot: usize,
}

/// Why a script riding a region stopped when its rectangle was deleted.
pub const DRAWING_REMOVED: &str = "its region drawing was removed";

/// The armed strategies of one pane and the work they park for the tab.
/// See the module docs.
pub struct PaneStrategies<D> {
    /// Armed strategy instances riding this pane's drawings.
    pub anchors: StrategyAnchors<D>,
    /// Pine scripts armed alarm-only on this pane's bars, in arming order
    /// (`crate::script_strategies`).
    pub scripts: Vec<ScriptInstance<D>>,
    /// Closed bars awaiting evaluation, each with the slot it closed at.
    /// Queued only while instances exist and drained in the same ingestion
    /// sweep, so the slot and the drawings' anchors are read against one
    /// cut of the series.
    /// Each with whether it is judged: `false` is a close nobody heard live
    /// (a refilled hole), which a script only warms on.
    pending: Vec<(LiveClose, bool)>,
    /// The drawing whose "Add strategy…" was clicked; drained into the
    /// arming dialog over this pane.
    popup_request: Option<D>,
    /// Simulator commands a disarm or removal owes the paper account —
    /// cancelling a resting retest limit — drained on the same frame.
    cleanup: Vec<Command>,
}

impl<D> Default for PaneStrategies<D> {
    fn default() -> Self {
        Self {
            anchors: StrategyAnchors::default(),
            scripts: Vec::new(),
            pending: Vec::new(),
            popup_request: None,
            cleanup: Vec::new(),
        }
    }
}

impl<D: Copy + PartialEq> PaneStrategies<D> {
    /// Queue the bar that closed at `slot`, with its footprint POC, only
    /// while an instance listens: an idle chart clones nothing on the
    /// per-trade path. The slot is the composed one (venue history prefix +
    /// live bars), the space the drawings' anchors live in.
    ///
    /// A close that is not `judged` — heard after the fact, as a refill —
    /// trades and rings nothing: drawn instances never see it, and a script
    /// warms on it in order, so its history stays the chart's copy's.
    pub fn queue_closed(&mut self, bar: &Bar, poc: Option<Decimal>, slot: usize, judged: bool) {
        let listens = if judged {
            !self.is_empty()
        } else {
            self.scripts.iter().any(ScriptInstance::watching)
        };
        if listens {
            let bar = bar.clone();
            self.pending.push((LiveClose { bar, poc, slot }, judged));
        }
    }

    /// Whether nothing on this pane listens to its bars: no drawn instance,
    /// and no script still watching.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty() && !self.scripts.iter().any(ScriptInstance::watching)
    }

    /// The closed bars awaiting evaluation.
    pub fn pending(&self) -> impl ExactSizeIterator<Item = &LiveClose> {
        self.pending.iter().map(|(close, _)| close)
    }

    /// Whether any instance carries an alarm: a watching script always does.
    #[must_use]
    pub fn any_alarm(&self) -> bool {
        self.anchor_alarm() || self.scripts.iter().any(ScriptInstance::watching)
    }

    /// Whether a drawn instance carries an alarm — the only kind that
    /// previews the forming bar.
    fn anchor_alarm(&self) -> bool {
        (self.anchors.instances.iter()).any(|instance| instance.alarm.is_some())
    }

    /// Halt every script riding a drawing `exists` denies, naming why.
    fn halt_orphan_scripts(&mut self, exists: impl Fn(D) -> bool) {
        for script in &mut self.scripts {
            if script
                .placement
                .drawing()
                .is_some_and(|drawing| !exists(drawing))
            {
                script.halt(DRAWING_REMOVED);
            }
        }
    }

    /// The closed bars awaiting evaluation, drained, each with whether it
    /// is judged.
    #[must_use]
    pub fn take_bars(&mut self) -> Vec<(LiveClose, bool)> {
        std::mem::take(&mut self.pending)
    }

    /// Ask for the arming dialog over `drawing`.
    pub fn request_popup(&mut self, drawing: D) {
        self.popup_request = Some(drawing);
    }

    /// The drawing whose arming dialog was asked for, if any.
    #[must_use]
    pub fn popup_request(&self) -> Option<D> {
        self.popup_request
    }

    /// The arming dialog request, drained.
    pub fn take_popup_request(&mut self) -> Option<D> {
        self.popup_request.take()
    }

    /// Re-arm every ended instance riding `drawing`, re-warming a ruler on
    /// `warmup(depth)` when its disarm named a *rebuilt series* (a replay
    /// seek, a bar-spec change, a market switch). Without the re-warm,
    /// "re-armed" silently means "warming up for another twenty bars". An
    /// instance still watching is left as it is.
    pub fn rearm(&mut self, drawing: D, mut warmup: impl FnMut(usize) -> Vec<Bar>) {
        let reached = self.anchors.on_drawing_mut(drawing);
        for instance in reached.filter(|instance| instance.rearmable()) {
            rearm_instance(instance, &mut warmup);
        }
    }

    /// Remove the drawn instance at `index`, queueing the sweep of its
    /// resting entry; its drawing's other instances stay.
    pub(crate) fn remove_instance_at(&mut self, index: usize) {
        let instance = self.anchors.instances.remove(index);
        self.cleanup.extend(instance.armed.pending_entry_cancel());
    }

    /// Sweep instances whose drawing no longer `exists`, queueing what they
    /// owe for the same-frame drain — for the deletion paths that have no
    /// id in hand (delete-all, undo, redo).
    pub fn sweep_orphans(&mut self, exists: impl Fn(D) -> bool) {
        self.halt_orphan_scripts(&exists);
        if self.anchors.is_empty() {
            return;
        }
        let cleanup = self.anchors.drop_orphans(exists);
        self.cleanup.extend(cleanup);
    }

    /// The cleanup commands queued so far, drained.
    #[must_use]
    pub fn take_cleanup(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.cleanup)
    }

    /// Remove every instance riding `drawing` and queue the sweep of their
    /// pending entries — every "the bot dies with its drawing" path funnels
    /// through here.
    pub fn remove_for_drawing(&mut self, drawing: D) {
        self.halt_orphan_scripts(|riding| riding != drawing);
        let cleanup = self.anchors.remove_for_drawing(drawing);
        self.cleanup.extend(cleanup);
    }

    /// Call off every strategy riding `drawing` still watching, as the
    /// user, and queue the sweep of whatever they left resting. An ended
    /// one keeps its reason. Nothing to call off is a no-op.
    pub fn disarm(&mut self, drawing: D) {
        let reached = self.anchors.on_drawing_mut(drawing);
        for instance in reached.filter(|instance| instance.watching()) {
            let cleanup = instance.armed.disarm(DisarmReason::User);
            self.cleanup.extend(cleanup);
        }
    }

    /// Disarm every instance for `reason`, forgetting the bars queued for
    /// the series that ended; answers with the cleanup the instances owe.
    pub fn disarm_all(&mut self, reason: DisarmReason) -> Vec<Command> {
        self.pending.clear();
        for script in &mut self.scripts {
            script.disarm(reason);
        }
        self.anchors.disarm_all(reason)
    }

    /// Arm `form` on `drawing`, warmed on `warmup(depth)`, once `check` —
    /// the chart's judgement of the drawing — passed, beside any instance
    /// already riding it. Answers with the cue its alarm rings with, if it
    /// carries one.
    ///
    /// # Errors
    ///
    /// The sentence the trader reads: a form that does not compile, or
    /// whatever `check` refused.
    pub fn arm(
        &mut self,
        drawing: D,
        form: &StoredPreset,
        preset: String,
        check: impl FnOnce() -> Result<(), String>,
        warmup: impl FnOnce(usize) -> Vec<Bar>,
    ) -> Result<Option<Cue>, String> {
        let Some(CompiledPreset {
            params,
            force,
            alarm,
        }) = form.to_kernel()
        else {
            return Err(
                "a field does not parse: quantity, factors and multipliers must be numbers, \
                 and an instance that neither trades nor alarms cannot be armed"
                    .to_owned(),
            );
        };
        check()?;
        let mut armed = ArmedStrategy::new(params, Box::new(ForceTrigger::new(force)));
        // Armed means armed now, not after another twenty bars of warmup
        // the trader cannot see the reason for.
        armed.warm(&warmup(armed.trigger().warmup_bars()));
        self.anchors.arm(AnchoredInstance {
            drawing,
            preset,
            spec: form.clone(),
            armed,
            alarm: alarm.map(|setup| quantick_strategy::SignalAlarm::new(setup.params)),
            cue: alarm.map(|setup| setup.cue).unwrap_or_default(),
            mark: AlarmMark::Quiet,
        });
        Ok(alarm.map(|setup| setup.cue))
    }
}

/// Re-arm one ended instance (see [`PaneStrategies::rearm`]).
pub(crate) fn rearm_instance<D>(
    instance: &mut AnchoredInstance<D>,
    mut warmup: impl FnMut(usize) -> Vec<Bar>,
) {
    let series_changed = matches!(
        instance.armed.state(),
        ArmedState::Disarmed { reason } if reason.resets_series()
    );
    instance.armed.rearm();
    if series_changed {
        let bars = warmup(instance.armed.trigger().warmup_bars());
        instance.armed.warm(&bars);
    }
}

/// One print as the strategies hear it: the venue events the account
/// produced consuming it, and the time the alarms are judged at.
#[derive(Clone, Copy)]
pub struct Print<'a> {
    pub events: &'a [VenueEvent],
    pub now_ms: u64,
}

/// A pane's bar still forming, for the alarm's preview.
pub struct Forming {
    pub bar: Bar,
    pub progress: Option<BarProgress>,
    /// The slot the forming bar will close at.
    pub slot: usize,
}

/// What one sweep asks of the tab.
#[derive(Debug, Default)]
pub struct Evaluation {
    /// The alarm sounds raised, oldest first.
    pub alarms: Vec<Cue>,
    /// How many instances are still watching; zero lets the account stop
    /// listening for them.
    pub watching: usize,
}

/// Every pane's strategies, keyed by the pane's id.
pub struct TabStrategies<D> {
    panes: BTreeMap<u64, PaneStrategies<D>>,
    /// The id the next armed script takes; never reused in the tab's life.
    pub(crate) next_script_id: u64,
    /// What the strategy desk keeps beside the instances.
    pub(crate) desk: DeskBook<D>,
}

impl<D> Default for TabStrategies<D> {
    fn default() -> Self {
        Self {
            panes: BTreeMap::new(),
            next_script_id: 1,
            desk: DeskBook::default(),
        }
    }
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// Every pane that has held a strategy, in id order.
    pub(crate) fn panes(&self) -> impl Iterator<Item = (u64, &PaneStrategies<D>)> {
        self.panes.iter().map(|(&id, pane)| (id, pane))
    }

    /// Every pane that has held a strategy, in id order, to act on.
    pub(crate) fn panes_mut(&mut self) -> impl Iterator<Item = (u64, &mut PaneStrategies<D>)> {
        self.panes.iter_mut().map(|(&id, pane)| (id, pane))
    }

    /// The strategies of pane `id`, when it has ever held one.
    #[must_use]
    pub fn pane(&self, id: u64) -> Option<&PaneStrategies<D>> {
        self.panes.get(&id)
    }

    /// The strategies of pane `id`, opened empty on first use.
    pub fn pane_mut(&mut self, id: u64) -> &mut PaneStrategies<D> {
        self.panes.entry(id).or_default()
    }

    /// The strategies of pane `id`, only when it has held one — for the
    /// per-frame paths that must not open an entry for every pane.
    pub fn existing_mut(&mut self, id: u64) -> Option<&mut PaneStrategies<D>> {
        self.panes.get_mut(&id)
    }

    /// Whether any pane carries an instance.
    #[must_use]
    pub fn any_armed(&self) -> bool {
        self.panes.values().any(|pane| !pane.is_empty())
    }

    /// The instances riding pane `id`'s drawings, if any.
    #[must_use]
    pub fn anchors(&self, id: u64) -> Option<&StrategyAnchors<D>> {
        self.pane(id).map(|pane| &pane.anchors)
    }

    /// How many of pane `id`'s instances are watching.
    #[must_use]
    pub fn watching(&self, id: u64) -> usize {
        self.anchors(id).map_or(0, StrategyAnchors::watching)
    }

    /// Drop a pane that no longer exists, and whatever rode it.
    pub fn forget_pane(&mut self, id: u64) {
        self.panes.remove(&id);
    }

    /// Queue the bar pane `id` closed at `slot` (see
    /// [`PaneStrategies::queue_closed`]). Opens no entry: the per-trade path.
    pub fn queue_closed(
        &mut self,
        id: u64,
        bar: &Bar,
        poc: Option<Decimal>,
        slot: usize,
        judged: bool,
    ) {
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.queue_closed(bar, poc, slot, judged);
        }
    }

    /// Whether any instance carries an alarm — the gate before a clock read.
    #[must_use]
    pub fn any_alarm(&self) -> bool {
        self.panes.values().any(PaneStrategies::any_alarm)
    }

    /// Disarm every pane's instances for `reason` (see
    /// [`PaneStrategies::disarm_all`]); answers with what they owe.
    pub fn disarm_all(&mut self, reason: DisarmReason) -> Vec<Command> {
        let mut cleanup = Vec::new();
        for pane in self.panes.values_mut() {
            cleanup.extend(pane.disarm_all(reason));
        }
        cleanup
    }

    /// Sweep every pane's instances whose drawing no longer `exists`.
    pub fn sweep_orphans(&mut self, exists: impl Fn(u64, D) -> bool) {
        for (&id, pane) in &mut self.panes {
            pane.sweep_orphans(|drawing| exists(id, drawing));
        }
    }

    /// The cleanup every pane queued, drained in `panes` order.
    #[must_use]
    pub fn take_cleanup(&mut self, panes: &[u64]) -> Vec<Command> {
        let mut cleanup = Vec::new();
        for id in panes {
            if let Some(pane) = self.panes.get_mut(id) {
                cleanup.append(&mut pane.cleanup);
            }
        }
        cleanup
    }

    /// Run the instances of `panes`, in that order, against a print the
    /// account has already consumed: deliver its events, drop instances
    /// whose drawing no longer `exists` (their cleanup applied at once),
    /// judge each queued closed bar against `region`, then preview the
    /// `forming` bar for the alarms. Commands reach the account through
    /// `port` only; the sounds come back in the answer.
    pub fn evaluate<P: StrategyPort + ?Sized>(
        &mut self,
        panes: &[u64],
        port: &mut P,
        print: Print<'_>,
        exists: impl Fn(u64, D) -> bool,
        mut region: impl FnMut(u64, D, usize) -> Option<(Region, bool)>,
        mut forming: impl FnMut(u64) -> Option<Forming>,
    ) -> Evaluation {
        let Print { events, now_ms } = print;
        self.desk.heard(now_ms);
        let mut answer = Evaluation::default();
        for &id in panes {
            let Some(pane) = self.panes.get_mut(&id) else {
                continue;
            };
            if pane.is_empty() {
                continue;
            }
            // The batch to each instance, applying what it answers with —
            // the self-protection close after a dropped bracket.
            runner::deliver_print_events(&mut pane.anchors.instances, events, port);
            let bars = pane.take_bars();
            if !bars.is_empty() {
                // An instance whose drawing was deleted from a surface that
                // could not remove it directly dies here, and its resting
                // entry with it — through the same funnel.
                for command in pane.anchors.drop_orphans(|drawing| exists(id, drawing)) {
                    let _ = port.apply(command);
                }
                // A script riding a region stops with its drawing, and says so.
                pane.halt_orphan_scripts(|drawing| exists(id, drawing));
            }
            for (close, judged) in &bars {
                if !judged {
                    for script in &mut pane.scripts {
                        script.warm(&close.bar, close.poc);
                    }
                    continue;
                }
                answer.alarms.extend(runner::step_closed_bar(
                    &mut pane.anchors.instances,
                    &close.bar,
                    close.slot,
                    |drawing, slot| region(id, drawing, slot),
                    port,
                    now_ms,
                ));
                self.desk.count_drawn(id, &pane.anchors.instances, now_ms);
                for script in &mut pane.scripts {
                    // Paused from the desk: its script keeps its history.
                    if self.desk.script_paused(script.id) {
                        script.warm(&close.bar, close.poc);
                        continue;
                    }
                    let resolved =
                        (script.placement).resolve(|drawing| region(id, drawing, close.slot));
                    let rang =
                        script.on_closed_bar(&close.bar, close.poc, resolved.as_ref(), now_ms);
                    if rang.is_some() {
                        self.desk.fired(InstanceRef::Script(script.id), now_ms);
                    }
                    answer.alarms.extend(rang);
                }
            }
            // The bar still forming, judged for the alarm only. Per print,
            // so it opens with the question that costs least.
            let any_alarm = pane.anchor_alarm();
            if let Some(Forming {
                bar,
                progress,
                slot,
            }) = any_alarm.then(|| forming(id)).flatten()
            {
                for instance in &mut pane.anchors.instances {
                    let wants = (instance.alarm.as_ref())
                        .is_some_and(|alarm| alarm.wants_forming_check(progress, now_ms));
                    if !wants {
                        continue;
                    }
                    let resolved = region(id, instance.drawing, slot);
                    let (shape, active) = runner::placement_or_hold(resolved.as_ref());
                    answer.alarms.extend(
                        instance.alarm_on_forming_bar(&bar, shape, active, progress, now_ms),
                    );
                }
            }
            answer.watching += pane.anchors.watching();
        }
        answer
    }
}
