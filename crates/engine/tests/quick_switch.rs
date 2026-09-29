//! A typed number read as every registered bar at that size — ProfitChart's
//! quick period switch, driven by the registry rather than a fixed list.
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration};

fn configs(number: u64) -> Vec<String> {
    BUILTIN_BARS
        .quick_candidates(number)
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
        ]
    );
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
