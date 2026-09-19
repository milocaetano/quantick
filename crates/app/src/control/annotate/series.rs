//! The annotation validator reads a pane through its headless series port.

use super::{ChartAnnotationInput, capability_unavailable, parse_price};
use crate::pane::ChartPane;
use quantick_chart_interaction::{
    annotation::{self, InputAnchor, Refusal, Series},
    quick_range::{Owner, RangeContext},
};
use quantick_control::error::ControlError;

struct PaneSeries<'a> {
    tab: u64,
    pane: &'a ChartPane,
}

impl Series for PaneSeries<'_> {
    fn context(&self) -> RangeContext {
        RangeContext {
            owner: Owner {
                tab: self.tab,
                pane: self.pane.id,
                layout: self.pane.layout_id().map(|id| id.0),
            },
            revision: self.pane.pagination_revision(),
        }
    }
    fn slots(&self) -> usize {
        self.pane.slots()
    }
    fn slot_at_time(&self, time: i64) -> Option<usize> {
        self.pane.slot_at_time(time)
    }
    fn open_time(&self, slot: usize) -> Option<i64> {
        self.pane.slot_open_time(slot)
    }
    fn time_at_position(&self, bar: f32) -> Option<i64> {
        self.pane.series_read().anchor_time(bar)
    }
    fn draft_in_progress(&self) -> bool {
        self.pane.drawings.draft().is_some()
    }
}

pub(super) fn resolve(
    pane: &ChartPane,
    tab: u64,
    input: &ChartAnnotationInput,
    required: usize,
) -> Result<Vec<annotation::ResolvedAnchor>, ControlError> {
    let anchors = input
        .anchors
        .iter()
        .map(|anchor| {
            Ok(InputAnchor {
                bar: anchor.validation_bar(input.chart_reference.is_some())?,
                price: parse_price(&anchor.price)?,
                time_ms: anchor.time_unix_ms,
            })
        })
        .collect::<Result<Vec<_>, ControlError>>()?;
    let reference = input
        .chart_reference
        .map(|reference| annotation::ChartReference {
            pane_id: reference.pane_id.get(),
            revision: reference.series_revision.get(),
            layout_id: reference.layout_id.map(|id| id.get()),
        });
    annotation::resolve(&PaneSeries { tab, pane }, &anchors, reference, required).map_err(refused)
}

/// v1's time-only anchor: the slot a market time falls on, and the time that slot opened.
pub(super) fn resolve_slot(
    pane: &ChartPane,
    tab: u64,
    time_unix_ms: i64,
) -> Result<(usize, i64), ControlError> {
    slot_of(&PaneSeries { tab, pane }, time_unix_ms)
}

fn slot_of(series: &impl Series, time_unix_ms: i64) -> Result<(usize, i64), ControlError> {
    if series.slots() == 0 {
        return Err(refused(Refusal::NoBars));
    }
    let slot = series
        .slot_at_time(time_unix_ms)
        .ok_or_else(|| refused(Refusal::NoCoveringBar))?;
    Ok((slot, series.open_time(slot).unwrap_or(time_unix_ms)))
}

/// One refusal, one answer, whichever wire version asked.
pub(super) fn refused(refusal: Refusal) -> ControlError {
    let mut error = if refusal.is_unavailable() {
        capability_unavailable(refusal.message())
    } else {
        ControlError::invalid_request(refusal.message())
    };
    error
        .context
        .next_steps
        .extend(refusal.next_step().map(str::to_owned));
    error
}

#[cfg(test)]
#[path = "tests/series_tests.rs"]
mod series_tests;
