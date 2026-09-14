//! The bar vocabulary — `tick:50`, `time:1m`, `imbalance:volume:500`,
//! `trades:2000` — as the one definition every consumer reads.
//!
//! The chart and the backtest used to carry a copy each. These tests pin the
//! engine's copy: what parses, what is refused, what a spec writes back, what
//! a zero parameter becomes, and — the point of owning it here — that the
//! builder a spec names cuts each hand-computed golden into exactly its
//! committed bars.

use std::str::FromStr as _;

use quantick_engine::{
    Bar, BarKind, BarSpec, BarSpecError, DECIMAL_PARAM_FLOOR, DEFAULT_TIME_INTERVAL_MS,
    ImbalanceUnit, MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS, Side, Trade, fixture,
    fmt_time_interval, golden,
};
use rust_decimal::Decimal;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

/// One committed golden per kind the fixtures cover, named by the spec
/// string a trader would type for it. The app and the backtest hold their
/// own copies of this table and assert against the same files.
const GOLDENS: [(&str, &str, &str); 5] = [
    (
        "tick:3",
        include_str!("fixtures/tick_trades.csv"),
        include_str!("fixtures/tick_n3_expected.csv"),
    ),
    (
        "volume:5.0",
        include_str!("fixtures/volume_trades.csv"),
        include_str!("fixtures/volume_t5_expected.csv"),
    ),
    (
        "dollar:500",
        include_str!("fixtures/dollar_trades.csv"),
        include_str!("fixtures/dollar_t500_expected.csv"),
    ),
    (
        "time:1s",
        include_str!("fixtures/time_trades.csv"),
        include_str!("fixtures/time_i1000_expected.csv"),
    ),
    (
        "imbalance:8",
        include_str!("fixtures/imbalance_trades.csv"),
        include_str!("fixtures/imbalance_t8_expected.csv"),
    ),
];

fn cut(spec: &BarSpec, trades: &[Trade]) -> Vec<Bar> {
    let mut builder = spec.build();
    trades
        .iter()
        .filter_map(|trade| builder.push(trade))
        .collect()
}

#[test]
fn the_builder_a_spec_names_cuts_each_golden_exactly() {
    for (text, trades_csv, expected_csv) in GOLDENS {
        let spec = BarSpec::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        let trades = fixture::parse_trades(trades_csv).expect("trade fixture parses");
        let expected = fixture::parse_bars(expected_csv).expect("expected fixture parses");
        let first = cut(&spec, &trades);
        assert_eq!(
            fixture::write_bars(&first),
            fixture::write_bars(&cut(&spec, &trades)),
            "{text} cut the same trades two different ways"
        );
        if let Some(report) = golden::diff_bars(&expected, &first) {
            panic!("{text}: {report}");
        }
    }
}

/// The one thing a per-kind table takes on trust: that `ALL` is the whole of
/// `BarKind`. The match is exhaustive and unmatchable by a wildcard, so adding
/// a variant breaks this line at compile time; the assertion then catches the
/// variant that was added here but forgotten in `ALL`.
#[test]
fn barkind_all_lists_every_variant() {
    let named = [
        BarKind::Tick,
        BarKind::Volume,
        BarKind::Dollar,
        BarKind::Time,
        BarKind::Imbalance,
        BarKind::Trades,
    ];
    for kind in named {
        match kind {
            BarKind::Tick
            | BarKind::Volume
            | BarKind::Dollar
            | BarKind::Time
            | BarKind::Imbalance
            | BarKind::Trades => {}
        }
        assert!(
            BarKind::ALL.contains(&kind),
            "{kind:?} is a bar kind BarKind::ALL does not list, so no per-kind table has a slot for it"
        );
        assert_eq!(
            kind.default_spec().kind(),
            kind,
            "{kind:?}'s default is another kind"
        );
    }
    assert_eq!(
        BarKind::ALL.len(),
        named.len(),
        "BarKind::ALL and this test disagree about how many kinds there are"
    );
}

/// What a fresh chart opens each kind on. Written as the config strings a
/// workspace saves, because those are what must not move.
#[test]
fn each_kind_opens_on_the_default_it_always_had() {
    let written: Vec<String> = BarKind::ALL
        .iter()
        .map(|kind| kind.default_spec().to_config_string())
        .collect();
    assert_eq!(
        written,
        [
            "tick:50",
            "volume:5",
            "dollar:500000",
            "time:1m",
            "imbalance:100",
            "trades:2000"
        ]
    );
    assert_eq!(DEFAULT_TIME_INTERVAL_MS, 60_000);
}

/// A zero parameter reaches a spec from outside — a workspace file, a config
/// line, a control call — and no kind may pass one on. The counted kinds
/// floor at one; the two measured in `Decimal` floor at
/// [`DECIMAL_PARAM_FLOOR`]. Volume is the one that bites: a bar closing on no
/// quantity closes on every trade.
#[test]
fn a_zero_parameter_is_clamped_to_a_rule_that_closes_on_something() {
    for (zeroed, floored) in [
        (BarSpec::Tick(0), BarSpec::Tick(1)),
        (BarSpec::Trades(0), BarSpec::Trades(1)),
        (BarSpec::Time(0), BarSpec::Time(1)),
        (
            BarSpec::Imbalance(ImbalanceUnit::Volume, 0),
            BarSpec::Imbalance(ImbalanceUnit::Volume, 1),
        ),
        (
            BarSpec::Volume(Decimal::ZERO),
            BarSpec::Volume(DECIMAL_PARAM_FLOOR),
        ),
        (
            BarSpec::Dollar(Decimal::ZERO),
            BarSpec::Dollar(DECIMAL_PARAM_FLOOR),
        ),
    ] {
        assert_eq!(zeroed.clamped(), floored, "{zeroed:?}");
    }
    assert_eq!(BarSpec::Tick(377).clamped(), BarSpec::Tick(377));
}

/// Whatever a chart is showing, a config or a saved workspace can name — and
/// naming it gets that chart back.
#[test]
fn every_bar_spec_survives_the_config_round_trip() {
    for spec in [
        BarSpec::Tick(50),
        BarSpec::Trades(2000),
        BarSpec::Imbalance(ImbalanceUnit::Trades, 100),
        BarSpec::Imbalance(ImbalanceUnit::Volume, 500),
        BarSpec::Imbalance(ImbalanceUnit::Dollar, 2500),
        BarSpec::Volume(dec("5.25")),
        BarSpec::Dollar(dec("500000")),
        BarSpec::Time(60_000),
        BarSpec::Time(3_600_000),
        // A parameter with no round unit: the suffix ladder has to fall
        // through to bare milliseconds rather than rounding it away.
        BarSpec::Time(1_500),
    ] {
        let text = spec.to_config_string();
        assert_eq!(
            BarSpec::parse(&text),
            Ok(spec),
            "'{text}' did not come back as the spec that wrote it"
        );
    }
}

/// `default_bars`, a saved workspace and the backtest's `--bars` speak one
/// vocabulary — every kind, every interval suffix, padding tolerated.
#[test]
fn bar_specs_parse_in_the_config_vocabulary() {
    assert_eq!(BarSpec::parse("tick:50"), Ok(BarSpec::Tick(50)));
    assert_eq!(BarSpec::parse("trades:2000"), Ok(BarSpec::Trades(2000)));
    assert_eq!(
        BarSpec::parse("imbalance:100"),
        Ok(BarSpec::Imbalance(ImbalanceUnit::Trades, 100)),
        "the pre-units short form still names tick imbalance bars"
    );
    assert_eq!(
        BarSpec::parse("imbalance:trades:100"),
        Ok(BarSpec::Imbalance(ImbalanceUnit::Trades, 100))
    );
    assert_eq!(
        BarSpec::parse("imbalance:volume:500"),
        Ok(BarSpec::Imbalance(ImbalanceUnit::Volume, 500))
    );
    assert_eq!(
        BarSpec::parse("imbalance:dollar:2500"),
        Ok(BarSpec::Imbalance(ImbalanceUnit::Dollar, 2500))
    );
    assert_eq!(BarSpec::parse("volume:5"), Ok(BarSpec::Volume(dec("5"))));
    assert_eq!(
        BarSpec::parse("volume:0.5"),
        Ok(BarSpec::Volume(dec("0.5")))
    );
    assert_eq!(
        BarSpec::parse("dollar:500000"),
        Ok(BarSpec::Dollar(dec("500000")))
    );
    assert_eq!(BarSpec::parse("time:1m"), Ok(BarSpec::Time(60_000)));
    assert_eq!(BarSpec::parse("time:90s"), Ok(BarSpec::Time(90_000)));
    assert_eq!(BarSpec::parse("time:1h"), Ok(BarSpec::Time(3_600_000)));
    assert_eq!(BarSpec::parse("time:1500ms"), Ok(BarSpec::Time(1_500)));
    assert_eq!(BarSpec::parse("time:60000"), Ok(BarSpec::Time(60_000)));
    assert_eq!(BarSpec::parse(" time : 5m "), Ok(BarSpec::Time(300_000)));
}

/// Whatever the status bar can say, a config can ask for: the interval
/// formatter and the parser are inverses over the whole domain shape.
#[test]
fn time_interval_labels_round_trip_through_the_parser() {
    for ms in [
        MIN_TIME_INTERVAL_MS,
        1_500,
        60_000,
        300_000,
        3_600_000,
        MAX_TIME_INTERVAL_MS,
    ] {
        let label = fmt_time_interval(ms);
        assert_eq!(
            BarSpec::parse(&format!("time:{label}")),
            Ok(BarSpec::Time(ms)),
            "{label}"
        );
    }
}

/// A spec no live control could produce must not come in through a config or
/// a command line either — its only symptom would be a chart nobody asked for.
#[test]
fn a_spec_no_control_could_produce_does_not_parse() {
    for bad in [
        "",
        "tick",
        "tick:",
        "tick:0",
        "tick:-5",
        "trades:0",
        "volume:0",
        "dollar:nope",
        "imbalance:1.5",
        "imbalance:volume:0",
        "imbalance:volume:1.5",
        "imbalance:notional:500",
        "imbalance:volume:",
        "time:0",
        "time:50ms",
        "time:25h",
        "time:1w",
        "grid:1",
        "candles:5",
    ] {
        let error = BarSpec::parse(bad).expect_err(bad);
        assert!(!error.to_string().is_empty(), "{bad} must explain itself");
    }
    // The message is the point: an operator who typed it wrong gets the
    // vocabulary back, not "invalid input".
    assert!(
        BarSpec::parse("candles:5")
            .expect_err("rejected")
            .to_string()
            .contains("tick")
    );
}

/// A caller that is not a person — a control call, a script repairing its own
/// request — reads why a spec was refused from the variant, not by parsing
/// prose; a person still reads the same sentence as before.
#[test]
fn a_refusal_names_its_reason_as_a_variant_and_keeps_its_sentence() {
    let cases: [(&str, BarSpecError, &str); 8] = [
        (
            "tick",
            BarSpecError::NotKindParameter {
                text: "tick".to_owned(),
            },
            "'tick' is not a kind:parameter bar spec, like 'time:1m'",
        ),
        (
            "candles:5",
            BarSpecError::UnknownKind {
                kind: "candles".to_owned(),
            },
            "unknown bar kind 'candles'; one of tick, volume, dollar, time, imbalance, trades",
        ),
        (
            "tick:0",
            BarSpecError::NotPositiveCount {
                kind: BarKind::Tick,
                param: "0".to_owned(),
            },
            "tick bars need a positive whole number, got '0'",
        ),
        (
            "dollar:nope",
            BarSpecError::NotPositiveNumber {
                kind: BarKind::Dollar,
                param: "nope".to_owned(),
            },
            "dollar bars need a positive number, got 'nope'",
        ),
        (
            "imbalance:notional:500",
            BarSpecError::UnknownImbalanceUnit {
                unit: "notional".to_owned(),
            },
            "unknown imbalance unit 'notional'; one of trades, volume, dollar",
        ),
        (
            "imbalance:volume:1.5",
            BarSpecError::NotPositiveTarget {
                target: "1.5".to_owned(),
            },
            "imbalance bars need a positive whole trade target, got '1.5'",
        ),
        (
            "time:1w",
            BarSpecError::NotAnInterval {
                text: "1w".to_owned(),
            },
            "'1w' is not a time interval, like '1m' or '30s'",
        ),
        (
            "time:50ms",
            BarSpecError::IntervalOutOfRange {
                ms: 50,
                param: "50ms".to_owned(),
            },
            "time interval '50ms' is outside 100ms..=1d — the domain both time-bar controls accept",
        ),
    ];
    for (text, variant, sentence) in cases {
        let refusal = BarSpec::parse(text).expect_err(text);
        assert_eq!(refusal, variant, "{text}");
        assert_eq!(refusal.to_string(), sentence, "{text}");
    }
}

#[test]
fn only_the_size_measuring_rules_need_a_traded_volume() {
    // Volume and dollar bars measure size, so a venue that prints none can
    // only fake them.
    assert!(BarKind::Volume.needs_traded_volume());
    assert!(BarKind::Dollar.needs_traded_volume());
    // The others count events, not quantity — imbalance answering for its
    // default trades unit, which sums a signed ±1 per trade.
    assert!(!BarKind::Tick.needs_traded_volume());
    assert!(!BarKind::Time.needs_traded_volume());
    assert!(!BarKind::Imbalance.needs_traded_volume());
    assert!(!BarKind::Trades.needs_traded_volume());
}

#[test]
fn only_the_deal_count_rule_needs_a_deal_counter() {
    for kind in BarKind::ALL {
        assert_eq!(
            kind.needs_deal_counter(),
            kind == BarKind::Trades,
            "{kind:?}"
        );
    }
}

/// One vocabulary for every surface that names a timeframe: the summary speaks
/// the chips' own labels, falling back to finer units only where no coarser
/// one writes the value back exactly.
#[test]
fn time_summaries_speak_the_chips_language() {
    assert_eq!(fmt_time_interval(60_000), "1m");
    assert_eq!(fmt_time_interval(300_000), "5m");
    assert_eq!(fmt_time_interval(900_000), "15m");
    assert_eq!(fmt_time_interval(3_600_000), "1h");
    assert_eq!(
        fmt_time_interval(90_000),
        "90s",
        "90s is not a round minute"
    );
    assert_eq!(fmt_time_interval(1_000), "1s");
    assert_eq!(fmt_time_interval(1_500), "1500ms");
    assert_eq!(BarSpec::Time(60_000).summary(), "time(1m)");
    assert_eq!(BarSpec::Time(500).summary(), "time(500ms)");
    assert_eq!(BarSpec::Tick(100).summary(), "tick(100)");
    assert_eq!(BarSpec::Trades(2000).summary(), "trades(2000)");
    assert_eq!(
        BarSpec::Imbalance(ImbalanceUnit::Volume, 500).summary(),
        "imbalance(volume 500)"
    );
}

#[test]
fn build_dispatches_every_kind() {
    let trade = Trade {
        agg_id: 1,
        timestamp_ms: 1_100,
        price: dec("100"),
        quantity: dec("1.0"),
        side: Side::Buy,
    };
    // Tick(1) and Imbalance(1) close on the first trade; the others simply
    // must build and accept a trade without panicking.
    for spec in [
        BarSpec::Tick(1),
        BarSpec::Volume(dec("1.0")),
        BarSpec::Dollar(dec("100")),
        BarSpec::Time(1),
        BarSpec::Imbalance(ImbalanceUnit::Trades, 1),
        BarSpec::Imbalance(ImbalanceUnit::Volume, 1),
        BarSpec::Imbalance(ImbalanceUnit::Dollar, 1),
        BarSpec::Trades(1),
    ] {
        let kind = spec.kind();
        let mut builder = spec.build();
        let closed = builder.push(&trade);
        if matches!(kind, BarKind::Tick | BarKind::Imbalance) {
            assert!(closed.is_some(), "{kind:?}(1) closes immediately");
        }
    }
}
