//! Exact time-interval admission for closed canonical candles.
//!
//! A node covers a dyadic interval of ordered i64 timestamps. Candles covering
//! its entire interval contribute to `cover`; `all` also includes descendants.
//! Queries visit only two boundary paths plus fully covered subtrees, at most
//! four nodes per timestamp bit, regardless of session length or late data.
//! The append-only owner inserts once per closed candle, never per trade.
use std::ops::Range;

#[derive(Debug, Default)]
pub(super) struct TimeIndex {
    root: Node,
}

#[derive(Debug, Default)]
struct Node {
    cover: Option<(usize, usize)>,
    all: Option<(usize, usize)>,
    children: [Option<Box<Node>>; 2],
}

fn union(a: Option<(usize, usize)>, b: Option<(usize, usize)>) -> Option<(usize, usize)> {
    match (a, b) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
        (a, b) => a.or(b),
    }
}

fn ordered(time: i64) -> u64 {
    (time as u64) ^ (1 << 63)
}

impl TimeIndex {
    pub(super) fn insert(&mut self, low: i64, high: i64, slot: usize) {
        debug_assert!(low <= high);
        self.root
            .insert(0, u64::MAX, ordered(low), ordered(high), slot);
    }

    pub(super) fn slots(&self, from: i64, until: i64) -> Option<Range<usize>> {
        self.query(from, until, &mut 0)
    }

    fn query(&self, from: i64, until: i64, visited: &mut usize) -> Option<Range<usize>> {
        if from >= until {
            return None;
        }
        self.root
            .query(0, u64::MAX, ordered(from), ordered(until - 1), visited)
            .map(|(first, last)| first..last + 1)
    }
}

impl Node {
    fn insert(&mut self, low: u64, high: u64, from: u64, until: u64, slot: usize) {
        self.all = union(self.all, Some((slot, slot)));
        if from <= low && high <= until {
            self.cover = union(self.cover, Some((slot, slot)));
            return;
        }
        let middle = low + (high - low) / 2;
        if from <= middle {
            self.children[0]
                .get_or_insert_with(Default::default)
                .insert(low, middle, from, until, slot);
        }
        if until > middle {
            self.children[1]
                .get_or_insert_with(Default::default)
                .insert(middle + 1, high, from, until, slot);
        }
    }

    fn query(
        &self,
        low: u64,
        high: u64,
        from: u64,
        until: u64,
        visited: &mut usize,
    ) -> Option<(usize, usize)> {
        *visited += 1;
        if from <= low && high <= until {
            return self.all;
        }
        let middle = low + (high - low) / 2;
        let mut found = self.cover;
        if from <= middle
            && let Some(child) = &self.children[0]
        {
            found = union(found, child.query(low, middle, from, until, visited));
        }
        if until > middle
            && let Some(child) = &self.children[1]
        {
            found = union(found, child.query(middle + 1, high, from, until, visited));
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_viewport_work_stays_bounded_after_late_data_in_short_and_long_sessions() {
        for length in [128, 131_072] {
            let mut index = TimeIndex::default();
            index.insert(1000, 1001, 0);
            index.insert(1, 2, 1); // One late candle must never select a linear fallback.
            for slot in 2..length {
                let low = 2000 + slot as i64 * 10;
                index.insert(low, low + 4, slot);
            }
            let from = 2000 + (length - 4) as i64 * 10;
            let mut visited = 0;
            assert_eq!(
                index.query(from, from + 40, &mut visited),
                Some(length - 4..length)
            );
            assert!(
                visited <= 4 * i64::BITS as usize,
                "length={length}, visited={visited}"
            );
            println!("time admission candles={length} visited_nodes={visited}");
        }
    }

    #[test]
    fn overlapping_unordered_and_extreme_intervals_match_exact_full_scan() {
        let intervals = [
            (i64::MIN, i64::MIN),
            (80, 100),
            (-3, 4),
            (1, 2),
            (-100, 150),
            (90, 95),
            (i64::MAX, i64::MAX),
        ];
        let mut index = TimeIndex::default();
        for (slot, &(low, high)) in intervals.iter().enumerate() {
            index.insert(low, high, slot);
        }
        for from in [
            -101,
            -100,
            -4,
            0,
            2,
            5,
            79,
            90,
            100,
            101,
            151,
            i64::MIN,
            i64::MAX - 1,
        ] {
            for width in [0, 1, 5, 100] {
                let until = from.saturating_add(width);
                let matches: Vec<_> = intervals
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, &(low, high))| {
                        (from < until && high >= from && low < until).then_some(slot)
                    })
                    .collect();
                let expected = matches
                    .first()
                    .map(|first| *first..matches.last().unwrap() + 1);
                assert_eq!(
                    index.slots(from, until),
                    expected,
                    "from={from}, until={until}"
                );
            }
        }
    }
}
