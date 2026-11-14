//! Reusable observation adapter. Consumers supply facts and clocks; this owner
//! caches bindings and geometry and elects one evaluator for mirrored markets.
use crate::{
    AlertBook, AlertEventSink, AlertInput, BarIdentity, DrawingFact, DurableDrawingRef, Geometry,
    MarketRef, ObservationKind, ResetReason, SourceContext, Unavailable,
};
use std::path::Path;

/// Borrowed identity avoids constructing source strings on every print.
#[derive(Clone, Copy)]
pub enum SourceKey<'a> {
    Live(&'a str),
    Replay(&'a Path),
}
pub struct Source<'a> {
    pub key: SourceKey<'a>,
    pub symbol: &'a str,
    pub generation: u64,
    pub availability: Result<(), Unavailable>,
}
/// The native series identity is opaque to this adapter. Its text is requested
/// only when a new binding is installed.
/// Known mirrors are established by the drawing owner, never inferred from
/// equal geometry. Duplicates within one pane remain ambiguous regardless.
#[derive(Clone, Copy)]
pub struct Drawing<'a> {
    pub target: &'a DurableDrawingRef,
    pub geometry: Result<Geometry, Unavailable>,
    pub known_mirror: bool,
}
pub struct Pane<'a, S> {
    pub id: u64,
    pub spec: &'a S,
    pub drawing_revision: u64,
    pub hydrating: bool,
}
#[derive(Clone, Copy)]
pub struct Observation {
    pub kind: ObservationKind,
    pub prefix_slots: usize,
    pub committed_slots: usize,
    pub has_partial: bool,
    pub close_slot: Option<usize>,
    pub price: Option<f64>,
    pub utc_ms: i64,
    pub session_ms: u64,
}
impl Observation {
    /// A trade after the final close still belongs to that committed bar when
    /// the aggregator did not start a partial. Never invent the next bar.
    #[must_use]
    pub fn slot(self) -> usize {
        self.close_slot.unwrap_or_else(|| {
            self.prefix_slots
                + self
                    .committed_slots
                    .saturating_sub(usize::from(!self.has_partial))
        })
    }
}
pub struct Observer<S> {
    sources: Vec<SourceCache>,
    panes: Vec<PaneCache<S>>,
    current: usize,
    generation: u64,
    availability: Result<(), Unavailable>,
    frame: u64,
    epoch: u64,
    current_panes: Vec<u64>,
}
struct LifecycleOnly;
impl AlertEventSink for LifecycleOnly {
    fn try_push(&mut self, _: crate::AlertEvent) -> bool {
        false
    }
}
struct SourceCache {
    source: SourceContext,
    symbol: String,
}
struct PaneCache<S> {
    id: u64,
    source: usize,
    spec: S,
    market: MarketRef,
    generation: u64,
    epoch: u64,
    seen: u64,
    owner: bool,
    revision: Option<u64>,
    last: Observation,
    hydrating: bool,
    availability: Result<(), Unavailable>,
    targets: Vec<DurableDrawingRef>,
    drawings: Vec<Projected>,
}
struct Projected {
    target: DurableDrawingRef,
    revision: u64,
    geometry: Result<Geometry, Unavailable>,
    known_mirror: bool,
}
impl<S> Default for Observer<S> {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            panes: Vec::new(),
            current: 0,
            generation: 0,
            availability: Err(Unavailable::SourceInactive),
            frame: 0,
            epoch: 0,
            current_panes: Vec::new(),
        }
    }
}
impl<S: Copy + PartialEq> Observer<S> {
    pub fn begin(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }
    pub fn end(&mut self, book: &mut AlertBook, utc_ms: i64, session_ms: u64) {
        for pane in &self.panes {
            if pane.owner && pane.seen != self.frame {
                book.reset(
                    &pane.market,
                    ResetReason::SourceChanged,
                    BarIdentity {
                        epoch: pane.epoch,
                        sequence: 0,
                    },
                );
            }
        }
        self.panes.retain(|pane| pane.seen == self.frame);
        // Prefer an available observer, keeping a healthy incumbent stable.
        for index in 0..self.panes.len() {
            if self.panes[index].healthy() {
                self.elect(index, book);
            }
        }
        for index in 0..self.panes.len() {
            self.elect(index, book);
        }
        self.prune_sources();
        // Every mirror has now refreshed. Preflight cannot observe stale geometry
        // merely because its canonical source happened to be visited first.
        for index in 0..self.panes.len() {
            if self.panes[index].owner {
                let mut observation = self.panes[index].last;
                observation.kind = ObservationKind::Lifecycle;
                observation.utc_ms = utc_ms;
                observation.session_ms = session_ms;
                self.evaluate(index, book, &mut LifecycleOnly, observation);
            }
        }
        book.tick(utc_ms, session_ms);
    }
    /// Select a source before its pane/reset callbacks. `end` finishes this
    /// selection; the next census or batch starts with another `source` call.
    pub fn source(&mut self, source: Source<'_>) {
        self.current_panes.clear();
        self.current = self
            .sources
            .iter()
            .position(|cached| {
                cached.symbol == source.symbol
                    && match (&cached.source, source.key) {
                        (SourceContext::Live { feed }, SourceKey::Live(name)) => feed == name,
                        (SourceContext::Replay { recording }, SourceKey::Replay(path)) => {
                            Path::new(recording) == path
                        }
                        _ => false,
                    }
            })
            .unwrap_or_else(|| {
                self.sources.push(SourceCache {
                    source: match source.key {
                        SourceKey::Live(feed) => SourceContext::Live {
                            feed: feed.to_owned(),
                        },
                        SourceKey::Replay(path) => SourceContext::Replay {
                            recording: path.to_string_lossy().into_owned(),
                        },
                    },
                    symbol: source.symbol.to_owned(),
                });
                self.sources.len() - 1
            });
        self.generation = source.generation;
        self.availability = source.availability;
    }
    pub fn reset(&mut self, book: &mut AlertBook, reason: ResetReason) {
        self.epoch = self.epoch.wrapping_add(1);
        for pane in &mut self.panes {
            if pane.source == self.current && self.current_panes.contains(&pane.id) {
                // Reanchoring can change geometry without changing drawing revision,
                // including on a mirror that does not own market evaluation.
                pane.revision = None;
                if pane.owner {
                    pane.epoch = self.epoch;
                    book.reset(
                        &pane.market,
                        reason,
                        BarIdentity {
                            epoch: pane.epoch,
                            sequence: 0,
                        },
                    );
                }
            }
        }
    }
    pub fn pane<'a>(
        &mut self,
        book: &mut AlertBook,
        out: &mut impl AlertEventSink,
        facts: Pane<'_, S>,
        observation: Observation,
        aggregation: impl FnOnce() -> String,
        drawings: impl Iterator<Item = Drawing<'a>>,
    ) {
        let index = self
            .panes
            .iter()
            .position(|pane| {
                pane.id == facts.id && pane.source == self.current && pane.spec == *facts.spec
            })
            .unwrap_or_else(|| {
                let source = &self.sources[self.current];
                let owner = !self.panes.iter().any(|pane| {
                    pane.source == self.current && pane.spec == *facts.spec && pane.owner
                });
                self.epoch = self.epoch.wrapping_add(1);
                self.panes.push(PaneCache {
                    id: facts.id,
                    source: self.current,
                    spec: *facts.spec,
                    market: MarketRef {
                        source: source.source.clone(),
                        symbol: source.symbol.clone(),
                        aggregation: aggregation(),
                    },
                    generation: self.generation,
                    epoch: self.epoch,
                    seen: self.frame,
                    owner,
                    revision: None,
                    last: observation,
                    hydrating: facts.hydrating,
                    availability: self.availability,
                    targets: Vec::new(),
                    drawings: Vec::new(),
                });
                self.panes.len() - 1
            });
        let pane = &mut self.panes[index];
        if !self.current_panes.contains(&facts.id) {
            self.current_panes.push(facts.id);
        }
        pane.seen = self.frame;
        pane.last = observation;
        pane.hydrating = facts.hydrating;
        pane.availability = self.availability;
        if pane.generation != self.generation {
            pane.generation = self.generation;
            pane.revision = None;
            self.epoch = self.epoch.wrapping_add(1);
            pane.epoch = self.epoch;
            if pane.owner {
                book.reset(
                    &pane.market,
                    ResetReason::Reconnect,
                    BarIdentity {
                        epoch: pane.epoch,
                        sequence: 0,
                    },
                );
            }
        }
        pane.refresh(book, facts.drawing_revision, drawings);
        self.elect(index, book);
        if !self.panes[index].owner || observation.kind == ObservationKind::Lifecycle {
            return;
        }
        self.evaluate(index, book, out, observation);
    }
    fn elect(&mut self, index: usize, book: &mut AlertBook) {
        let candidate = &self.panes[index];
        let incumbent = self
            .panes
            .iter()
            .position(|pane| pane.owner && pane.market == candidate.market);
        if incumbent == Some(index)
            || incumbent.is_some_and(|owner| self.panes[owner].healthy() || !candidate.healthy())
        {
            return;
        }
        if let Some(owner) = incumbent {
            self.panes[owner].owner = false;
        }
        self.epoch = self.epoch.wrapping_add(1);
        let pane = &mut self.panes[index];
        pane.owner = true;
        pane.epoch = self.epoch;
        // Different subscribers need not have identical bar sequences or tape
        // coverage. Handover establishes a new baseline, never replays a print.
        book.reset(
            &pane.market,
            ResetReason::SourceChanged,
            BarIdentity {
                epoch: pane.epoch,
                sequence: 0,
            },
        );
    }
    fn prune_sources(&mut self) {
        for index in (0..self.sources.len()).rev() {
            if self.panes.iter().any(|pane| pane.source == index) {
                continue;
            }
            self.sources.remove(index);
            for pane in &mut self.panes {
                if pane.source > index {
                    pane.source -= 1;
                }
            }
        }
        self.current = 0;
        self.current_panes.clear();
    }
    fn evaluate(
        &self,
        index: usize,
        book: &mut AlertBook,
        out: &mut impl AlertEventSink,
        observation: Observation,
    ) {
        let pane = &self.panes[index];
        let slot = observation.slot();
        let mut input = AlertInput {
            market: &pane.market,
            utc_ms: observation.utc_ms,
            session_ms: observation.session_ms,
            bar: BarIdentity {
                epoch: pane.epoch,
                sequence: slot as u64,
            },
            kind: observation.kind,
            coordinate: slot as f64,
            price: observation.price.unwrap_or(f64::NAN),
            availability: pane.availability.and(if pane.hydrating {
                Err(Unavailable::Hydrating)
            } else if observation.price.is_none() {
                Err(Unavailable::SourceInactive)
            } else {
                Ok(())
            }),
            drawings: &[],
        };
        if pane.targets.is_empty() {
            book.evaluate(&input, out);
            return;
        }
        const LIMIT: usize = crate::MAX_ALERTS * crate::MAX_CONDITIONS;
        // One slot per referenced target; unrelated drawings cannot consume it.
        // A conflicting duplicate becomes a structured unavailable fact in-place.
        let mut projected: [DrawingFact<'_>; LIMIT] = std::array::from_fn(|_| DrawingFact {
            target: &pane.targets[0],
            revision: 0,
            geometry: Err(Unavailable::MissingDrawing),
        });
        let mut count = 0;
        let mut mirrors = [false; LIMIT];
        for cached in self
            .panes
            .iter()
            .filter(|cached| cached.market == pane.market && cached.revision.is_some())
        {
            for drawing in &cached.drawings {
                if !pane.targets.contains(&drawing.target) {
                    continue;
                }
                if let Some(existing_index) = projected[..count]
                    .iter()
                    .position(|fact| *fact.target == drawing.target)
                {
                    let existing = &mut projected[existing_index];
                    if !mirrors[existing_index]
                        || !drawing.known_mirror
                        || existing.geometry != drawing.geometry
                    {
                        existing.geometry = Err(Unavailable::AmbiguousDrawing);
                    }
                    existing.revision = existing.revision.max(drawing.revision);
                } else {
                    projected[count] = DrawingFact {
                        target: &drawing.target,
                        revision: drawing.revision,
                        geometry: drawing.geometry,
                    };
                    mirrors[count] = drawing.known_mirror;
                    count += 1;
                }
            }
        }
        input.drawings = &projected[..count];
        book.evaluate(&input, out);
    }
}
impl<S> PaneCache<S> {
    fn healthy(&self) -> bool {
        self.availability.is_ok() && !self.hydrating && self.last.price.is_some_and(f64::is_finite)
    }

    fn refresh<'a>(
        &mut self,
        book: &AlertBook,
        revision: u64,
        drawings: impl Iterator<Item = Drawing<'a>>,
    ) {
        if !book.drawing_targets(&self.market).eq(self.targets.iter()) {
            self.targets = book.drawing_targets(&self.market).cloned().collect();
            self.revision = None;
        }
        if self.revision == Some(revision) {
            return;
        }
        let mut projected: Vec<Projected> = Vec::new();
        if !self.targets.is_empty() {
            for drawing in drawings {
                if !self.targets.contains(drawing.target) {
                    continue;
                }
                if let Some(existing) = projected
                    .iter_mut()
                    .find(|item| item.target == *drawing.target)
                {
                    existing.geometry = Err(Unavailable::AmbiguousDrawing);
                    continue;
                }
                projected.push(Projected {
                    target: drawing.target.clone(),
                    revision: 0,
                    geometry: drawing.geometry,
                    known_mirror: drawing.known_mirror,
                });
            }
        }
        for drawing in &mut projected {
            drawing.revision = self
                .drawings
                .iter()
                .find(|old| old.target == drawing.target)
                .map_or(0, |old| {
                    old.revision
                        .wrapping_add(u64::from(old.geometry != drawing.geometry))
                });
        }
        self.drawings = projected;
        self.revision = Some(revision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn see(observer: &mut Observer<u8>, book: &mut AlertBook, source: Source<'_>, id: u64) {
        observer.source(source);
        observer.pane(
            book,
            &mut LifecycleOnly,
            Pane {
                id,
                spec: &1,
                drawing_revision: 0,
                hydrating: false,
            },
            Observation {
                kind: ObservationKind::Lifecycle,
                prefix_slots: 0,
                committed_slots: 1,
                has_partial: false,
                close_slot: None,
                price: Some(99.0),
                utc_ms: 1,
                session_ms: 1,
            },
            || "tick:1".into(),
            std::iter::empty(),
        );
    }
    #[test]
    fn closed_and_switched_sources_are_pruned_and_live_indexes_remapped() {
        let mut observer = Observer::default();
        let mut book = AlertBook::default();
        for index in 0..100 {
            let identity = format!("recording-{index}");
            observer.begin();
            see(
                &mut observer,
                &mut book,
                Source {
                    key: SourceKey::Replay(Path::new(&identity)),
                    symbol: "TEST",
                    generation: 0,
                    availability: Ok(()),
                },
                1,
            );
            see(
                &mut observer,
                &mut book,
                Source {
                    key: SourceKey::Live("fixture"),
                    symbol: "STABLE",
                    generation: 0,
                    availability: Ok(()),
                },
                2,
            );
            observer.end(&mut book, 1, 1);
            assert_eq!(observer.sources.len(), 2);
            assert_eq!(observer.panes.len(), 2);
            for pane in &observer.panes {
                assert_eq!(observer.sources[pane.source].source, pane.market.source);
                assert_eq!(observer.sources[pane.source].symbol, pane.market.symbol);
            }
        }
        observer.begin();
        see(
            &mut observer,
            &mut book,
            Source {
                key: SourceKey::Live("fixture"),
                symbol: "STABLE",
                generation: 0,
                availability: Ok(()),
            },
            2,
        );
        observer.end(&mut book, 1, 1);
        assert_eq!(observer.sources.len(), 1);
        assert_eq!(observer.panes[0].source, 0);
        observer.begin();
        observer.end(&mut book, 1, 1);
        assert!(observer.sources.is_empty());
        assert!(observer.panes.is_empty());
    }
}
