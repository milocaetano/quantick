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
        self.pane.anchor_time(bar)
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
    annotation::resolve(&PaneSeries { tab, pane }, &anchors, reference, required).map_err(|error| {
        match error {
            Refusal::AnchorCount { expected, actual } => ControlError::invalid_request(format!("annotation takes exactly {expected} anchor(s), not {actual}")),
            Refusal::DraftInProgress => capability_unavailable("the trader is drawing on that pane right now; an annotation would land in their unfinished object"),
            Refusal::StaleReference => capability_unavailable("the range belongs to a different pane, layout or series revision; draw the range again"),
            Refusal::NoBars => capability_unavailable("that chart has no bars yet, so an anchor has nothing to land on"),
            other => ControlError::invalid_request(format!("invalid annotation coordinates: {other:?}")),
        }
    })
}
