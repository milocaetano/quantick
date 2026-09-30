//! The tape memory's groups: native facts that became one dot, and the
//! collision merge that decides which groups become one.
//!
//! A group's mark is always [`combine_tape_facts`] over its facts in key
//! order, and a merge must keep it so. Refolding every fact of every merged
//! group made each closed window cost the whole frontier's facts, which on a
//! wide WIN tape held a whole-window reread for tens of seconds. So a merge
//! reads only each group's moments ([`GroupDisc`]), a group that merged with
//! nothing is kept as it is, and a group whose facts all follow another's
//! continues that group's fold instead of starting it again: the same fold,
//! step for step, so the same bytes.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::dots::{native_tape_radius, window_start};
use super::tape::{Full, MergeDisc, TapeMoment, TapeReference, collide, combine_tape_facts};
use super::{AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry, TapeDotView};
use crate::config::{BubbleStyle, LiveLaneStyle};

#[cfg(test)]
#[path = "tests/dots_tests/tape_tests/group_merge_tests.rs"]
mod group_merge_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NativeKey(pub(super) i64, pub(super) Decimal);

#[derive(Debug, Clone)]
pub(super) struct Group {
    pub(super) mark: AggressionPrimitive,
    pub(super) price_quantity: Decimal,
    // Native keys survive venue trade-ID restarts. Keeping their exact facts
    // also permits a late execution or canonical eviction to update one group.
    pub(super) sources: BTreeMap<NativeKey, AggressionPrimitive>,
    /// The quantity-weighted mean time of the facts, from the mark.
    mean_ms: Decimal,
    /// Where the last merge placed the group, which the next one usually
    /// places it at again.
    placed: Placement,
}

/// Where one merge placed a group: its price's height in the merge's price
/// window, which every window of one pass shares, and the radius the full
/// size it was sized against gave it.
#[derive(Debug, Clone, Copy, Default)]
struct Placement {
    y: Option<(PriceWindow, f64)>,
    radius: Option<(Decimal, f32)>,
}

impl Group {
    pub(super) fn of(sources: BTreeMap<NativeKey, AggressionPrimitive>) -> Self {
        let mark = combine_tape_facts(sources.values()).expect("a group has native facts");
        let price_quantity = sources
            .values()
            .map(|mark| mark.price * mark.quantity)
            .sum();
        Self::holding(mark, price_quantity, sources)
    }

    fn holding(
        mark: AggressionPrimitive,
        price_quantity: Decimal,
        sources: BTreeMap<NativeKey, AggressionPrimitive>,
    ) -> Self {
        Self {
            mean_ms: mark.timestamp_quantity / mark.quantity,
            placed: Placement::default(),
            mark,
            price_quantity,
            sources,
        }
    }

    /// The quantity-weighted mean time of the facts.
    pub(super) fn mean_ms(&self) -> Decimal {
        self.mean_ms
    }

    /// [`PriceWindow::y_unclamped`] of the mark's price in `prices`.
    fn price_y(&self, prices: PriceWindow) -> f64 {
        match self.placed.y {
            Some((held, y)) if held == prices => y,
            _ => prices.y_unclamped(self.mark.price).unwrap_or_default(),
        }
    }

    pub(super) fn visible_at(&self, now_ms: i64, window_ms: i64) -> bool {
        visible(
            self.mark.timestamp_quantity,
            self.mark.quantity,
            now_ms,
            window_ms,
        )
    }

    pub(super) fn retained_at(&self, now_ms: i64, window_ms: i64) -> bool {
        // An expired aggregate still consumes its later native members until
        // they leave the input. Retain their facts so a genuine late update
        // can move the complete group's mean back into view without duplication.
        self.visible_at(now_ms, window_ms)
            || self
                .sources
                .keys()
                .any(|key| key.0 >= now_ms.saturating_sub(window_ms))
    }

    pub(super) fn refresh(&mut self) {
        if let Some(mark) = combine_tape_facts(self.sources.values()) {
            self.mean_ms = mark.timestamp_quantity / mark.quantity;
            self.placed = Placement::default();
            self.mark = mark;
            self.price_quantity = self
                .sources
                .values()
                .map(|mark| mark.price * mark.quantity)
                .sum();
        }
    }

    pub(super) fn reference_quantity(&self, opening_bursts: &[i64]) -> Decimal {
        // Without a fact in an opening window the sum below is the fold's
        // own quantity, addend for addend.
        if !self.bursting(opening_bursts) {
            return self.mark.quantity;
        }
        self.sources
            .iter()
            .filter(|(key, _)| !opening_bursts.contains(&key.0))
            .map(|(_, mark)| mark.quantity)
            .sum()
    }

    pub(super) fn coincides(&self, other: &Self) -> bool {
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

    /// Whether any fact lies in one of `opening_bursts`' windows.
    fn bursting(&self, opening_bursts: &[i64]) -> bool {
        opening_bursts.iter().any(|window| {
            self.sources
                .range(NativeKey(*window, Decimal::MIN)..=NativeKey(*window, Decimal::MAX))
                .next()
                .is_some()
        })
    }

    fn first_key(&self) -> NativeKey {
        *self
            .sources
            .keys()
            .next()
            .expect("a group has native facts")
    }

    fn last_key(&self) -> NativeKey {
        *self
            .sources
            .keys()
            .next_back()
            .expect("a group has native facts")
    }

    /// [`Group::of`] over every member's facts. When the members' facts
    /// follow one another in key order, the first member's fold goes on
    /// over the rest, which is that fold itself; interleaved members are
    /// folded again from their facts.
    pub(super) fn merge(mut members: Vec<Self>) -> Self {
        members.sort_by_key(Self::first_key);
        if in_key_order(&members) {
            let mut members = members.into_iter();
            let first = members.next().expect("a merge has members");
            return first.followed_by(members);
        }
        let mut members = members.into_iter();
        let mut sources = members.next().expect("a merge has members").sources;
        for member in members {
            sources.extend(member.sources);
        }
        Self::of(sources)
    }

    /// The mark and exact price moment [`Group::merge`] would give `members`,
    /// read without taking them apart.
    fn merged_mark(members: &[&Self]) -> (AggressionPrimitive, Decimal) {
        let mut members = members.to_vec();
        members.sort_by_key(|group| group.first_key());
        if in_key_order(&members) {
            let (first, later) = members.split_first().expect("a merge has members");
            let mut moment = TapeMoment::resumed(first.mark.clone(), first.price_quantity);
            for fact in later.iter().flat_map(|group| group.sources.values()) {
                moment.absorb_fact(fact);
            }
            let price_quantity = moment.price_quantity();
            return (moment.finish(Decimal::ONE), price_quantity);
        }
        let mut facts: Vec<(&NativeKey, &AggressionPrimitive)> = members
            .iter()
            .flat_map(|group| group.sources.iter())
            .collect();
        facts.sort_by_key(|(key, _)| **key);
        let price_quantity = facts
            .iter()
            .map(|(_, mark)| mark.price * mark.quantity)
            .sum();
        let mark =
            combine_tape_facts(facts.into_iter().map(|(_, fact)| fact)).expect("a merge has facts");
        (mark, price_quantity)
    }

    /// This group with the facts of `later`, groups in key order after it
    /// and after each other, folded on as [`combine_tape_facts`] folds them.
    fn followed_by(self, later: impl Iterator<Item = Self>) -> Self {
        let Self {
            mark,
            price_quantity,
            mut sources,
            ..
        } = self;
        let mut moment = TapeMoment::resumed(mark, price_quantity);
        for (key, fact) in later.flat_map(|group| group.sources) {
            moment.absorb_fact(&fact);
            sources.insert(key, fact);
        }
        let price_quantity = moment.price_quantity();
        Self::holding(moment.finish(Decimal::ONE), price_quantity, sources)
    }
}

/// Whether each group's facts all come before the next group's, groups
/// sorted by their first fact.
fn in_key_order<G: std::borrow::Borrow<Group>>(groups: &[G]) -> bool {
    groups
        .windows(2)
        .all(|pair| pair[0].borrow().last_key() < pair[1].borrow().first_key())
}

/// Whether a group with these moments has its mean time inside the window.
pub(super) fn visible(
    timestamp_quantity: Decimal,
    quantity: Decimal,
    now_ms: i64,
    window_ms: i64,
) -> bool {
    timestamp_quantity >= Decimal::from(now_ms.saturating_sub(window_ms)) * quantity
}

/// A group as the collision merge reads it: where it sits in the merge's
/// frame, how much it holds, and which groups it has absorbed. It moves as
/// [`TapeMoment`] moves a whole mark, on exactly the same moments.
struct GroupDisc {
    x: f64,
    y: f64,
    quantity: Decimal,
    reference_quantity: Decimal,
    /// The quantity-weighted positions once the disc has absorbed another;
    /// until then they are its own position times its quantity.
    x_quantity: Option<Decimal>,
    y_quantity: Option<Decimal>,
    at_now: bool,
    agg_id: u64,
    first_timestamp_ms: i64,
    /// The radius at the full size of this serial, and that size.
    radius_cache: Option<(u64, Decimal, f32)>,
    /// A lone group's radius from the last merge, used only against the very
    /// same full size, digit for digit.
    seed: Option<(Decimal, f32)>,
    members: Members,
}

/// The groups one disc stands for, by their index in the merge.
enum Members {
    One(usize),
    Many(Vec<usize>),
}

impl Members {
    fn join(&mut self, other: Self) {
        let mut all = match std::mem::replace(self, Self::Many(Vec::new())) {
            Self::One(index) => vec![index],
            Self::Many(indices) => indices,
        };
        match other {
            Self::One(index) => all.push(index),
            Self::Many(indices) => all.extend(indices),
        }
        *self = Self::Many(all);
    }
}

impl GroupDisc {
    fn x_quantity(&self) -> Decimal {
        self.x_quantity
            .unwrap_or_else(|| Decimal::from_f64_retain(self.x).unwrap_or_default() * self.quantity)
    }

    fn y_quantity(&self) -> Decimal {
        self.y_quantity
            .unwrap_or_else(|| Decimal::from_f64_retain(self.y).unwrap_or_default() * self.quantity)
    }
}

impl MergeDisc for GroupDisc {
    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn first_timestamp_ms(&self) -> i64 {
        self.first_timestamp_ms
    }

    fn agg_id(&self) -> u64 {
        self.agg_id
    }

    fn quantity(&self) -> Decimal {
        self.quantity
    }

    fn reference_quantity(&self) -> Decimal {
        self.reference_quantity
    }

    fn radius(
        &mut self,
        _sizing: DotSizing,
        bubbles: &BubbleStyle,
        _lane: &LiveLaneStyle,
        full: Full,
    ) -> f32 {
        // Every group is a live mark of a native tape, which
        // [`DotSizing::radius`] sizes by quantity alone.
        if let Some((serial, _, radius)) = self.radius_cache
            && serial == full.serial
        {
            return radius;
        }
        let radius = match self.seed.take() {
            Some((held, radius)) if held.serialize() == full.quantity.serialize() => radius,
            _ => native_tape_radius(bubbles, self.quantity, full.quantity),
        };
        self.radius_cache = Some((full.serial, full.quantity, radius));
        radius
    }

    fn absorb(&mut self, other: Self, right_x: f64) {
        let x_quantity = self.x_quantity() + other.x_quantity();
        let y_quantity = self.y_quantity() + other.y_quantity();
        self.radius_cache = None;
        self.seed = None;
        self.reference_quantity += other.reference_quantity;
        self.at_now |= other.at_now;
        self.quantity += other.quantity;
        self.first_timestamp_ms = self.first_timestamp_ms.min(other.first_timestamp_ms);
        self.agg_id = self.agg_id.min(other.agg_id);
        self.members.join(other.members);
        self.x = if self.at_now {
            right_x
        } else {
            (x_quantity / self.quantity).to_f64().unwrap_or(self.x)
        };
        self.y = (y_quantity / self.quantity).to_f64().unwrap_or(self.y);
        self.x_quantity = Some(x_quantity);
        self.y_quantity = Some(y_quantity);
    }
}

/// Which of `groups` merge, as the output's members in output order; `None`
/// where the merge passes them through as they are. The existing geometric
/// merger operates on indivisible retained groups: each is placed in a frame
/// spanning just these groups, and temporary local IDs identify them, never a
/// venue's reused IDs.
fn merge_plan(
    groups: &[&Group],
    view: TapeDotView,
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    reference: TapeReference<'_>,
    preview_now: Option<i64>,
) -> Option<(Vec<Members>, Vec<Placement>)> {
    if groups.len() < 2 {
        return None;
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
    if !sizing.native_tape || !geometry.valid() || bubbles.max_radius <= 0.0 {
        return None;
    }
    let heights: Vec<f64> = groups.iter().map(|group| group.price_y(prices)).collect();
    let (tape, mut other): (Vec<_>, Vec<_>) = groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let mark = &group.mark;
            let x = if preview_now.is_some_and(|now| {
                window_start(mark.last_timestamp_ms, view.dot_window_ms)
                    == window_start(now, view.dot_window_ms)
            }) {
                1.0
            } else {
                ((group.mean_ms() - Decimal::from(low_time)) / time_span)
                    .to_f64()
                    .unwrap_or_default()
            };
            let disc = GroupDisc {
                x,
                y: heights[index],
                quantity: mark.quantity,
                reference_quantity: reference
                    .quantities
                    .get(index)
                    .copied()
                    .unwrap_or(mark.quantity),
                x_quantity: None,
                y_quantity: None,
                at_now: x == geometry.right_x,
                agg_id: index as u64 + 1,
                first_timestamp_ms: mark.first_timestamp_ms,
                radius_cache: None,
                seed: group.placed.radius,
                members: Members::One(index),
            };
            (mark.live, disc)
        })
        .partition(|(live, disc)| *live && geometry.holds(disc.quantity, disc.x, disc.y));
    let (merged, _) = collide(
        tape.into_iter().map(|(_, disc)| disc).collect(),
        sizing,
        bubbles,
        lane,
        geometry,
        reference.minimum_full,
        !reference.quantities.is_empty(),
    );
    let mut placed: Vec<Placement> = heights
        .into_iter()
        .map(|y| Placement {
            y: Some((prices, y)),
            radius: None,
        })
        .collect();
    for disc in &merged {
        if let (Members::One(index), Some((_, full, radius))) = (&disc.members, disc.radius_cache) {
            placed[*index].radius = Some((full, radius));
        }
    }
    other.extend(merged.into_iter().map(|disc| (true, disc)));
    other.sort_by(|(_, a), (_, b)| {
        a.quantity
            .cmp(&b.quantity)
            .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });
    Some((
        other.into_iter().map(|(_, disc)| disc.members).collect(),
        placed,
    ))
}

/// Merge `groups` into the retained groups the frontier holds next.
pub(super) fn merge_groups(
    groups: Vec<Group>,
    view: TapeDotView,
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    reference: TapeReference<'_>,
) -> Vec<Group> {
    let Some((plan, placed)) = merge_plan(
        &groups.iter().collect::<Vec<_>>(),
        view,
        sizing,
        bubbles,
        lane,
        reference,
        None,
    ) else {
        return groups;
    };
    let mut slots: Vec<Option<Group>> = groups
        .into_iter()
        .zip(placed)
        .map(|(mut group, placed)| {
            group.placed = placed;
            Some(group)
        })
        .collect();
    let mut take = |index: usize| slots[index].take().expect("a group merges once");
    plan.into_iter()
        .map(|members| match members {
            Members::One(only) => take(only),
            Members::Many(members) => Group::merge(members.into_iter().map(&mut take).collect()),
        })
        .collect()
}

/// One dot the preview draws: its mark, and its volume eligible for the
/// automatic reference when opening bursts are kept out of scale.
pub(super) struct PreviewDot {
    pub(super) mark: AggressionPrimitive,
    pub(super) reference_quantity: Decimal,
}

/// The frontier and the forming windows as this frame draws them, merged at
/// `now_ms` without taking a retained group apart: the frontier keeps its
/// groups for the windows still to close.
#[allow(clippy::too_many_arguments)]
pub(super) fn preview_groups(
    groups: &[&Group],
    view: TapeDotView,
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    reference: TapeReference<'_>,
    now_ms: Option<i64>,
    opening_bursts: &[i64],
) -> Vec<PreviewDot> {
    let whole = |group: &Group| PreviewDot {
        mark: group.mark.clone(),
        reference_quantity: group.reference_quantity(opening_bursts),
    };
    let Some((plan, _)) = now_ms
        .and_then(|now| merge_plan(groups, view, sizing, bubbles, lane, reference, Some(now)))
    else {
        return groups.iter().map(|group| whole(group)).collect();
    };
    plan.into_iter()
        .map(|members| match members {
            Members::One(only) => whole(groups[only]),
            Members::Many(members) => {
                let members: Vec<&Group> = members.iter().map(|index| groups[*index]).collect();
                let (mark, _) = Group::merged_mark(&members);
                let reference_quantity =
                    merged_reference_quantity(&members, opening_bursts).unwrap_or(mark.quantity);
                PreviewDot {
                    mark,
                    reference_quantity,
                }
            }
        })
        .collect()
}

/// [`Group::reference_quantity`] of the group [`Group::merge`] would give
/// `members` (the same sum, over the same facts in the same order); `None`
/// where no fact is in an opening window, and the sum is the group's own
/// quantity.
fn merged_reference_quantity(members: &[&Group], opening_bursts: &[i64]) -> Option<Decimal> {
    if !members.iter().any(|group| group.bursting(opening_bursts)) {
        return None;
    }
    let mut facts: Vec<(&NativeKey, &AggressionPrimitive)> = members
        .iter()
        .flat_map(|group| group.sources.iter())
        .collect();
    facts.sort_by_key(|(key, _)| **key);
    Some(
        facts
            .iter()
            .filter(|(key, _)| !opening_bursts.contains(&key.0))
            .map(|(_, mark)| mark.quantity)
            .sum(),
    )
}

/// The settled half of the reference a closing window's merge starts from,
/// kept across one pass over the windows: the largest eligible and factual
/// quantity among the settled groups still in view. The pass's clock only
/// moves on, so a group that left the view is popped once and never read
/// again, and ties go to the later group, as a fold over the settled groups
/// in order leaves them.
#[derive(Default)]
pub(super) struct SettledReference {
    eligible: BinaryHeap<Held>,
    factual: BinaryHeap<Held>,
    /// Settled groups already held.
    held: usize,
}

/// One settled group's value, with the moments that say whether it is still
/// in view.
struct Held {
    value: Decimal,
    index: usize,
    timestamp_quantity: Decimal,
    quantity: Decimal,
}

impl PartialEq for Held {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Held {}

impl PartialOrd for Held {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Held {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value
            .cmp(&other.value)
            .then(self.index.cmp(&other.index))
    }
}

impl SettledReference {
    /// The reference a merge at `(now_ms, window_ms)` starts from over
    /// `settled`,
    /// which only grew since the last call, and `candidates`: the largest
    /// eligible quantity in view, else the largest factual one.
    pub(super) fn minimum_full(
        &mut self,
        settled: &[Group],
        candidates: &[Group],
        sizing: DotSizing,
        (now_ms, window_ms): (i64, i64),
        opening_bursts: &[i64],
    ) -> Decimal {
        if sizing.typed_full.is_some() {
            return Decimal::ZERO;
        }
        for (index, group) in settled.iter().enumerate().skip(self.held) {
            let held = |value| Held {
                value,
                index,
                timestamp_quantity: group.mark.timestamp_quantity,
                quantity: group.mark.quantity,
            };
            if !opening_bursts.is_empty() {
                self.eligible
                    .push(held(group.reference_quantity(opening_bursts)));
            }
            self.factual.push(held(group.mark.quantity));
        }
        self.held = settled.len();
        let factual = top_in_view(&mut self.factual, now_ms, window_ms);
        let eligible = if opening_bursts.is_empty() {
            factual
        } else {
            top_in_view(&mut self.eligible, now_ms, window_ms)
        };
        let (eligible, factual) = candidates
            .iter()
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
                (
                    eligible.unwrap_or(Decimal::ZERO),
                    factual.unwrap_or(Decimal::ZERO),
                ),
                |(eligible, factual), (next, quantity)| (eligible.max(next), factual.max(quantity)),
            );
        if eligible > Decimal::ZERO {
            eligible
        } else {
            factual
        }
    }
}

/// The largest held value still in view at `now_ms`, popping every larger
/// one that left it.
fn top_in_view(heap: &mut BinaryHeap<Held>, now_ms: i64, window_ms: i64) -> Option<Decimal> {
    while let Some(top) = heap.peek() {
        if visible(top.timestamp_quantity, top.quantity, now_ms, window_ms) {
            return Some(top.value);
        }
        heap.pop();
    }
    None
}
