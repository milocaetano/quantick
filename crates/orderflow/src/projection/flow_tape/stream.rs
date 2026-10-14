//! Source packets are nondropping; view requests may supersede older layouts.
use super::constants::{
    EVICTION_RESET_DIVISOR, KEEP_MARGIN_SLOTS, MAX_SOURCE_CHUNK, PUBLICATION_GROWTH_FACTOR,
    SOURCE_CHUNK,
};
use super::*;
use std::ops::Range;

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
    /// One past the newest ordinal the publication held.
    newest: usize,
    at_capacity: bool,
}
impl FlowPublication {
    fn rebase(&mut self, requested: &Range<usize>, loaded: usize) {
        if self.last.as_ref().is_some_and(|last| {
            (last.requested != *requested
                && (last.requested.end <= requested.start || requested.end <= last.requested.start))
                // Substantial eviction starts a fresh partial; a small live pan does not.
                || loaded.saturating_mul(EVICTION_RESET_DIVISOR) < last.loaded
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
    /// A change is published at once when it completes the request, reaches
    /// the cache bound, or holds a newer print than the last publication —
    /// no milestone ever delays a new print. Older history a cold fill adds
    /// waits for geometric milestones, which bound total cold-fill work, or
    /// for the host's quiet `flush`.
    fn due(
        &self,
        request: &FlowRequest,
        coverage: Coverage,
        at_capacity: bool,
        flush: bool,
    ) -> bool {
        let Coverage { loaded, newest } = coverage;
        self.dirty(request, loaded, at_capacity)
            && (flush
                || loaded == request.requested.len()
                || at_capacity
                || self.last.as_ref().is_none_or(|last| {
                    newest > last.newest
                        || loaded
                            >= last
                                .loaded
                                .saturating_mul(PUBLICATION_GROWTH_FACTOR)
                                .max(SOURCE_CHUNK)
                }))
    }
    fn record(&mut self, request: &FlowRequest, coverage: Coverage, at_capacity: bool) {
        self.last = Some(PublishedCoverage {
            layout_revision: request.layout_revision,
            requested: request.requested.clone(),
            loaded: coverage.loaded,
            newest: coverage.newest,
            at_capacity,
        });
    }
}
/// How much of a request a source holds, and how far its newest print runs.
#[derive(Clone, Copy)]
struct Coverage {
    loaded: usize,
    newest: usize,
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
        let coverage = Coverage {
            loaded: self.source.coverage().count(request.requested.clone()),
            newest: self.source.coverage().newest(request.requested.clone()),
        };
        let at_capacity = self.source.len() == MAX_FLOW_EXECUTIONS;
        if !self.publication.due(request, coverage, at_capacity, flush) {
            return None;
        }
        let frame = self.project(request);
        self.publication.record(request, coverage, at_capacity);
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
#[derive(Default)]
pub struct FlowSession<R> {
    request: Option<FlowRequest>,
    submitted: FlowCoverage,
    unsent: Option<FlowChunk>,
    runner: R,
    progress: FlowProgress,
    frame: Option<Arc<FlowTapeFrame>>,
    /// The frame on screen when the series was recut, in the new series'
    /// coordinates ([`Self::carry_across`]): drawn until the new source
    /// covers its prints.
    carried: Option<Arc<FlowTapeFrame>>,
    /// The newest print the last capture saw ([`Self::note_tail`]).
    tail: Option<FlowTail>,
    ignore_opening: bool,
}
impl<R: FlowRunner> FlowSession<R> {
    pub fn active(config: &crate::HeatmapConfig) -> bool {
        (config.volume_dots.tick_bubbles.is_some()
            || (config.live_lane.native() && config.volume_dots.enabled))
            && config.show_aggressions
            && !config.tape_only()
    }
    pub fn replaces_history(&self, config: &crate::HeatmapConfig) -> bool {
        Self::active(config) && self.request.is_some()
    }
    pub fn frame(&self) -> Option<&FlowTapeFrame> {
        self.frame.as_deref()
    }
    /// Immutable publication identity, including distinct partial frames.
    pub fn frame_handle(&self) -> Option<&Arc<FlowTapeFrame>> {
        self.frame.as_ref()
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
    /// The newest print the last capture saw, which tells the next capture
    /// whether a new epoch is a recut of the same tape.
    pub fn tail(&self) -> Option<FlowTail> {
        self.tail
    }
    pub fn note_tail(&mut self, tail: Option<FlowTail>) {
        self.tail = tail;
    }
    /// The series was recut into `epoch` from the same tape — a History
    /// install, a footprint refold. Keep drawing `carry` of the frame on
    /// screen, moved into the new series' coordinates, until the new
    /// epoch's newest-first fill covers its prints: the spheres neither
    /// vanish nor refill from the oldest candle. Call it before
    /// [`Self::project`] with the new epoch; anything else is a new tape.
    pub fn carry_across(
        &mut self,
        epoch: u64,
        carry: impl FnOnce(&FlowTapeFrame) -> Option<FlowTapeFrame>,
    ) {
        if self.request.as_ref().is_none_or(|held| held.epoch == epoch) {
            return;
        }
        self.carried = self.frame.as_deref().and_then(carry).map(Arc::new);
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
            // A recut of the same tape draws its carried frame meanwhile.
            self.carried = self
                .carried
                .take()
                .filter(|carried| carried.source_revision == request.epoch);
            self.frame.clone_from(&self.carried);
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
            // A publication still filling keeps the carried spheres its
            // newest-first fill has not reached; a complete one replaces them.
            self.frame = Some(match &self.carried {
                Some(carried)
                    if carried.source_revision == frame.source_revision
                        && frame.omitted_executions > 0 =>
                {
                    Arc::new(frame.with_carried(carried))
                }
                _ => {
                    self.carried = None;
                    frame
                }
            });
        }
        let room = MAX_FLOW_EXECUTIONS.saturating_sub(self.submitted.count(0..usize::MAX));
        if self.unsent.is_none()
            && room > 0
            && let Some(gap) = self.submitted.last_gap(request.requested.clone())
        {
            // Newest first, so the candles at the live edge fill before older
            // ones. Keep the first partial small, then use the already
            // accepted coverage to grow cold-fill throughput without an
            // unbounded copy.
            let chunk = self
                .submitted
                .count(request.requested.clone())
                .clamp(SOURCE_CHUNK, MAX_SOURCE_CHUNK);
            let start = gap.start.max(gap.end.saturating_sub(chunk.min(room)));
            self.unsent = Some(capture(start..gap.end));
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

#[cfg(test)]
#[path = "tests/recorded_performance.rs"]
mod recorded_performance;
