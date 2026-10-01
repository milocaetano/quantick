//! Distance-based tape aggregation at execution coordinates.
//!
//! The spatial index only finds neighbours. It never supplies a coordinate:
//! every output stays at the quantity-weighted position of its executions,
//! except the open dot that follows NOW. Each merge removes one indexed dot,
//! so a dense tape needs no repeated all-pairs collision pass.

use std::collections::BTreeMap;

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
        mark.live && self.holds(mark.quantity, mark.x, mark.y)
    }

    /// Whether a live dot of `quantity` at normalized `(x, y)` is on the tape.
    pub(super) fn holds(self, quantity: Decimal, x: f64, y: f64) -> bool {
        quantity > Decimal::ZERO
            && (self.left_x..=self.right_x).contains(&x)
            && (0.0..=1.0).contains(&y)
    }

    fn position(self, mark: &AggressionPrimitive) -> (f64, f64) {
        self.point(mark.x, mark.y)
    }

    /// Pixels from the tape's left end and top of normalized `(x, y)`.
    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.left_x) / (self.right_x - self.left_x) * f64::from(self.width_px),
            y * f64::from(self.height_px),
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
    position_tape_with(
        marks,
        now_ms,
        lane_window_ms,
        lane_start_x,
        Some(dot_window_ms),
    );
}

/// [`position_tape_at`], with the forming dot's pin at now optional: a
/// frozen past has no forming window, so every mark sits at its centroid.
pub(super) fn position_tape_with(
    marks: &mut [AggressionPrimitive],
    now_ms: i64,
    lane_window_ms: i64,
    lane_start_x: f64,
    forming_dot_window_ms: Option<i64>,
) {
    if lane_window_ms <= 0 || !lane_start_x.is_finite() || lane_start_x >= 1.0 {
        return;
    }
    let window = Decimal::from(lane_window_ms);
    let from = Decimal::from(now_ms) - window;
    let forming = forming_dot_window_ms.map(|dot| (dot, window_start(now_ms, dot)));
    for mark in marks
        .iter_mut()
        .filter(|mark| mark.live && mark.quantity > Decimal::ZERO)
    {
        mark.x = if forming
            .is_some_and(|(dot, window)| window_start(mark.last_timestamp_ms, dot) == window)
        {
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
pub(super) struct TapeMoment {
    mark: AggressionPrimitive,
    /// Volume eligible for the automatic reference; factual size and moments
    /// always use the complete mark quantity, including any opening burst.
    reference_quantity: Decimal,
    price_quantity: Decimal,
    x_quantity: Decimal,
    y_quantity: Decimal,
    at_now: bool,
    /// Exact radius calculation reused while this quantity and full scale hold.
    radius_cache: Option<(u64, f32)>,
    /// How many leading trade and liquidity-event ids are already sorted and
    /// unique: a resumed fold's, which [`Self::finish`] need not sort again.
    sorted_ids: (usize, usize),
}

impl TapeMoment {
    pub(super) fn new(mark: AggressionPrimitive, right_x: f64) -> Self {
        Self {
            reference_quantity: mark.quantity,
            price_quantity: mark.price * mark.quantity,
            x_quantity: Decimal::from_f64_retain(mark.x).unwrap_or_default() * mark.quantity,
            y_quantity: Decimal::from_f64_retain(mark.y).unwrap_or_default() * mark.quantity,
            at_now: mark.x == right_x,
            radius_cache: None,
            sorted_ids: (0, 0),
            mark,
        }
    }

    /// Resume the fold that finished as `mark` over facts whose exact price
    /// moment is `price_quantity`, placed nowhere, so later facts join it as
    /// they would have joined the fold itself ([`combine_tape_facts`]).
    pub(super) fn resumed(mark: AggressionPrimitive, price_quantity: Decimal) -> Self {
        Self {
            reference_quantity: mark.quantity,
            price_quantity,
            x_quantity: Decimal::ZERO,
            y_quantity: Decimal::ZERO,
            at_now: false,
            radius_cache: None,
            sorted_ids: (mark.agg_ids.len(), mark.liquidity_event_ids.len()),
            mark,
        }
    }

    /// The exact price moment folded so far.
    pub(super) fn price_quantity(&self) -> Decimal {
        self.price_quantity
    }

    pub(super) fn absorb(&mut self, mut other: Self, right_x: f64) {
        self.x_quantity += other.x_quantity;
        self.y_quantity += other.y_quantity;
        self.at_now |= other.at_now;
        self.mark.agg_ids.append(&mut other.mark.agg_ids);
        self.mark
            .liquidity_event_ids
            .append(&mut other.mark.liquidity_event_ids);
        self.fold(
            other.reference_quantity,
            other.price_quantity,
            &other.mark,
            right_x,
        );
    }

    /// [`Self::absorb`] of `fact` placed nowhere, into a fold of facts placed
    /// nowhere ([`combine_tape_facts`]), without taking the fact: its x and y
    /// moments are zero, so the fold's stay what they are.
    pub(super) fn absorb_fact(&mut self, fact: &AggressionPrimitive) {
        self.mark.agg_ids.extend_from_slice(&fact.agg_ids);
        self.mark
            .liquidity_event_ids
            .extend_from_slice(&fact.liquidity_event_ids);
        self.fold(fact.quantity, fact.price * fact.quantity, fact, 1.0);
    }

    /// Everything [`Self::absorb`] takes from `other` but its drawing moments
    /// and ids.
    fn fold(
        &mut self,
        reference_quantity: Decimal,
        price_quantity: Decimal,
        other: &AggressionPrimitive,
        right_x: f64,
    ) {
        self.radius_cache = None;
        self.reference_quantity += reference_quantity;
        self.price_quantity += price_quantity;
        let mark = &mut self.mark;
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
        sort_unique_after(&mut self.mark.agg_ids, self.sorted_ids.0);
        sort_unique_after(&mut self.mark.liquidity_event_ids, self.sorted_ids.1);
        self.mark.size = normalized_area_size(self.mark.quantity, full);
        self.mark
    }
}

/// Sort `ids` and drop repeats, where the first `sorted` are already sorted
/// and unique: only what follows them is sorted, unless it reaches below
/// them. The result is `sort_unstable` then `dedup` of the whole.
pub(super) fn sort_unique_after(ids: &mut Vec<u64>, sorted: usize) {
    let sorted = sorted.min(ids.len());
    ids[sorted..].sort_unstable();
    if sorted > 0 && sorted < ids.len() && ids[sorted] <= ids[sorted - 1] {
        ids.sort_unstable();
        ids.dedup();
        return;
    }
    let mut kept = sorted;
    for read in sorted..ids.len() {
        if kept > 0 && ids[read] == ids[kept - 1] {
            continue;
        }
        ids[kept] = ids[read];
        kept += 1;
    }
    ids.truncate(kept);
}

/// What the collision merge ([`collide`]) reads of a disc, and how a disc
/// grows by absorbing its neighbour. A whole mark ([`TapeMoment`]) and the
/// bare moments the tape memory merges its groups with both answer it, so
/// both are merged by the one algorithm.
pub(super) trait MergeDisc: Sized {
    /// Normalized timeline position.
    fn x(&self) -> f64;
    /// Normalized price position, from the top.
    fn y(&self) -> f64;
    fn first_timestamp_ms(&self) -> i64;
    fn agg_id(&self) -> u64;
    fn quantity(&self) -> Decimal;
    /// The volume the automatic full size is taken from.
    fn reference_quantity(&self) -> Decimal;
    /// The disc's radius against `full`, cached while its quantity and the
    /// full size hold.
    fn radius(
        &mut self,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        full: Full,
    ) -> f32;
    /// Optional source-locality constraint; ordinary Tape discs retain unrestricted merging.
    fn permits_merge(&self, _other: &Self) -> bool {
        true
    }
    fn absorb(&mut self, other: Self, right_x: f64);
}

impl MergeDisc for TapeMoment {
    fn x(&self) -> f64 {
        self.mark.x
    }

    fn y(&self) -> f64 {
        self.mark.y
    }

    fn first_timestamp_ms(&self) -> i64 {
        self.mark.first_timestamp_ms
    }

    fn agg_id(&self) -> u64 {
        self.mark.agg_id
    }

    fn quantity(&self) -> Decimal {
        self.mark.quantity
    }

    fn reference_quantity(&self) -> Decimal {
        self.reference_quantity
    }

    fn radius(
        &mut self,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        full: Full,
    ) -> f32 {
        if let Some((held, radius)) = self.radius_cache
            && held == full.serial
        {
            return radius;
        }
        let radius = sizing.radius(bubbles, lane, &self.mark, full.quantity);
        self.radius_cache = Some((full.serial, radius));
        radius
    }

    fn absorb(&mut self, other: Self, right_x: f64) {
        TapeMoment::absorb(self, other, right_x);
    }
}

/// The full size a merge sizes its discs against, and how many times it
/// has grown: it only grows, so an equal count is an equal size.
#[derive(Debug, Clone, Copy)]
pub(super) struct Full {
    pub(super) quantity: Decimal,
    pub(super) serial: u64,
}

type Cell = (i64, i64);

/// A lookup grid, never a drawing grid. Its cell width bounds the search;
/// all collision tests still use the original execution coordinates. Each
/// cell lists its dots by ascending index, the order they are tried in.
struct Neighbours {
    cells: BTreeMap<Cell, Vec<usize>>,
    span_px: f64,
    geometry: TapeDotGeometry,
}

impl Neighbours {
    fn new(span_px: f64, geometry: TapeDotGeometry) -> Self {
        Self {
            cells: BTreeMap::new(),
            span_px,
            geometry,
        }
    }

    fn cell(&self, x: f64, y: f64) -> Cell {
        let (x, y) = self.geometry.point(x, y);
        (
            (x / self.span_px).floor() as i64,
            (y / self.span_px).floor() as i64,
        )
    }

    /// Indices only grow, so a push keeps each cell ascending.
    fn insert(&mut self, index: usize, x: f64, y: f64) {
        self.cells.entry(self.cell(x, y)).or_default().push(index);
    }

    fn remove(&mut self, index: usize, x: f64, y: f64) {
        let cell = self.cell(x, y);
        if let Some(indices) = self.cells.get_mut(&cell) {
            if let Some(at) = indices.iter().position(|held| *held == index) {
                indices.remove(at);
            }
            if indices.is_empty() {
                self.cells.remove(&cell);
            }
        }
    }

    /// The dots in the 3x3 cells around `(x, y)`, column by column, each
    /// column from its top cell down: one ordered range per column.
    fn candidates(&self, x: f64, y: f64) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = self.cell(x, y);
        let (top, bottom) = (y.saturating_sub(1), y.saturating_add(1));
        (-1..=1)
            .filter_map(move |dx| x.checked_add(dx))
            .flat_map(move |column| {
                self.cells
                    .range((column, top)..=(column, bottom))
                    .flat_map(|(_, indices)| indices.iter().copied())
            })
    }
}

/// The collision merge itself: `tape`, every disc of it on the tape, taken
/// in time then price order, each absorbing the nearest disc it would
/// obscure until none is left. Areas start from the largest reference
/// volume when `by_reference`, else from the pane's full size, never under
/// `minimum_full`. The surviving discs come back in the order they were
/// placed, with the reference they finished at.
pub(super) fn collide<D: MergeDisc>(
    mut tape: Vec<D>,
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    geometry: TapeDotGeometry,
    minimum_full: Decimal,
    by_reference: bool,
) -> (Vec<D>, Decimal) {
    tape.sort_by(|a, b| {
        a.x()
            .total_cmp(&b.x())
            .then_with(|| a.y().total_cmp(&b.y()))
            .then_with(|| a.first_timestamp_ms().cmp(&b.first_timestamp_ms()))
            .then_with(|| a.agg_id().cmp(&b.agg_id()))
    });
    let mut full = starting_full(
        sizing,
        tape.iter().map(D::quantity),
        by_reference.then(|| tape.iter().map(D::reference_quantity)),
        minimum_full,
    );
    let mut neighbours = Neighbours::new(f64::from(bubbles.max_radius) * 2.0, geometry);
    let mut active: Vec<Option<D>> = Vec::with_capacity(tape.len());
    let mut serial = 0_u64;
    for mut pending in tape {
        loop {
            if sizing.typed_full.is_none() {
                // Existing discs only shrink as a new largest dot grows, so
                // they cannot create a new collision with one another.
                let reference = pending.reference_quantity();
                serial += u64::from(reference > full);
                full = full.max(reference);
            }
            let at = Full {
                quantity: full,
                serial,
            };
            let radius = pending.radius(sizing, bubbles, lane, at);
            // No disc is wider than the style's largest, so no clearance is.
            let reach = f64::from(radius + bubbles.max_radius);
            let (x, y) = geometry.point(pending.x(), pending.y());
            let collision = neighbours
                .candidates(pending.x(), pending.y())
                .filter_map(|index| {
                    let other = active[index].as_mut()?;
                    let (other_x, other_y) = geometry.point(other.x(), other.y());
                    let dx = other_x - x;
                    let dy = other_y - y;
                    if dx.abs() >= reach || dy.abs() >= reach {
                        return None;
                    }
                    let other_radius = other.radius(sizing, bubbles, lane, at);
                    let clearance = f64::from(
                        radius + other_radius
                            - SMALLER_DOT_OVERLAP_SHARE * radius.min(other_radius),
                    );
                    if dx.abs() >= clearance || dy.abs() >= clearance {
                        return None;
                    }
                    let distance = dx.hypot(dy);
                    (distance < clearance && pending.permits_merge(other)).then_some((
                        index,
                        distance,
                        other.agg_id(),
                    ))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.cmp(&b.2)))
                .map(|candidate| candidate.0);
            let Some(index) = collision else { break };
            let absorbed = active[index].take().expect("an indexed dot is active");
            neighbours.remove(index, absorbed.x(), absorbed.y());
            pending.absorb(absorbed, geometry.right_x);
        }
        neighbours.insert(active.len(), pending.x(), pending.y());
        active.push(Some(pending));
    }
    (active.into_iter().flatten().collect(), full)
}

/// The reference a merge starts from: the largest eligible volume on the
/// tape (`reference`, one per disc, when the burst is kept out of scale),
/// or the pane's full size over `quantities`; never under `minimum_full`,
/// never zero.
fn starting_full(
    sizing: DotSizing,
    quantities: impl Iterator<Item = Decimal>,
    reference: Option<impl Iterator<Item = Decimal>>,
    minimum_full: Decimal,
) -> Decimal {
    let initial = match reference {
        Some(reference) if sizing.typed_full.is_none() => reference.max().unwrap_or_default(),
        _ => {
            let full = sizing
                .typed_full
                .unwrap_or_else(|| quantities.max().unwrap_or_default());
            if full > Decimal::ZERO {
                full
            } else {
                Decimal::ONE
            }
        }
    };
    let full = initial.max(minimum_full);
    if full <= Decimal::ZERO {
        Decimal::ONE
    } else {
        full
    }
}

/// Combine tape dots whose discs would obscure each other. Areas stay
/// proportional to exact quantity and the visible maximum is recalculated
/// as merges grow. Neighbours may overlap by one tenth of the smaller
/// radius, so even a tiny dot beside a large one keeps its own visible disc.
///
/// The optional native tape owns this behaviour ([`DotSizing::native_tape`]).
/// Other panes pass through unchanged, including their existing ordering
/// and sizing.
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
    if !sizing.native_tape || !geometry.valid() || bubbles.max_radius <= 0.0 {
        return marks.to_vec();
    }
    let (tape, other): (Vec<_>, Vec<_>) = marks
        .iter()
        .enumerate()
        .map(|(index, mark)| {
            let mut moment = TapeMoment::new(mark.clone(), geometry.right_x);
            if let Some(quantity) = reference.quantities.get(index) {
                moment.reference_quantity = *quantity;
            }
            moment
        })
        .partition(|moment| geometry.visible(&moment.mark));
    let (merged, full) = collide(
        tape,
        sizing,
        bubbles,
        lane,
        geometry,
        reference.minimum_full,
        !reference.quantities.is_empty(),
    );
    let mut other: Vec<_> = other.into_iter().map(|moment| moment.mark).collect();
    other.extend(merged.into_iter().map(|dot| dot.finish(full)));
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
pub(super) fn combine_tape_facts<'a>(
    marks: impl IntoIterator<Item = &'a AggressionPrimitive>,
) -> Option<AggressionPrimitive> {
    let mut marks = marks.into_iter();
    let mut first = marks.next()?.clone();
    first.x = 0.0;
    first.y = 0.0;
    let mut moment = TapeMoment::new(first, 1.0);
    for mark in marks {
        moment.absorb_fact(mark);
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
    let mut neighbours = Neighbours::new(
        f64::from(bubbles.max_radius.max(f32::MIN_POSITIVE)) * 2.0,
        geometry,
    );
    let mut share = 1.0_f64;
    for (index, mark) in shown.iter().enumerate() {
        let (x, y) = geometry.position(mark);
        for other in neighbours.candidates(mark.x, mark.y) {
            let (other_x, other_y) = geometry.position(shown[other]);
            let clearance = f64::from(
                radii[index] + radii[other]
                    - SMALLER_DOT_OVERLAP_SHARE * radii[index].min(radii[other]),
            );
            if clearance > 0.0 {
                share = share.min((x - other_x).hypot(y - other_y) / clearance);
            }
        }
        neighbours.insert(index, mark.x, mark.y);
    }
    bubbles.max_radius * share.clamp(0.0, 1.0) as f32
}
