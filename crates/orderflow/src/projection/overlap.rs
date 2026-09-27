//! The overlap fold: bubbles whose discs would touch on the canvas become one
//! mark.
//!
//! A dense tape — an MT5 feed folding several deals into each tick is the
//! case that asked for this — piles bubbles on top of each other: a green
//! disc half under a red one reads as neither, and ten specks inside a tenth
//! of a second read as noise. The budget fold (`fold`) does not help, because
//! a frame can be well inside its budget and still unreadable; the question
//! here is geometric, not a count. So, when the trader opts in, every group of
//! discs that would touch is folded into one mark: the exact summed quantity,
//! the union of the evidence, a pie when both sides are in it, and the `⊕n`
//! label that says the canvas did this, not the market.
//!
//! The decision needs pixels the normalized projection does not carry, so the
//! chart hands them over as a [`PaneGeometry`] and the radius comes from the
//! same [`bubble_radius`] the painter draws with. Nothing here reads a clock
//! or iterates a hash: the same frame and the same geometry fold the same way.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::config::{HeatmapConfig, bubble_radius};
use crate::history::AggressorSide;
use crate::timeline::BarTimeline;

use super::fold::{absorb, fold_chunk};
use super::model::{AggressionPrimitive, HeatmapProjection, frame_order};

/// The pixel geometry of the pane a frame is drawn on.
///
/// Only what an overlap test needs: how far apart two marks land on screen.
/// The candles and the tape are measured separately because they are mapped
/// separately — a bar slot is `px_per_bar` wide, the tape's one region is
/// `lane_width_px` wide — and the fold never compares a mark across them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneGeometry {
    /// Pixels between two neighbouring bars on the candles.
    pub px_per_bar: f32,
    /// Width of the live lane, in pixels. Zero when no lane is drawn.
    pub lane_width_px: f32,
    /// Height of the chart, in pixels.
    pub height_px: f32,
}

/// How many times the fold re-measures after merging.
///
/// A fold is bigger than any of its members, so it can reach a neighbour none
/// of them touched; each pass settles those, and a pile-up converges in two or
/// three. The cap bounds a frame's cost on a pathological tape — what it can
/// leave behind is at worst a touching pair, never a lost contract.
const MAX_OVERLAP_PASSES: usize = 8;

/// One mark's disc on screen, in pixels.
///
/// Offsets that are the same for every mark of a pane — the chart's left and
/// top edge, the viewport's scroll, which way up the chart is — are left out:
/// they move every disc together and so cannot make two of them touch.
#[derive(Debug, Clone, Copy)]
struct Disc {
    x: f64,
    y: f64,
    radius: f64,
}

impl Disc {
    fn touches(self, other: Self) -> bool {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let reach = self.radius + other.radius;
        dx * dx + dy * dy < reach * reach
    }
}

fn unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// What may fold with what: the pane, the bar a candle mark claims, and —
/// only while one side is hidden — the side.
///
/// A pane because a tape mark and a candle mark are clipped, sized and
/// switched separately; a bar because a mark in a bar's slot says that bar
/// traded it, the same reason the budget fold keys on it. The side is kept
/// apart only when a side is hidden: the renderer withholds a two-sided mark
/// then (a pie with a hidden half would state a quantity the canvas is not
/// showing), so a pie there would delete the visible side's marks too.
type GroupKey = (bool, Option<usize>, Option<u8>);

impl HeatmapProjection {
    /// Fold every group of bubbles whose discs would overlap into one mark.
    ///
    /// Does nothing unless [`HeatmapConfig::bubble_overlap_merge`] is on, so
    /// off is today's frame exactly. On, the marks come back in the frame's
    /// drawing order and [`folded_aggressions`](Self::folded_aggressions)
    /// counts the marks this removed on top of what the budget folded.
    pub fn merge_overlapping_bubbles(
        &mut self,
        geometry: PaneGeometry,
        timeline: &BarTimeline,
        config: &HeatmapConfig,
    ) {
        if !config.bubble_overlap_merge || self.aggressions.len() < 2 {
            return;
        }
        let both_sides = config.show_buy_aggressions && config.show_sell_aggressions;
        let mut groups: BTreeMap<GroupKey, Vec<AggressionPrimitive>> = BTreeMap::new();
        for mark in std::mem::take(&mut self.aggressions) {
            let bar = if mark.live {
                None
            } else {
                timeline
                    .locate(mark.first_timestamp_ms)
                    .map(|position| position.bar_index)
            };
            let side = (!both_sides).then_some(match mark.side {
                AggressorSide::Buy => 0,
                AggressorSide::Sell => 1,
            });
            groups.entry((mark.live, bar, side)).or_default().push(mark);
        }

        let scale = Scale::new(geometry, timeline.region_count(), config);
        let before: usize = groups.values().map(Vec::len).sum();
        let mut merged = Vec::with_capacity(before);
        for ((live, _, _), members) in groups {
            // The scale each pane's marks were drawn on — a fold is sized
            // against it, never rescaling the marks it left alone.
            let reference = if live {
                self.aggression_reference
            } else {
                self.summary_reference
            };
            merged.extend(fold_touching(members, reference, &scale));
        }
        merged.sort_by(frame_order);
        self.folded_aggressions += before - merged.len();
        self.aggressions = merged;
    }
}

/// Everything that turns a normalized mark into its disc on screen.
struct Scale {
    regions: f64,
    geometry: PaneGeometry,
    candle_radii: (f32, f32),
    lane_radii: (f32, f32),
    side_offset: f64,
}

impl Scale {
    fn new(geometry: PaneGeometry, regions: usize, config: &HeatmapConfig) -> Self {
        let bubbles = &config.bubbles;
        Self {
            regions: regions as f64,
            geometry,
            candle_radii: (bubbles.min_radius, bubbles.max_radius),
            lane_radii: config.live_lane.scaled_radii(bubbles),
            side_offset: f64::from(bubbles.side_offset),
        }
    }

    /// Where the painter will put this mark and how big it will draw it —
    /// the painter's own rules, restated without the offsets every disc
    /// shares: the same radius function, and the same lean off the price
    /// row by buy share.
    fn disc(&self, mark: &AggressionPrimitive) -> Disc {
        let (px_per_region, (minimum, maximum)) = if mark.live {
            (f64::from(self.geometry.lane_width_px), self.lane_radii)
        } else {
            (f64::from(self.geometry.px_per_bar), self.candle_radii)
        };
        let lean = (unit(f64::from(mark.buy_share)) - 0.5) * 2.0;
        Disc {
            x: unit(mark.x) * self.regions * px_per_region,
            y: unit(mark.y) * f64::from(self.geometry.height_px) + lean * self.side_offset,
            radius: f64::from(bubble_radius(mark.size, minimum, maximum)),
        }
    }
}

/// Fold one group until no two of its discs touch, or the pass cap is spent.
fn fold_touching(
    mut marks: Vec<AggressionPrimitive>,
    reference: Decimal,
    scale: &Scale,
) -> Vec<AggressionPrimitive> {
    for _ in 0..MAX_OVERLAP_PASSES {
        if marks.len() < 2 {
            break;
        }
        // A canonical order first, so which marks land in which fold never
        // depends on the order they arrived in.
        marks.sort_by(frame_order);
        let Some(mut roots) = touching_components(&marks, scale) else {
            break;
        };
        let mut components: BTreeMap<usize, Vec<AggressionPrimitive>> = BTreeMap::new();
        for (index, mark) in marks.into_iter().enumerate() {
            let root = find(&mut roots, index);
            components.entry(root).or_default().push(mark);
        }
        marks = components
            .into_values()
            .map(|members| {
                if members.len() == 1 {
                    members.into_iter().next().expect("one member")
                } else {
                    fold_chunk(members, reference, absorb)
                }
            })
            .collect();
    }
    marks
}

/// Union every pair of touching discs; `None` when no two touch.
///
/// Swept along x, so a pair is only measured when its centres are closer than
/// the widest reach in the group — a dense tape is sorted once and compared
/// against its neighbours, not against every other mark.
fn touching_components(marks: &[AggressionPrimitive], scale: &Scale) -> Option<Vec<usize>> {
    let discs: Vec<Disc> = marks.iter().map(|mark| scale.disc(mark)).collect();
    let widest = discs.iter().map(|disc| disc.radius).fold(0.0, f64::max);
    let mut by_x: Vec<usize> = (0..discs.len()).collect();
    by_x.sort_by(|&a, &b| discs[a].x.total_cmp(&discs[b].x).then(a.cmp(&b)));
    let mut roots: Vec<usize> = (0..discs.len()).collect();
    let mut any = false;
    for (position, &a) in by_x.iter().enumerate() {
        for &b in &by_x[position + 1..] {
            if discs[b].x - discs[a].x >= discs[a].radius + widest {
                break;
            }
            if discs[a].touches(discs[b]) {
                let (root_a, root_b) = (find(&mut roots, a), find(&mut roots, b));
                // The lower index wins, so the root of a component is a
                // property of the canonical order, not of the sweep.
                roots[root_a.max(root_b)] = root_a.min(root_b);
                any = true;
            }
        }
    }
    any.then_some(roots)
}

fn find(roots: &mut [usize], mut index: usize) -> usize {
    while roots[index] != index {
        roots[index] = roots[roots[index]];
        index = roots[index];
    }
    index
}
