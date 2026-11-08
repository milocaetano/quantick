//! Replay the pre-IR native fixture through the public Pine compiler and runner.
use quantick_indicators::{Ctx, Indicator, IndicatorBar};
use quantick_pine::{ScriptIndicator, compile};
use std::collections::BTreeMap;

pub fn kernel_goldens() -> usize {
    assert!(
        compile(
            "//@version=5\nindicator(\"arity\")\nplot(ta.vwma(close, volume, 4))\n",
            "arity"
        )
        .is_err()
    );
    assert!(
        compile(
            "//@version=5\nindicator(\"named\")\nplot(ta.vwma(length = 4, source = close))\n",
            "named"
        )
        .is_ok()
    );
    let mut states = BTreeMap::new();
    let mut cells = 0;
    for line in include_str!("../golden/ir/native-kernels.csv")
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = line.split(',').collect();
        let length: usize = fields[0].parse().unwrap();
        let index: i32 = fields[1].parse().unwrap();
        let name = fields[2];
        let script = states.entry((length, name)).or_insert_with(|| {
            let call = match name {
                "WindowSum" => format!("math.sum(close, {length})"),
                "TrTrue" => "ta.tr(true)".into(),
                "TrFalse" => "ta.tr(false)".into(),
                "Vwma" => format!("ta.vwma(close, {length})"),
                "Atr" => format!("ta.atr({length})"),
                "Cum" => "ta.cum(close)".into(),
                "BarsSince" => "ta.barssince(volume > 0)".into(),
                "ValueWhen" => format!("ta.valuewhen(volume > 0, close, {})", length - 1),
                "PivotHigh" | "PivotLow" => {
                    format!("ta.{}(close, {length}, 1)", name.to_lowercase())
                }
                "Cross" | "CrossOver" | "CrossUnder" => {
                    format!("ta.{}(close, volume) ? 1 : 0", name.to_lowercase())
                }
                _ => format!("ta.{}(close, {length})", name.to_lowercase()),
            };
            let source = format!("//@version=5\nindicator(\"native\")\nplot({call})\n");
            ScriptIndicator::new(
                compile(&source, name).unwrap_or_else(|e| panic!("{name}: {e:?}")),
                &source,
            )
        });
        let x = if matches!(index, 3 | 11 | 37) {
            f64::NAN
        } else {
            f64::from((index * 7) % 19 - 9) / 3.0
        };
        let b = if index % 7 == 0 {
            0.0
        } else {
            f64::from(index % 5 - 2)
        };
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: x,
            high: x + 1.0,
            low: x - 1.0,
            close: x,
            buy_volume: b,
            sell_volume: 0.0,
            trade_count: 0.0,
        };
        let mut ctx = Ctx {
            bar_index: index as usize,
            cvd: &[],
        };
        let first = script
            .preview(&bar, &mut ctx)
            .unwrap_or_else(|e| panic!("{line}: {e:?}"))
            .values[0];
        let second = script
            .preview(&bar, &mut ctx)
            .unwrap_or_else(|e| panic!("{line}: {e:?}"))
            .values[0];
        script.on_close(&bar, &mut ctx).unwrap();
        let committed = script.plots().column(script.descriptor().plots[0].id)[index as usize];
        for actual in [first, second, committed] {
            if fields[3] == "na" {
                assert!(actual.is_nan(), "{line}: {actual}");
            } else {
                assert_eq!(
                    actual.to_bits(),
                    u64::from_str_radix(fields[3], 16).unwrap(),
                    "{line}: {actual}"
                );
            }
        }
        cells += 1;
    }
    cells
}
