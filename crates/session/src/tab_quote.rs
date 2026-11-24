//! What a market tab says about its market besides its name: the asset's
//! icon, the last price in the instrument's decimals and the day's change
//! against the reference its venue supplied. No reference, no change: the
//! tab never derives one from the tape.

use std::cmp::Ordering;

use quantick_engine::Trade;
use quantick_sources::day_reference::DayReference;
use rust_decimal::Decimal;

/// The decimals a day's change is written in.
pub const CHANGE_DECIMALS: u32 = 2;

/// A tab's asset icon: a glyph on a disc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabIcon {
    pub glyph: String,
    /// The disc's colour from the feed config; `None` is the neutral disc.
    pub rgb: Option<[u8; 3]>,
}

impl TabIcon {
    /// The configured glyph and colour, else the symbol's first letter on a
    /// neutral disc.
    #[must_use]
    pub fn of(symbol: &str, configured: Option<(&str, [u8; 3])>) -> Self {
        todo!("{symbol} {configured:?}")
    }
}

/// The day's change as the tab writes it, and its direction for the ink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayChange {
    /// `+1.12%`, `−0.38%`, `0.00%`.
    pub text: String,
    pub sign: Ordering,
}

/// The price and the day's change a tab shows; either may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabQuote {
    pub price: Option<String>,
    pub change: Option<DayChange>,
}

/// The quote for a tab whose newest print is `last`, against the venue's
/// `reference`, priced in the decimals of the instrument's `tick`.
#[must_use]
pub fn quote(
    last: Option<&Trade>,
    reference: Option<&DayReference>,
    tick: Option<Decimal>,
) -> TabQuote {
    todo!("{last:?} {reference:?} {tick:?}")
}

/// `price` in the decimals of `tick`, without a thousands separator; with no
/// tick known, as the venue printed it, trailing zeros dropped.
#[must_use]
pub fn format_price(price: Decimal, tick: Option<Decimal>) -> String {
    todo!("{price} {tick:?}")
}

/// A change in percent, signed, in [`CHANGE_DECIMALS`].
#[must_use]
pub fn format_change(percent: Decimal) -> DayChange {
    todo!("{percent}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use quantick_sources::day_reference::UTC_DAY_MS;

    const DAY: i64 = 20_400 * UTC_DAY_MS;

    fn dec(text: &str) -> Decimal {
        text.parse().expect("a decimal")
    }

    fn print(id: u64, at_ms: i64, price: &str) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: at_ms,
            price: dec(price),
            quantity: dec("0.01"),
            side: Side::Buy,
        }
    }

    /// A fixture tape: the day's change reads its newest print against the
    /// venue's daily open.
    #[test]
    fn the_change_reads_the_newest_print_against_the_venue_reference() {
        let tape = [
            print(1, DAY + 1_000, "76490.00"),
            print(2, DAY + 2_000, "76350.10"),
            print(3, DAY + 3_000, "76200.00"),
        ];
        let open = DayReference::daily_open(DAY, dec("76490"));
        let quote = quote(tape.last(), Some(&open), Some(dec("0.50")));
        assert_eq!(quote.price.as_deref(), Some("76200.00"));
        let change = quote.change.expect("a reference");
        assert_eq!(change.text, "\u{2212}0.38%");
        assert_eq!(change.sign, Ordering::Less);
        let settled = DayReference::previous_settlement(dec("171750"));
        let up = super::quote(Some(&print(4, DAY, "172455")), Some(&settled), None);
        assert_eq!(
            up.change.map(|c| (c.text, c.sign)),
            Some(("+0.41%".into(), Ordering::Greater))
        );
    }

    #[test]
    fn without_a_reference_the_tab_shows_its_price_and_no_change() {
        let last = print(1, DAY, "3241.5");
        let quote = quote(Some(&last), None, None);
        assert_eq!(quote.price.as_deref(), Some("3241.5"));
        assert_eq!(quote.change, None, "never computed from the tape");
        let yesterday = DayReference::daily_open(DAY - UTC_DAY_MS, dec("3200"));
        assert_eq!(
            super::quote(Some(&last), Some(&yesterday), None).change,
            None
        );
        assert_eq!(
            super::quote(None, Some(&yesterday), None),
            TabQuote::default()
        );
    }

    #[test]
    fn a_price_takes_the_ticks_decimals_and_no_thousands_separator() {
        assert_eq!(format_price(dec("76200"), Some(dec("0.50"))), "76200.00");
        assert_eq!(
            format_price(dec("76200.01000000"), Some(dec("0.01"))),
            "76200.01"
        );
        assert_eq!(format_price(dec("172455"), Some(dec("5"))), "172455");
        assert_eq!(format_price(dec("5412.5"), Some(dec("0.5"))), "5412.5");
        assert_eq!(format_price(dec("0.61420000"), None), "0.6142");
        assert_eq!(format_price(dec("1234567.5"), None), "1234567.5");
    }

    #[test]
    fn a_change_is_signed_in_two_decimals() {
        assert_eq!(format_change(dec("1.1234")).text, "+1.12%");
        assert_eq!(format_change(dec("-2.005")).text, "\u{2212}2.01%");
        let flat = format_change(dec("-0.001"));
        assert_eq!((flat.text.as_str(), flat.sign), ("0.00%", Ordering::Equal));
    }

    #[test]
    fn an_asset_without_an_icon_wears_its_first_letter_on_a_neutral_disc() {
        let configured = TabIcon::of("BTCUSDT", Some(("B", [0xF7, 0x93, 0x1A])));
        assert_eq!(configured.glyph, "B");
        assert_eq!(configured.rgb, Some([0xF7, 0x93, 0x1A]));
        let bare = TabIcon::of("wdoz26", None);
        assert_eq!((bare.glyph.as_str(), bare.rgb), ("W", None));
        assert_eq!(TabIcon::of("", None).glyph, "?");
    }
}
