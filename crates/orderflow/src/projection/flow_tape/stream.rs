//! Source packets are nondropping; view requests may supersede older layouts.
use super::*;
use std::ops::Range;

// Small pans reuse whole candles while the retained source stays bounded.
const KEEP_MARGIN_SLOTS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct FlowRequest {
    pub epoch: u64,
    pub layout_revision: u64,
    pub source_count: usize,
    pub requested: Range<usize>,
    pub keep: FlowKeep,
    pub view: FlowTapeView,
    pub opening_windows: Vec<i64>,
    /// Canonical first daily source ordinals, selected before viewport admission.
    pub opening_ordinals: Vec<usize>,
}
/// Matching whole-candle and canonical ordinal bounds of the retained window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowKeep {
    pub slots: Range<usize>,
    pub ordinals: Range<usize>,
}

#[derive(Debug)]
pub struct OwnedFlowExecution {
    pub ordinal: usize,
    pub slot: usize,
    pub accepted_ordinal: usize,
    pub trade: Trade,
}
#[derive(Debug)]
pub struct FlowChunk {
    pub epoch: u64,
    pub ticks_per_bar: Decimal,
    pub executions: Vec<OwnedFlowExecution>,
}
#[derive(Debug, Default, Clone, Copy)]
pub struct FlowProgress {
    pub pending: bool,
    pub submitted_executions: usize,
    pub requested_executions: usize,
    pub layout_revision: u64,
    pub current_source_count: usize,
    pub requested_first_ordinal: usize,
    pub requested_end_ordinal: usize,
}
#[derive(Default)]
struct FlowPublication {
    last: Option<PublishedCoverage>,
}
struct PublishedCoverage {
    layout_revision: u64,
    requested: Range<usize>,
    loaded: usize,
    at_capacity: bool,
}
impl FlowPublication {
    fn rebase(&mut self, requested: &Range<usize>, loaded: usize) {
        if self.last.as_ref().is_some_and(|last| {
            (last.requested != *requested
                && (last.requested.end <= requested.start || requested.end <= last.requested.start))
                // Substantial eviction starts a fresh partial; a small live pan does not.
                || loaded.saturating_mul(2) < last.loaded
        }) {
            self.last = None;
        }
    }
    fn dirty(&self, request: &FlowRequest, loaded: usize, at_capacity: bool) -> bool {
        self.last.as_ref().is_none_or(|last| {
            last.layout_revision != request.layout_revision
                || last.requested != request.requested
                || last.loaded != loaded
                || last.at_capacity != at_capacity
        })
    }
    fn due(&self, request: &FlowRequest, loaded: usize, at_capacity: bool, flush: bool) -> bool {
        self.dirty(request, loaded, at_capacity)
            && (flush
                || loaded == request.requested.len()
                || at_capacity
                || self.last.as_ref().is_none_or(|last| {
                    // Geometric milestones bound total cold-fill projection work.
                    loaded >= last.loaded.saturating_mul(2).max(SOURCE_CHUNK)
                }))
    }
    fn record(&mut self, request: &FlowRequest, loaded: usize, at_capacity: bool) {
        self.last = Some(PublishedCoverage {
            layout_revision: request.layout_revision,
            requested: request.requested.clone(),
            loaded,
            at_capacity,
        });
    }
}
#[derive(Default)]
pub struct FlowWorkerCache {
    epoch: Option<u64>,
    source: FlowTapeSource,
    keep: Option<FlowKeep>,
    publication: FlowPublication,
}
impl FlowWorkerCache {
    pub fn select_epoch(&mut self, epoch: u64) {
        if self.epoch != Some(epoch) {
            self.epoch = Some(epoch);
            self.source = FlowTapeSource::default();
            self.keep = None;
            self.publication = FlowPublication::default();
        }
    }
    pub fn select_request(&mut self, request: &FlowRequest) {
        self.select_epoch(request.epoch);
        if self.keep.as_ref() != Some(&request.keep) {
            self.source.retain(&request.keep);
            self.keep = Some(request.keep.clone());
        }
        self.publication.rebase(
            &request.requested,
            self.source.coverage().count(request.requested.clone()),
        );
    }
    pub fn append(&mut self, chunk: &FlowChunk) {
        if self.epoch != Some(chunk.epoch) {
            return;
        }
        self.source.append(
            chunk
                .executions
                .iter()
                .filter(|item| {
                    self.keep.as_ref().is_none_or(|keep| {
                        keep.ordinals.contains(&item.ordinal) && keep.slots.contains(&item.slot)
                    })
                })
                .map(|item| FlowExecution {
                    ordinal: item.ordinal,
                    slot: item.slot,
                    accepted_ordinal: item.accepted_ordinal,
                    ticks_per_bar: chunk.ticks_per_bar,
                    trade: &item.trade,
                    opening: false,
                }),
        );
    }
    pub fn project(&self, request: &FlowRequest) -> Arc<FlowTapeFrame> {
        Arc::new(self.source.project(
            request.epoch,
            request.layout_revision,
            request.source_count,
            request.requested.clone(),
            request.view,
            FlowOpeningSelection {
                windows: &request.opening_windows,
                ordinals: &request.opening_ordinals,
            },
        ))
    }
    /// Fold packets freely; project geometric partials, completed requests and quiet flushes.
    /// A moving layout alone does not restart a large unfinished source fill.
    pub fn project_if_due(
        &mut self,
        request: &FlowRequest,
        flush: bool,
    ) -> Option<Arc<FlowTapeFrame>> {
        let loaded = self.source.coverage().count(request.requested.clone());
        let at_capacity = self.source.len() == MAX_FLOW_EXECUTIONS;
        if !self.publication.due(request, loaded, at_capacity, flush) {
            return None;
        }
        let frame = self.project(request);
        self.publication.record(request, loaded, at_capacity);
        Some(frame)
    }
    /// The host only needs an idle timeout while an unpublished change exists.
    pub fn publication_pending(&self, request: &FlowRequest) -> bool {
        self.publication.dirty(
            request,
            self.source.coverage().count(request.requested.clone()),
            self.source.len() == MAX_FLOW_EXECUTIONS,
        )
    }
    pub fn loaded(&self) -> usize {
        self.source.len()
    }
}

/// Host transport only: it may own a thread; this crate owns admission and adoption.
pub trait FlowRunner: Default {
    /// Replace view metadata; `notify` asks an idle runner to project without a packet.
    /// True means a stopped runner was restarted and needs retained source again.
    fn request(&mut self, request: &FlowRequest, notify: bool) -> bool;
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>>;
    fn finished(&self) -> Option<Arc<FlowTapeFrame>>;
}
const SOURCE_CHUNK: usize = 2048;
#[derive(Default)]
pub struct FlowSession<R> {
    request: Option<FlowRequest>,
    submitted: FlowCoverage,
    unsent: Option<FlowChunk>,
    runner: R,
    progress: FlowProgress,
    frame: Option<Arc<FlowTapeFrame>>,
    ignore_opening: bool,
}
impl<R: FlowRunner> FlowSession<R> {
    pub fn active(config: &crate::HeatmapConfig) -> bool {
        config.live_lane.native()
            && config.volume_dots.enabled
            && config.show_aggressions
            && !config.tape_only()
    }
    pub fn replaces_history(&self, config: &crate::HeatmapConfig) -> bool {
        Self::active(config) && self.request.is_some()
    }
    pub fn frame(&self) -> Option<&FlowTapeFrame> {
        self.frame.as_deref()
    }
    pub fn progress(&self) -> FlowProgress {
        self.progress
    }
    pub fn ignore_opening(&self) -> bool {
        self.ignore_opening
    }
    pub fn set_ignore_opening(&mut self, value: bool) -> bool {
        let changed = self.ignore_opening != value;
        self.ignore_opening = value;
        changed
    }
    pub fn clear(&mut self) {
        let ignore_opening = self.ignore_opening;
        *self = Self::default();
        self.ignore_opening = ignore_opening;
    }
    /// Retain a whole-candle neighborhood, recapturing evicted facts on later visits.
    pub fn keep_for(
        &self,
        epoch: u64,
        slots: Range<usize>,
        total_slots: usize,
        span: impl Fn(Range<usize>) -> Range<usize>,
    ) -> FlowKeep {
        let held = self.request.as_ref().filter(|held| {
            held.epoch == epoch
                && held.keep.slots.start <= slots.start
                && held.keep.slots.end >= slots.end
        });
        let keep_slots = held.map_or_else(
            || {
                slots.start.saturating_sub(KEEP_MARGIN_SLOTS)
                    ..slots.end.saturating_add(KEEP_MARGIN_SLOTS).min(total_slots)
            },
            |held| held.keep.slots.clone(),
        );
        let ordinals = span(keep_slots.clone());
        if ordinals.len() <= MAX_FLOW_EXECUTIONS {
            FlowKeep {
                slots: keep_slots,
                ordinals,
            }
        } else {
            FlowKeep {
                ordinals: span(slots.clone()),
                slots,
            }
        }
    }
    fn clear_admission(&mut self) {
        self.submitted = FlowCoverage::default();
        self.unsent = None;
        self.frame = None;
    }
    /// At most one bounded source packet is materialized. Full queues retain it.
    pub fn project(
        &mut self,
        mut request: FlowRequest,
        capture: impl FnOnce(Range<usize>) -> FlowChunk,
    ) {
        if self
            .request
            .as_ref()
            .is_none_or(|held| held.epoch != request.epoch)
        {
            self.clear_admission();
        }
        request.layout_revision = self.request.as_ref().map_or(0, |held| {
            held.layout_revision
                + u64::from(
                    held.view != request.view
                        || held.opening_windows != request.opening_windows
                        || held.opening_ordinals != request.opening_ordinals,
                )
        });
        self.submitted.retain(request.keep.ordinals.clone());
        if let Some(packet) = self.unsent.as_mut() {
            packet.executions.retain(|item| {
                request.keep.ordinals.contains(&item.ordinal)
                    && request.keep.slots.contains(&item.slot)
            });
            if packet.executions.is_empty() {
                self.unsent = None;
            }
        }
        let source_follows = self.unsent.is_some()
            || (self.submitted.count(0..usize::MAX) < MAX_FLOW_EXECUTIONS
                && self
                    .submitted
                    .first_gap(request.requested.clone())
                    .is_some());
        let changed = self.request.as_ref().is_none_or(|held| {
            held.epoch != request.epoch
                || held.layout_revision != request.layout_revision
                || held.requested != request.requested
                || held.keep != request.keep
        });
        if self.runner.request(&request, changed && !source_follows) {
            self.clear_admission();
        }
        if let Some(frame) = self.runner.finished()
            && frame.source_revision == request.epoch
        {
            self.frame = Some(frame);
        }
        let room = MAX_FLOW_EXECUTIONS.saturating_sub(self.submitted.count(0..usize::MAX));
        if self.unsent.is_none()
            && room > 0
            && let Some(gap) = self.submitted.first_gap(request.requested.clone())
        {
            let end = gap
                .end
                .min(gap.start.saturating_add(SOURCE_CHUNK.min(room)));
            self.unsent = Some(capture(gap.start..end));
        }
        if let Some(packet) = self.unsent.take() {
            let sent = packet
                .executions
                .first()
                .zip(packet.executions.last())
                .map(|(first, last)| first.ordinal..last.ordinal + 1);
            match self.runner.submit(packet) {
                Ok(()) => {
                    if let Some(range) = sent {
                        self.submitted.insert(range);
                        self.submitted.retain(request.keep.ordinals.clone());
                    }
                }
                Err(packet) => self.unsent = Some(*packet),
            }
        }
        self.progress = FlowProgress {
            pending: !request.requested.is_empty()
                && self.frame.as_ref().is_none_or(|frame| {
                    frame.layout_revision != request.layout_revision
                        || frame.requested_ordinals != request.requested
                        || frame.omitted_executions > 0
                }),
            submitted_executions: self.submitted.count(request.requested.clone()),
            requested_executions: request.requested.len(),
            current_source_count: request.source_count,
            requested_first_ordinal: request.requested.start,
            requested_end_ordinal: request.requested.end,
            layout_revision: request.layout_revision,
        };
        self.request = Some(request);
    }
}

#[cfg(test)]
#[path = "tests/session.rs"]
mod tests;
