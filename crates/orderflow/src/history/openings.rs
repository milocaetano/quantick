//! The first recorded native tape window per UTC date, independent of retention.

use crate::constants::{NATIVE_TAPE_WINDOW_MS, RECORDED_DATES};

/// Milliseconds in one UTC day: a calendar fact, not a tunable.
const UTC_DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Default)]
pub struct RecordedOpenings(Vec<i64>);

/// Canonical first execution per UTC date, scoped to the source epoch by its owner.
/// Unlike the opening window, this nominates at most one regional FLOW group.
#[derive(Debug, Clone, Default)]
pub struct RecordedOpeningAnchors(Vec<(i64, usize)>);

impl RecordedOpeningAnchors {
    pub fn observe(&mut self, timestamp_ms: i64, ordinal: usize) {
        let day = timestamp_ms.div_euclid(UTC_DAY_MS);
        let candidate = (timestamp_ms, ordinal);
        match self
            .0
            .binary_search_by_key(&day, |(time, _)| time.div_euclid(UTC_DAY_MS))
        {
            Ok(index) => self.0[index] = self.0[index].min(candidate),
            Err(index) => self.0.insert(index, candidate),
        }
        let newest = self.0.last().unwrap().0.div_euclid(UTC_DAY_MS);
        self.0
            .retain(|(time, _)| time.div_euclid(UTC_DAY_MS) > newest - RECORDED_DATES);
    }

    pub fn ordinals(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().map(|(_, ordinal)| *ordinal)
    }
}

impl RecordedOpenings {
    /// The aligned native 100 ms window used for opening classification.
    #[must_use]
    pub fn window_start(timestamp_ms: i64) -> i64 {
        timestamp_ms
            .div_euclid(NATIVE_TAPE_WINDOW_MS)
            .saturating_mul(NATIVE_TAPE_WINDOW_MS)
    }

    pub fn observe(&mut self, timestamp_ms: i64) {
        let window = Self::window_start(timestamp_ms);
        let day = window.div_euclid(UTC_DAY_MS);
        match self
            .0
            .binary_search_by_key(&day, |time| time.div_euclid(UTC_DAY_MS))
        {
            Ok(index) => self.0[index] = self.0[index].min(window),
            Err(index) => self.0.insert(index, window),
        }
        let newest = self
            .0
            .last()
            .copied()
            .unwrap_or(window)
            .div_euclid(UTC_DAY_MS);
        self.0
            .retain(|time| time.div_euclid(UTC_DAY_MS) > newest - RECORDED_DATES);
    }

    /// Whether a print lies in its UTC date's first recorded 100 ms window.
    #[must_use]
    pub fn contains(&self, timestamp_ms: i64) -> bool {
        let window = Self::window_start(timestamp_ms);
        self.0.binary_search(&window).is_ok()
    }

    pub(crate) fn merge(&mut self, windows: &[i64]) {
        for &window in windows {
            self.observe(window);
        }
    }

    pub fn windows(&self) -> &[i64] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_uses_earliest_timestamp_then_source_order_and_retains_only_supported_dates() {
        let mut anchors = RecordedOpeningAnchors::default();
        anchors.observe(1070, 0);
        anchors.observe(1010, 8);
        anchors.observe(1010, 3);
        anchors.observe(1011, 1);
        assert_eq!(anchors.ordinals().collect::<Vec<_>>(), [3]);
        for day in 1..=8 {
            anchors.observe(day * UTC_DAY_MS + 1000, day as usize + 10);
        }
        assert_eq!(
            anchors.ordinals().collect::<Vec<_>>(),
            (11..=18).collect::<Vec<_>>()
        );
        anchors.observe(500, 30);
        assert_eq!(anchors.ordinals().count(), 8);
    }
}
