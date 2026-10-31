//! Width-bounded presentation of the existing market arrangement.
use std::ops::Range;

/// Keep the selected market visible, with the overflow menu reaching the rest.
#[must_use]
pub fn visible_tabs(count: usize, active: usize, slots: usize) -> Range<usize> {
    let slots = slots.min(count);
    let end = active.saturating_add(1).max(slots).min(count);
    end.saturating_sub(slots)..end
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_overflow_market_is_visible_without_changing_identity() {
        assert_eq!(visible_tabs(10, 9, 2), 8..10);
        assert_eq!(visible_tabs(10, 0, 2), 0..2);
        assert_eq!(visible_tabs(1, 0, 5), 0..1);
        assert_eq!(visible_tabs(0, 0, 0), 0..0);
        assert_eq!(visible_tabs(10, 9, 0), 10..10);
    }
}
