//! C21: supplied timestamps, fixed offsets, and preview/commit stability.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, calendar::CalendarTimezone, compile};

fn bar(time: i64) -> IndicatorBar {
    IndicatorBar {
        open_time: time,
        close_time: time,
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    }
}
fn load(source: &str) -> ScriptIndicator {
    ScriptIndicator::new(
        compile(source, "calendar.pine").unwrap_or_else(|e| panic!("{e:?}")),
        source,
    )
}
#[test]
fn calendar_from_bar_time() {
    let source = include_str!("fixtures/calendar/fields.pine");
    let fixture = include_str!("fixtures/calendar/timestamps.csv");
    let golden = include_str!("golden/calendar/fields.csv");
    for (input, expected) in fixture.lines().skip(1).zip(golden.lines().skip(1)) {
        let (time, zone) = input.split_once(',').unwrap();
        let expected: Vec<f64> = expected.split(',').map(|v| v.parse().unwrap()).collect();
        let mut script = load(source).with_timezone(zone.parse().unwrap());
        let mut ctx = Ctx {
            bar_index: 0,
            cvd: &[0.0],
        };
        let b = bar(time.parse().unwrap());
        let before = format!("{:?}", script.plots());
        let first = script.preview(&b, &mut ctx).unwrap();
        let second = script.preview(&b, &mut ctx).unwrap();
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
        assert_eq!(before, format!("{:?}", script.plots()));
        script.on_close(&b, &mut ctx).unwrap();
        for (i, value) in expected.iter().enumerate() {
            assert_eq!(script.plots().column(PlotId::new(i)), &[*value], "{input}");
            assert_eq!(first.values[i], *value, "preview {input}");
        }
    }
}
#[test]
fn calendar_function_offsets_and_rebind() {
    let source = "//@version=5\nindicator(\"t\")\nplot(year)\nplot(year(time))\nplot(year(time, \"UTC+01:00\"))\nplot(dayofweek(0))\n";
    let script = load(source).with_timezone("UTC-03:00".parse().unwrap());
    let mut script = script.rebind(&[]).unwrap();
    script
        .on_close(
            &bar(1_609_459_200_000),
            &mut Ctx {
                bar_index: 0,
                cvd: &[0.0],
            },
        )
        .unwrap();
    for (i, v) in [2020.0, 2020.0, 2021.0, 4.0].iter().enumerate() {
        assert_eq!(script.plots().column(PlotId::new(i)), &[*v]);
    }
    script.reset();
    script
        .on_close(
            &bar(1_609_459_200_000),
            &mut Ctx {
                bar_index: 0,
                cvd: &[0.0],
            },
        )
        .unwrap();
    assert_eq!(script.plots().column(PlotId::new(0)), &[2020.0]);
}
#[test]
fn calendar_rejects_unsupported_zones_and_bad_timestamps() {
    for zone in [
        "America/New_York",
        "GMT",
        "UTC+24:00",
        "UTC-01:60",
        "UTC+1:00",
        "UTC+00:0x",
        " UTC",
        "",
    ] {
        assert!(zone.parse::<CalendarTimezone>().is_err(), "{zone}");
    }
    for expression in [
        "year(0, \"America/New_York\")",
        "year(0.5)",
        "year(9007199254740992)",
        "year(true)",
        "year(0, 1)",
        "year()",
        "year(0, \"UTC\", 1)",
    ] {
        let source = format!("//@version=5\nindicator(\"t\")\nplot({expression})\n");
        if matches!(
            expression,
            "year(0.5)" | "year(true)" | "year(0, 1)" | "year()" | "year(0, \"UTC\", 1)"
        ) {
            let Err(errors) = compile(&source, "calendar.pine") else {
                panic!("{expression} must fail at load");
            };
            assert!(
                errors
                    .iter()
                    .any(|e| e.code == quantick_pine::ErrorCode::PineType)
            );
            continue;
        }
        let mut script = load(&source);
        let result = script.on_close(
            &bar(0),
            &mut Ctx {
                bar_index: 0,
                cvd: &[0.0],
            },
        );
        assert!(result.is_err(), "{expression}");
        assert!(script.plots().column(PlotId::new(0)).is_empty());
    }
}
#[test]
fn calendar_na_and_extreme_bar_timestamps() {
    let mut script =
        load("//@version=5\nindicator(\"t\")\nplot(year(na))\nplot(minute)\nplot(month)\n");
    for (i, t) in [i64::MIN, i64::MAX].into_iter().enumerate() {
        script
            .on_close(
                &bar(t),
                &mut Ctx {
                    bar_index: i,
                    cvd: &[0.0, 0.0],
                },
            )
            .unwrap();
        assert!(script.plots().column(PlotId::new(0))[i].is_nan());
        assert!((0.0..60.0).contains(&script.plots().column(PlotId::new(1))[i]));
        assert!((1.0..=12.0).contains(&script.plots().column(PlotId::new(2))[i]));
    }
}
