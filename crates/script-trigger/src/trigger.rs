//! [`ScriptTrigger`]: one compiled script judged on every closed bar.

use std::cell::RefCell;
use std::rc::Rc;

use quantick_engine::Side;
use quantick_indicators::{Ctx, FootprintBar, Indicator, IndicatorBar};
use quantick_pine::ScriptIndicator;
use quantick_strategy::{Signal, Trigger, TriggerContext};
use rust_decimal::Decimal;

use crate::script::{Script, ScriptFault};

/// How many closed bars re-warm a script at arm time: the deepest history a
/// script may read through a dynamic offset (`MAX_BARS_BACK_CAP`), so its
/// series and kernels stand where a chart that loaded those bars has them.
pub const WARMUP_BARS: usize = quantick_pine::compile::MAX_BARS_BACK_CAP;

/// The stable refusal name of a trigger whose script failed: it judges
/// nothing more until the instance is armed again.
pub const SCRIPT_FAULT: &str = "script error";

/// What the last judged bar left.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Last {
    Nothing,
    Quiet,
    Fired(String),
}

/// The running script, shared between the trigger the kernel owns and the
/// instance that hands it each bar's footprint and reads its fault.
///
/// Shared because the kernel's port carries an engine [`Bar`] and nothing
/// else: the point of control a script reads as `poc` is pane data, so the
/// instance parks it here for the one bar about to be judged. The fault
/// travels back the same way, since the port's `&dyn Trigger` cannot be
/// asked for a type of its own.
///
/// [`Bar`]: quantick_engine::Bar
pub(crate) struct ScriptRun {
    indicator: ScriptIndicator,
    script: String,
    sides: Vec<(String, Side)>,
    /// The side the instance hunts: when alerts of both sides hold on one
    /// bar, this one wins, so neither instance is silenced by the other.
    hunted: Side,
    /// The host-maintained cumulative delta, one value per bar judged.
    cvd: Vec<f64>,
    /// The footprint POC of the bar about to be judged, taken by it.
    pub(crate) poc: Option<Decimal>,
    pub(crate) fault: Option<ScriptFault>,
    last: Last,
}

/// The trigger the kernel owns; its run is shared with the instance.
pub struct ScriptTrigger {
    run: Rc<RefCell<ScriptRun>>,
}

impl ScriptTrigger {
    /// A fresh run of `script` for an instance hunting `hunted`, and the
    /// handle the instance keeps on it.
    pub(crate) fn new(script: &Script, hunted: Side) -> (Self, Rc<RefCell<ScriptRun>>) {
        let indicator = ScriptIndicator::new(script.compiled().clone(), script.source());
        let run = Rc::new(RefCell::new(ScriptRun {
            indicator,
            script: script.name().to_owned(),
            sides: script.sides().to_vec(),
            hunted,
            cvd: Vec::new(),
            poc: None,
            fault: None,
            last: Last::Nothing,
        }));
        (
            Self {
                run: Rc::clone(&run),
            },
            run,
        )
    }
}

impl ScriptRun {
    /// Forget what the last judged bar left, as a fresh arm has judged none.
    pub(crate) fn forget_last(&mut self) {
        self.last = Last::Nothing;
    }

    /// Run the script over one closed bar, answering the side of the alert
    /// that held, if any.
    fn judge(&mut self, cx: &TriggerContext<'_>) -> Option<Side> {
        let poc = self.poc.take();
        if self.fault.is_some() {
            return None;
        }
        let bar = IndicatorBar::from(&FootprintBar {
            bar: cx.bar.clone(),
            poc,
        });
        self.cvd
            .push(self.cvd.last().copied().unwrap_or(0.0) + bar.delta());
        let mut ctx = Ctx {
            bar_index: self.cvd.len() - 1,
            cvd: &self.cvd,
        };
        if self.indicator.on_close(&bar, &mut ctx).is_err() {
            // A failed commit run always leaves its fault behind.
            self.fault = self.indicator.runtime_fault().map(ScriptFault::runtime);
            return None;
        }
        let fired = self.indicator.take_fired_alerts();
        let side_of = |title: &str| {
            (self.sides.iter())
                .find(|(known, _)| known == title)
                .map(|(_, side)| *side)
        };
        let mut held = fired
            .iter()
            .filter_map(|alert| Some((side_of(&alert.title)?, &alert.title)));
        let first = held.next();
        let chosen = (first.into_iter().chain(held))
            .find(|(side, _)| *side == self.hunted)
            .or(first);
        self.last = match chosen {
            Some((_, title)) => Last::Fired(title.clone()),
            None => Last::Quiet,
        };
        chosen.map(|(side, _)| side)
    }
}

impl Trigger for ScriptTrigger {
    fn on_closed_bar(&mut self, cx: &TriggerContext<'_>) -> Option<Signal> {
        let side = self.run.borrow_mut().judge(cx)?;
        // The close is what fired: the script judged the closed bar, and the
        // bar's range is the one projection an alarm-only instance has.
        Some(Signal {
            side,
            reference: cx.bar.close,
            projection: cx.bar.high - cx.bar.low,
        })
    }

    fn status(&self) -> String {
        let run = self.run.borrow();
        if let Some(fault) = &run.fault {
            return format!("{}: {fault}", run.script);
        }
        match &run.last {
            Last::Nothing => format!("{}: waiting for bars", run.script),
            Last::Quiet => format!("{}: no alert", run.script),
            Last::Fired(title) => format!("{}: {title}", run.script),
        }
    }

    fn refusal(&self) -> Option<&'static str> {
        let run = self.run.borrow();
        if run.fault.is_some() {
            return Some(SCRIPT_FAULT);
        }
        match run.last {
            Last::Nothing => Some("no bars judged yet"),
            Last::Quiet => Some("no alert held on this bar"),
            Last::Fired(_) => None,
        }
    }

    /// The series was rebuilt: the script starts over. A fault stays — the
    /// instance it disarmed is re-armed by a new arm, not by a reset.
    fn reset(&mut self) {
        let mut run = self.run.borrow_mut();
        run.indicator.reset();
        run.cvd.clear();
        run.poc = None;
        run.last = Last::Nothing;
    }

    fn warmup_bars(&self) -> usize {
        WARMUP_BARS
    }
}
