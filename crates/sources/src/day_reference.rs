//! The price a venue says a market's day is measured from, so a tab can read
//! its day's change. Only a venue supplies one: a crypto venue's 00:00 UTC
//! daily open, an exchange's previous settlement. Nothing here derives one
//! from the tape — a tab without a reference shows no change at all.

use rust_decimal::Decimal;

/// One UTC day, the span a crypto venue's daily candle covers.
pub const UTC_DAY_MS: i64 = 86_400_000;

/// What the reference price is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayReferenceKind {
    /// The open of the venue's daily candle, which starts at 00:00 UTC.
    DailyOpen,
    /// The exchange's settlement of the previous session.
    PreviousSettlement,
}

/// A venue-supplied day reference and the market time it holds for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayReference {
    pub price: Decimal,
    pub kind: DayReferenceKind,
    /// The first market millisecond it holds for; `None` from a venue that
    /// does not say when its session began.
    pub from_ms: Option<i64>,
    /// The first market millisecond it no longer holds for; `None` while the
    /// venue sends no end, as a settlement read at connect does.
    pub until_ms: Option<i64>,
}

impl DayReference {
    /// The daily open of the UTC day starting at `day_start_ms`.
    #[must_use]
    pub fn daily_open(day_start_ms: i64, price: Decimal) -> Self {
        Self {
            price,
            kind: DayReferenceKind::DailyOpen,
            from_ms: Some(day_start_ms),
            until_ms: Some(day_start_ms.saturating_add(UTC_DAY_MS)),
        }
    }

    /// The previous session's settlement, holding until the venue sends
    /// another.
    #[must_use]
    pub fn previous_settlement(price: Decimal) -> Self {
        Self {
            price,
            kind: DayReferenceKind::PreviousSettlement,
            from_ms: None,
            until_ms: None,
        }
    }

    /// Whether the reference holds for a print at `at_ms`.
    #[must_use]
    pub fn covers(&self, at_ms: i64) -> bool {
        self.from_ms.is_none_or(|from| at_ms >= from)
            && self.until_ms.is_none_or(|until| at_ms < until)
    }

    /// The change from the reference to `last`, printed at `at_ms`, in
    /// percent; `None` when the reference does not hold there or is not a
    /// positive price.
    #[must_use]
    pub fn change_percent(&self, last: Decimal, at_ms: i64) -> Option<Decimal> {
        (self.covers(at_ms) && self.price > Decimal::ZERO)
            .then(|| (last - self.price) / self.price * Decimal::ONE_HUNDRED)
    }
}

/// The start of the UTC day holding `at_ms`.
#[must_use]
pub fn utc_day_start(at_ms: i64) -> i64 {
    at_ms.div_euclid(UTC_DAY_MS) * UTC_DAY_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> Decimal {
        text.parse().expect("a decimal")
    }

    const DAY: i64 = 20_000 * UTC_DAY_MS;

    #[test]
    fn a_day_starts_at_utc_midnight() {
        assert_eq!(utc_day_start(DAY), DAY);
        assert_eq!(utc_day_start(DAY + UTC_DAY_MS - 1), DAY);
        assert_eq!(utc_day_start(DAY + UTC_DAY_MS), DAY + UTC_DAY_MS);
        assert_eq!(utc_day_start(-1), -UTC_DAY_MS, "before 1970 floors too");
    }

    #[test]
    fn a_daily_open_holds_for_its_utc_day_only() {
        let open = DayReference::daily_open(DAY, dec("76490"));
        assert_eq!(open.kind, DayReferenceKind::DailyOpen);
        assert!(!open.covers(DAY - 1));
        assert!(open.covers(DAY));
        assert!(open.covers(DAY + UTC_DAY_MS - 1));
        assert!(
            !open.covers(DAY + UTC_DAY_MS),
            "the next day needs its own open"
        );
    }

    #[test]
    fn a_settlement_holds_until_the_venue_sends_another() {
        let settled = DayReference::previous_settlement(dec("173170"));
        assert_eq!(settled.kind, DayReferenceKind::PreviousSettlement);
        assert!(settled.covers(0) && settled.covers(DAY));
    }

    #[test]
    fn the_change_is_last_against_the_reference_in_percent() {
        let open = DayReference::daily_open(DAY, dec("80000"));
        assert_eq!(
            open.change_percent(dec("80896"), DAY + 5),
            Some(dec("1.12"))
        );
        assert_eq!(
            open.change_percent(dec("79696"), DAY + 5),
            Some(dec("-0.38"))
        );
        assert_eq!(open.change_percent(dec("80000"), DAY + 5), Some(dec("0")));
        assert_eq!(open.change_percent(dec("80896"), DAY + UTC_DAY_MS), None);
        let zero = DayReference::previous_settlement(dec("0"));
        assert_eq!(
            zero.change_percent(dec("1"), DAY),
            None,
            "never divided by zero"
        );
    }
}
