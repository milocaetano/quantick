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
    let tick = BUILTIN_BARS.parse("tick:50").unwrap();
    assert_eq!(tick.quick_label(), "tick(50)", "a kind without a noun");
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
    // 30 hours passes a day.
    let thirty = configs(30);
    assert!(thirty.contains(&"time:30m".to_owned()));
    assert!(!thirty.iter().any(|spec| spec == "time:30h"));
    let big = configs(100_000);
    assert!(!big.iter().any(|spec| spec.starts_with("time:")));
    assert!(big.contains(&"tick:100000".to_owned()));
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
