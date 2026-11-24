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
    /// The first market millisecond it holds for.
    pub from_ms: i64,
    /// The first market millisecond it no longer holds for: a print outside
    /// the window gets no change, never one against another session's price.
    pub until_ms: i64,
}

impl DayReference {
    /// The daily open of the UTC day starting at `day_start_ms`.
    #[must_use]
    pub fn daily_open(day_start_ms: i64, price: Decimal) -> Self {
        Self {
            price,
            kind: DayReferenceKind::DailyOpen,
            from_ms: day_start_ms,
            until_ms: day_start_ms.saturating_add(UTC_DAY_MS),
        }
    }

    /// The previous session's settlement, holding for prints from `from_ms`
    /// until `until_ms`: the span the venue vouches it for.
    #[must_use]
    pub fn previous_settlement(price: Decimal, from_ms: i64, until_ms: i64) -> Self {
        Self {
            price,
            kind: DayReferenceKind::PreviousSettlement,
            from_ms,
            until_ms,
        }
    }

    /// Whether the reference holds for a print at `at_ms`.
    #[must_use]
    pub fn covers(&self, at_ms: i64) -> bool {
        (self.from_ms..self.until_ms).contains(&at_ms)
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

/// What a feed has said about its market's price: the venue's day
/// reference and decimals, and the most decimals its live prints have shown.
/// A tab holds one per feed session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VenueQuote {
    /// The newest day reference; `None` until the venue sends one, or after
    /// it withdrew it.
    pub day: Option<DayReference>,
    /// The decimals the venue says it quotes the price in.
    pub decimals: Option<u32>,
    /// The most decimals a live print has shown, trailing zeros aside: what
    /// a price is written in when the venue never said. Only ever grows.
    pub tape_decimals: u32,
}

impl VenueQuote {
    /// Fold one live print's price into [`Self::tape_decimals`].
    pub fn observe_price(&mut self, price: Decimal) {
        self.tape_decimals = self.tape_decimals_with(price);
    }

    /// [`Self::tape_decimals`] had `price` been folded too. One modulo on
    /// the settled path: a price padded past the decimals already seen
    /// (Binance's `76200.01000000`) normalizes only when the padding hides a
    /// new digit.
    #[must_use]
    pub fn tape_decimals_with(&self, price: Decimal) -> u32 {
        let (scale, seen) = (price.scale(), self.tape_decimals);
        let unit = 10_i128.checked_pow(scale.saturating_sub(seen));
        if scale > seen && unit.is_some_and(|unit| price.mantissa() % unit != 0) {
            price.normalize().scale()
        } else {
            seen
        }
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
    fn a_settlement_holds_only_for_the_span_the_venue_vouches_it_for() {
        let settled = DayReference::previous_settlement(dec("173170"), DAY, DAY + 10);
        assert_eq!(settled.kind, DayReferenceKind::PreviousSettlement);
        assert!(settled.covers(DAY) && settled.covers(DAY + 9));
        assert!(!settled.covers(DAY - 1), "a print before the reading");
        assert!(!settled.covers(DAY + 10), "the next session's print");
    }

    #[test]
    fn a_quote_keeps_the_most_decimals_its_prints_have_shown() {
        let mut quote = VenueQuote::default();
        quote.observe_price(dec("76200.01000000"));
        assert_eq!(quote.tape_decimals, 2, "padding is no decimal");
        for price in ["76200.00000000", "76200.10000000", "76200"] {
            quote.observe_price(dec(price));
        }
        assert_eq!(quote.tape_decimals, 2, "never shrinks");
        assert_eq!(quote.tape_decimals_with(dec("76200.125")), 3, "not folded");
        assert_eq!(quote.tape_decimals, 2);
        quote.observe_price(dec("76200.125"));
        assert_eq!(quote.tape_decimals, 3);
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
        let zero = DayReference::previous_settlement(dec("0"), 0, DAY + 1);
        assert_eq!(
            zero.change_percent(dec("1"), DAY),
            None,
            "never divided by zero"
        );
    }
}
