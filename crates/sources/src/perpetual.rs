//! The one rule for a perpetual market's symbol in the app.
//!
//! A venue that lists a perpetual beside a cash market under the same name
//! (Binance's USD-M `BTCUSDT` beside Spot's) gets an app identity of its own:
//! the venue's symbol with [`SUFFIX`]. The suffix names the market in config,
//! tabs and saved looks; it never reaches the venue.

/// What the app appends to a perpetual's venue symbol.
pub const SUFFIX: &str = ".p";

/// The venue's symbol for `symbol`: upper case, without the suffix in any case.
#[must_use]
pub fn wire_symbol(symbol: &str) -> String {
    let upper = symbol.to_ascii_uppercase();
    let suffix = SUFFIX.to_ascii_uppercase();
    upper.strip_suffix(&suffix).unwrap_or(&upper).to_owned()
}

/// The app's canonical spelling of the perpetual `symbol` names.
#[must_use]
pub fn display_symbol(symbol: &str) -> String {
    format!("{}{SUFFIX}", wire_symbol(symbol))
}

/// Whether `symbol` is spelled the way config must list a perpetual: upper-case
/// letters and digits, then the suffix.
#[must_use]
pub fn is_canonical(symbol: &str) -> bool {
    symbol.strip_suffix(SUFFIX).is_some_and(|wire| {
        !wire.is_empty()
            && wire
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suffix_names_the_market_and_never_reaches_the_venue() {
        assert_eq!(wire_symbol("btcusdt.p"), "BTCUSDT");
        assert_eq!(wire_symbol("BTCUSDT.P"), "BTCUSDT");
        assert_eq!(wire_symbol("BTCUSDT"), "BTCUSDT");
        assert_eq!(display_symbol("BTCUSDT.P"), "BTCUSDT.p");
        assert_eq!(display_symbol("btcusdt"), "BTCUSDT.p");
        assert_eq!(display_symbol(&display_symbol("ETHUSDT")), "ETHUSDT.p");
    }

    #[test]
    fn only_the_canonical_spelling_is_a_configured_perpetual() {
        assert!(is_canonical("BTCUSDT.p"));
        assert!(is_canonical("1000PEPEUSDT.p"));
        for refused in ["BTCUSDT", "btcusdt.p", "BTCUSDT.P", "BTC USDT.p", ".p", ""] {
            assert!(!is_canonical(refused), "{refused}");
        }
        assert!(is_canonical(&display_symbol("btcusdt")));
    }
}
