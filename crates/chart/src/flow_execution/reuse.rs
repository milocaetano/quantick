//! What a FLOW drawing keeps from the one before it. A region's triangles
//! follow from its key and the pane's look alone, so under one look a run of
//! regions whose keys held paints what it painted. The paint order is cut
//! into runs by the keys themselves: a new print changes the runs holding
//! the regions it changed, and every other run is kept whole.
use super::{FlowPresentation, FlowRegionRole};
use quantick_orderflow::projection::flow_tape::FlowTapeFrame;
use std::ops::Range;

/// How far ahead in the previous paint order a region is looked for. A new
/// print grows, adds or moves a few regions; every other region keeps its
/// place in the order, a few steps from where the walk expects it.
const REUSE_LOOKAHEAD: usize = 16;

/// A run ends after a region whose key hashes to a multiple of this: runs of
/// about this many regions, cut where the content says, never by position.
const RUN_SPREAD: u64 = 48;

/// The most regions one run holds, whatever the hashes say.
const RUN_MAX: usize = 192;

/// What a region's triangles depend on beyond the pane's look: its role, its
/// displayed circle, its sides and, for a regional peak, the emphasis that
/// picks its colours. Exact bits, so equal keys paint equal triangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowRegionKey {
    role: FlowRegionRole,
    center: [u32; 2],
    radius: u32,
    shares: [u64; 2],
    emphasis: u32,
}

impl FlowRegionKey {
    /// Whether the region paints in the pass after the footprint.
    fn foreground(&self) -> bool {
        self.role == FlowRegionRole::Peak
    }

    /// Whether a run ends at this region: a hash of the key alone, so a
    /// region ends a run in every frame it holds in.
    fn ends_run(&self) -> bool {
        let words = [
            u64::from(self.center[0]),
            u64::from(self.center[1]),
            u64::from(self.radius),
            self.shares[0],
            self.shares[1],
            u64::from(self.emphasis),
            self.role as u64,
        ];
        let hash = words.iter().fold(0x9E37_79B9_7F4A_7C15_u64, |hash, word| {
            (hash ^ word)
                .wrapping_mul(0xFF51_AFD7_ED55_8CCD)
                .rotate_left(29)
        });
        hash.is_multiple_of(RUN_SPREAD)
    }
}

impl FlowPresentation {
    /// Each region's key, in paint order. `tick` says whether the look is
    /// tick spheres, whose colours never follow a region's emphasis.
    #[must_use]
    pub fn region_keys(&self, frame: &FlowTapeFrame, tick: bool) -> Vec<FlowRegionKey> {
        self.regions
            .iter()
            .map(|region| {
                let dot = &frame.dots[region.dot_index];
                let (buy, sell) = dot.side_shares();
                let emphasis = if tick || region.role != FlowRegionRole::Peak {
                    0.0
                } else {
                    frame.ordinary_region_opacity(dot)
                };
                FlowRegionKey {
                    role: region.role,
                    center: region.disc.center.map(f32::to_bits),
                    radius: region.disc.radius.to_bits(),
                    shares: [buy.to_bits(), sell.to_bits()],
                    emphasis: emphasis.to_bits(),
                }
            })
            .collect()
    }
}

/// For each key of `current`, the index of an equal key in `previous`: both
/// paint orders walked together, a key found where the walk expects it or at
/// most [`REUSE_LOOKAHEAD`] places past it. Every match is an equal key, so a
/// region the walk misses is only built again, never drawn wrong.
fn reusable_regions(previous: &[FlowRegionKey], current: &[FlowRegionKey]) -> Vec<Option<usize>> {
    let mut next = 0;
    current
        .iter()
        .map(|key| {
            let ahead = previous
                .get(next..)?
                .iter()
                .take(REUSE_LOOKAHEAD)
                .position(|held| held == key)?;
            next += ahead + 1;
            Some(next - 1)
        })
        .collect()
}

/// The paint order cut into runs a drawing builds whole and a later one
/// keeps whole: a run ends where a region's own key says so, at [`RUN_MAX`]
/// regions, or where the pass changes, so no run spans both passes.
fn paint_runs(keys: &[FlowRegionKey]) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    for (index, key) in keys.iter().enumerate() {
        let next = keys.get(index + 1);
        if key.ends_run()
            || index + 1 - start == RUN_MAX
            || next.is_none_or(|next| next.foreground() != key.foreground())
        {
            runs.push(start..index + 1);
            start = index + 1;
        }
    }
    runs
}

/// For each run of `current`, the index of the run of `previous` holding
/// the same keys in the same order, found from where the walk of
/// [`reusable_regions`] places its first region.
fn reusable_runs(
    (previous, previous_runs): (&[FlowRegionKey], &[Range<usize>]),
    (current, current_runs): (&[FlowRegionKey], &[Range<usize>]),
) -> Vec<Option<usize>> {
    let held = reusable_regions(previous, current);
    current_runs
        .iter()
        .map(|run| {
            let first = held.get(run.start).copied().flatten()?;
            let index = previous_runs.partition_point(|old| old.start < first);
            let old = previous_runs.get(index).filter(|old| old.start == first)?;
            (previous[old.clone()] == current[run.clone()]).then_some(index)
        })
        .collect()
}

/// The runs of one paint order and what each was built into: the keys and
/// cuts the next order finds its runs by, and each run's pass (0 the
/// footprint, 1 the peaks after it) and payload in paint order. The three are
/// one value, so no consumer pairs a payload with its run by position.
#[derive(Clone, Debug)]
pub struct FlowRunCache<T> {
    keys: Vec<FlowRegionKey>,
    cuts: Vec<Range<usize>>,
    runs: Vec<(usize, T)>,
}

impl<T> Default for FlowRunCache<T> {
    fn default() -> Self {
        let (keys, cuts, runs) = (Vec::new(), Vec::new(), Vec::new());
        Self { keys, cuts, runs }
    }
}

impl<T: Clone> FlowRunCache<T> {
    /// The runs of `keys` in paint order: a run whose keys held since this
    /// cache keeps its payload, every other is built from its regions' range.
    #[must_use]
    pub fn rebuild(
        &self,
        keys: Vec<FlowRegionKey>,
        mut build: impl FnMut(Range<usize>) -> T,
    ) -> Self {
        let cuts = paint_runs(&keys);
        let held = reusable_runs((&self.keys, &self.cuts), (&keys, &cuts));
        let runs = cuts.iter().zip(held).map(|(cut, held)| {
            let pass = usize::from(keys[cut.start].foreground());
            match held {
                Some(held) => (pass, self.runs[held].1.clone()),
                None => (pass, build(cut.clone())),
            }
        });
        let runs = runs.collect();
        Self { keys, cuts, runs }
    }
}

impl<T> FlowRunCache<T> {
    /// Each run's pass and payload, in paint order.
    #[must_use]
    pub fn runs(&self) -> &[(usize, T)] {
        &self.runs
    }

    /// Each run's payload in paint order, to change in place.
    pub fn payloads_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.runs.iter_mut().map(|(_, payload)| payload)
    }
}

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
