//! A typed number read as every registered bar at that size — ProfitChart's
//! quick period switch, driven by the registry rather than a fixed list.
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration, quick_query_text};

fn configs(number: u64) -> Vec<String> {
    BUILTIN_BARS
        .quick_candidates(number)
        .into_iter()
        .map(BarConfiguration::to_config_string)
        .collect()
}

fn matches(query: &str) -> Vec<String> {
    BUILTIN_BARS
        .quick_matches(query)
        .into_iter()
        .map(BarConfiguration::to_config_string)
        .collect()
}

#[test]
fn fifteen_lists_every_registered_kind_with_durations_first() {
    assert_eq!(
        configs(15),
        [
            "time:15m",
            "time:15s",
            "time:15h",
            "tick:15",
            "volume:15",
            "dollar:15",
            "imbalance:15",
            "imbalance:volume:15",
            "imbalance:dollar:15",
            "trades:15",
            "renko:15",
        ]
    );
}

#[test]
fn fifty_r_lists_renko_alone_by_its_own_name() {
    for query in ["50R", "50r"] {
        let listed = BUILTIN_BARS.quick_matches(query);
        assert_eq!(
            listed
                .iter()
                .map(|config| config.to_config_string())
                .collect::<Vec<_>>(),
            ["renko:50"],
            "{query}"
        );
        assert_eq!(listed[0].quick_label(), "50 Ticks (Renko)");
    }
}

#[test]
fn a_bare_number_offers_renko_among_its_other_readings() {
    assert_eq!(matches("50"), configs(50));
    assert!(configs(50).contains(&"renko:50".to_owned()));
}

#[test]
fn a_letter_no_kind_declares_or_no_number_lists_nothing() {
    for query in ["50X", "50RR", "R", "", "5R0"] {
        assert!(matches(query).is_empty(), "{query}");
    }
}

#[test]
fn one_tick_is_no_renko_brick() {
    assert!(matches("1R").is_empty());
    assert!(!configs(1).iter().any(|spec| spec.starts_with("renko")));
    let refusal = BUILTIN_BARS.parse("renko:1").unwrap_err();
    assert_eq!(refusal.to_string(), "renko bars need at least 2 ticks");
}

#[test]
fn typed_text_keeps_its_digits_and_the_last_letter() {
    for (typed, kept) in [
        ("50", "50"),
        ("50R", "50R"),
        ("5R0", "50R"),
        ("50RT", "50T"),
        ("5 0-R", "50R"),
        ("1234567890R", "123456789R"),
    ] {
        assert_eq!(quick_query_text(typed), kept, "{typed}");
    }
}

#[test]
fn durations_outside_the_interval_range_are_left_out() {
    // 700 hours passes four weeks, the longest fixed interval.
    let seven_hundred = configs(700);
    assert!(seven_hundred.contains(&"time:700m".to_owned()));
    assert!(!seven_hundred.iter().any(|spec| spec == "time:700h"));
    let big = configs(10_000_000);
    assert!(!big.iter().any(|spec| spec.starts_with("time:")));
    assert!(big.contains(&"tick:10000000".to_owned()));
}

#[test]
fn zero_offers_nothing() {
    assert!(BUILTIN_BARS.quick_candidates(0).is_empty());
}

#[test]
fn every_candidate_round_trips_through_the_parser() {
    for number in [1, 5, 15, 60, 500] {
        for config in BUILTIN_BARS.quick_candidates(number) {
            assert_eq!(
                BUILTIN_BARS.parse(&config.to_config_string()).unwrap(),
                config
            );
        }
    }
}

#[test]
fn a_unit_letter_lists_time_bars_in_that_unit() {
    for (query, listed) in [
        ("1w", "time:1w"),
        ("1mo", "time:1mo"),
        ("3MO", "time:3mo"),
        ("5m", "time:5m"),
        ("5M", "time:5m"),
        ("30s", "time:30s"),
        ("4h", "time:4h"),
    ] {
        assert_eq!(matches(query), [listed], "{query}");
    }
}

#[test]
fn a_shared_letter_lists_every_kind_declaring_it_durations_first() {
    for (query, listed) in [
        ("2000t", &["tick:2000", "trades:2000"][..]),
        ("2000T", &["tick:2000", "trades:2000"][..]),
        ("1d", &["time:1d", "dollar:1"][..]),
        ("2D", &["time:2d", "dollar:2"][..]),
        ("10v", &["volume:10"][..]),
        ("15r", &["renko:15"][..]),
        (
            "20i",
            &["imbalance:20", "imbalance:volume:20", "imbalance:dollar:20"][..],
        ),
    ] {
        assert_eq!(matches(query), listed, "{query}");
    }
}

#[test]
fn a_reading_its_definition_rejects_stays_left_out() {
    assert_eq!(matches("29d"), ["dollar:29"]);
    assert!(matches("0t").is_empty());
}

fn labels(query: &str) -> Vec<String> {
    BUILTIN_BARS
        .quick_matches(query)
        .into_iter()
        .map(BarConfiguration::quick_label)
        .collect()
}

#[test]
fn every_row_reads_as_words() {
    for (query, rows) in [
        ("2000t", &["2000 Ticks", "2000 Trades"][..]),
        ("1t", &["1 Tick", "1 Trade"][..]),
        ("1d", &["1 Day", "1 Dollar"][..]),
        ("29d", &["29 Dollars"][..]),
        ("10v", &["10 Volume"][..]),
        ("15m", &["15 Minutes"][..]),
        ("1m", &["1 Minute"][..]),
        ("15s", &["15 Seconds"][..]),
        ("1s", &["1 Second"][..]),
        ("4h", &["4 Hours"][..]),
        ("1w", &["1 Week"][..]),
        ("1mo", &["1 Month"][..]),
        ("3mo", &["3 Months"][..]),
        ("15r", &["15 Ticks (Renko)"][..]),
        (
            "20i",
            &[
                "20 Imbalance (Trades)",
                "20 Imbalance (Volume)",
                "20 Imbalance (Dollar)",
            ][..],
        ),
    ] {
        assert_eq!(labels(query), rows, "{query}");
    }
}

#[test]
fn a_bare_number_keeps_its_order_with_the_same_row_text() {
    assert_eq!(
        labels("15"),
        [
            "15 Minutes",
            "15 Seconds",
            "15 Hours",
            "15 Ticks",
            "15 Volume",
            "15 Dollars",
            "15 Imbalance (Trades)",
            "15 Imbalance (Volume)",
            "15 Imbalance (Dollar)",
            "15 Trades",
            "15 Ticks (Renko)",
        ]
    );
}

#[test]
fn a_unit_past_the_interval_range_lists_nothing() {
    for query in ["13mo", "5w", "0d"] {
        assert!(matches(query).is_empty(), "{query}");
    }
}

#[test]
fn typed_text_keeps_a_month_suffix_whole() {
    for (typed, kept) in [
        ("1mo", "1mo"),
        ("1MO", "1MO"),
        ("1m", "1m"),
        ("1d", "1d"),
        ("1moR", "1R"),
        ("1om", "1m"),
    ] {
        assert_eq!(quick_query_text(typed), kept, "{typed}");
    }
}
