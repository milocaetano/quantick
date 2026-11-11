//! Zoom regions: whole candles chosen by the zoom bound a merge, so a
//! compressed tick chart pools what one second of market time keeps apart.
use super::*;

/// One print a candle unless `per_candle` says more, `seconds` apart, all at
/// one price.
fn tape(candles: usize, per_candle: usize, seconds: i64) -> Vec<Trade> {
    (0..candles * per_candle)
        .map(|n| trade(n as i64 * seconds * 1_000, 100, 100, Side::Buy))
        .collect()
}

fn project(
    trades: &[Trade],
    per_candle: usize,
    px_per_candle: f32,
    region: FlowRegionSpan,
    clip: (i64, i64),
) -> FlowTapeFrame {
    let candles = trades.len() / per_candle;
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / per_candle,
                accepted_ordinal: ordinal % per_candle,
                ticks_per_bar: Decimal::from(per_candle),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: candles,
            clip_left: clip.0.into(),
            clip_right: clip.1.into(),
            width_px: px_per_candle * candles as f32,
            height_px: 200.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            region,
            exclude_opening: false,
        },
    )
}

fn members(dot: &FlowTapeDot) -> Vec<usize> {
    let mut ordinals: Vec<_> = dot.members.iter().map(|member| member.ordinal).collect();
    ordinals.sort_unstable();
    ordinals
}

/// The reported clutter: prints seconds apart on one narrow candle are one
/// pixel apart on screen, and market-time regions never pool them.
#[test]
fn seconds_apart_on_one_candle_pool_only_in_a_zoom_region() {
    let trades = tape(1, 6, 2);
    let by_time = project(&trades, 6, 8.0, FlowRegionSpan::Time, (0, 1));
    assert_eq!(by_time.dots.len(), 6, "each second keeps its own sphere");
    let by_candle = project(&trades, 6, 8.0, FlowRegionSpan::Candles(1), (0, 1));
    assert_eq!(by_candle.dots.len(), 1);
    assert_eq!(by_candle.dots[0].mark.quantity, Decimal::from(600));
    assert_eq!(by_candle.dots[0].native_cells, 6);
    assert_eq!(members(&by_candle.dots[0]), (0..6).collect::<Vec<_>>());
    assert_eq!(by_candle.effective_reference, Some(600.into()));
}

/// Zooming out is a wider rung: neighbouring candles pool, each rung a whole
/// union of the one before, and every print is in exactly one sphere.
#[test]
fn a_wider_rung_pools_neighbouring_candles() {
    let trades = tape(8, 1, 30);
    let quantities = |candles: usize| -> Vec<Decimal> {
        project(&trades, 1, 2.0, FlowRegionSpan::Candles(candles), (0, 8))
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .collect()
    };
    assert_eq!(quantities(1), vec![Decimal::from(100); 8]);
    assert_eq!(quantities(2), vec![Decimal::from(200); 4]);
    assert_eq!(quantities(4), vec![Decimal::from(400); 2]);
    assert_eq!(quantities(8), vec![Decimal::from(800)]);
    let pooled = project(&trades, 1, 2.0, FlowRegionSpan::Candles(4), (0, 8));
    assert_eq!(members(&pooled.dots[0]), vec![0, 1, 2, 3]);
    assert_eq!(members(&pooled.dots[1]), vec![4, 5, 6, 7]);
    assert_eq!((pooled.dots[0].first_slot, pooled.dots[0].end_slot), (0, 4));
}

/// The hull still bounds a sphere: a region wider than a merge may span
/// keeps several spheres rather than one walking the whole of it.
#[test]
fn a_zoom_region_never_outgrows_the_merge_hull() {
    let trades = tape(16, 1, 30);
    let frame = project(&trades, 1, 10.0, FlowRegionSpan::Candles(16), (0, 16));
    assert!(frame.dots.len() > 1, "160 px of prints are not one sphere");
    for dot in &frame.dots {
        let span = (dot.end_slot - dot.first_slot) as f32 * 10.0;
        assert!(span <= 60.0, "a sphere spans {span} px of candles");
    }
    let total: Decimal = frame.dots.iter().map(|dot| dot.mark.quantity).sum();
    assert_eq!(total, Decimal::from(1_600));
}

/// A pan moves the painted clip through a region. The region merges whole
/// first, so the sphere on screen keeps every member, the ones the clip has
/// passed included; a sphere whose centre left the clip leaves whole.
#[test]
fn a_pan_never_regroups_a_zoom_region() {
    let trades = tape(8, 1, 30);
    let region = FlowRegionSpan::Candles(4);
    let whole = project(&trades, 1, 4.0, region, (0, 8));
    let panned = project(&trades, 1, 4.0, region, (1, 8));
    assert_eq!(panned.dots, whole.dots, "both centres are still on screen");
    assert_eq!(panned.offscreen_executions, 0);
    let past = project(&trades, 1, 4.0, region, (3, 8));
    assert_eq!(
        past.dots.len(),
        1,
        "the first sphere's centre left the clip"
    );
    assert_eq!(past.dots[0], whole.dots[1]);
    assert_eq!(past.offscreen_executions, 4);
    // Market time clips first, as it always has.
    let by_time = project(&trades, 1, 4.0, FlowRegionSpan::Time, (3, 8));
    assert_eq!(by_time.offscreen_executions, 3);
}

/// A new print reaches only its own region: every closed region's sphere is
/// the same sphere, byte for byte, before and after it.
#[test]
fn a_new_print_leaves_closed_regions_alone() {
    let mut trades = tape(8, 2, 10);
    let region = FlowRegionSpan::Candles(4);
    let before = project(&trades[..14], 2, 4.0, region, (0, 8));
    trades[15].quantity = 9_000.into();
    let after = project(&trades, 2, 4.0, region, (0, 8));
    assert_eq!(before.dots[0].members, after.dots[0].members);
    assert_eq!(before.dots[0].mark.quantity, after.dots[0].mark.quantity);
    assert_eq!(
        before.dots[0].candle_position,
        after.dots[0].candle_position
    );
    assert!(
        after.dots[0].radius < before.dots[0].radius,
        "only its size is relative"
    );
}
