//! A pane lent out beside the series its tab session owns for it and the
//! workers its tab keeps for it: the one address every pass that reads bars
//! goes through. The pane itself holds no bars; the tab pairs it with them by
//! the pane's id.

use std::ops::{Deref, DerefMut};

use quantick_session::PaneSeries;

use super::ChartPane;
use crate::tab::pane_workers::PaneWorkers;

/// A pane beside its series and workers, borrowed together.
#[derive(Clone, Copy)]
pub(crate) struct PaneAt<P, S, W> {
    pub pane: P,
    pub series: S,
    pub workers: W,
}

/// A pane and its series, read.
pub(crate) type PaneRef<'a> = PaneAt<&'a ChartPane, &'a PaneSeries, &'a PaneWorkers>;
/// A pane and its series, written.
pub(crate) type PaneMut<'a> = PaneAt<&'a mut ChartPane, &'a mut PaneSeries, &'a mut PaneWorkers>;

/// A part of a lent pane, read through `&` or `&mut` alike: the bounds every
/// reading pass over a [`PaneAt`] is written against.
pub(crate) trait LentPane: Deref<Target = ChartPane> {}
impl<T: Deref<Target = ChartPane>> LentPane for T {}
/// See [`LentPane`].
pub(crate) trait LentSeries: Deref<Target = PaneSeries> {}
impl<T: Deref<Target = PaneSeries>> LentSeries for T {}
/// See [`LentPane`].
pub(crate) trait LentWorkers: Deref<Target = PaneWorkers> {}
impl<T: Deref<Target = PaneWorkers>> LentWorkers for T {}

impl<P: Deref<Target = ChartPane>, S, W> Deref for PaneAt<P, S, W> {
    type Target = ChartPane;
    fn deref(&self) -> &ChartPane {
        &self.pane
    }
}

impl<P: DerefMut<Target = ChartPane>, S, W> DerefMut for PaneAt<P, S, W> {
    fn deref_mut(&mut self) -> &mut ChartPane {
        &mut self.pane
    }
}

impl<P: LentPane, S: LentSeries, W: LentWorkers> PaneAt<P, S, W> {
    /// The same pane, read.
    pub(crate) fn view(&self) -> PaneRef<'_> {
        PaneAt {
            pane: &self.pane,
            series: &self.series,
            workers: &self.workers,
        }
    }
}

/// A pane that owns its series and workers outright: a pane standing alone,
/// outside any tab.
#[cfg(test)]
pub(crate) struct PaneParts {
    pub pane: ChartPane,
    pub series: PaneSeries,
    pub workers: PaneWorkers,
}

#[cfg(test)]
impl PaneParts {
    pub(crate) fn new(pane: ChartPane) -> Self {
        let series = PaneSeries::new(pane.spec.spec());
        Self {
            pane,
            series,
            workers: PaneWorkers::default(),
        }
    }
    pub(crate) fn flow(
        id: u64,
        spec: impl Into<crate::state::BarConfiguration>,
        symbol: String,
    ) -> Self {
        Self::new(ChartPane::flow(id, spec, symbol))
    }
    pub(crate) fn time(id: u64, interval_ms: i64) -> Self {
        Self::new(ChartPane::time(id, interval_ms))
    }
    pub(crate) fn at(&mut self) -> PaneMut<'_> {
        PaneAt {
            pane: &mut self.pane,
            series: &mut self.series,
            workers: &mut self.workers,
        }
    }
    pub(crate) fn view(&self) -> PaneRef<'_> {
        PaneAt {
            pane: &self.pane,
            series: &self.series,
            workers: &self.workers,
        }
    }
}
