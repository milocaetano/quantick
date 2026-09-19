//! The registration port is exercised without a chart or a legacy enum variant.
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration, BarRegistry};
use quantick_engine::bar_selection::{
    BarInputAvailability, BarSelection, SelectionCommand, SelectionEffect,
};
use quantick_engine::{BarSpec, fixture, golden};
#[path = "support/seventh_bar.rs"]
mod seventh_bar;

const TAPE: &str = include_str!("fixtures/tick_trades.csv");
const EXPECTED: &str = include_str!("fixtures/tick_n3_expected.csv");

#[test]
fn subfloor_parameter_commands_compare_the_effective_configuration() {
    for id in ["volume", "dollar"] {
        let config = BUILTIN_BARS.find(id).unwrap().default_config();
        let mut selection = BarSelection::new(config);
        let command = SelectionCommand::Parameter {
            name: config.definition().parameter.name,
            value: rust_decimal::Decimal::new(1, 9),
        };
        assert_eq!(
            selection
                .update(command, BarInputAvailability::PRINTS)
                .unwrap(),
            SelectionEffect::Changed
        );
        let effective = selection.spec();
        assert_eq!(effective.parameter(), quantick_engine::DECIMAL_PARAM_FLOOR);
        assert_eq!(
            selection
                .update(command, BarInputAvailability::PRINTS)
                .unwrap(),
            SelectionEffect::Unchanged
        );
        assert_eq!(selection.settle(effective), SelectionEffect::Unchanged);
        let before = selection.clone();
        assert!(
            selection
                .update(
                    SelectionCommand::Parameter {
                        name: config.definition().parameter.name,
                        value: rust_decimal::Decimal::ZERO,
                    },
                    BarInputAvailability::PRINTS
                )
                .is_err()
        );
        assert_eq!(selection, before, "normalization cannot hide invalid input");
    }
}

#[test]
fn subfloor_replace_commands_compare_the_effective_configuration() {
    for id in ["volume", "dollar"] {
        let raw = BUILTIN_BARS.parse(&format!("{id}:0.000000001")).unwrap();
        assert_eq!(
            raw.parameter(),
            rust_decimal::Decimal::new(1, 9),
            "strict parser acceptance and value remain unchanged"
        );
        let mut selection = BarSelection::default();
        let command = SelectionCommand::Replace(raw);
        assert_eq!(
            selection
                .update(command, BarInputAvailability::PRINTS)
                .unwrap(),
            SelectionEffect::Changed
        );
        let effective = selection.spec();
        assert_eq!(effective, raw.clamped());
        assert_eq!(
            selection
                .update(command, BarInputAvailability::PRINTS)
                .unwrap(),
            SelectionEffect::Unchanged
        );
        assert_eq!(selection.settle(effective), SelectionEffect::Unchanged);
        assert_eq!(selection.pending(), None);
    }
}

#[test]
fn registry_factory_cuts_the_committed_tape() {
    let config = BUILTIN_BARS.parse("tick:3").unwrap();
    let mut builder = config.build();
    let bars: Vec<_> = fixture::parse_trades(TAPE)
        .unwrap()
        .iter()
        .filter_map(|trade| builder.push(trade))
        .collect();
    assert_eq!(
        golden::diff_bars(&fixture::parse_bars(EXPECTED).unwrap(), &bars),
        None
    );
}

#[test]
fn retained_parameters_and_deferred_rebuild_belong_to_the_headless_owner() {
    let mut selection = BarSelection::new(BarSpec::Tick(377));
    let inputs = BarInputAvailability::PRINTS;
    selection
        .update(SelectionCommand::Select("volume"), inputs)
        .unwrap();
    selection
        .update(
            SelectionCommand::Parameter {
                name: "quantity",
                value: rust_decimal::Decimal::new(125, 1),
            },
            inputs,
        )
        .unwrap();
    assert_eq!(
        selection.spec(),
        BarSpec::Volume(rust_decimal::Decimal::new(125, 1))
    );
    selection
        .update(SelectionCommand::Select("tick"), inputs)
        .unwrap();
    assert_eq!(selection.spec(), BarSpec::Tick(377));
    assert_eq!(
        selection.retained("volume"),
        &BarSpec::Volume(rust_decimal::Decimal::new(125, 1))
    );
    let applied = BarConfiguration::from(BarSpec::Tick(50));
    assert_eq!(selection.settle(applied), SelectionEffect::Pending);
    assert_eq!(
        selection.settle(applied),
        SelectionEffect::Rebuild(selection.spec())
    );
    assert_eq!(
        selection.settle(selection.spec()),
        SelectionEffect::Unchanged
    );
}

#[test]
fn unavailable_and_unknown_requests_leave_selection_unchanged() {
    let mut selection = BarSelection::default();
    let original = selection.clone();
    assert!(
        selection
            .update(
                SelectionCommand::Select("trades"),
                BarInputAvailability::PRINTS
            )
            .is_err()
    );
    assert!(
        selection
            .update(
                SelectionCommand::Select("missing"),
                BarInputAvailability::PRINTS
            )
            .is_err()
    );
    assert_eq!(selection, original);
    assert!(BUILTIN_BARS.parse("tick:0").is_err());
    assert!(BUILTIN_BARS.parse("volume:-1").is_err());
    assert!(BUILTIN_BARS.parse("time:99ms").is_err());
    assert!(BUILTIN_BARS.parse("imbalance:unknown:10").is_err());
}

#[test]
fn every_family_member_exposes_the_same_parameter_contract() {
    let expected = [
        ("tick:50", "count", "trades", false, false),
        ("volume:5", "quantity", "base_asset_quantity", true, false),
        (
            "dollar:500000",
            "notional",
            "quote_asset_notional",
            true,
            false,
        ),
        ("time:1m", "interval_ms", "milliseconds", false, false),
        ("imbalance:100", "target", "target_trades", false, false),
        ("trades:2000", "count", "deals", false, true),
    ];
    for (definition, (text, name, unit, volume, deals)) in
        BUILTIN_BARS.definitions().iter().zip(expected)
    {
        let config = definition.default_config();
        assert_eq!(config.to_config_string(), text);
        assert_eq!(config.definition().parameter.name, name);
        assert_eq!(config.definition().parameter.unit, unit);
        assert_eq!(config.requirements().traded_volume, volume);
        assert_eq!(config.requirements().deal_counter, deals);
        assert_eq!(BUILTIN_BARS.parse(text).unwrap(), config);
        assert!(config.with_parameter("unknown", 3.into()).is_err());
    }
    for unit in ["volume", "dollar"] {
        let config = BUILTIN_BARS
            .parse(&format!("imbalance:{unit}:100"))
            .unwrap();
        assert!(config.requirements().traded_volume);
        assert_eq!(config.choice(), Some(unit));
    }
}

#[test]
fn duration_parameter_does_not_imply_fixed_time_partition() {
    use quantick_engine::bar_registry::{BarDefinition, definitions::TIME};
    static WINDOW: BarDefinition = BarDefinition {
        id: "window",
        fixed_time_interval: false,
        ..TIME
    };
    let registry = BarRegistry::new([&WINDOW]).unwrap();
    let config = registry.parse("window:1m").unwrap();
    assert_eq!(config.parameter().to_string(), "60000");
    assert_eq!(config.time_interval_ms(), None);
    assert_eq!(
        BUILTIN_BARS.parse("time:1m").unwrap().time_interval_ms(),
        Some(60_000)
    );
}

#[test]
fn legacy_projection_refuses_a_different_definition_reusing_a_legacy_name() {
    use quantick_engine::bar_registry::{BarDefinition, definitions::VOLUME};
    static OTHER: BarDefinition = BarDefinition {
        id: "tick",
        ..VOLUME
    };
    let config = BarRegistry::new([&OTHER])
        .unwrap()
        .parse("tick:0.1")
        .unwrap();
    assert!(BarSpec::try_from(config).is_err());
}

#[test]
fn legacy_restore_keeps_the_exact_positive_floors() {
    use quantick_engine::{DECIMAL_PARAM_FLOOR, ImbalanceUnit};
    use rust_decimal::Decimal;
    for (initial, expected) in [
        (BarSpec::Tick(0), BarSpec::Tick(1)),
        (BarSpec::Trades(0), BarSpec::Trades(1)),
        (BarSpec::Time(0), BarSpec::Time(1)),
        (
            BarSpec::Imbalance(ImbalanceUnit::Trades, 0),
            BarSpec::Imbalance(ImbalanceUnit::Trades, 1),
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
        assert_eq!(BarSelection::new(initial).spec(), expected);
    }
}

#[test]
fn availability_distinguishes_a_missing_counter_from_a_missing_reading() {
    use quantick_engine::bar_selection::BarUnavailable;
    let selection = BarSelection::default();
    let quoted = BarInputAvailability {
        traded_volume: false,
        deal_counter: false,
        deal_count: false,
    };
    assert_eq!(
        selection.availability("trades", quoted).unwrap(),
        Some(BarUnavailable::DealCounter)
    );
    assert_eq!(
        selection
            .availability(
                "trades",
                BarInputAvailability {
                    deal_counter: true,
                    ..quoted
                }
            )
            .unwrap(),
        Some(BarUnavailable::DealCount)
    );
    assert_eq!(
        selection
            .availability(
                "trades",
                BarInputAvailability {
                    deal_count: true,
                    ..quoted
                }
            )
            .unwrap(),
        None,
        "a loaded day works before hello"
    );
    assert_eq!(
        selection.availability("volume", quoted).unwrap(),
        Some(BarUnavailable::TradedVolume)
    );
    assert_eq!(selection.availability("tick", quoted).unwrap(), None);
}

#[test]
fn retained_imbalance_units_cannot_bypass_volume_availability() {
    let mut selection = BarSelection::new(BarSpec::parse("imbalance:volume:100").unwrap());
    selection
        .update(
            SelectionCommand::Select("tick"),
            BarInputAvailability::PRINTS,
        )
        .unwrap();
    let before = selection.clone();
    let quoted = BarInputAvailability {
        traded_volume: false,
        ..BarInputAvailability::PRINTS
    };
    assert!(
        selection
            .update(SelectionCommand::Select("imbalance"), quoted)
            .is_err()
    );
    assert_eq!(selection, before);
}

#[test]
fn changing_or_reverting_a_pending_selection_never_rebuilds_the_stale_rule() {
    let applied = BarSpec::Tick(50).into();
    let mut selection = BarSelection::new(BarSpec::Tick(100));
    assert_eq!(selection.settle(applied), SelectionEffect::Pending);
    selection
        .update(
            SelectionCommand::Parameter {
                name: "count",
                value: 200.into(),
            },
            BarInputAvailability::PRINTS,
        )
        .unwrap();
    assert_eq!(selection.settle(applied), SelectionEffect::Pending);
    assert_eq!(
        selection.settle(applied),
        SelectionEffect::Rebuild(BarSpec::Tick(200).into())
    );
    selection.set(BarSpec::Tick(100));
    assert_eq!(selection.settle(applied), SelectionEffect::Pending);
    selection.set(applied);
    assert_eq!(selection.settle(applied), SelectionEffect::Unchanged);
    assert_eq!(selection.pending(), None);
}

#[test]
fn a_seventh_definition_uses_the_production_selection_projection_and_factory() {
    // One definition and one registration; no consumer knows its identity.
    let registry = BarRegistry::new(
        BUILTIN_BARS
            .definitions()
            .iter()
            .copied()
            .chain([&seventh_bar::SEVENTH]),
    )
    .unwrap();
    let config = registry.parse("probe:3").unwrap();
    let mut selection = BarSelection::with_registry(registry, config).unwrap();
    selection
        .update(
            SelectionCommand::Select("tick"),
            BarInputAvailability::PRINTS,
        )
        .unwrap();
    selection
        .update(
            SelectionCommand::Select("probe"),
            BarInputAvailability::PRINTS,
        )
        .unwrap();
    assert_eq!(selection.spec().definition().id, "probe");
    assert_eq!(selection.spec().parameter().to_string(), "3");
    assert_eq!(selection.spec().to_config_string(), "probe:3");
    assert!(
        BarSpec::try_from(selection.spec()).is_err(),
        "the legacy enum refuses a new identity honestly"
    );
    let mut builder = selection.spec().build();
    let bars: Vec<_> = fixture::parse_trades(TAPE)
        .unwrap()
        .iter()
        .filter_map(|trade| builder.push(trade))
        .collect();
    assert_eq!(
        golden::diff_bars(&fixture::parse_bars(EXPECTED).unwrap(), &bars),
        None
    );
}
