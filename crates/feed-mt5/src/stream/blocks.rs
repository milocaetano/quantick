//! Synchronous candle accumulation and acknowledged depth-generation transitions.
#[cfg(test)]
#[path = "blocks_owner_tests.rs"]
mod owner_tests;

use crate::{
    depth::{BookMapper, BookStats},
    protocol,
    rates::{RateMapper, RateStats},
};
use quantick_engine::Bar;
use quantick_orderbook::{DepthEvent, DepthResyncReason, DepthStatus};
use std::collections::BTreeMap;

pub(super) const MAX_BARS_PER_BLOCK: usize = 1_000_000;
pub(super) struct RatesBlock {
    interval_ms: i64,
    mapper: RateMapper,
    bars: BTreeMap<i64, Bar>,
    truncated: bool,
}
impl RatesBlock {
    pub(super) fn new(interval_ms: i64, server_utc_offset_s: i64) -> Self {
        Self {
            interval_ms,
            mapper: RateMapper::new(interval_ms, server_utc_offset_s),
            bars: BTreeMap::new(),
            truncated: false,
        }
    }
    /// True exactly once when this block first reaches its cap.
    pub(super) fn absorb(&mut self, chunk: &protocol::RateChunk) -> bool {
        for row in &chunk.bars {
            if self.bars.len() >= MAX_BARS_PER_BLOCK {
                let first = !self.truncated;
                self.truncated = true;
                return first;
            }
            if let Some(bar) = self.mapper.map(row) {
                self.bars.insert(bar.open_time, bar);
            }
        }
        false
    }
    pub(super) fn len(&self) -> usize {
        self.bars.len()
    }
    pub(super) fn stats(&self) -> (RateStats, i64) {
        (self.mapper.stats, self.interval_ms)
    }
    pub(super) fn finish(self) -> (i64, Vec<Bar>, bool) {
        (
            self.interval_ms,
            self.bars.into_values().collect(),
            self.truncated,
        )
    }
}

enum DepthContinuation {
    Idle,
    Open {
        image: protocol::Book,
        base: u64,
    },
    Map(protocol::Book),
    Synchronized {
        generation: u64,
        status: DepthStatus,
    },
}

pub(super) struct DepthSession {
    symbol: String,
    mapper: Option<BookMapper>,
    publishing: bool,
    last_seq: Option<u64>,
    missing_capability_reported: bool,
    next: DepthContinuation,
}
impl DepthSession {
    pub(super) fn new(hello: &protocol::Hello, symbol: String) -> Self {
        Self {
            mapper: hello.book_levels.map(|levels| {
                BookMapper::new(
                    symbol.clone(),
                    0,
                    Some(levels),
                    hello.tick_size.as_deref(),
                    hello.server_utc_offset_s,
                )
            }),
            symbol,
            publishing: false,
            last_seq: None,
            missing_capability_reported: false,
            next: DepthContinuation::Idle,
        }
    }
    pub(super) fn declared(&self) -> bool {
        self.mapper.is_some()
    }
    pub(super) fn set_server_utc_offset_s(&mut self, offset: i64) {
        if let Some(mapper) = self.mapper.as_mut() {
            mapper.set_server_utc_offset_s(offset);
        }
    }
    fn status(&self, generation: u64, status: DepthStatus) -> DepthEvent {
        DepthEvent::Status {
            symbol: self.symbol.clone(),
            generation,
            status,
        }
    }
    pub(super) fn observe(
        &mut self,
        image: protocol::Book,
        enabled: bool,
        base: u64,
        offset: &mut u64,
    ) -> DepthStep {
        let Some(mapper) = self.mapper.as_mut() else {
            return DepthStep::Done;
        };
        let lost = self
            .last_seq
            .is_some_and(|last| image.seq != last.saturating_add(1));
        if !enabled {
            if self.publishing {
                let generation = mapper.generation();
                self.publishing = false;
                self.last_seq = None;
                mapper.restart(generation);
                self.next = DepthContinuation::Idle;
                return DepthStep::Publish(self.status(generation, DepthStatus::Stopped));
            }
            return DepthStep::Done;
        }
        if !self.publishing || mapper.generation() != base.saturating_add(*offset) || lost {
            if lost {
                let generation = mapper.generation();
                self.next = DepthContinuation::Open { image, base };
                return DepthStep::Publish(self.status(
                    generation,
                    DepthStatus::Resyncing {
                        reason: DepthResyncReason::SourceRestarted {
                            cause: "book_images_lost",
                        },
                    },
                ));
            }
            return self.open(image, base, offset);
        }
        self.map(image)
    }
    fn open(&mut self, image: protocol::Book, base: u64, offset: &mut u64) -> DepthStep {
        *offset = offset.saturating_add(1);
        let generation = base.saturating_add(*offset);
        self.mapper
            .as_mut()
            .expect("declared depth")
            .restart(generation);
        self.publishing = true;
        self.next = DepthContinuation::Map(image);
        DepthStep::Publish(self.status(generation, DepthStatus::Connecting))
    }
    fn map(&mut self, image: protocol::Book) -> DepthStep {
        self.last_seq = Some(image.seq);
        let mapper = self.mapper.as_mut().expect("declared depth");
        let mapped = mapper.map_with_diagnostics(&image);
        self.next = match &mapped.event {
            Some(DepthEvent::Snapshot { .. }) => DepthContinuation::Synchronized {
                generation: mapper.generation(),
                status: mapper.synchronized_status(),
            },
            _ => DepthContinuation::Idle,
        };
        DepthStep::Mapped(mapped)
    }
    pub(super) fn acknowledged(&mut self, offset: &mut u64) -> DepthStep {
        match std::mem::replace(&mut self.next, DepthContinuation::Idle) {
            DepthContinuation::Idle => DepthStep::Done,
            DepthContinuation::Open { image, base } => self.open(image, base, offset),
            DepthContinuation::Map(image) => self.map(image),
            DepthContinuation::Synchronized { generation, status } => {
                DepthStep::Publish(self.status(generation, status))
            }
        }
    }
    pub(super) fn missing(&mut self, enabled: bool, base: u64) -> Option<DepthEvent> {
        if self.mapper.is_some() || self.missing_capability_reported || !enabled {
            return None;
        }
        self.missing_capability_reported = true;
        Some(self.status(
            base,
            DepthStatus::Disconnected {
                error_class: "bridge_without_depth",
            },
        ))
    }
    pub(super) fn close(&self) -> (Option<BookStats>, Option<DepthEvent>) {
        let Some(mapper) = &self.mapper else {
            return (None, None);
        };
        (
            Some(mapper.stats),
            self.publishing.then(|| {
                self.status(
                    mapper.generation(),
                    DepthStatus::Disconnected {
                        error_class: "bridge_lost",
                    },
                )
            }),
        )
    }
}

pub(super) enum DepthStep {
    Done,
    Publish(DepthEvent),
    Mapped(crate::depth::MappedBook),
}
