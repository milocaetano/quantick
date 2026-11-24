//! What a market tab says about its market besides its name: the asset's
//! icon, the last price in the instrument's decimals and the day's change
//! against the reference its venue supplied. No reference, no change: the
//! tab never derives one from the tape.
//!
//! The decimals hold still per tab ([`price_decimals`]), so a print that
//! ends in a zero does not shorten the price and shift the strip.

use std::cmp::Ordering;

use quantick_engine::Trade;
use quantick_sources::day_reference::DayReference;
use rust_decimal::{Decimal, RoundingStrategy};

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
        match configured {
            Some((glyph, rgb)) => Self {
                glyph: glyph.to_owned(),
                rgb: Some(rgb),
            },
            None => Self {
                glyph: symbol
                    .chars()
                    .next()
                    .map_or_else(|| "?".to_owned(), |first| first.to_uppercase().collect()),
                rgb: None,
            },
        }
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

impl TabQuote {
    /// What the scene says of the quote on a tab at `stage`: the price, the
    /// day's change and the stage, each named even when missing.
    #[must_use]
    pub fn scene_words(&self, stage: crate::tab_chip::Squeeze) -> String {
        let price = self.price.as_deref().unwrap_or("none");
        let change = self.change.as_ref();
        let change = change.map_or("no venue reference", |change| change.text.as_str());
        format!("price {price}; day {change}; stage {}", stage.name())
    }
}

/// The decimals a tab writes its market's price in, fixed for the tab: the
/// configured `tick`'s, else the `venue`'s own, else the most the tape has
/// printed in so far (`seen`, which never shrinks).
#[must_use]
pub fn price_decimals(tick: Option<Decimal>, venue: Option<u32>, seen: u32) -> u32 {
    let configured = tick.filter(|tick| *tick > Decimal::ZERO);
    configured
        .map(|tick| tick.scale())
        .or(venue)
        .unwrap_or(seen)
}

/// The quote for a tab whose newest print is `last`, against the venue's
/// `reference`, priced in `decimals` ([`price_decimals`]).
#[must_use]
pub fn quote(last: Option<&Trade>, reference: Option<&DayReference>, decimals: u32) -> TabQuote {
    let Some(last) = last else {
        return TabQuote::default();
    };
    let change = reference.and_then(|day| day.change_percent(last.price, last.timestamp_ms));
    TabQuote {
        price: Some(format_price(last.price, decimals)),
        change: change.map(format_change),
    }
}

/// `price` in `decimals`, without a thousands separator.
#[must_use]
pub fn format_price(price: Decimal, decimals: u32) -> String {
    let mut price = price.round_dp_with_strategy(decimals, RoundingStrategy::MidpointAwayFromZero);
    price.rescale(decimals);
    price.to_string()
}

/// A change in percent, signed, in [`CHANGE_DECIMALS`].
#[must_use]
pub fn format_change(percent: Decimal) -> DayChange {
    let rounded =
        percent.round_dp_with_strategy(CHANGE_DECIMALS, RoundingStrategy::MidpointAwayFromZero);
    let sign = rounded.cmp(&Decimal::ZERO);
    let mut size = rounded.abs();
    size.rescale(CHANGE_DECIMALS);
    let lead = match sign {
        Ordering::Greater => "+",
        Ordering::Less => "\u{2212}",
        Ordering::Equal => "",
    };
    DayChange {
        text: format!("{lead}{size}%"),
        sign,
    }
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
        let quote = quote(tape.last(), Some(&open), 2);
        assert_eq!(quote.price.as_deref(), Some("76200.00"));
        let change = quote.change.expect("a reference");
        assert_eq!(change.text, "\u{2212}0.38%");
        assert_eq!(change.sign, Ordering::Less);
        let settled = DayReference::previous_settlement(dec("171750"), DAY, DAY + UTC_DAY_MS);
        let up = super::quote(Some(&print(4, DAY, "172455")), Some(&settled), 0);
        assert_eq!(
            up.change.map(|c| (c.text, c.sign)),
            Some(("+0.41%".into(), Ordering::Greater))
        );
    }

    #[test]
    fn without_a_reference_the_tab_shows_its_price_and_no_change() {
        let last = print(1, DAY, "3241.5");
        let quote = quote(Some(&last), None, 1);
        assert_eq!(quote.price.as_deref(), Some("3241.5"));
        assert_eq!(quote.change, None, "never computed from the tape");
        let yesterday = DayReference::daily_open(DAY - UTC_DAY_MS, dec("3200"));
        assert_eq!(super::quote(Some(&last), Some(&yesterday), 1).change, None);
        assert_eq!(super::quote(None, Some(&yesterday), 1), TabQuote::default());
    }

    /// A settlement read after the session's last print is the next
    /// session's reference, never this one's; the next server day waits for
    /// its own reading.
    #[test]
    fn a_settlement_shows_no_change_for_a_print_outside_its_window() {
        let settled = DayReference::previous_settlement(dec("171750"), DAY + 5_000, DAY + 9_000);
        let at = |id, ms| super::quote(Some(&print(id, ms, "172455")), Some(&settled), 0);
        assert_eq!(at(1, DAY + 4_999).change, None);
        assert!(at(2, DAY + 5_000).change.is_some());
        assert_eq!(at(3, DAY + 9_000).change, None);
    }

    #[test]
    fn a_price_takes_its_decimals_and_no_thousands_separator() {
        assert_eq!(format_price(dec("76200"), 2), "76200.00");
        assert_eq!(format_price(dec("76200.01000000"), 2), "76200.01");
        assert_eq!(format_price(dec("172455"), 0), "172455");
        assert_eq!(format_price(dec("5412.5"), 1), "5412.5");
        assert_eq!(format_price(dec("1234567.5"), 1), "1234567.5");
    }

    /// The configured tick first, then the venue's own decimals, then the
    /// most the tape has shown; each holds still while prints end in zeros.
    #[test]
    fn a_tabs_decimals_come_from_tick_then_venue_then_the_tape() {
        assert_eq!(
            price_decimals(Some(dec("0.50")), Some(1), 3),
            2,
            "configured"
        );
        assert_eq!(price_decimals(Some(dec("5")), Some(2), 3), 0);
        assert_eq!(
            price_decimals(Some(dec("0")), Some(1), 3),
            1,
            "no real tick"
        );
        assert_eq!(
            price_decimals(None, Some(2), 0),
            2,
            "MT5 digits, Binance tick"
        );
        assert_eq!(price_decimals(None, None, 4), 4, "the tape's own");
        let fixed = price_decimals(None, None, 2);
        let prices = ["76200.10", "76200", "76200.25"].map(|p| format_price(dec(p), fixed));
        assert_eq!(prices, ["76200.10", "76200.00", "76200.25"]);
    }

    #[test]
    fn the_scene_names_price_change_and_stage() {
        use crate::tab_chip::Squeeze;
        let none = quote(None, None, 0);
        assert_eq!(
            none.scene_words(Squeeze::Stacked),
            "price none; day no venue reference; stage stacked"
        );
        let quoted = TabQuote {
            price: Some("172455".into()),
            change: Some(format_change(dec("0.41"))),
        };
        assert_eq!(
            quoted.scene_words(Squeeze::OneLine),
            "price 172455; day +0.41%; stage one_line"
        );
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
