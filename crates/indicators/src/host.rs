//! [`IndicatorHost`] — the multi-indicator manager.
//!
//! The host owns the ordered set of active indicators (native or scripted —
//! it only ever sees the [`Indicator`] trait), converts each engine
//! [`Bar`] to an [`IndicatorBar`] **once** for all of them, maintains the one
//! shared cross-bar series (`cvd`), and isolates failures: a runtime error
//! inside one indicator disables *that* indicator (error state with bar
//! index + message for the UI) and never poisons its neighbours.
//!
//! Pure and thread-free — the worker thread that feeds a chart lives
//! app-side. A backtester or bot drives this same type directly: push bars,
//! read committed plot columns. That symmetry is the point.
//!
//! The host retains the projected bar history (`IndicatorBar` is 9 machine
//! words; 100k bars ≈ 7 MB). That is what lets [`add`](IndicatorHost::add)
//! and [`replace`](IndicatorHost::replace) catch a newcomer up over the full
//! history without asking the caller to re-supply bars — an input change is
//! "construct with new inputs, replace, host replays", so a running script
//! never observes an input changing mid-stream.

use quantick_engine::Bar;

use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
use crate::output::{PlotBuffer, PreviewFrame};

/// Stable handle to one hosted indicator instance. Ids are never reused
/// within a host's lifetime, so a stale id simply misses instead of hitting
/// the wrong indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstanceId(u64);

struct Instance {
    id: InstanceId,
    indicator: Box<dyn Indicator>,
    /// Set when a commit/preview/catch-up run failed; the host stops
    /// evaluating the instance until it is replaced or a rebuild clears it.
    error: Option<EvalError>,
    /// Latest preview frame (latest-wins; stale frames are dropped on every
    /// close and on `set_partial(None)`).
    preview: Option<PreviewFrame>,
}

/// See the module docs.
#[derive(Default)]
pub struct IndicatorHost {
    instances: Vec<Instance>,
    next_id: u64,
    /// Projected history, one entry per committed (closed) bar.
    bars: Vec<IndicatorBar>,
    /// Committed cvd column (running sum of bar delta), same length as
    /// `bars` — except transiently during a preview, when the forming bar's
    /// staged value is pushed and popped (truncate-don't-clone).
    cvd: Vec<f64>,
}

impl IndicatorHost {
    /// An empty host: no indicators, no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an indicator, replaying the retained history into it so it is
    /// immediately in sync with every other instance. A replay failure lands
    /// in the instance's error state, exactly like a live failure.
    pub fn add(&mut self, mut indicator: Box<dyn Indicator>) -> InstanceId {
        let id = InstanceId(self.next_id);
        self.next_id += 1;
        let error = Self::catch_up(indicator.as_mut(), &self.bars, &self.cvd).err();
        self.instances.push(Instance {
            id,
            indicator,
            error,
            preview: None,
        });
        id
    }

    /// Swap the implementation behind `id` (input change, script reload) and
    /// replay history into the newcomer. The running instance never sees an
    /// input change mid-stream — it is replaced wholesale. Returns false for
    /// an unknown id.
    pub fn replace(&mut self, id: InstanceId, mut indicator: Box<dyn Indicator>) -> bool {
        let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) else {
            return false;
        };
        instance.error = Self::catch_up(indicator.as_mut(), &self.bars, &self.cvd).err();
        instance.indicator = indicator;
        instance.preview = None;
        true
    }

    /// Remove an indicator. Returns false for an unknown id.
    pub fn remove(&mut self, id: InstanceId) -> bool {
        let before = self.instances.len();
        self.instances.retain(|i| i.id != id);
        self.instances.len() != before
    }

    /// Commit one closed bar: project it once, extend the shared cvd, run
    /// every healthy indicator's commit run. A failing indicator enters its
    /// error state; the others are unaffected.
    pub fn push_closed_bar(&mut self, bar: &Bar) {
        self.commit_bar(&IndicatorBar::from(bar));
    }

    /// Run previews against the forming bar (or clear them with `None`).
    ///
    /// The forming bar's cvd is staged onto the committed column and popped
    /// right after the previews — the same truncate-don't-clone discipline
    /// the series store uses, so a preview costs no allocation here.
    pub fn set_partial(&mut self, partial: Option<&Bar>) {
        let Some(bar) = partial else {
            for instance in &mut self.instances {
                instance.preview = None;
            }
            return;
        };
        let bar = IndicatorBar::from(bar);
        let bar_index = self.bars.len();
        let staged_cvd = self.cvd.last().copied().unwrap_or(0.0) + bar.delta();
        self.cvd.push(staged_cvd);
        let cvd = &self.cvd;
        for instance in &mut self.instances {
            if instance.error.is_some() {
                continue;
            }
            let mut ctx = Ctx { bar_index, cvd };
            match instance.indicator.preview(&bar, &mut ctx) {
                Ok(frame) => instance.preview = Some(frame),
                Err(e) => {
                    instance.error = Some(e);
                    instance.preview = None;
                }
            }
        }
        self.cvd.pop();
    }

    /// Reset everything and replay `bars` from scratch (spec switch, replay
    /// seek, prepended history). Error states clear — a deterministic
    /// failure will honestly re-surface at the same bar.
    pub fn rebuild(&mut self, bars: &[Bar], partial: Option<&Bar>) {
        self.bars.clear();
        self.cvd.clear();
        for instance in &mut self.instances {
            instance.indicator.reset();
            instance.error = None;
            instance.preview = None;
        }
        for bar in bars {
            self.commit_bar(&IndicatorBar::from(bar));
        }
        self.set_partial(partial);
    }

    fn commit_bar(&mut self, bar: &IndicatorBar) {
        let bar_index = self.bars.len();
        self.bars.push(*bar);
        let prev = self.cvd.last().copied().unwrap_or(0.0);
        self.cvd.push(prev + bar.delta());
        let cvd = &self.cvd;
        for instance in &mut self.instances {
            // A close invalidates any forming-bar preview, error or not.
            instance.preview = None;
            if instance.error.is_some() {
                continue;
            }
            let mut ctx = Ctx { bar_index, cvd };
            if let Err(e) = instance.indicator.on_close(bar, &mut ctx) {
                instance.error = Some(e);
            }
        }
    }

    /// Replay retained history into a fresh indicator, reproducing exactly
    /// the per-bar cvd prefix each live commit run saw — determinism demands
    /// that a caught-up instance is indistinguishable from one added at bar
    /// zero.
    fn catch_up(
        indicator: &mut dyn Indicator,
        bars: &[IndicatorBar],
        cvd: &[f64],
    ) -> Result<(), EvalError> {
        indicator.reset();
        for (bar_index, bar) in bars.iter().enumerate() {
            let mut ctx = Ctx {
                bar_index,
                cvd: &cvd[..=bar_index],
            };
            indicator.on_close(bar, &mut ctx)?;
        }
        Ok(())
    }

    fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    /// Ids of the hosted indicators, in insertion (render) order.
    pub fn ids(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.instances.iter().map(|i| i.id)
    }

    /// Number of hosted indicators.
    #[must_use]
    pub fn indicator_count(&self) -> usize {
        self.instances.len()
    }

    /// Descriptor of one instance.
    #[must_use]
    pub fn descriptor(&self, id: InstanceId) -> Option<&IndicatorDescriptor> {
        self.instance(id).map(|i| i.indicator.descriptor())
    }

    /// Committed plot columns of one instance — the read path a renderer,
    /// backtester or bot consumes.
    #[must_use]
    pub fn plots(&self, id: InstanceId) -> Option<&PlotBuffer> {
        self.instance(id).map(|i| i.indicator.plots())
    }

    /// Error state of one instance, if it failed.
    #[must_use]
    pub fn error(&self, id: InstanceId) -> Option<&EvalError> {
        self.instance(id).and_then(|i| i.error.as_ref())
    }

    /// Latest preview frame of one instance, if a bar is forming.
    #[must_use]
    pub fn preview(&self, id: InstanceId) -> Option<&PreviewFrame> {
        self.instance(id).and_then(|i| i.preview.as_ref())
    }

    /// Number of committed bars replayed so far.
    #[must_use]
    pub fn bar_count(&self) -> usize {
        self.bars.len()
    }

    /// The committed shared cvd column (one value per committed bar).
    #[must_use]
    pub fn cvd(&self) -> &[f64] {
        &self.cvd
    }

    /// Change counter of one instance's draw objects (`None`: the indicator
    /// draws none). Cheap — the worker polls this to decide when to
    /// re-publish the set.
    #[must_use]
    pub fn objects_revision(&self, id: InstanceId) -> Option<u64> {
        self.instance(id)
            .and_then(|i| i.indicator.objects())
            .map(crate::objects::ObjectStore::revision)
    }

    /// The renderable object set of one instance.
    #[must_use]
    pub fn objects_snapshot(&self, id: InstanceId) -> Option<crate::objects::ObjectSnapshot> {
        self.instance(id)
            .and_then(|i| i.indicator.objects())
            .map(crate::objects::ObjectStore::snapshot)
    }
}
