//! A feed's `sync` table, the limits that pause new entries, and its trade
//! stream's idle timeout.

use super::*;

/// A feed's `sync` limits are read per key over the defaults; a limit
/// under a millisecond and a key nobody reads are refused at load.
#[test]
fn a_feeds_sync_limits_load_over_the_defaults() {
    let config = |sync: &str| {
        format!(
            r#"
                default_feed = "f"
                default_symbol = "AAA"
                [[feeds]]
                id = "f"
                name = "F"
                provider = "metatrader"
                symbols = ["AAA"]
                {sync}
            "#
        )
    };
    let load = |sync: &str| {
        parse(
            &config(sync),
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
    };
    let limits = |max_latency_ms, max_tape_age_ms| quantick_sources::sync::SyncLimits {
        max_latency_ms,
        max_tape_age_ms,
        max_connecting_ms: 0,
    };
    let absent = load("").expect("no sync table");
    assert_eq!(absent.sync_limits("f"), limits(500, 5_000));
    assert_eq!(absent.sync_limits("unknown"), limits(500, 5_000));
    let one = load("sync = { max_latency_ms = 250 }").expect("one key");
    assert_eq!(one.sync_limits("f"), limits(250, 5_000));
    let zero = load("sync = { max_tape_age_ms = 0 }").expect_err("zero");
    assert!(zero.to_string().contains("sync.max_tape_age_ms"), "{zero}");
    assert!(
        load("sync = { max_lag_ms = 9 }").is_err(),
        "a typo is refused"
    );
}

/// A feed's trade-stream idle timeout defaults to 30 s, takes an override,
/// and refuses zero or more than an hour at load.
#[test]
fn a_feeds_idle_timeout_loads_over_the_default() {
    let load = |line: &str| {
        parse(
            &format!(
                r#"
                    default_feed = "f"
                    default_symbol = "BTCUSDT"
                    [[feeds]]
                    id = "f"
                    name = "F"
                    provider = "binance"
                    symbols = ["BTCUSDT"]
                    {line}
                "#
            ),
            ConfigSource::Embedded,
            &AddedSymbols::default(),
        )
    };
    let idle = |config: &AppConfig| config.feed("f").map(FeedConfig::idle_timeout);
    let absent = load("").expect("no idle_timeout_secs");
    assert_eq!(idle(&absent), Some(std::time::Duration::from_secs(30)));
    let set = load("idle_timeout_secs = 90").expect("an override");
    assert_eq!(idle(&set), Some(std::time::Duration::from_secs(90)));
    for refused in ["0", "3601", "18446744073709551615"] {
        let error = load(&format!("idle_timeout_secs = {refused}")).expect_err(refused);
        assert!(error.to_string().contains("idle_timeout_secs"), "{error}");
    }
}
