//! [`ScriptIndicator`] — a compiled script behind the [`Indicator`] trait.
//!
//! This is where the language meets the runtime contract: commit runs
//! mutate persistent state and append exactly one plot row (as the last
//! step, so an error never leaves a half row); preview runs execute against
//! a snapshot and roll everything back except `varip` (§2.3). The host —
//! and therefore the chart, the backtest and the bot — cannot tell a
//! scripted indicator from a native one.

use quantick_indicators::{
    Ctx, EvalError, Indicator, IndicatorBar, IndicatorDescriptor, InputValue, PlotBuffer,
    PreviewFrame,
};

use crate::compile::CompiledScript;
use crate::eval::{Eval, ScriptState};

/// A loaded script instance: compiled tables + bound inputs + running state.
pub struct ScriptIndicator {
    compiled: CompiledScript,
    /// The source text, kept for rendering runtime error spans as
    /// line:column (the compiled tables only carry byte offsets).
    source: String,
    descriptor: IndicatorDescriptor,
    inputs: Vec<InputValue>,
    plots: PlotBuffer,
    state: ScriptState,
    /// Reused row scratch (one cell per plot).
    row: Vec<f64>,
}

impl ScriptIndicator {
    /// Instantiate with every input at its declared default.
    #[must_use]
    pub fn new(compiled: CompiledScript, source: impl Into<String>) -> Self {
        let defaults: Vec<InputValue> = compiled
            .inputs
            .iter()
            .map(quantick_indicators::InputSpec::default_value)
            .collect();
        Self::with_inputs(compiled, source, defaults)
    }

    /// Instantiate with bound input values (the settings UI's apply path:
    /// construct anew, `host.replace`, host replays history).
    ///
    /// # Panics
    ///
    /// Panics if `inputs` does not match the compiled input count — the
    /// caller binds values against the very specs this compile produced.
    #[must_use]
    pub fn with_inputs(
        compiled: CompiledScript,
        source: impl Into<String>,
        inputs: Vec<InputValue>,
    ) -> Self {
        assert_eq!(
            inputs.len(),
            compiled.inputs.len(),
            "bound inputs must match the compiled input specs"
        );
        let descriptor = IndicatorDescriptor {
            title: compiled.title.clone(),
            short_title: compiled.short_title.clone(),
            overlay: compiled.overlay,
            plots: compiled.plots.clone(),
            inputs: compiled.inputs.clone(),
        };
        let plots = PlotBuffer::new(compiled.plots.len());
        let state = ScriptState::new(compiled.global_slots);
        let row = vec![f64::NAN; compiled.plots.len()];
        Self {
            compiled,
            source: source.into(),
            descriptor,
            inputs,
            plots,
            state,
            row,
        }
    }

    /// Whether this script ever created a draw object — scripts that never
    /// do stay on the objects-free fast path (no snapshots published).
    fn draws_objects(&self) -> bool {
        let (lines, boxes, labels) = self.state.objects.counts();
        lines + boxes + labels > 0 || self.state.objects.revision() > 0
    }

    /// Load-time warnings (surfaced by the indicator panel).
    #[must_use]
    pub fn warnings(&self) -> &[crate::error::PineError] {
        &self.compiled.warnings
    }

    fn run_once(
        &mut self,
        bar: &IndicatorBar,
        ctx: &Ctx<'_>,
        is_commit: bool,
    ) -> Result<(), EvalError> {
        self.row.fill(f64::NAN);
        let mut eval = Eval::new(
            &self.compiled,
            &self.inputs,
            bar,
            ctx,
            &mut self.state,
            &mut self.row,
            is_commit,
        );
        eval.run().map_err(|error| {
            let (line, col) = error.span.line_col(&self.source);
            EvalError {
                bar_index: ctx.bar_index,
                message: format!(
                    "{}:{}: {} ({})",
                    line,
                    col,
                    error.message,
                    error.code.as_str()
                ),
            }
        })
    }
}

impl Indicator for ScriptIndicator {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        self.run_once(bar, ctx, true)?;
        self.state.commit_bar(self.compiled.max_bars_back);
        // The append is the last step: an Err above leaves no half row.
        self.plots.push_row(&self.row);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        let snapshot = self.state.snapshot();
        let result = self.run_once(partial, ctx, false);
        // The forming bar's transient objects travel in the frame; the
        // committed store is restored right after (preview rollback).
        let objects =
            (result.is_ok() && self.draws_objects()).then(|| self.state.objects.snapshot());
        self.state.restore(snapshot, &self.compiled.slot_modes);
        result?;
        let mut frame = PreviewFrame::new(self.row.clone());
        frame.objects = objects;
        Ok(frame)
    }

    fn reset(&mut self) {
        self.state.reset();
        self.plots.clear();
        self.row.fill(f64::NAN);
    }

    fn objects(&self) -> Option<&quantick_indicators::ObjectStore> {
        self.draws_objects().then_some(&self.state.objects)
    }
}
