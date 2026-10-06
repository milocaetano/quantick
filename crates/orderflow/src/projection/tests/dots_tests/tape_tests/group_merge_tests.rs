//! The tape memory's merge reads moments, keeps groups that merged with
//! nothing and continues folds instead of refolding every fact; it must draw
//! exactly what refolding every fact of every group drew. These tests hold
//! it against that algorithm as it stood (the `legacy` module below, kept
//! verbatim as the oracle) on seeded random tapes: every mark compared by
//! its debug text, so a Decimal's scale counts, not only its value.

use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::super::dots::window_start;
use super::super::tape::{TapeReference, sort_unique_after};
use super::{Group, NativeKey, SettledReference, merge_groups, preview_groups};
use crate::config::{BubbleStyle, LiveLaneStyle};
use crate::history::{AggressorSide, RestingSide};
use crate::projection::{
    AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry, TapeDotView,
};

/// The merge as it stood before it read moments: whole marks, cloned per
/// window, and every output group refolded from its facts.
mod legacy {
    use super::*;

    const SMALLER_DOT_OVERLAP_SHARE: f32 = 0.1;

    pub(super) struct Moment {
        pub(super) mark: AggressionPrimitive,
        pub(super) reference_quantity: Decimal,
        price_quantity: Decimal,
        x_quantity: Decimal,
        y_quantity: Decimal,
        at_now: bool,
        radius_cache: Option<(Decimal, f32)>,
    }

    impl Moment {
        pub(super) fn new(mark: AggressionPrimitive, right_x: f64) -> Self {
            Self {
                reference_quantity: mark.quantity,
                price_quantity: mark.price * mark.quantity,
                x_quantity: Decimal::from_f64_retain(mark.x).unwrap_or_default() * mark.quantity,
                y_quantity: Decimal::from_f64_retain(mark.y).unwrap_or_default() * mark.quantity,
                at_now: mark.x == right_x,
                radius_cache: None,
                mark,
            }
        }

        fn radius(
            &mut self,
            sizing: DotSizing,
            bubbles: &BubbleStyle,
            lane: &LiveLaneStyle,
            full: Decimal,
        ) -> f32 {
            if let Some((reference, radius)) = self.radius_cache
                && reference == full
            {
                return radius;
            }
            let radius = sizing.radius(bubbles, lane, &self.mark, full);
            self.radius_cache = Some((full, radius));
            radius
        }

        pub(super) fn absorb(&mut self, other: Self, right_x: f64) {
            self.radius_cache = None;
            self.reference_quantity += other.reference_quantity;
            self.price_quantity += other.price_quantity;
            self.x_quantity += other.x_quantity;
            self.y_quantity += other.y_quantity;
            self.at_now |= other.at_now;
            let mark = &mut self.mark;
            let mut other = other.mark;
            let low = mark.price_bucket.min(other.price_bucket);
            let high =
                (mark.price_bucket + mark.price_span).max(other.price_bucket + other.price_span);
            mark.quantity += other.quantity;
            mark.buy_quantity += other.buy_quantity;
            mark.matched_quantity += other.matched_quantity;
            mark.trade_count = mark.trade_count.saturating_add(other.trade_count);
            mark.first_timestamp_ms = mark.first_timestamp_ms.min(other.first_timestamp_ms);
            mark.last_timestamp_ms = mark.last_timestamp_ms.max(other.last_timestamp_ms);
            mark.timestamp_quantity += other.timestamp_quantity;
            mark.agg_id = mark.agg_id.min(other.agg_id);
            mark.agg_ids.append(&mut other.agg_ids);
            mark.liquidity_event_ids
                .append(&mut other.liquidity_event_ids);
            if mark.generation != other.generation {
                mark.generation = None;
            }
            mark.price_bucket = low;
            mark.price_span = high - low;
            mark.price = self.price_quantity / mark.quantity;
            mark.x = if self.at_now {
                right_x
            } else {
                (self.x_quantity / mark.quantity).to_f64().unwrap_or(mark.x)
            };
            mark.y = (self.y_quantity / mark.quantity).to_f64().unwrap_or(mark.y);
            mark.buy_share = (mark.buy_quantity / mark.quantity).to_f32().unwrap_or(0.0);
            mark.matched_fraction = (mark.matched_quantity / mark.quantity)
                .to_f32()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let buys_dominate = mark.buy_quantity >= mark.quantity - mark.buy_quantity;
            mark.side = if buys_dominate {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            };
            mark.consumed_side = if buys_dominate {
                RestingSide::Ask
            } else {
                RestingSide::Bid
            };
        }

        pub(super) fn finish(mut self, full: Decimal) -> AggressionPrimitive {
            self.mark.agg_ids.sort_unstable();
            self.mark.agg_ids.dedup();
            self.mark.liquidity_event_ids.sort_unstable();
            self.mark.liquidity_event_ids.dedup();
            self.mark.size = crate::projection::normalized_area_size(self.mark.quantity, full);
            self.mark
        }
    }

    fn position(geometry: TapeDotGeometry, mark: &AggressionPrimitive) -> (f64, f64) {
        (
            (mark.x - geometry.left_x) / (geometry.right_x - geometry.left_x)
                * f64::from(geometry.width_px),
            mark.y * f64::from(geometry.height_px),
        )
    }

    struct Neighbours {
        cells: BTreeMap<(i64, i64), BTreeSet<usize>>,
        span_px: f64,
        geometry: TapeDotGeometry,
    }

    impl Neighbours {
        fn cell(&self, mark: &AggressionPrimitive) -> (i64, i64) {
            let (x, y) = position(self.geometry, mark);
            (
                (x / self.span_px).floor() as i64,
                (y / self.span_px).floor() as i64,
            )
        }

        fn insert(&mut self, index: usize, mark: &AggressionPrimitive) {
            self.cells.entry(self.cell(mark)).or_default().insert(index);
        }

        fn remove(&mut self, index: usize, mark: &AggressionPrimitive) {
            let cell = self.cell(mark);
            if let Some(indices) = self.cells.get_mut(&cell) {
                indices.remove(&index);
                if indices.is_empty() {
                    self.cells.remove(&cell);
                }
            }
        }

        fn candidates(&self, mark: &AggressionPrimitive) -> Vec<usize> {
            let (x, y) = self.cell(mark);
            let mut found = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    found.extend(self.cells.get(&(x + dx, y + dy)).into_iter().flatten());
                }
            }
            found
        }
    }

    fn visible(geometry: TapeDotGeometry, mark: &AggressionPrimitive) -> bool {
        mark.live
            && mark.quantity > Decimal::ZERO
            && (geometry.left_x..=geometry.right_x).contains(&mark.x)
            && (0.0..=1.0).contains(&mark.y)
    }

    pub(super) fn merge_marks(
        marks: &[AggressionPrimitive],
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        geometry: TapeDotGeometry,
        minimum_full: Decimal,
        quantities: &[Decimal],
    ) -> Vec<AggressionPrimitive> {
        if !sizing.native_tape || !geometry.valid() || bubbles.max_radius <= 0.0 {
            return marks.to_vec();
        }
        let (mut tape, other): (Vec<_>, Vec<_>) = marks
            .iter()
            .enumerate()
            .map(|(index, mark)| {
                (
                    mark.clone(),
                    quantities.get(index).copied().unwrap_or(mark.quantity),
                )
            })
            .partition(|(mark, _)| visible(geometry, mark));
        tape.sort_by(|(a, _), (b, _)| {
            a.x.total_cmp(&b.x)
                .then_with(|| a.y.total_cmp(&b.y))
                .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        });
        let initial = if sizing.typed_full.is_none() && !quantities.is_empty() {
            tape.iter()
                .map(|(_, quantity)| *quantity)
                .max()
                .unwrap_or_default()
        } else {
            sizing.full_quantity(tape.iter().map(|(mark, _)| mark), true)
        };
        let mut full = initial.max(minimum_full);
        if full <= Decimal::ZERO {
            full = Decimal::ONE;
        }
        let mut other: Vec<_> = other.into_iter().map(|(mark, _)| mark).collect();
        let mut neighbours = Neighbours {
            cells: BTreeMap::new(),
            span_px: f64::from(bubbles.max_radius) * 2.0,
            geometry,
        };
        let mut active: Vec<Option<Moment>> = Vec::with_capacity(tape.len());
        for (mark, reference_quantity) in tape {
            let mut pending = Moment::new(mark, geometry.right_x);
            pending.reference_quantity = reference_quantity;
            loop {
                if sizing.typed_full.is_none() {
                    full = full.max(pending.reference_quantity);
                }
                let radius = pending.radius(sizing, bubbles, lane, full);
                let (x, y) = position(geometry, &pending.mark);
                let collision = neighbours
                    .candidates(&pending.mark)
                    .into_iter()
                    .filter_map(|index| {
                        let other = active[index].as_mut()?;
                        let other_radius = other.radius(sizing, bubbles, lane, full);
                        let other = &other.mark;
                        let (other_x, other_y) = position(geometry, other);
                        let clearance = f64::from(
                            radius + other_radius
                                - SMALLER_DOT_OVERLAP_SHARE * radius.min(other_radius),
                        );
                        let dx = other_x - x;
                        let dy = other_y - y;
                        if dx.abs() >= clearance || dy.abs() >= clearance {
                            return None;
                        }
                        let distance = dx.hypot(dy);
                        (distance < clearance).then_some((index, distance, other.agg_id))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.cmp(&b.2)))
                    .map(|candidate| candidate.0);
                let Some(index) = collision else { break };
                let absorbed = active[index].take().expect("an indexed dot is active");
                neighbours.remove(index, &absorbed.mark);
                pending.absorb(absorbed, geometry.right_x);
            }
            neighbours.insert(active.len(), &pending.mark);
            active.push(Some(pending));
        }
        other.extend(active.into_iter().flatten().map(|dot| dot.finish(full)));
        other.sort_by(|a, b| {
            a.quantity
                .cmp(&b.quantity)
                .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        });
        other
    }

    /// A group as the legacy memory held it: its mark refolded from its
    /// facts, its exact price moment, and the facts.
    #[derive(Clone)]
    pub(super) struct Held {
        pub(super) mark: AggressionPrimitive,
        pub(super) price_quantity: Decimal,
        pub(super) sources: BTreeMap<NativeKey, AggressionPrimitive>,
    }

    pub(super) fn combine(
        marks: impl IntoIterator<Item = AggressionPrimitive>,
    ) -> AggressionPrimitive {
        let mut marks = marks.into_iter().map(|mut mark| {
            mark.x = 0.0;
            mark.y = 0.0;
            mark
        });
        let mut moment = Moment::new(marks.next().expect("a group has facts"), 1.0);
        for mark in marks {
            moment.absorb(Moment::new(mark, 1.0), 1.0);
        }
        moment.finish(Decimal::ONE)
    }

    impl Held {
        pub(super) fn of(sources: BTreeMap<NativeKey, AggressionPrimitive>) -> Self {
            Self {
                mark: combine(sources.values().cloned()),
                price_quantity: sources
                    .values()
                    .map(|mark| mark.price * mark.quantity)
                    .sum(),
                sources,
            }
        }

        pub(super) fn reference_quantity(&self, opening_bursts: &[i64]) -> Decimal {
            self.sources
                .iter()
                .filter(|(key, _)| !opening_bursts.contains(&key.0))
                .map(|(_, mark)| mark.quantity)
                .sum()
        }

        pub(super) fn visible_at(&self, now_ms: i64, window_ms: i64) -> bool {
            self.mark.timestamp_quantity
                >= Decimal::from(now_ms.saturating_sub(window_ms)) * self.mark.quantity
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn merge_groups(
        groups: Vec<Held>,
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        minimum_full: Decimal,
        quantities: &[Decimal],
        preview_now: Option<i64>,
    ) -> Vec<Held> {
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
        let prices = PriceWindow::new(low_price, high_price).expect("a positive span");
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
                    ((mark.timestamp_quantity / mark.quantity - Decimal::from(low_time))
                        / time_span)
                        .to_f64()
                        .unwrap_or_default()
                };
                mark.y = prices.y_unclamped(mark.price).unwrap_or_default();
                mark
            })
            .collect();
        merge_marks(
            &tagged,
            sizing,
            bubbles,
            lane,
            geometry,
            minimum_full,
            quantities,
        )
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
            Held::of(sources)
        })
        .collect()
    }
}

/// A seeded generator: the tests are the same on every run and machine.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// One native fact at `key`, of a tape quoted in `tick`s.
fn fact(
    random: &mut Seeded,
    key: NativeKey,
    tick: Decimal,
    next_id: &mut u64,
) -> AggressionPrimitive {
    let prints = 1 + random.below(4) as usize;
    let agg_ids: Vec<u64> = (0..prints)
        .map(|_| {
            *next_id += 1 + random.below(3);
            *next_id
        })
        .collect();
    let quantity = if random.chance(30) {
        // Fractional quantities keep a scale of their own.
        Decimal::new(1 + random.below(40_000) as i64, 1 + random.below(3) as u32)
    } else {
        Decimal::from(1 + random.below(40))
    };
    let buy_quantity = if random.chance(50) {
        quantity
    } else if random.chance(50) {
        Decimal::ZERO
    } else {
        (quantity / Decimal::from(3)).round_dp(quantity.scale())
    };
    let first_timestamp_ms = key.0 + random.below(100) as i64;
    let last_timestamp_ms =
        first_timestamp_ms + random.below((key.0 + 100 - first_timestamp_ms) as u64) as i64;
    let timestamp_quantity = Decimal::from(first_timestamp_ms) * quantity
        + Decimal::from(last_timestamp_ms - first_timestamp_ms);
    let matched_quantity = if random.chance(20) {
        quantity.min(Decimal::from(random.below(5)))
    } else {
        Decimal::ZERO
    };
    AggressionPrimitive {
        agg_id: agg_ids[0],
        trade_count: agg_ids.len(),
        agg_ids,
        generation: match random.below(3) {
            0 => None,
            other => Some(other),
        },
        side: if buy_quantity * Decimal::TWO >= quantity {
            AggressorSide::Buy
        } else {
            AggressorSide::Sell
        },
        consumed_side: if buy_quantity * Decimal::TWO >= quantity {
            RestingSide::Ask
        } else {
            RestingSide::Bid
        },
        quantity,
        buy_share: (buy_quantity / quantity).to_f32().unwrap_or(0.0),
        live: true,
        price_bucket: key.1,
        price_span: tick,
        price: key.1,
        first_timestamp_ms,
        last_timestamp_ms,
        timestamp_quantity,
        matched_quantity,
        buy_quantity,
        matched_fraction: (matched_quantity / quantity).to_f32().unwrap_or(0.0),
        liquidity_event_ids: if random.chance(10) {
            vec![random.below(50), random.below(50)]
        } else {
            Vec::new()
        },
        folded_marks: random.below(3) as u32,
        x: 0.0,
        y: 0.0,
        size: 0.0,
    }
}

/// One merge's inputs: candidate groups packed into a frontier's worth of
/// pixels, and the view, sizing and reference they are merged under.
struct Scenario {
    groups: Vec<BTreeMap<NativeKey, AggressionPrimitive>>,
    view: TapeDotView,
    sizing: DotSizing,
    bubbles: BubbleStyle,
    lane: LiveLaneStyle,
    minimum_full: Decimal,
    opening_bursts: Vec<i64>,
    /// Whether the merge sizes by eligible volume, the bursts kept out.
    by_reference: bool,
}

fn scenario(seed: u64) -> Scenario {
    let mut random = Seeded(seed);
    let tick = if random.chance(50) {
        Decimal::from(5)
    } else {
        Decimal::new(5, 2)
    };
    let window_ms = [10_000, 60_000, 300_000, 1_800_000][random.below(4) as usize];
    let now_ms = 1_790_000_000_000 + random.below(1_000_000) as i64;
    let geometry = TapeDotGeometry {
        left_x: [0.0, 0.5, 0.65][random.below(3) as usize],
        right_x: 1.0,
        width_px: 150.0 + random.below(900) as f32,
        height_px: 200.0 + random.below(500) as f32,
    };
    let low = Decimal::from(190_000) * tick / Decimal::from(5);
    let rows = 4 + random.below(60) as i64;
    let prices = PriceWindow::new(low, low + tick * Decimal::from(rows)).expect("a span");
    // A frontier spans a handful of the window's pixels.
    let span_ms = (window_ms / 40).max(500);
    let windows = (span_ms / 100).max(2);
    let mut keys: BTreeSet<NativeKey> = BTreeSet::new();
    // Never more keys than the windows and rows hold.
    let room = usize::try_from(windows * (rows + 1)).unwrap_or(usize::MAX);
    let wanted = (2 + random.below(120) as usize).min(room);
    while keys.len() < wanted {
        let window = window_start(now_ms - 1 - random.below(windows as u64 * 100) as i64, 100);
        let row = random.below(rows as u64 + 1) as i64;
        keys.insert(NativeKey(window, low + tick * Decimal::from(row)));
    }
    let mut next_id = random.below(1_000);
    let facts: Vec<(NativeKey, AggressionPrimitive)> = keys
        .into_iter()
        .map(|key| (key, fact(&mut random, key, tick, &mut next_id)))
        .collect();
    // Groups hold runs of facts: some whole windows, some interleaved.
    let mut groups: Vec<BTreeMap<NativeKey, AggressionPrimitive>> = Vec::new();
    for (key, fact) in facts {
        if groups.is_empty() || random.chance(45) {
            groups.push(BTreeMap::new());
        }
        let at = random.below(groups.len() as u64) as usize;
        let at = if random.chance(60) {
            groups.len() - 1
        } else {
            at
        };
        groups[at].insert(key, fact);
    }
    groups.retain(|group| !group.is_empty());
    let windows_seen: Vec<i64> = groups
        .iter()
        .flat_map(|group| group.keys().map(|key| key.0))
        .collect();
    let opening_bursts = if random.chance(40) {
        (0..1 + random.below(2))
            .map(|_| windows_seen[random.below(windows_seen.len() as u64) as usize])
            .collect()
    } else {
        Vec::new()
    };
    let bubbles = BubbleStyle {
        max_radius: 3.0 + random.below(20) as f32,
        ..BubbleStyle::default()
    };
    Scenario {
        groups,
        view: TapeDotView {
            now_ms,
            window_ms,
            dot_window_ms: 100,
            evicted_through_ms: None,
            prices,
            geometry,
        },
        sizing: DotSizing {
            native_tape: true,
            tape_column_px: 2.0,
            candle_column_px: 12.0,
            px_per_price: 12.0,
            typed_full: random
                .chance(15)
                .then(|| Decimal::from(1 + random.below(200))),
        },
        bubbles,
        lane: LiveLaneStyle::default(),
        minimum_full: if random.chance(50) {
            Decimal::from(random.below(120))
        } else {
            Decimal::ZERO
        },
        by_reference: random.chance(50),
        opening_bursts,
    }
}

fn text(mark: &AggressionPrimitive) -> String {
    format!("{mark:?}")
}

fn sources_text(sources: &BTreeMap<NativeKey, AggressionPrimitive>) -> Vec<String> {
    sources
        .iter()
        .map(|(key, fact)| format!("{key:?} {fact:?}"))
        .collect()
}

#[test]
fn merging_groups_keeps_every_mark_the_refolding_merge_drew() {
    for seed in 0..400 {
        let case = scenario(seed);
        let current: Vec<Group> = case.groups.iter().cloned().map(Group::of).collect();
        let held: Vec<legacy::Held> = case.groups.iter().cloned().map(legacy::Held::of).collect();
        for (group, held) in current.iter().zip(&held) {
            assert_eq!(
                text(&group.mark),
                text(&held.mark),
                "seed {seed}: a group's fold"
            );
        }
        let quantities: Vec<Decimal> = if case.by_reference {
            held.iter()
                .map(|group| group.reference_quantity(&case.opening_bursts))
                .collect()
        } else {
            Vec::new()
        };
        let expected = legacy::merge_groups(
            held,
            case.view,
            case.sizing,
            &case.bubbles,
            &case.lane,
            case.minimum_full,
            &quantities,
            None,
        );
        let actual = merge_groups(
            current,
            case.view,
            case.sizing,
            &case.bubbles,
            &case.lane,
            TapeReference {
                minimum_full: case.minimum_full,
                quantities: &quantities,
            },
        );
        assert_eq!(actual.len(), expected.len(), "seed {seed}");
        for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(
                text(&actual.mark),
                text(&expected.mark),
                "seed {seed}, group {index}"
            );
            assert_eq!(
                format!("{:?}", actual.price_quantity),
                format!("{:?}", expected.price_quantity),
                "seed {seed}, group {index}"
            );
            assert_eq!(
                sources_text(&actual.sources),
                sources_text(&expected.sources),
                "seed {seed}, group {index}"
            );
            assert_eq!(
                format!("{:?}", actual.reference_quantity(&case.opening_bursts)),
                format!("{:?}", expected.reference_quantity(&case.opening_bursts)),
                "seed {seed}, group {index}"
            );
        }
    }
}

#[test]
fn the_preview_draws_what_merging_copies_of_the_frontier_drew() {
    for seed in 1_000..1_300 {
        let case = scenario(seed);
        let current: Vec<Group> = case.groups.iter().cloned().map(Group::of).collect();
        let held: Vec<legacy::Held> = case.groups.iter().cloned().map(legacy::Held::of).collect();
        let quantities: Vec<Decimal> = if case.by_reference {
            held.iter()
                .map(|group| group.reference_quantity(&case.opening_bursts))
                .collect()
        } else {
            Vec::new()
        };
        let now = Some(case.view.now_ms);
        let expected = legacy::merge_groups(
            held,
            case.view,
            case.sizing,
            &case.bubbles,
            &case.lane,
            case.minimum_full,
            &quantities,
            now,
        );
        let actual = preview_groups(
            &current.iter().collect::<Vec<_>>(),
            case.view,
            case.sizing,
            &case.bubbles,
            &case.lane,
            TapeReference {
                minimum_full: case.minimum_full,
                quantities: &quantities,
            },
            now,
            &case.opening_bursts,
        );
        assert_eq!(actual.len(), expected.len(), "seed {seed}");
        for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(
                text(&actual.mark),
                text(&expected.mark),
                "seed {seed}, dot {index}"
            );
            assert_eq!(
                format!("{:?}", actual.reference_quantity),
                format!("{:?}", expected.reference_quantity(&case.opening_bursts)),
                "seed {seed}, dot {index}"
            );
        }
    }
}

#[test]
fn a_merged_group_is_the_group_folded_from_all_its_facts() {
    for seed in 2_000..2_400 {
        let case = scenario(seed);
        let members: Vec<Group> = case.groups.iter().cloned().map(Group::of).collect();
        let every: BTreeMap<NativeKey, AggressionPrimitive> =
            case.groups.iter().flat_map(|group| group.clone()).collect();
        let expected = legacy::Held::of(every);
        let merged = Group::merge(members);
        assert_eq!(text(&merged.mark), text(&expected.mark), "seed {seed}");
        assert_eq!(
            format!("{:?}", merged.price_quantity),
            format!("{:?}", expected.price_quantity),
            "seed {seed}"
        );
        assert_eq!(
            sources_text(&merged.sources),
            sources_text(&expected.sources),
            "seed {seed}"
        );
    }
}

#[test]
fn the_settled_reference_is_the_fold_over_the_settled_groups_it_replaced() {
    for seed in 3_000..3_200 {
        let case = scenario(seed);
        let mut random = Seeded(seed);
        let groups: Vec<Group> = case.groups.iter().cloned().map(Group::of).collect();
        let held: Vec<legacy::Held> = case.groups.iter().cloned().map(legacy::Held::of).collect();
        let first = groups
            .iter()
            .map(|group| group.mark.first_timestamp_ms)
            .min()
            .unwrap();
        let last = groups
            .iter()
            .map(|group| group.mark.last_timestamp_ms)
            .max()
            .unwrap();
        // A window a third of the groups' spread, walked across it: groups
        // come into view and leave it as a pass's windows close.
        let window_ms = ((last - first) / 3).max(1);
        let mut now_ms = first;
        let mut settled = 0;
        let mut reference = SettledReference::default();
        while settled < groups.len() {
            now_ms += 1 + random.below(((last - first) / 10).max(1) as u64) as i64;
            settled += random.below(3) as usize;
            settled = settled.min(groups.len());
            let candidates = (settled + random.below(6) as usize).min(groups.len());
            let actual = reference.minimum_full(
                &groups[..settled],
                &groups[settled..candidates],
                case.sizing,
                (now_ms, window_ms),
                &case.opening_bursts,
            );
            let fold = held[..candidates]
                .iter()
                .filter(|group| group.visible_at(now_ms, window_ms))
                .map(|group| {
                    (
                        if case.opening_bursts.is_empty() {
                            group.mark.quantity
                        } else {
                            group.reference_quantity(&case.opening_bursts)
                        },
                        group.mark.quantity,
                    )
                })
                .fold(
                    (Decimal::ZERO, Decimal::ZERO),
                    |(eligible, factual), (next, quantity)| {
                        (eligible.max(next), factual.max(quantity))
                    },
                );
            let expected = if case.sizing.typed_full.is_some() {
                Decimal::ZERO
            } else if fold.0 > Decimal::ZERO {
                fold.0
            } else {
                fold.1
            };
            assert_eq!(
                format!("{actual:?}"),
                format!("{expected:?}"),
                "seed {seed} at {now_ms}, {settled} settled"
            );
        }
    }
}

#[test]
fn sorting_what_follows_a_sorted_prefix_is_sorting_the_whole() {
    let mut random = Seeded(7);
    for _ in 0..2_000 {
        let mut prefix: Vec<u64> = (0..random.below(20)).map(|_| random.below(100)).collect();
        prefix.sort_unstable();
        prefix.dedup();
        let sorted = prefix.len();
        let mut ids = prefix;
        ids.extend((0..random.below(20)).map(|_| random.below(140)));
        let mut expected = ids.clone();
        expected.sort_unstable();
        expected.dedup();
        sort_unique_after(&mut ids, sorted);
        assert_eq!(ids, expected);
    }
}
