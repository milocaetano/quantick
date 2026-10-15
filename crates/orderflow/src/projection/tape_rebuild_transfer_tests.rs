//! A navigation handoff retains what the pane actually painted.

use super::*;
use crate::projection::{
    AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry, TapeDotView,
};
use crate::{BubbleStyle, LiveLaneStyle};
use quantick_engine::Side;
use quantick_orderbook::BookSide;
use rust_decimal::Decimal;

#[derive(Default)]
struct Runner {
    ready: Option<(u64, TapeDotMemory)>,
}

impl TapeRebuildRunner for Runner {
    fn start(&mut self, _: u64, rebuild: TapeRebuild) -> Result<(), Box<TapeRebuild>> {
        Err(Box::new(rebuild))
    }

    fn finished(&mut self) -> Result<Option<(u64, TapeDotMemory)>, RunnerStopped> {
        Ok(self.ready.take())
    }
}

#[test]
fn navigation_takes_the_painted_stand_in_and_rejects_its_late_rebuild() {
    let mark = AggressionPrimitive {
        agg_id: 42,
        agg_ids: vec![42],
        generation: None,
        side: Side::Buy,
        consumed_side: BookSide::Ask,
        quantity: 4.into(),
        buy_share: 1.0,
        live: true,
        price_bucket: 100.into(),
        price_span: Decimal::ONE,
        price: 100.into(),
        trade_count: 1,
        first_timestamp_ms: 1_000,
        last_timestamp_ms: 1_000,
        timestamp_quantity: 4_000.into(),
        matched_quantity: Decimal::ZERO,
        buy_quantity: 4.into(),
        matched_fraction: 0.0,
        liquidity_event_ids: Vec::new(),
        x: 0.0,
        y: 0.0,
        size: 0.0,
        folded_marks: 0,
    };
    let view = TapeDotView {
        now_ms: 5_000,
        window_ms: 30_000,
        dot_window_ms: 100,
        evicted_through_ms: None,
        prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 300.0,
            height_px: 500.0,
        },
    };
    let sizing = DotSizing {
        native_tape: true,
        tape_column_px: 1.0,
        candle_column_px: 1.0,
        px_per_price: 1.0,
        typed_full: Some(100.into()),
    };
    let bubbles = BubbleStyle::default();
    let lane = LiveLaneStyle {
        stable_size: true,
        ..Default::default()
    };
    let mut painted = TapeDotMemory::default();
    let expected = painted.project(&[mark], view, sizing, &bubbles, &lane, &[]);
    assert_eq!(painted.retained_group_count(), 1);
    let late = painted.clone();
    let abandoned = Arc::new(AtomicBool::new(false));
    let mut rebuilds = TapeRebuilds::new(Runner::default());
    rebuilds.generation = 7;
    rebuilds.running = Some((7, Arc::clone(&abandoned)));
    rebuilds.stand_in = Some(Arc::new(painted));
    let mut slot = TapeDotMemory::default();
    let taken = rebuilds.take_displayed(&mut slot);
    let actual = taken.draw_retained(&[], view, sizing, &bubbles, &lane, &[]);
    assert_eq!(actual.marks, expected.marks);
    assert_eq!(taken.retained_group_count(), 1);
    assert_eq!(slot.retained_group_count(), 0);
    assert!(abandoned.load(atomic::Ordering::Relaxed));
    assert!(!rebuilds.is_running());
    rebuilds.runner.ready = Some((7, late));
    rebuilds.adopt(&mut slot);
    assert_eq!(
        slot.retained_group_count(),
        0,
        "an old owner cannot write after handoff"
    );
}
