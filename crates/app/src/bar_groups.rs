//! The folded candles a grouped chart draws, kept between frames.
//!
//! Squeezed past the zoom where a bar owns a candle, several bars share one
//! ([`crate::viewport::Viewport::bars_per_slot`]) and the candle drawn for them
//! is their exact fold. Folding is cheap per bar and ruinous per frame: at the
//! zoom-out floor a window holds tens of thousands of bars, and re-folding all
//! of them sixty times a second is the whole frame budget spent restating what
//! did not change.
//!
//! Almost nothing does change. Groups are aligned on absolute bar indices, so
//! group *j* is the same group from frame to frame however far the view pans,
//! and a closed series only ever grows at the end. This cache exploits exactly
//! that: it folds each bar once, appends to the last group or opens a new one,
//! and rebuilds from scratch only when the multiple changes or the series is
//! re-cut under it.
//!
//! Closed bars only. The forming bar changes with every print and belongs to
//! whichever group is last; the renderer folds it in at draw time, which costs
//! one group's worth of work rather than invalidating the cache per trade.
//!
//! Pure: bars in, folded bars out, no egui and no clock.

use quantick_engine::Bar;

use crate::resample::merge_into;

/// The folded candles for one pane, at one grouping multiple.
#[derive(Debug, Default)]
pub struct BarGroups {
    /// Bars per group this was built for. Zero means "nothing built yet".
    per_slot: usize,
    /// The series revision this was built from.
    revision: u64,
    /// How long the venue prefix was. It sits *in front* of the prints, so a
    /// prefix that grew moved every bar's index and with it every group —
    /// invisible to the revision, which belongs to the print series alone.
    prefix_len: usize,
    /// How many closed bars have been folded into [`Self::slots`].
    folded: usize,
    /// One folded bar per group, in order from group zero.
    slots: Vec<Bar>,
}

impl BarGroups {
    /// The folded candles, one per group, from group zero.
    #[must_use]
    pub fn slots(&self) -> &[Bar] {
        &self.slots
    }

    /// Fold whatever is new.
    ///
    /// `prefix` and `closed` are the pane's two runs of closed bars in order
    /// (the venue history in front of the bars cut from prints); `revision`
    /// is the series' own, so a rebuild under the cache is noticed.
    ///
    /// Cheap in the common case: same multiple, same revision, a few bars
    /// appended. Everything else rebuilds, which is one pass over bars the
    /// caller was going to walk anyway.
    pub fn refresh(&mut self, per_slot: usize, revision: u64, prefix: &[Bar], closed: &[Bar]) {
        let per_slot = per_slot.max(1);
        let total = prefix.len() + closed.len();
        // A shortened series is a different series: bars behind `folded` may
        // have changed identity even where the count still fits.
        if per_slot != self.per_slot
            || revision != self.revision
            || prefix.len() != self.prefix_len
            || total < self.folded
        {
            self.per_slot = per_slot;
            self.revision = revision;
            self.prefix_len = prefix.len();
            self.folded = 0;
            self.slots.clear();
            self.slots.reserve(total / per_slot + 1);
        }
        if total == self.folded {
            return;
        }
        for (offset, bar) in prefix
            .iter()
            .chain(closed.iter())
            .enumerate()
            .skip(self.folded)
        {
            let group = offset / per_slot;
            match self.slots.get_mut(group) {
                Some(folded) => merge_into(folded, bar),
                None => self.slots.push(bar.clone()),
            }
        }
        self.folded = total;
    }

    /// Forget everything, so the next refresh rebuilds. For a pane whose
    /// series was replaced wholesale.
    pub fn clear(&mut self) {
        self.per_slot = 0;
        self.revision = 0;
        self.prefix_len = 0;
        self.folded = 0;
        self.slots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn bar(seed: i64) -> Bar {
        Bar {
            open_time: seed * 1_000,
            close_time: seed * 1_000 + 999,
            open: Decimal::from(100 + seed),
            high: Decimal::from(110 + seed),
            low: Decimal::from(90 + seed),
            close: Decimal::from(105 + seed),
            buy_volume: Decimal::from(2),
            sell_volume: Decimal::from(3),
            trade_count: 7,
        }
    }

    /// The fold itself, against `resample::for_each_group` — the renderer's
    /// uncached path and the one the exactness tests are written against.
    fn reference(per_slot: usize, bars: &[Bar]) -> Vec<Bar> {
        let mut out = Vec::new();
        crate::resample::for_each_group(bars.iter(), 0, per_slot, |_, bar| out.push(bar.clone()));
        out
    }

    #[test]
    fn folding_matches_the_uncached_path() {
        let bars: Vec<Bar> = (0..37).map(bar).collect();
        for per_slot in [1_usize, 2, 5, 8, 37, 50] {
            let mut groups = BarGroups::default();
            groups.refresh(per_slot, 1, &[], &bars);
            assert_eq!(groups.slots(), reference(per_slot, &bars), "k={per_slot}");
        }
    }

    /// The point of the cache: appending bars folds only the new ones, and
    /// lands on exactly what a full rebuild would have produced — including
    /// the group that was still filling when the last frame ended.
    #[test]
    fn appending_bars_extends_the_last_group_without_rebuilding() {
        let bars: Vec<Bar> = (0..20).map(bar).collect();
        let mut incremental = BarGroups::default();
        for grown in 1..=bars.len() {
            incremental.refresh(3, 1, &[], &bars[..grown]);
            let fresh = reference(3, &bars[..grown]);
            assert_eq!(incremental.slots(), fresh, "after {grown} bars");
        }
    }

    /// The two runs are one series: a venue prefix in front of the bars cut
    /// from prints, grouped across the seam like any other neighbours.
    #[test]
    fn the_prefix_and_the_prints_group_as_one_series() {
        let all: Vec<Bar> = (0..12).map(bar).collect();
        let mut groups = BarGroups::default();
        groups.refresh(5, 1, &all[..7], &all[7..]);
        assert_eq!(groups.slots(), reference(5, &all));
    }

    /// The regression the prefix length exists for: history arriving in
    /// *front* of the prints shifts every bar's index, so every group is a
    /// different group — and the print series' own revision cannot say so.
    #[test]
    fn a_grown_prefix_regroups_from_the_start() {
        let all: Vec<Bar> = (0..12).map(bar).collect();
        let mut groups = BarGroups::default();
        groups.refresh(4, 1, &all[..4], &all[4..]);
        assert_eq!(groups.slots(), reference(4, &all));

        // Four older bars arrive in front; the same twelve prints now start
        // at index four, and the groups are cut somewhere else entirely.
        let older: Vec<Bar> = (100..104).map(bar).collect();
        let mut grown: Vec<Bar> = older.clone();
        grown.extend(all.iter().cloned());
        groups.refresh(4, 1, &grown[..8], &grown[8..]);
        assert_eq!(groups.slots(), reference(4, &grown));
    }

    /// A new multiple, a re-cut series, or a series that shrank each mean the
    /// folded bars are about something else now.
    #[test]
    fn a_changed_multiple_or_revision_rebuilds() {
        let bars: Vec<Bar> = (0..12).map(bar).collect();
        let mut groups = BarGroups::default();
        groups.refresh(4, 1, &[], &bars);
        assert_eq!(groups.slots().len(), 3);

        groups.refresh(3, 1, &[], &bars);
        assert_eq!(groups.slots(), reference(3, &bars), "new multiple");

        // Same length, different series: the revision is what says so.
        let others: Vec<Bar> = (100..112).map(bar).collect();
        groups.refresh(3, 2, &[], &others);
        assert_eq!(groups.slots(), reference(3, &others), "new revision");

        // And a series that shrank under the same revision.
        groups.refresh(3, 2, &[], &others[..5]);
        assert_eq!(groups.slots(), reference(3, &others[..5]), "shorter");
    }

    #[test]
    fn clearing_forces_the_next_refresh_to_rebuild() {
        let bars: Vec<Bar> = (0..9).map(bar).collect();
        let mut groups = BarGroups::default();
        groups.refresh(3, 1, &[], &bars);
        groups.clear();
        assert!(groups.slots().is_empty());
        groups.refresh(3, 1, &[], &bars);
        assert_eq!(groups.slots(), reference(3, &bars));
    }

    #[test]
    fn an_empty_series_folds_to_nothing() {
        let mut groups = BarGroups::default();
        groups.refresh(4, 1, &[], &[]);
        assert!(groups.slots().is_empty());
    }

    /// What the cache costs, in numbers rather than in argument. Not a CI
    /// check — a wall clock in a shared runner asserts nothing — but the
    /// measurement the zoom-out floor was chosen against, kept runnable:
    /// `cargo test -p quantick-app --bin quantick-app -- --ignored --nocapture
    /// fold_cost`.
    ///
    /// The two numbers that matter are the rebuild (paid once per step of the
    /// grouping ladder, so a couple of dozen times across a whole zoom-out)
    /// and the append (paid every frame, and the reason the cache exists).
    #[test]
    #[ignore = "a measurement, not a check"]
    fn fold_cost() {
        let bars: Vec<Bar> = (0..130_000).map(bar).collect();
        let mut groups = BarGroups::default();

        let started = std::time::Instant::now();
        groups.refresh(400, 1, &[], &bars);
        let rebuild = started.elapsed();

        let mut appended = std::time::Duration::ZERO;
        let mut series = bars.clone();
        for extra in 0..600 {
            series.push(bar(200_000 + extra));
            let started = std::time::Instant::now();
            groups.refresh(400, 1, &[], &series);
            appended += started.elapsed();
        }

        println!(
            "fold cost over {} bars: rebuild {rebuild:?}, append {:?} per bar over 600 frames",
            bars.len(),
            appended / 600,
        );
    }
}
