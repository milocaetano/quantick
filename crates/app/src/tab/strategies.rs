//! Running the armed strategy instances against the bars that just closed.
//!
//! The bridge between a print landing and a paper account hearing about it:
//! evaluate whatever the panes have armed, route the simulator's answers back
//! to the instance that asked. Arming and the drawing menu's cleanup are the
//! tab's (`tab/panes.rs`), over each pane's own `PaneStrategies`.
//! Deliberately cheap when nothing is armed — the common case, and the one a
//! per-trade path must not start charging for.

/// Evaluate the panes against the print the paper account has already consumed.
/// The supplied clock only governs alarms; commands still acknowledge immediately.
pub(super) fn evaluate(
    paper: &mut crate::paper_trading::PaperTrading,
    flow_pane: &mut crate::pane::ChartPane,
    time_panes: &mut [crate::pane::ChartPane],
    pending_sounds: &mut Vec<crate::audio::Cue>,
    now_ms: i64,
) {
    let now_ms = u64::try_from(now_ms).unwrap_or(0);
    let print_events = paper.account_mut().drain_bot_events();
    let mut watching = 0;
    let mut sounds: Vec<crate::audio::Cue> = Vec::new();
    // The alarm's sounds come from `main`; walking every pane in the
    // context stack rather than a single time pane comes from this branch.
    // Both are wanted: a strategy armed on the second stacked chart has to
    // ring like one armed on the first.
    for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
        if pane.strategies.anchors.is_empty() {
            continue;
        }
        // The batch to each instance, applying what it answers with —
        // the self-protection close after a dropped bracket — through the
        // runner the backtest steps its instance through too.
        quantick_strategy::runner::deliver_print_events(
            &mut pane.strategies.anchors.instances,
            &print_events,
            paper,
        );
        let bars = pane.strategies.take_bars();
        if !bars.is_empty() {
            // An instance whose drawing was deleted from a surface that
            // could not remove it directly dies here, in the sweep.
            let alive: Vec<crate::drawings::DrawingId> = pane
                .strategies
                .anchors
                .instances
                .iter()
                .map(|instance| instance.drawing)
                .filter(|id| pane.drawings.index_of(*id).is_some())
                .collect();
            let orphan_cleanup = pane
                .strategies
                .anchors
                .drop_orphans(|id| alive.contains(&id));
            for command in orphan_cleanup {
                // A dead drawing's bot must not leave its entry resting
                // with no badge over it; swept through the same funnel.
                let _ = paper.account_mut().apply_strategy_command(command);
            }
        }
        for (bar, slot) in &bars {
            // Region, flat gate, kernel, alarm, then the commands applied
            // and echoed — instance by instance, in the shared runner. The
            // region comes from the drawing as it stands for this slot.
            let drawings = &pane.drawings;
            sounds.extend(quantick_strategy::runner::step_closed_bar(
                &mut pane.strategies.anchors.instances,
                bar,
                *slot,
                |drawing, slot| crate::pane::strategies::drawing_region(drawings, drawing, slot),
                paper,
                now_ms,
            ));
        }
        // The bar still forming, judged for the alarm only. Nothing
        // here can place an order or move a state machine: the kernel's
        // preview path is `&self` all the way down, and this is the one
        // caller of it.
        //
        // This whole block is per *print*, so it opens with the question
        // that costs least: does any instance on this pane even carry an
        // alarm? A pane full of ordinary strategies answers no and pays
        // nothing further — not the partial bar's copy, not the
        // progress read.
        let any_alarm = pane
            .strategies
            .anchors
            .instances
            .iter()
            .any(|instance| instance.alarm.is_some());
        if let Some(partial) = any_alarm.then(|| pane.state.partial().cloned()).flatten() {
            let progress = (pane.state.bar_progress_at(None))
                .and_then(quantick_chart::bar_progress::BarProgressReading::fixed_progress);
            let slot = pane.closed_slots();
            for index in 0..pane.strategies.anchors.instances.len() {
                // Cheapest gate next, before the region is resolved: on
                // all but a handful of prints the alarm answers "not
                // yet" and this costs one comparison.
                let wants = pane.strategies.anchors.instances[index]
                    .alarm
                    .as_ref()
                    .is_some_and(|alarm| alarm.wants_forming_check(progress, now_ms));
                if !wants {
                    continue;
                }
                let drawing = pane.strategies.anchors.instances[index].drawing;
                let (region, active) = quantick_strategy::runner::region_or_hold(
                    crate::pane::strategies::drawing_region(&pane.drawings, drawing, slot),
                );
                sounds.extend(
                    pane.strategies.anchors.instances[index]
                        .alarm_on_forming_bar(&partial, &region, active, progress, now_ms),
                );
            }
        }
        watching += pane.strategies.anchors.watching();
    }
    paper.account_mut().set_bot_listening(watching > 0);
    pending_sounds.extend(sounds);
}

/// The paper account as the strategy runner reaches it: commands go in
/// through the same funnel every strategy command takes, and the flat gate
/// is the account's own.
impl quantick_strategy::runner::StrategyPort for crate::paper_trading::PaperTrading {
    fn apply(&mut self, command: quantick_sim::Command) -> Vec<quantick_sim::VenueEvent> {
        self.account_mut().apply_strategy_command(command)
    }

    fn is_flat(&self) -> bool {
        crate::paper_trading::PaperTrading::is_flat(self)
    }
}
