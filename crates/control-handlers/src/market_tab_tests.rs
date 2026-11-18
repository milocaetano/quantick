use super::*;
use crate::test_support::fake_config;

/// One futures feed offering two contracts, rolling the first onto the
/// second and the second onto one it does not offer.
fn config() -> AppConfig {
    fake_config(&[(
        "b3",
        &["WINZ26", "WING27"],
        &[("WINZ26", "WING27"), ("WING27", "WINJ27")],
    )])
}

#[test]
fn the_config_names_the_next_contract_of_a_configured_feed_only() {
    let config = config();
    assert_eq!(
        next_contract(&config, "b3", "WINZ26").as_deref(),
        Some("WING27")
    );
    assert_eq!(
        next_contract(&config, "b3", "WINJ27"),
        None,
        "no roll is named"
    );
    assert_eq!(
        next_contract(&config, "binance", "WINZ26"),
        None,
        "no such feed"
    );
}

#[test]
fn a_feed_offers_only_the_symbols_its_config_lists() {
    let config = config();
    assert!(feed_offers(&config, "b3", Some("WING27")));
    assert!(
        !feed_offers(&config, "b3", Some("WINJ27")),
        "a named roll is not an offer"
    );
    assert!(
        feed_offers(&config, "b3", None),
        "any symbol of a configured feed"
    );
    assert!(
        !feed_offers(&config, "binance", None),
        "an unconfigured feed offers nothing"
    );
}
