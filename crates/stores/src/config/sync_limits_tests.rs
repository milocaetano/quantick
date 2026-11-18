//! A feed's `sync` table: the limits that pause new entries.

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
