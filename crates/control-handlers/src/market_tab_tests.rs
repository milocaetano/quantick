use super::*;
use serde_json::json;

/// A config of one futures feed offering two contracts, rolling the first
/// onto the second and the second onto one it does not offer.
fn config() -> AppConfig {
    serde_json::from_value(json!({
        "default_feed": "b3",
        "default_symbol": "WINZ26",
        "feeds": [{
            "id": "b3",
            "name": "B3",
            "provider": "metatrader",
            "symbols": ["WINZ26", "WING27"],
            "contract_rolls": { "WINZ26": "WING27", "WING27": "WINJ27" },
        }],
    }))
    .expect("a valid config")
}

#[test]
fn the_config_names_the_next_contract_of_a_listed_feed_only() {
    let config = config();
    assert_eq!(
        config.next_contract("b3", "WINZ26").as_deref(),
        Some("WING27")
    );
    assert_eq!(
        config.next_contract("b3", "WINJ27"),
        None,
        "no roll is named"
    );
    assert_eq!(
        config.next_contract("binance", "WINZ26"),
        None,
        "no such feed"
    );
}

#[test]
fn a_feed_offers_only_the_symbols_its_config_lists() {
    let config = config();
    assert!(config.feed_offers("b3", "WING27"));
    assert!(
        !config.feed_offers("b3", "WINJ27"),
        "a named roll is not an offer"
    );
    assert!(!config.feed_offers("binance", "WINZ26"));
}

#[test]
fn the_choices_are_the_configured_feeds_with_their_rolls() {
    let choices = config().source_choices();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].feed_id, "b3");
    assert_eq!(choices[0].symbols, ["WINZ26", "WING27"]);
    assert_eq!(choices[0].contract_rolls["WINZ26"], "WING27");
}
