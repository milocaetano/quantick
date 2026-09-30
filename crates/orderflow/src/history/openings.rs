//! The first recorded native tape window per UTC date, independent of retention.

use crate::native_tape::NATIVE_TAPE_WINDOW_MS;

const UTC_DAY_MS: i64 = 86_400_000;
/// Seven days of supported history plus the current date. WIN trades during
/// the UTC date of its B3 daytime session; this is not an exchange auction flag.
const RECORDED_DATES: i64 = 8;

#[derive(Debug, Clone, Default)]
pub struct RecordedOpenings(Vec<i64>);

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
