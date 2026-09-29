//! Distance-based tape aggregation at execution coordinates.
//!
//! The spatial index only finds neighbours. It never supplies a coordinate:
//! every output stays at the quantity-weighted position of its executions,
//! except the open dot that follows NOW. Each merge removes one indexed dot,
//! so a dense tape needs no repeated all-pairs collision pass.

use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::dots::window_start;
use super::{AggressionPrimitive, DotSizing, normalized_area_size};
use crate::config::{BubbleStyle, LiveLaneStyle};
use crate::history::{AggressorSide, RestingSide};

/// Allowed intersection depth as a share of the smaller disc's radius.
const SMALLER_DOT_OVERLAP_SHARE: f32 = 0.1;

/// The linear drawing space of the tape, after any common edge padding.
/// Both the painter and this projection must use the same effective span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeDotGeometry {
    /// Normalized timeline coordinate at the left end of the tape.
    pub left_x: f64,
    /// Normalized timeline coordinate at NOW.
    pub right_x: f64,
    /// Pixel distance from the oldest visible timestamp to NOW.
    pub width_px: f32,
    /// Pixel height of the price axis.
    pub height_px: f32,
}

impl TapeDotGeometry {
    pub(super) fn valid(self) -> bool {
        self.left_x.is_finite()
            && self.right_x.is_finite()
            && self.right_x > self.left_x
            && self.width_px.is_finite()
            && self.width_px >= 0.0
            && self.height_px.is_finite()
            && self.height_px > 0.0
    }

    pub(super) fn visible(self, mark: &AggressionPrimitive) -> bool {
        mark.live
            && mark.quantity > Decimal::ZERO
            && (self.left_x..=self.right_x).contains(&mark.x)
            && (0.0..=1.0).contains(&mark.y)
    }

    fn position(self, mark: &AggressionPrimitive) -> (f64, f64) {
        (
            (mark.x - self.left_x) / (self.right_x - self.left_x) * f64::from(self.width_px),
            mark.y * f64::from(self.height_px),
        )
    }
}

/// Place a retained tape projection at the current time without waiting for
/// another worker frame. A newly closed dot recovers its exact execution
/// mean; an expired dot moves left of the lane for the painter to clip.
/// Candle primitives are unchanged. Call only for the optional tape pane.
pub fn position_tape_at(
    marks: &mut [AggressionPrimitive],
    now_ms: i64,
    lane_window_ms: i64,
    lane_start_x: f64,
    dot_window_ms: i64,
) {
    if lane_window_ms <= 0 || !lane_start_x.is_finite() || lane_start_x >= 1.0 {
        return;
    }
    let window = Decimal::from(lane_window_ms);
    let from = Decimal::from(now_ms) - window;
    let forming_window = window_start(now_ms, dot_window_ms);
    for mark in marks
        .iter_mut()
        .filter(|mark| mark.live && mark.quantity > Decimal::ZERO)
    {
        mark.x = if window_start(mark.last_timestamp_ms, dot_window_ms) == forming_window {
            1.0
        } else {
            let mean = mark.timestamp_quantity / mark.quantity;
            let fraction = ((mean - from) / window).to_f64().unwrap_or_default();
            lane_start_x + (1.0 - lane_start_x) * fraction
        };
    }
}

/// Exact moments prevent repeated centroid averaging from losing quantity.
/// Coordinates begin at the projection's final floating-point boundary; the
/// accumulation itself uses Decimal and converts back only to draw.
struct TapeMoment {
    mark: AggressionPrimitive,
    /// Volume eligible for the automatic reference; factual size and moments
    /// always use the complete mark quantity, including any opening burst.
    reference_quantity: Decimal,
    price_quantity: Decimal,
    x_quantity: Decimal,
    y_quantity: Decimal,
    at_now: bool,
    /// Exact radius calculation reused while this quantity and full scale hold.
    radius_cache: Option<(Decimal, f32)>,
}

impl TapeMoment {
    fn new(mark: AggressionPrimitive, right_x: f64) -> Self {
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

    fn absorb(&mut self, other: Self, right_x: f64) {
        self.radius_cache = None;
        self.reference_quantity += other.reference_quantity;
        self.price_quantity += other.price_quantity;
        self.x_quantity += other.x_quantity;
        self.y_quantity += other.y_quantity;
        self.at_now |= other.at_now;
        let mark = &mut self.mark;
        let mut other = other.mark;
        let low = mark.price_bucket.min(other.price_bucket);
        let high = (mark.price_bucket + mark.price_span).max(other.price_bucket + other.price_span);
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

    fn finish(mut self, full: Decimal) -> AggressionPrimitive {
        self.mark.agg_ids.sort_unstable();
        self.mark.agg_ids.dedup();
        self.mark.liquidity_event_ids.sort_unstable();
        self.mark.liquidity_event_ids.dedup();
        self.mark.size = normalized_area_size(self.mark.quantity, full);
        self.mark
    }
}

type Cell = (i64, i64);

/// A lookup grid, never a drawing grid. Its cell width bounds the search;
/// all collision tests still use the original execution coordinates.
struct Neighbours {
    cells: BTreeMap<Cell, BTreeSet<usize>>,
    span_px: f64,
    geometry: TapeDotGeometry,
}

impl Neighbours {
    fn cell(&self, mark: &AggressionPrimitive) -> Cell {
        let (x, y) = self.geometry.position(mark);
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

    fn candidates(&self, mark: &AggressionPrimitive) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = self.cell(mark);
        (-1..=1).flat_map(move |dx| {
            (-1..=1).flat_map(move |dy| {
                self.cells
                    .get(&(x + dx, y + dy))
                    .into_iter()
                    .flatten()
                    .copied()
            })
        })
    }
}

/// Combine tape dots whose discs would obscure each other. Areas stay
/// proportional to exact quantity and the visible maximum is recalculated
/// as merges grow. Neighbours may overlap by one tenth of the smaller
/// radius, so even a tiny dot beside a large one keeps its own visible disc.
///
/// The optional native tape owns this behaviour. Other panes pass through
/// unchanged, including their existing ordering and sizing.
#[must_use]
pub fn merge_tape_dots(
    marks: &[AggressionPrimitive],
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    geometry: TapeDotGeometry,
) -> Vec<AggressionPrimitive> {
    merge_tape_dots_with_reference(
        marks,
        sizing,
        bubbles,
        lane,
        geometry,
        TapeReference {
            minimum_full: Decimal::ZERO,
            quantities: &[],
        },
    )
}

/// Optional exact reference quantities, aligned with the input marks. An
/// empty slice retains the ordinary factual-volume normalization unchanged.
pub(super) struct TapeReference<'a> {
    pub minimum_full: Decimal,
    pub quantities: &'a [Decimal],
}

pub(super) fn merge_tape_dots_with_reference(
    marks: &[AggressionPrimitive],
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    geometry: TapeDotGeometry,
    reference: TapeReference<'_>,
) -> Vec<AggressionPrimitive> {
    if !lane.native() || !geometry.valid() || bubbles.max_radius <= 0.0 {
        return marks.to_vec();
    }
    let (mut tape, other): (Vec<_>, Vec<_>) = marks
        .iter()
        .enumerate()
        .map(|(index, mark)| {
            (
                mark.clone(),
                reference
                    .quantities
                    .get(index)
                    .copied()
                    .unwrap_or(mark.quantity),
            )
        })
        .partition(|(mark, _)| geometry.visible(mark));
    tape.sort_by(|(a, _), (b, _)| {
        a.x.total_cmp(&b.x)
            .then_with(|| a.y.total_cmp(&b.y))
            .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });
    let initial = if sizing.typed_full.is_none() && !reference.quantities.is_empty() {
        tape.iter()
            .map(|(_, quantity)| *quantity)
            .max()
            .unwrap_or_default()
    } else {
        sizing.full_quantity(tape.iter().map(|(mark, _)| mark), true)
    };
    let mut full = initial.max(reference.minimum_full);
    if full <= Decimal::ZERO {
        full = Decimal::ONE;
    }
    let mut other: Vec<_> = other.into_iter().map(|(mark, _)| mark).collect();
    let mut neighbours = Neighbours {
        cells: BTreeMap::new(),
        span_px: f64::from(bubbles.max_radius) * 2.0,
        geometry,
    };
    let mut active: Vec<Option<TapeMoment>> = Vec::with_capacity(tape.len());
    for (mark, reference_quantity) in tape {
        let mut pending = TapeMoment::new(mark, geometry.right_x);
        pending.reference_quantity = reference_quantity;
        loop {
            if sizing.typed_full.is_none() {
                // Existing discs only shrink as a new largest dot grows, so
                // they cannot create a new collision with one another.
                full = full.max(pending.reference_quantity);
            }
            let radius = pending.radius(sizing, bubbles, lane, full);
            let (x, y) = geometry.position(&pending.mark);
            let collision = neighbours
                .candidates(&pending.mark)
                .filter_map(|index| {
                    let other = active[index].as_mut()?;
                    let other_radius = other.radius(sizing, bubbles, lane, full);
                    let other = &other.mark;
                    let (other_x, other_y) = geometry.position(other);
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

/// Rebuild a retained group from its exact native facts, never from an
/// already divided price centroid. Drawing coordinates are assigned later.
pub(super) fn combine_tape_facts(
    marks: impl IntoIterator<Item = AggressionPrimitive>,
) -> Option<AggressionPrimitive> {
    let mut marks = marks.into_iter().map(|mut mark| {
        mark.x = 0.0;
        mark.y = 0.0;
        mark
    });
    let mut moment = TapeMoment::new(marks.next()?, 1.0);
    for mark in marks {
        moment.absorb(TapeMoment::new(mark, 1.0), 1.0);
    }
    Some(moment.finish(Decimal::ONE))
}

/// One shared radius cap preserves all area ratios when an axis transform
/// or an expiring maximum brings otherwise immutable groups closer together.
pub(super) fn tape_radius_limit(
    marks: &[AggressionPrimitive],
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    geometry: TapeDotGeometry,
) -> f32 {
    let shown: Vec<_> = marks.iter().filter(|mark| geometry.visible(mark)).collect();
    let full = sizing.full_quantity(shown.iter().copied(), true);
    let radii: Vec<_> = shown
        .iter()
        .map(|mark| sizing.radius(bubbles, lane, mark, full))
        .collect();
    let mut neighbours = Neighbours {
        cells: BTreeMap::new(),
        span_px: f64::from(bubbles.max_radius.max(f32::MIN_POSITIVE)) * 2.0,
        geometry,
    };
    let mut share = 1.0_f64;
    for (index, mark) in shown.iter().enumerate() {
        let (x, y) = geometry.position(mark);
        for other in neighbours.candidates(mark) {
            let (other_x, other_y) = geometry.position(shown[other]);
            let clearance = f64::from(
                radii[index] + radii[other]
                    - SMALLER_DOT_OVERLAP_SHARE * radii[index].min(radii[other]),
            );
            if clearance > 0.0 {
                share = share.min((x - other_x).hypot(y - other_y) / clearance);
            }
        }
        neighbours.insert(index, mark);
    }
    bubbles.max_radius * share.clamp(0.0, 1.0) as f32
}
