//! The one rule for a perpetual market's symbol in the app.

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
