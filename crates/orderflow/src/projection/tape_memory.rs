//! Causal tape groups: the frontier may grow, settled memberships do not.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::dots::window_start;
use super::tape::{TapeReference, position_tape_with, tape_radius_limit};
use super::tape_group::{
    Group, NativeKey, PreviewDot, SettledReference, merge_groups, preview_groups,
};
use super::tiers::aggression_primitive;
use super::{
    AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry, TapeFacts, TapeLineage,
    TapeOverlay, draws_bubble, normalized_area_size,
};
use crate::config::theme::OrderflowRenderStyle;
use crate::config::{BubbleStyle, LiveLaneStyle};
use crate::interaction::AggressionCluster;

#[cfg(test)]
#[path = "tests/dots_tests/tape_tests/native_fact_tests.rs"]
mod native_fact_tests;

#[path = "tape_past_memory.rs"]
mod past;

#[path = "tape_work.rs"]
mod work;
pub use work::TapeWork;

pub use past::PastTapeMemory;

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

/// Per-pane display history. Only newly closed native windows are merged;
/// each settled group retains its source facts until its whole centroid exits.
#[derive(Debug, Default, Clone)]
pub struct TapeDotMemory {
    epoch: Option<TapeDotView>,
    settled: Vec<Group>,
    frontier: Vec<Group>,
    /// A frozen past has no forming window to pin at its right edge.
    frozen: bool,
    /// What the last sealed frame left reconciled; `None` rereads every cell.
    ingested: Option<Ingested>,
    /// Native cells handed to reconciliation by the last projection.
    reconciled: usize,
    /// The newest closed native window merged into the groups.
    merged_through: Option<i64>,
    /// Raised once the rebuild this memory reconciles in can no longer be
    /// adopted ([`crate::projection::TapeRebuild`]): the pass stops at the
    /// next window instead of finishing work nobody will draw.
    abandoned: Option<Arc<AtomicBool>>,
}

/// Every cell of `lineage` before `below_ms` has been reconciled, under the
/// same floor, bubble switches and eviction horizon.
#[derive(Debug, Clone)]
struct Ingested {
    lineage: Arc<TapeLineage>,
    below_ms: i64,
    floor: Decimal,
    switches: (bool, bool, bool),
    horizon: Option<i64>,
    /// The overlay's keys: when one leaves the overlay, its published cell
    /// has to be read again, sealed or not.
    overlay_keys: Vec<NativeKey>,
}

/// A sealed published tape, the accepted prints beside it, and the switches
/// that decide which of its cells the painter draws.
#[derive(Clone, Copy)]
pub struct TapeSource<'a> {
    pub facts: &'a TapeFacts,
    pub overlay: Option<&'a TapeOverlay>,
    pub style: &'a OrderflowRenderStyle,
}

impl TapeSource<'_> {
    /// The floor and bubble switches the source's cells are drawn under; a
    /// reconciled prefix is trusted only under the same ones.
    fn reading(self) -> (Decimal, (bool, bool, bool)) {
        let floor = self
            .overlay
            .map_or(self.facts.floor, |overlay| overlay.floor);
        let style = self.style;
        (
            floor,
            (style.lane_aggression_layer, style.show_buy, style.show_sell),
        )
    }
}

impl TapeDotMemory {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Reconcile under `abandoned`: once it is raised, what is left of a
    /// pass is skipped and the memory is fit only to be dropped.
    pub fn abandon_when(&mut self, abandoned: Option<Arc<AtomicBool>>) {
        self.abandoned = abandoned;
    }

    fn is_abandoned(&self) -> bool {
        self.abandoned
            .as_ref()
            .is_some_and(|flag| flag.load(atomic::Ordering::Relaxed))
    }

    pub fn retained_group_count(&self) -> usize {
        self.settled.len() + self.frontier.len()
    }

    /// How many native cells the last projection handed to reconciliation.
    /// A sealed tape keeps this to the cells after its seal and the pending
    /// prints, however long the tape window is.
    pub fn reconciled_cells(&self) -> usize {
        self.reconciled
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
        if !keys_native_cells(sizing, view) {
            return TapeDotFrame {
                marks: marks.to_vec(),
                max_radius: bubbles.max_radius,
                full_quantity: sizing.full_quantity(marks, true),
            };
        }
        self.prepare_epoch(view);
        // Marks carry no seal: the next sealed frame rereads every cell.
        self.ingested = None;
        let incoming = native_facts(marks, view);
        self.settle(
            incoming,
            None,
            marks,
            view,
            sizing,
            bubbles,
            lane,
            opening_bursts,
        )
    }

    /// Where the reconciled prefix of `source`'s lineage ends, when the
    /// memory holds one it can trust at `view`: the same lineage, floor and
    /// switches, under a horizon that only moved on. `None` rereads it all.
    fn reconciled_below(&self, source: TapeSource<'_>, view: TapeDotView) -> Option<i64> {
        let seal = source.facts.seal.as_ref()?;
        let (floor, switches) = source.reading();
        self.ingested
            .as_ref()
            .filter(|state| {
                Arc::ptr_eq(&state.lineage, &seal.lineage)
                    && state.floor == floor
                    && state.switches == switches
                    && horizon_holds(state.horizon, view)
            })
            .map(|state| state.below_ms)
    }

    /// [`Self::project`] for a sealed published tape and the accepted prints
    /// beside it, reading only what can have changed since the last frame of
    /// the same lineage: the cells after its seal, the overlay, and the
    /// published cells of keys that left the overlay. `marks` supply only the
    /// frame's candle marks, which pass through. `None` where the tape is not
    /// keyed on native cells, for the caller's complete path.
    #[allow(clippy::too_many_arguments)]
    pub fn project_sealed(
        &mut self,
        source: TapeSource<'_>,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        opening_bursts: &[i64],
    ) -> Option<TapeDotFrame> {
        let seal = source.facts.seal.as_ref()?;
        if !keys_native_cells(sizing, view) || seal.window_ms != view.dot_window_ms {
            return None;
        }
        self.prepare_epoch(view);
        let (floor, switches) = source.reading();
        let below = self.reconciled_below(source, view);
        let style = source.style;
        let window =
            |cell: &AggressionCluster| window_start(cell.first_timestamp_ms, view.dot_window_ms);
        let from = source.overlay.map(|overlay| overlay.from_ms);
        let drawn = |cell: &AggressionCluster| {
            let start = window(cell);
            cell.quantity > Decimal::ZERO
                && cell.quantity >= floor
                && from.is_none_or(|from| start >= from)
                && !view
                    .evicted_through_ms
                    .is_some_and(|horizon| start <= horizon)
                && start >= view.now_ms.saturating_sub(view.window_ms)
        };
        let key = |cell: &AggressionCluster| NativeKey(window(cell), cell.price_bucket);
        let cells = &source.facts.clusters;
        let first = below.map_or(0, |below| {
            cells.partition_point(|cell| window(cell) < below)
        });
        let overlay: &[AggressionCluster] = source.overlay.map_or(&[], |overlay| &overlay.cells);
        let mut overlay_keys: Vec<NativeKey> = overlay.iter().map(key).collect();
        overlay_keys.sort_unstable();
        // Keys that left the overlay since the last frame read their published
        // cell again: the overlay's fold of it may differ from the worker's.
        let returning =
            self.ingested
                .as_ref()
                .filter(|_| below.is_some())
                .map_or(Vec::new(), |state| {
                    state
                        .overlay_keys
                        .iter()
                        .filter(|key| key.0 < below.unwrap_or(i64::MIN))
                        .filter(|key| overlay_keys.binary_search(key).is_err())
                        .filter_map(|key| {
                            let low = cells.partition_point(|cell| window(cell) < key.0);
                            cells[low..]
                                .iter()
                                .take_while(|cell| window(cell) == key.0)
                                .find(|cell| cell.price_bucket == key.1)
                        })
                        .collect()
                });
        // Later entries win: an overlay cell supersedes the published one
        // before either is filtered, as the complete pending frame does.
        let current: BTreeMap<NativeKey, &AggressionCluster> = returning
            .into_iter()
            .chain(&cells[first..])
            .chain(overlay)
            .map(|cell| (key(cell), cell))
            .collect();
        let owned: Vec<(NativeKey, AggressionPrimitive)> = current
            .into_iter()
            .filter(|(_, cell)| drawn(cell))
            .map(|(key, cell)| (key, aggression_primitive(cell.clone(), 0.0, 0.0, 0.0, true)))
            .filter(|(_, mark)| draws_bubble(style, mark))
            .collect();
        let incoming: BTreeMap<NativeKey, &AggressionPrimitive> =
            owned.iter().map(|(key, mark)| (*key, mark)).collect();
        let frame = self.settle(
            incoming,
            below,
            marks,
            view,
            sizing,
            bubbles,
            lane,
            opening_bursts,
        );
        self.ingested = Some(Ingested {
            lineage: Arc::clone(&seal.lineage),
            below_ms: seal
                .through_ms
                .min(window_start(view.now_ms, view.dot_window_ms)),
            floor,
            switches,
            horizon: view.evicted_through_ms,
            overlay_keys,
        });
        Some(frame)
    }

    /// Reconcile `incoming` with the retained groups and draw the frame.
    /// `below`: every retained cell before it is already current, so only
    /// the groups reaching it are walked.
    #[allow(clippy::too_many_arguments)]
    fn settle(
        &mut self,
        mut incoming: BTreeMap<NativeKey, &AggressionPrimitive>,
        below: Option<i64>,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        opening_bursts: &[i64],
    ) -> TapeDotFrame {
        self.reconciled = incoming.len();
        let mut changed = self.replace_facts(&mut incoming, view.evicted_through_ms, below);
        let mut windows: BTreeMap<i64, BTreeMap<NativeKey, AggressionPrimitive>> = BTreeMap::new();
        for (key, mark) in incoming {
            windows
                .entry(key.0)
                .or_default()
                .insert(key, normalized_native_fact(mark));
        }
        let mut forming = Vec::new();
        let mut settled_reference = SettledReference::default();
        let mut last_closed = None;
        for (window, sources) in windows {
            if self.is_abandoned() {
                return TapeDotFrame {
                    marks: Vec::new(),
                    max_radius: bubbles.max_radius,
                    full_quantity: Decimal::ONE,
                };
            }
            let groups = sources
                .into_iter()
                .map(|(key, mark)| Group::of(BTreeMap::from([(key, mark)])));
            let closed_at = window.saturating_add(view.dot_window_ms.max(1));
            if closed_at > view.now_ms {
                forming.extend(groups);
                continue;
            }
            // The settled groups a later window would no longer keep are only
            // read again as invisible, so they are let go once, after the last.
            self.seal_frontier_at(closed_at, view, bubbles.max_radius);
            last_closed = Some(closed_at);
            self.merged_through = Some(self.merged_through.map_or(window, |at| at.max(window)));
            let mut candidates = std::mem::take(&mut self.frontier);
            candidates.extend(groups);
            let quantities = reference_quantities(&candidates, opening_bursts);
            let reference = TapeReference {
                minimum_full: settled_reference.minimum_full(
                    &self.settled,
                    &candidates,
                    sizing,
                    (closed_at, view.window_ms),
                    opening_bursts,
                ),
                quantities: &quantities,
            };
            self.frontier = merge_groups(candidates, view, sizing, bubbles, lane, reference);
            changed = true;
        }
        if let Some(closed_at) = last_closed {
            self.settled
                .retain(|group| group.retained_at(closed_at, view.window_ms));
        }
        if changed {
            self.coalesce_coincident();
        }
        self.seal_at(view.now_ms, view, bubbles.max_radius);
        let has_forming = !forming.is_empty();
        let active: Vec<&Group> = self.frontier.iter().chain(&forming).collect();
        let quantities = reference_quantities(active.iter().copied(), opening_bursts);
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
        let active = preview_groups(
            &active,
            view,
            sizing,
            bubbles,
            lane,
            reference,
            has_forming.then_some(view.now_ms),
            opening_bursts,
        );
        // The same reference policy sizes frontier merges and final discs;
        // every factual quantity and weighted moment still includes the burst.
        let reference_quantities = (!opening_bursts.is_empty() && sizing.typed_full.is_none())
            .then(|| {
                self.settled
                    .iter()
                    .map(|group| group.reference_quantity(opening_bursts))
                    .chain(active.iter().map(|dot| dot.reference_quantity))
                    .collect::<Vec<_>>()
            });
        let shown: Vec<_> = self
            .settled
            .iter()
            .map(|group| group.mark.clone())
            .chain(active.into_iter().map(|dot: PreviewDot| dot.mark))
            .collect();
        self.draw(
            shown,
            reference_quantities,
            marks,
            view,
            sizing,
            bubbles,
            lane,
        )
    }

    /// Place, size and cap `shown`, the groups' marks with their volumes
    /// eligible for the automatic reference, and pass the candle marks of
    /// `marks` through.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        mut shown: Vec<AggressionPrimitive>,
        reference_quantities: Option<Vec<Decimal>>,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
    ) -> TapeDotFrame {
        position_tape_with(
            &mut shown,
            view.now_ms,
            view.window_ms,
            view.geometry.left_x,
            (!self.frozen).then_some(view.dot_window_ms),
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

    /// Whether `view` begins a new display epoch: another time window or
    /// pixel geometry merges every group again from its cells.
    fn starts_over(&self, view: TapeDotView) -> bool {
        self.epoch.is_some_and(|old| {
            old.window_ms != view.window_ms
                || old.dot_window_ms != view.dot_window_ms
                || old.geometry.width_px != view.geometry.width_px
                || old.geometry.height_px != view.geometry.height_px
        })
    }

    fn prepare_epoch(&mut self, view: TapeDotView) {
        if self.starts_over(view) {
            self.clear();
        }
        self.epoch = Some(view);
    }

    fn replace_facts(
        &mut self,
        incoming: &mut BTreeMap<NativeKey, &AggressionPrimitive>,
        evicted: Option<i64>,
        below: Option<i64>,
    ) -> bool {
        // Cells before `below` arrive only from the overlay or as keys that
        // left it; a group wholly before it is visited for those keys alone.
        let early: Vec<NativeKey> = below.map_or_else(Vec::new, |below| {
            incoming
                .range(..NativeKey(below, Decimal::MIN))
                .map(|(key, _)| *key)
                .collect()
        });
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
            let reaches = below.is_none_or(|below| {
                group
                    .sources
                    .last_key_value()
                    .is_some_and(|(key, _)| key.0 >= below)
            });
            if reaches {
                for (key, source) in &mut group.sources {
                    if let Some(next) = incoming.remove(key)
                        && !same_native_fact(source, next)
                    {
                        *source = normalized_native_fact(next);
                        changed = true;
                    }
                }
            } else {
                for key in &early {
                    if let Some(source) = group.sources.get_mut(key)
                        && let Some(next) = incoming.remove(key)
                        && !same_native_fact(source, next)
                    {
                        *source = normalized_native_fact(next);
                        changed = true;
                    }
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
            let key = (group.mean_ms(), group.mark.price);
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
        self.seal_frontier_at(now_ms, view, radius);
        self.settled
            .retain(|group| group.retained_at(now_ms, view.window_ms));
    }

    /// Settle the frontier groups no dot at `now_ms` can reach any more, and
    /// let go of the frontier groups `now_ms` no longer keeps.
    fn seal_frontier_at(&mut self, now_ms: i64, view: TapeDotView, radius: f32) {
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
        self.frontier
            .retain(|group| group.retained_at(now_ms, view.window_ms));
    }

    fn reference(
        &self,
        active: &[&Group],
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
            .chain(active.iter().copied())
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

/// Whether the view keys native cells at all; elsewhere marks pass through.
fn keys_native_cells(sizing: DotSizing, view: TapeDotView) -> bool {
    sizing.native_tape && view.geometry.valid() && view.window_ms > 0
}

/// Whether the reconciled cells survive `view`'s eviction horizon: it only
/// moved forward, or the cells it would give back have already expired.
fn horizon_holds(reconciled: Option<i64>, view: TapeDotView) -> bool {
    match (reconciled, view.evicted_through_ms) {
        (None, _) => true,
        (Some(old), Some(new)) if new >= old => true,
        (Some(old), _) => old < view.now_ms.saturating_sub(view.window_ms),
    }
}

fn reference_quantities<'a>(
    groups: impl IntoIterator<Item = &'a Group>,
    opening_bursts: &[i64],
) -> Vec<Decimal> {
    if opening_bursts.is_empty() {
        Vec::new()
    } else {
        groups
            .into_iter()
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
