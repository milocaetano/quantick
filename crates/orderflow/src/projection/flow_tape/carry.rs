//! A FLOW frame carried across a recut of the same tape — a History
//! install, a footprint refold — so the spheres on screen stay drawn at
//! their own prints while the new series' source refills, newest first.
use super::{FlowMember, FlowMembers, FlowTapeFrame};
use rust_decimal::Decimal;
use std::sync::Arc;

/// The newest print a FLOW capture saw, and how many prints of the tape were
/// history then: what tells a recut of the same tape (older prints added in
/// front, every later print where it was) from another tape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowTail {
    /// Prints that were history rather than live.
    pub backfill: usize,
    /// The newest print's ordinal in that series.
    pub ordinal: usize,
    pub agg_id: u64,
    pub timestamp_ms: i64,
}

/// Where a member's print sits in candle coordinates: its slot plus the
/// middle of its accepted position, as the source places it.
fn position(slot: usize, accepted_ordinal: usize, ticks_per_bar: Decimal) -> Decimal {
    Decimal::from(slot) + (Decimal::from(accepted_ordinal) + Decimal::new(5, 1)) / ticks_per_bar
}

/// `slot` moved as far as `from` moved to `to`, never below the first slot.
fn moved(slot: usize, from: usize, to: usize) -> usize {
    (slot + to).saturating_sub(from)
}

impl FlowTapeFrame {
    /// This frame in another series' coordinates, as `epoch`'s: every
    /// member at the ordinal, slot and accepted position `locate` gives its
    /// print there, each sphere moved as far as its first print moved. No
    /// fact changes; `None` when a print has no place in the new series.
    pub fn carried(
        &self,
        epoch: u64,
        ticks_per_bar: Decimal,
        locate: impl Fn(usize) -> Option<(usize, usize, usize)>,
    ) -> Option<Self> {
        if ticks_per_bar <= Decimal::ZERO {
            return None;
        }
        let mut dots = Vec::with_capacity(self.dots.len());
        for dot in &self.dots {
            let members = dot
                .members
                .iter()
                .map(|member| {
                    let (ordinal, candle_slot, accepted_ordinal) = locate(member.ordinal)?;
                    Some(FlowMember {
                        ordinal,
                        candle_slot,
                        accepted_ordinal,
                        source_id: member.source_id,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let (old, new) = (dot.members.iter().next()?, members.first()?);
            let mut carried = dot.clone();
            carried.candle_position +=
                position(new.candle_slot, new.accepted_ordinal, ticks_per_bar)
                    - position(old.candle_slot, old.accepted_ordinal, ticks_per_bar);
            carried.first_slot = moved(dot.first_slot, old.candle_slot, new.candle_slot);
            carried.end_slot = moved(dot.end_slot, old.candle_slot, new.candle_slot);
            carried.members = FlowMembers {
                count: members.len(),
                parts: vec![Arc::new(members)],
            };
            dots.push(carried);
        }
        let ends = self.requested_ordinals.end.checked_sub(1).and_then(&locate);
        let mut frame = Self {
            source_revision: epoch,
            dots,
            ..self.clone()
        };
        if let Some((end, slot, _)) = ends {
            let start = (end + 1).saturating_sub(self.requested_ordinals.len());
            frame.requested_ordinals = start..end + 1;
            frame.computed_through_ordinal = moved(
                self.computed_through_ordinal,
                self.requested_ordinals.end - 1,
                end,
            );
            frame.covered_from_ordinal = moved(
                self.covered_from_ordinal,
                self.requested_ordinals.end - 1,
                end,
            );
            let last_slot = self.view.end_slot.saturating_sub(1);
            frame.view.first_slot = moved(self.view.first_slot, last_slot, slot);
            frame.view.end_slot = moved(self.view.end_slot, last_slot, slot);
        }
        frame.order_for_paint();
        Some(frame)
    }

    /// This publication of a series still filling, with the spheres of
    /// `carried` its newest-first fill has not reached: every print of each
    /// kept sphere is older than the coverage this frame was computed from.
    #[must_use]
    pub(super) fn with_carried(&self, carried: &Self) -> Self {
        let mut frame = self.clone();
        frame.dots.extend(
            carried
                .dots
                .iter()
                .filter(|dot| {
                    dot.members
                        .iter()
                        .all(|member| member.ordinal < self.covered_from_ordinal)
                })
                .cloned(),
        );
        frame.rescale_visible_floor();
        frame.order_for_paint();
        frame
    }
}
