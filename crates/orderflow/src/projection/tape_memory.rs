//! Causal tape groups: the frontier may grow, settled memberships do not.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::dots::window_start;
use super::tape::{
    TapeReference, combine_tape_facts, merge_tape_dots_with_reference, tape_radius_limit,
};
use super::{
    AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry, normalized_area_size,
    position_tape_at,
};
use crate::config::{BubbleStyle, LiveLaneStyle};

#[cfg(test)]
#[path = "tests/dots_tests/tape_tests/native_fact_tests.rs"]
mod native_fact_tests;

/// Drawing inputs. Automatic price changes transform the retained facts;
/// explicit time-window or pixel-geometry changes start a new display epoch.
#[derive(Debug, Clone, Copy)]
pub struct TapeDotView {
    pub now_ms: i64,
    pub window_ms: i64,
    pub dot_window_ms: i64,
    pub evicted_through_ms: Option<i64>,
    pub prices: PriceWindow,
    pub geometry: TapeDotGeometry,
}

/// The visible groups and one common area scale; horizontal padding continues
/// to use the configured radius, never this possibly smaller collision cap.
pub struct TapeDotFrame {
    pub marks: Vec<AggressionPrimitive>,
    pub max_radius: f32,
    /// The exact reference used both for collision clearance and disc sizing.
    pub full_quantity: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct NativeKey(i64, Decimal);

#[derive(Debug, Clone)]
struct Group {
    mark: AggressionPrimitive,
    price_quantity: Decimal,
    // Native keys survive venue trade-ID restarts. Keeping their exact facts
    // also permits a late execution or canonical eviction to update one group.
    sources: BTreeMap<NativeKey, AggressionPrimitive>,
}

impl Group {
    fn of(sources: BTreeMap<NativeKey, AggressionPrimitive>) -> Self {
        let mark = combine_tape_facts(sources.values().cloned()).expect("a group has native facts");
        let price_quantity = sources
            .values()
            .map(|mark| mark.price * mark.quantity)
            .sum();
        Self {
            mark,
            price_quantity,
            sources,
        }
    }

    fn visible_at(&self, now_ms: i64, window_ms: i64) -> bool {
        self.mark.timestamp_quantity
            >= Decimal::from(now_ms.saturating_sub(window_ms)) * self.mark.quantity
    }

    fn retained_at(&self, now_ms: i64, window_ms: i64) -> bool {
        // An expired aggregate still consumes its later native members until
        // they leave the input. Retain their facts so a genuine late update
        // can move the complete group's mean back into view without duplication.
        self.visible_at(now_ms, window_ms)
            || self
                .sources
                .keys()
                .any(|key| key.0 >= now_ms.saturating_sub(window_ms))
    }

    fn refresh(&mut self) {
        if let Some(mark) = combine_tape_facts(self.sources.values().cloned()) {
            self.mark = mark;
            self.price_quantity = self
                .sources
                .values()
                .map(|mark| mark.price * mark.quantity)
                .sum();
        }
    }

    fn reference_quantity(&self, opening_bursts: &[i64]) -> Decimal {
        self.sources
            .iter()
            .filter(|(key, _)| !opening_bursts.contains(&key.0))
            .map(|(_, mark)| mark.quantity)
            .sum()
    }

    fn coincides(&self, other: &Self) -> bool {
        // Rounded quotients only select candidates; exact cross-products
        // prove equality before any native memberships can be combined.
        match (
            self.mark
                .timestamp_quantity
                .checked_mul(other.mark.quantity),
            other
                .mark
                .timestamp_quantity
                .checked_mul(self.mark.quantity),
            self.price_quantity.checked_mul(other.mark.quantity),
            other.price_quantity.checked_mul(self.mark.quantity),
        ) {
            (Some(a_time), Some(b_time), Some(a_price), Some(b_price)) => {
                a_time == b_time && a_price == b_price
            }
            _ => false,
        }
    }
}

/// Per-pane display history. Only newly closed native windows are merged;
/// each settled group retains its source facts until its whole centroid exits.
#[derive(Debug, Default)]
pub struct TapeDotMemory {
    epoch: Option<TapeDotView>,
    settled: Vec<Group>,
    frontier: Vec<Group>,
}

impl TapeDotMemory {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn retained_group_count(&self) -> usize {
        self.settled.len() + self.frontier.len()
    }

    /// Includes a group's factual centroid even after one native constituent
    /// crossed the left edge, so the independent tape axis still contains it.
    pub fn price_range(&self, now_ms: i64, window_ms: i64) -> Option<(f64, f64)> {
        self.settled
            .iter()
            .chain(&self.frontier)
            .filter(|group| group.visible_at(now_ms, window_ms))
            .filter_map(|group| group.mark.price.to_f64())
            .fold(None, |range, price| {
                Some(range.map_or((price, price), |(low, high): (f64, f64)| {
                    (low.min(price), high.max(price))
                }))
            })
    }

    pub fn project(
        &mut self,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        opening_bursts: &[i64],
    ) -> TapeDotFrame {
        if !lane.native() || !view.geometry.valid() || view.window_ms <= 0 {
            return TapeDotFrame {
                marks: marks.to_vec(),
                max_radius: bubbles.max_radius,
                full_quantity: sizing.full_quantity(marks, true),
            };
        }
        self.prepare_epoch(view);
        let mut incoming = native_facts(marks, view);
        let mut changed = self.replace_facts(&mut incoming, view.evicted_through_ms);
        let mut windows: BTreeMap<i64, BTreeMap<NativeKey, AggressionPrimitive>> = BTreeMap::new();
        for (key, mark) in incoming {
            windows
                .entry(key.0)
                .or_default()
                .insert(key, normalized_native_fact(mark));
        }
        let mut forming = Vec::new();
        for (window, sources) in windows {
            let groups = sources
                .into_iter()
                .map(|(key, mark)| Group::of(BTreeMap::from([(key, mark)])));
            let closed_at = window.saturating_add(view.dot_window_ms.max(1));
            if closed_at > view.now_ms {
                forming.extend(groups);
                continue;
            }
            self.seal_at(closed_at, view, bubbles.max_radius);
            let mut candidates = std::mem::take(&mut self.frontier);
            candidates.extend(groups);
            let quantities = reference_quantities(&candidates, opening_bursts);
            let reference = TapeReference {
                minimum_full: self.reference(
                    &candidates,
                    sizing,
                    closed_at,
                    view.window_ms,
                    opening_bursts,
                ),
                quantities: &quantities,
            };
            self.frontier = merge_groups(candidates, view, sizing, bubbles, lane, reference, None);
            changed = true;
        }
        if changed {
            self.coalesce_coincident();
        }
        self.seal_at(view.now_ms, view, bubbles.max_radius);
        let has_forming = !forming.is_empty();
        let mut active = self.frontier.clone();
        active.extend(forming);
        let quantities = reference_quantities(&active, opening_bursts);
        let reference = TapeReference {
            minimum_full: self.reference(
                &active,
                sizing,
                view.now_ms,
                view.window_ms,
                opening_bursts,
            ),
            quantities: &quantities,
        };
        let active = if has_forming {
            merge_groups(
                active,
                view,
                sizing,
                bubbles,
                lane,
                reference,
                Some(view.now_ms),
            )
        } else {
            active
        };
        // The same reference policy sizes frontier merges and final discs;
        // every factual quantity and weighted moment still includes the burst.
        let reference_quantities = (!opening_bursts.is_empty() && sizing.typed_full.is_none())
            .then(|| {
                self.settled
                    .iter()
                    .chain(&active)
                    .map(|group| group.reference_quantity(opening_bursts))
                    .collect::<Vec<_>>()
            });
        let mut shown: Vec<_> = self
            .settled
            .iter()
            .map(|group| group.mark.clone())
            .chain(active.into_iter().map(|group| group.mark))
            .collect();
        position_tape_at(
            &mut shown,
            view.now_ms,
            view.window_ms,
            view.geometry.left_x,
            view.dot_window_ms,
        );
        for mark in &mut shown {
            mark.y = view.prices.y_unclamped(mark.price).unwrap_or(mark.y);
        }
        let adjusted_full = reference_quantities.and_then(|quantities| {
            shown
                .iter()
                .zip(quantities)
                .filter(|(mark, _)| view.geometry.visible(mark))
                .map(|(_, quantity)| quantity)
                .max()
                .filter(|quantity| *quantity > Decimal::ZERO)
        });
        shown.retain(|mark| mark.x >= view.geometry.left_x && mark.x <= view.geometry.right_x);
        shown.sort_by(|a, b| {
            a.quantity
                .cmp(&b.quantity)
                .then(a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
                .then(a.agg_id.cmp(&b.agg_id))
        });
        let full = adjusted_full.unwrap_or_else(|| {
            sizing.full_quantity(
                shown.iter().filter(|mark| view.geometry.visible(mark)),
                true,
            )
        });
        for mark in &mut shown {
            mark.size = normalized_area_size(mark.quantity, full);
        }
        let final_sizing = DotSizing {
            typed_full: Some(full),
            ..sizing
        };
        let max_radius = tape_radius_limit(&shown, final_sizing, bubbles, lane, view.geometry);
        shown.extend(marks.iter().filter(|mark| !mark.live).cloned());
        TapeDotFrame {
            marks: shown,
            max_radius,
            full_quantity: full,
        }
    }

    fn prepare_epoch(&mut self, view: TapeDotView) {
        if self.epoch.is_some_and(|old| {
            old.window_ms != view.window_ms
                || old.dot_window_ms != view.dot_window_ms
                || old.geometry.width_px != view.geometry.width_px
                || old.geometry.height_px != view.geometry.height_px
        }) {
            self.clear();
        }
        self.epoch = Some(view);
    }

    fn replace_facts(
        &mut self,
        incoming: &mut BTreeMap<NativeKey, &AggressionPrimitive>,
        evicted: Option<i64>,
    ) -> bool {
        let mut any_changed = false;
        for group in self.settled.iter_mut().chain(&mut self.frontier) {
            let before = group.sources.len();
            if let Some(horizon) = evicted
                && group
                    .sources
                    .first_key_value()
                    .is_some_and(|(key, _)| key.0 <= horizon)
            {
                group.sources.retain(|key, _| key.0 > horizon);
            }
            let mut changed = before != group.sources.len();
            for (key, source) in &mut group.sources {
                if let Some(next) = incoming.remove(key)
                    && !same_native_fact(source, next)
                {
                    *source = normalized_native_fact(next);
                    changed = true;
                }
            }
            if changed {
                group.refresh();
                any_changed = true;
            }
        }
        self.settled.retain(|group| !group.sources.is_empty());
        self.frontier.retain(|group| !group.sources.is_empty());
        any_changed
    }

    fn coalesce_coincident(&mut self) {
        let mut candidates: BTreeMap<(Decimal, Decimal), Vec<usize>> = BTreeMap::new();
        let mut groups: Vec<(bool, Group)> = Vec::new();
        let retained = std::mem::take(&mut self.settled)
            .into_iter()
            .map(|group| (true, group))
            .chain(
                std::mem::take(&mut self.frontier)
                    .into_iter()
                    .map(|group| (false, group)),
            );
        for (sealed, group) in retained {
            let key = (
                group.mark.timestamp_quantity / group.mark.quantity,
                group.mark.price,
            );
            let indices = candidates.entry(key).or_default();
            if let Some(index) = indices
                .iter()
                .copied()
                .find(|index| groups[*index].1.coincides(&group))
            {
                let (was_sealed, existing) = &mut groups[index];
                *was_sealed |= sealed;
                existing.sources.extend(group.sources);
                existing.refresh();
            } else {
                indices.push(groups.len());
                groups.push((sealed, group));
            }
        }
        for (sealed, group) in groups {
            if sealed {
                self.settled.push(group);
            } else {
                self.frontier.push(group);
            }
        }
    }

    fn seal_at(&mut self, now_ms: i64, view: TapeDotView, radius: f32) {
        let frontier_ms = (2.0 * f64::from(radius) * view.window_ms as f64
            / f64::from(view.geometry.width_px.max(f32::MIN_POSITIVE)))
        .ceil()
        .min(view.window_ms as f64) as i64;
        let cutoff = now_ms.saturating_sub(frontier_ms.max(view.dot_window_ms));
        for group in std::mem::take(&mut self.frontier) {
            if group.mark.last_timestamp_ms <= cutoff {
                self.settled.push(group);
            } else {
                self.frontier.push(group);
            }
        }
        self.settled
            .retain(|group| group.retained_at(now_ms, view.window_ms));
        self.frontier
            .retain(|group| group.retained_at(now_ms, view.window_ms));
    }

    fn reference(
        &self,
        active: &[Group],
        sizing: DotSizing,
        now_ms: i64,
        window_ms: i64,
        opening_bursts: &[i64],
    ) -> Decimal {
        if sizing.typed_full.is_some() {
            return Decimal::ZERO;
        }
        let (eligible, factual) = self
            .settled
            .iter()
            .chain(active)
            .filter(|group| group.visible_at(now_ms, window_ms))
            .map(|group| {
                (
                    if opening_bursts.is_empty() {
                        group.mark.quantity
                    } else {
                        group.reference_quantity(opening_bursts)
                    },
                    group.mark.quantity,
                )
            })
            .fold(
                (Decimal::ZERO, Decimal::ZERO),
                |(eligible, factual), (next, quantity)| (eligible.max(next), factual.max(quantity)),
            );
        if eligible > Decimal::ZERO {
            eligible
        } else {
            factual
        }
    }
}

fn reference_quantities(groups: &[Group], opening_bursts: &[i64]) -> Vec<Decimal> {
    if opening_bursts.is_empty() {
        Vec::new()
    } else {
        groups
            .iter()
            .map(|group| group.reference_quantity(opening_bursts))
            .collect()
    }
}

fn native_facts(
    marks: &[AggressionPrimitive],
    view: TapeDotView,
) -> BTreeMap<NativeKey, &AggressionPrimitive> {
    marks
        .iter()
        .filter(|mark| mark.live && mark.quantity > Decimal::ZERO)
        .filter_map(|mark| {
            let window = window_start(mark.first_timestamp_ms, view.dot_window_ms);
            if view
                .evicted_through_ms
                .is_some_and(|horizon| window <= horizon)
            {
                return None;
            }
            if window < view.now_ms.saturating_sub(view.window_ms) {
                return None;
            }
            Some((NativeKey(window, mark.price_bucket), mark))
        })
        .collect()
}

fn normalized_native_fact(mark: &AggressionPrimitive) -> AggressionPrimitive {
    let mut source = mark.clone();
    source.x = 0.0;
    source.y = 0.0;
    source.size = 0.0;
    source
}

/// Compare retained facts without allocating their source/evidence vectors.
/// The exhaustive pattern makes new primitive fields require a decision;
/// every bound factual field participates, while only projection ink is ignored.
fn same_native_fact(left: &AggressionPrimitive, right: &AggressionPrimitive) -> bool {
    let AggressionPrimitive {
        agg_id,
        agg_ids,
        generation,
        side,
        consumed_side,
        quantity,
        buy_share,
        live,
        price_bucket,
        price_span,
        price,
        trade_count,
        first_timestamp_ms,
        last_timestamp_ms,
        timestamp_quantity,
        matched_quantity,
        buy_quantity,
        matched_fraction,
        liquidity_event_ids,
        folded_marks,
        x: _,
        y: _,
        size: _,
    } = left;
    *agg_id == right.agg_id
        && agg_ids == &right.agg_ids
        && *generation == right.generation
        && *side == right.side
        && *consumed_side == right.consumed_side
        && *quantity == right.quantity
        && *buy_share == right.buy_share
        && *live == right.live
        && *price_bucket == right.price_bucket
        && *price_span == right.price_span
        && *price == right.price
        && *trade_count == right.trade_count
        && *first_timestamp_ms == right.first_timestamp_ms
        && *last_timestamp_ms == right.last_timestamp_ms
        && *timestamp_quantity == right.timestamp_quantity
        && *matched_quantity == right.matched_quantity
        && *buy_quantity == right.buy_quantity
        && *matched_fraction == right.matched_fraction
        && liquidity_event_ids == &right.liquidity_event_ids
        && *folded_marks == right.folded_marks
}

/// The existing geometric merger operates on indivisible retained groups.
/// Temporary local IDs identify those groups, never a venue's reused IDs.
fn merge_groups(
    groups: Vec<Group>,
    view: TapeDotView,
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    reference: TapeReference<'_>,
    preview_now: Option<i64>,
) -> Vec<Group> {
    if groups.len() < 2 {
        return groups;
    }
    let low_time = groups
        .iter()
        .map(|group| group.mark.first_timestamp_ms)
        .min()
        .unwrap()
        .saturating_sub(1);
    let high_time = preview_now
        .unwrap_or_else(|| {
            groups
                .iter()
                .map(|group| group.mark.last_timestamp_ms)
                .max()
                .unwrap()
                .saturating_add(1)
        })
        .max(low_time.saturating_add(1));
    let low_price = groups
        .iter()
        .map(|group| group.mark.price)
        .min()
        .unwrap()
        .min(view.prices.low);
    let high_price = groups
        .iter()
        .map(|group| group.mark.price)
        .max()
        .unwrap()
        .max(view.prices.high);
    let prices = PriceWindow::new(low_price, high_price).expect("the view price span is positive");
    let time_span = Decimal::from(high_time) - Decimal::from(low_time);
    let geometry = TapeDotGeometry {
        left_x: 0.0,
        right_x: 1.0,
        width_px: view.geometry.width_px
            * (time_span / Decimal::from(view.window_ms))
                .to_f32()
                .unwrap_or(1.0),
        height_px: view.geometry.height_px
            * ((high_price - low_price) / (view.prices.high - view.prices.low))
                .to_f32()
                .unwrap_or(1.0),
    };
    let tagged: Vec<_> = groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let mut mark = group.mark.clone();
            mark.agg_id = index as u64 + 1;
            mark.agg_ids = vec![mark.agg_id];
            mark.x = if preview_now.is_some_and(|now| {
                window_start(mark.last_timestamp_ms, view.dot_window_ms)
                    == window_start(now, view.dot_window_ms)
            }) {
                1.0
            } else {
                ((mark.timestamp_quantity / mark.quantity - Decimal::from(low_time)) / time_span)
                    .to_f64()
                    .unwrap_or_default()
            };
            mark.y = prices.y_unclamped(mark.price).unwrap_or_default();
            mark
        })
        .collect();
    merge_tape_dots_with_reference(&tagged, sizing, bubbles, lane, geometry, reference)
        .into_iter()
        .map(|merged| {
            let sources = merged
                .agg_ids
                .iter()
                .flat_map(|id| {
                    groups[*id as usize - 1]
                        .sources
                        .iter()
                        .map(|(key, source)| (*key, source.clone()))
                })
                .collect();
            Group::of(sources)
        })
        .collect()
}
