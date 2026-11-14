use super::*;

fn bar(volume: f64, close: f64) -> IndicatorBar {
    IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 10.0,
        high: 12.0,
        low: 8.0,
        close,
        buy_volume: volume * 0.6,
        sell_volume: volume * 0.4,
        trade_count: 2.0,
    }
}

fn ctx() -> Ctx<'static> {
    Ctx {
        bar_index: 0,
        cvd: &[],
    }
}

#[test]
fn direction_uses_candle_body_and_zero_volume_is_valid() {
    let mut indicator = Volume::default();
    indicator.on_close(&bar(100.0, 11.0), &mut ctx()).unwrap();
    indicator.on_close(&bar(40.0, 9.0), &mut ctx()).unwrap();
    indicator.on_close(&bar(0.0, 10.0), &mut ctx()).unwrap();
    let growing = indicator.plots().column(PlotId::new(0));
    let falling = indicator.plots().column(PlotId::new(1));
    assert_eq!(growing[0], 100.0);
    assert!(growing[1].is_nan());
    assert_eq!(growing[2], 0.0);
    assert!(falling[0].is_nan());
    assert_eq!(falling[1], 40.0);
    assert!(
        indicator
            .plots()
            .column(PlotId::new(2))
            .iter()
            .all(|v| v.is_nan())
    );
}

#[test]
fn previews_replace_the_forming_volume_and_never_advance_sma() {
    let mut indicator = Volume::new(2, true);
    indicator.on_close(&bar(10.0, 11.0), &mut ctx()).unwrap();
    assert!(indicator.plots().column(PlotId::new(2))[0].is_nan());
    assert_eq!(
        indicator
            .preview(&bar(20.0, 11.0), &mut ctx())
            .unwrap()
            .values[2],
        15.0
    );
    for _ in 0..4 {
        let frame = indicator.preview(&bar(40.0, 9.0), &mut ctx()).unwrap();
        assert_eq!(frame.values[1], 40.0);
        assert_eq!(frame.values[2], 25.0);
    }
    indicator.on_close(&bar(40.0, 9.0), &mut ctx()).unwrap();
    indicator.on_close(&bar(60.0, 11.0), &mut ctx()).unwrap();
    assert_eq!(
        &indicator.plots().column(PlotId::new(2))[1..],
        &[25.0, 50.0]
    );
    indicator.reset();
    assert!(
        indicator
            .preview(&bar(20.0, 11.0), &mut ctx())
            .unwrap()
            .values[2]
            .is_nan()
    );
}

#[test]
fn missing_negative_and_infinite_volume_are_gaps_not_zero() {
    let mut indicator = Volume::new(1, true);
    for volume in [f64::NAN, f64::INFINITY, -1.0] {
        let frame = indicator.preview(&bar(volume, 11.0), &mut ctx()).unwrap();
        assert!(frame.values.iter().all(|value| value.is_nan()));
    }
}

#[test]
fn saved_inputs_rebind_with_bounded_length() {
    let native = crate::native::native("native.volume").unwrap();
    let saved = [InputValue::Bool(true), InputValue::Int(3)];
    assert_eq!(native.build_with(&saved).input_values(), saved);
    assert_eq!(
        native
            .build_with(&[InputValue::Bool(true), InputValue::Int(i64::MAX)])
            .input_values()[1],
        InputValue::Int(MAX_LENGTH as i64)
    );
}
