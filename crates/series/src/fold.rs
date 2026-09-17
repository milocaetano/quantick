//! Streaming bar and optional footprint composition, without retained tape.
use quantick_engine::bar_registry::BarConfiguration;
use quantick_engine::{
    Bar, BarBuilder, BarBuilderDiagnostics, BarFootprint, BarProgress, DealSample, Trade,
};
use rust_decimal::Decimal;

use crate::footprint::FormingFootprint;

/// A close belongs to the caller immediately. The fold never stores a
/// history of bars or ladders, nor a copy of the arriving trade.
pub struct ClosedSeriesBar {
    pub bar: Bar,
    pub footprint: Option<BarFootprint>,
}

/// Stateful streaming fold through an engine-registered builder.
///
/// The optional ladder is absent by default, including its storage. A
/// configuration is resolved once at construction, never per print.
pub struct SeriesFold {
    builder: Box<dyn BarBuilder>,
    footprint: Option<FormingFootprint>,
}

impl SeriesFold {
    #[must_use]
    pub fn new(spec: impl Into<BarConfiguration>) -> Self {
        Self {
            builder: spec.into().build(),
            footprint: None,
        }
    }

    /// Start a fresh fold with capture enabled. Reconfiguring an existing
    /// tape is a retained-series rebuild, not a mid-bar toggle here.
    #[must_use]
    pub fn with_footprints(spec: impl Into<BarConfiguration>, group: Decimal) -> Self {
        let mut fold = Self::new(spec);
        fold.set_footprint_capture(Some(group));
        fold
    }

    /// Consume one atomic print and return at most one close by value.
    #[inline(always)]
    pub fn push(&mut self, trade: &Trade) -> Option<ClosedSeriesBar> {
        let Some(footprint) = self.footprint.as_mut() else {
            return self.builder.push(trade).map(|bar| ClosedSeriesBar {
                bar,
                footprint: None,
            });
        };
        let uncounted_before = self.builder.diagnostics().uncounted_trades;
        let closed = self.builder.push(trade);
        let uncounted = self.builder.diagnostics().uncounted_trades != uncounted_before;
        let footprint = match (&closed, uncounted) {
            (_, false) => footprint.observe(trade, closed.as_ref()),
            (Some(bar), true) => footprint.close_without(bar),
            (None, true) => None,
        };
        closed.map(|bar| ClosedSeriesBar { bar, footprint })
    }

    pub fn observe_deals(&mut self, sample: DealSample) {
        if let Some(input) = self.builder.deal_counter_input() {
            input.observe(sample);
        }
    }

    /// Seed retained evidence with one port query, including an empty batch.
    /// Builders without that input return before walking the slice.
    pub(crate) fn seed_deals(&mut self, samples: &[DealSample]) {
        let Some(input) = self.builder.deal_counter_input() else {
            return;
        };
        for sample in samples {
            input.observe(*sample);
        }
    }

    #[must_use]
    pub fn partial(&self) -> Option<&Bar> {
        self.builder.partial()
    }

    #[must_use]
    pub fn partial_footprint(&self) -> Option<&BarFootprint> {
        self.footprint.as_ref().and_then(FormingFootprint::partial)
    }

    #[must_use]
    pub fn diagnostics(&self) -> BarBuilderDiagnostics {
        self.builder.diagnostics()
    }

    #[must_use]
    pub fn progress(&self) -> Option<BarProgress> {
        self.builder.progress()
    }

    #[must_use]
    pub fn footprint_enabled(&self) -> bool {
        self.footprint.is_some()
    }

    pub(crate) fn set_footprint_capture(&mut self, group: Option<Decimal>) {
        self.footprint = group.map(FormingFootprint::new);
    }

    /// A regroup replaces only capture state. The real builder may have
    /// seen a different subset of deferred readings than the scratch fold.
    pub(crate) fn adopt_footprint(&mut self, folded: Self) {
        self.footprint = folded.footprint;
    }
}
