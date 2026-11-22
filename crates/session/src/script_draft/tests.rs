use super::*;
use quantick_engine::Bar;
use quantick_script_trigger::ScriptPlacement;
use rust_decimal::Decimal;

const SOURCE: &str = include_str!("../../../app/scripts/renko_poc_flip.pine");
fn sample(slot: usize, open: i64, close: i64, poc: Option<i64>) -> HistoricalClose {
    let open = Decimal::from(open);
    let close = Decimal::from(close);
    HistoricalClose {
        close: crate::strategies::LiveClose {
            bar: Bar {
                open,
                high: open.max(close),
                low: open.min(close),
                close,
                open_time: 1,
                close_time: 2,
                sell_volume: Decimal::ONE,
                buy_volume: Decimal::ONE,
                trade_count: 2,
            },
            poc: poc.map(|n| Decimal::new(n, 2)),
            slot,
        },
        region: None,
    }
}
#[test]
fn draft_history_keeps_absolute_slots_time_missing_poc_and_boundaries() {
    let records = [
        (11, 10, Some(1050)),
        (10, 11, Some(1075)),
        (11, 12, Some(1150)),
        (12, 11, Some(1125)),
        (11, 12, Some(1100)),
        (12, 11, None),
        (11, 12, Some(1125)),
        (12, 11, Some(1125)),
        (11, 12, Some(1125)),
    ];
    let history: Vec<_> = records
        .into_iter()
        .enumerate()
        .map(|(i, (o, c, p))| sample(40 + i, o, c, p))
        .collect();
    let input = |history| DraftInput {
        id: "oracle.pine",
        source: SOURCE,
        placement: ScriptPlacement::<u64>::Anywhere,
        history,
    };
    let previewed = preview(input(&history)).unwrap();
    assert_eq!(
        previewed,
        [
            Marker {
                slot: 41,
                close_time: 2,
                side: Side::Buy
            },
            Marker {
                slot: 43,
                close_time: 2,
                side: Side::Sell
            }
        ]
    );
    assert_eq!(preview(input(&history[1..])).unwrap(), previewed[1..]);
    let mut tab = TabStrategies::default();
    let alarms = commit(&mut tab, 7, prepare(input(&history)).unwrap());
    assert_eq!(
        alarms
            .iter()
            .map(|a| (a.side, a.disarm_reason.as_deref()))
            .collect::<Vec<_>>(),
        [(Side::Buy, None), (Side::Sell, None)]
    );
}
#[test]
fn draft_rejects_compiler_admission_and_warmup_faults() {
    let history = [sample(4, 10, 11, None), sample(5, 11, 10, None)];
    let input = |source| DraftInput {
        id: "failure.pine",
        source,
        placement: ScriptPlacement::<u64>::Anywhere,
        history: &history,
    };
    let errors = preview(input("indicator(\"bad\")\nx = missing_symbol")).unwrap_err();
    assert!(errors[0].line.is_some() && errors[0].code.starts_with("PINE_"));
    assert_eq!(
        preview(input(
            "indicator(\"bad\")\nalertcondition(true,title=\"Either side\")"
        ))
        .unwrap_err()[0]
            .code,
        "ALARM_TITLE"
    );
    let fault = "indicator(\"loop\")\nalertcondition(true,title=\"Buy signal\")\nx = 0.0\nif bar_index == 1\n    while x >= 0\n        x := x + 1\nplot(x)";
    let errors = prepare(input(fault)).err().unwrap();
    assert_eq!(errors[0].code, "PINE_LOOP_BUDGET");
    assert_eq!(errors, preview(input(fault)).unwrap_err());
}
