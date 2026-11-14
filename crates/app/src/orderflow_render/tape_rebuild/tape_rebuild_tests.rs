use super::*;
use quantick_orderflow::config::theme::OrderflowRenderStyle;
use quantick_orderflow::projection::TapeDotGeometry;

#[test]
fn a_rebuild_lands_from_the_thread() {
    let mut thread = RebuildThread::default();
    let style = OrderflowRenderStyle::from_config(&Default::default(), [0, 0, 0, 255]);
    let rebuild = TapeRebuild {
        memory: Default::default(),
        abandoned: Default::default(),
        projection: std::sync::Arc::new(quantick_orderflow::HeatmapProjection::empty(
            true,
            quantick_orderflow::EffectiveGrouping::resolve(
                quantick_orderflow::DisplayGrouping::Native,
                rust_decimal::Decimal::ONE,
                rust_decimal::Decimal::from(100),
            ),
        )),
        bubbles: style.clone(),
        style,
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 100.0,
            height_px: 100.0,
        },
        time: None,
        prices: None,
        overlay: None,
    };
    assert!(thread.start(7, rebuild).is_ok());
    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    let landed = loop {
        if let Some((generation, _)) = thread.finished().expect("the thread runs") {
            break Some(generation);
        }
        if Instant::now() > deadline {
            break None;
        }
        std::thread::yield_now();
    };
    assert_eq!(landed, Some(7));
}
