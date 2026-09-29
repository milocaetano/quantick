//! A wide native tape must cost a frame what a narrow one does.
//!
//! The trader zoomed the WIN tape out to 23 minutes and the chart dropped to
//! 18 frames a second: every frame re-read, re-folded and re-placed every
//! native cell in the window, although only the newest can still change.
//! These tests drive the path the application runs — the worker publishing
//! on its own cadence, the accepted prints completing each frame in between,
//! the painter merging the tape into dots — and pin two things:
//!
//! * the cells a frame reconciles do not grow with the tape's window or with
//!   the history already played;
//! * the tape drawn is exactly the one the complete per-frame path draws,
//!   frame by frame, including across a late print and across evictions that
//!   reach inside the window.
use super::*;
use crate::config::VolumeDotStyle;
use crate::config::theme::OrderflowRenderStyle;
use crate::projection::{
    DotSizing, PendingTape, TapeDotFrame, TapeDotGeometry, TapeDotMemory, TapeFacts, draws_bubble,
    project_tape_frame, project_tape_frame_with_overlay,
};
use quantick_engine::{BarBuilder as _, Side, TickBarBuilder};

/// Market milliseconds between two painted frames.
const FRAME_MS: i64 = 100;
/// Painted frames per worker publication: the worker lags the painter.
const FRAMES_PER_PUBLICATION: usize = 3;
/// Prints per second of market time.
const PRINTS_PER_SECOND: i64 = 40;

fn print(agg_id: u64, timestamp_ms: i64) -> Trade {
    // A deterministic walk: bursts, repeats at one price, both sides.
    let step = agg_id as i64;
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::from(100 + (step * 7_919 % 23) - (step / 400 % 9)),
        quantity: Decimal::from(1 + step * 31 % 9),
        side: if step * 13 % 3 == 0 {
            Side::Sell
        } else {
            Side::Buy
        },
    }
}

/// `seconds` of prints, with one print delivered `late_by_ms` behind the
/// newest at `late_at` (by position), when asked.
fn tape(seconds: i64, late: Option<(usize, i64)>) -> Vec<Trade> {
    let mut prints: Vec<Trade> = (0..seconds * PRINTS_PER_SECOND)
        .map(|index| {
            let time = 1_000 + index * 1_000 / PRINTS_PER_SECOND + index % 7 * 3;
            print(index as u64 + 1, time)
        })
        .collect();
    if let Some((at, late_by_ms)) = late {
        let mut stale = print(
            prints.len() as u64 + 1,
            prints[at].timestamp_ms - late_by_ms,
        );
        stale.quantity = Decimal::from(4);
        prints.insert(at + 1, stale);
    }
    prints
}

fn config(window_ms: i64, max_aggressions: usize) -> HeatmapConfig {
    HeatmapConfig {
        show_aggressions: true,
        max_aggressions,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: false,
            show_aggressions: true,
            native_tape: true,
            window: crate::LaneWindow::Fixed { ms: window_ms },
            ..Default::default()
        },
        volume_dots: VolumeDotStyle {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// What one run observed.
struct Run {
    /// Cells the painter reconciled on each frame after the first.
    reconciled: Vec<usize>,
    /// The most native cells the published tape held at once.
    widest_tape: usize,
    /// Frames that carried their pending prints beside the published tape.
    overlaid: usize,
}

/// Play `prints` through the worker, the pending tape and two painters: the
/// sealed one the application runs, and the complete per-frame reference.
fn play(prints: &[Trade], config: &HeatmapConfig) -> Run {
    let mut worker = BookEngine::new("WINV26");
    worker.apply_visual_config(config.clone());
    let mut pending = PendingTape::default();
    let mut bars = TickBarBuilder::new(200);
    let mut closed: Vec<Bar> = Vec::new();
    let mut queued: Vec<(u64, Trade)> = Vec::new();
    let mut published: Option<Arc<VisibleOrderflow>> = None;
    let mut sealed = TapeDotMemory::default();
    let mut complete = TapeDotMemory::default();
    let started = std::time::Instant::now();
    let mut run = Run {
        reconciled: Vec::new(),
        widest_tape: 0,
        overlaid: 0,
    };
    let newest = prints.iter().map(|trade| trade.timestamp_ms).max().unwrap();
    let mut next = 0;
    let mut now_ms = prints[0].timestamp_ms;
    let mut frame_index = 0;
    while now_ms <= newest + FRAME_MS {
        while next < prints.len() && prints[next].timestamp_ms <= now_ms {
            let ordinal = pending.record(&prints[next], config);
            queued.push((ordinal, prints[next].clone()));
            if let Some(bar) = bars.push(&prints[next]) {
                closed.push(bar);
            }
            next += 1;
        }
        let request = ProjectionRequest {
            timeline_revision: next as u64,
            first_bar_index: closed.len().saturating_sub(40),
            closed: closed[closed.len().saturating_sub(40)..].to_vec(),
            partial: bars.partial().cloned(),
            lane: true,
            on_newest_bar: true,
            lane_reference_ms: Some(6_000),
            lane_now_ms: Some(now_ms),
            price_range: (80.0, 130.0),
            dot_zoom: Some(DotZoom {
                native_tape: true,
                tape_window_ms: 100,
                tape_level_ticks: 1,
                candle_level_ticks: 1,
                lane_bars: Vec::new(),
            }),
        };
        if frame_index % FRAMES_PER_PUBLICATION == 0 && !queued.is_empty() {
            let through = queued.last().map(|(ordinal, _)| *ordinal).unwrap();
            for (_, trade) in queued.drain(..) {
                worker.record_trade(&trade);
            }
            let at = started + std::time::Duration::from_millis(frame_index as u64 * 16);
            published = worker.project_at(&request, at);
            pending.acknowledge(through);
        }
        let frame = if pending.is_empty() {
            published.clone()
        } else {
            VisibleOrderflow::with_pending_overlay(&pending, config, &request, published.as_deref())
                .map(Arc::new)
        };
        if let Some(frame) = frame {
            if let Some(facts) = published
                .as_ref()
                .and_then(|frame| frame.projection.tape_facts.as_deref())
            {
                run.widest_tape = run.widest_tape.max(facts.clusters.len());
            }
            run.overlaid += usize::from(frame.tape_overlay.is_some());
            let (actual, expected) = paint(&frame, config, now_ms, &mut sealed, &mut complete);
            assert_eq!(
                actual.marks, expected.marks,
                "frame {frame_index} at {now_ms}"
            );
            assert_eq!(
                actual.max_radius, expected.max_radius,
                "frame {frame_index}"
            );
            assert_eq!(
                actual.full_quantity, expected.full_quantity,
                "frame {frame_index}"
            );
            run.reconciled.push(sealed.reconciled_cells());
        }
        now_ms += FRAME_MS;
        frame_index += 1;
    }
    run
}

/// Paint `frame` twice: through the sealed path, and through the complete
/// marks with no seal to trust, as every frame used to be painted.
fn paint(
    frame: &VisibleOrderflow,
    config: &HeatmapConfig,
    now_ms: i64,
    sealed: &mut TapeDotMemory,
    complete: &mut TapeDotMemory,
) -> (TapeDotFrame, TapeDotFrame) {
    let mut style = OrderflowRenderStyle::from_config(config, [0, 0, 0, 255]);
    style.dot_sizing = Some(DotSizing {
        native_tape: true,
        tape_column_px: 2.0,
        candle_column_px: 12.0,
        px_per_price: 12.0,
        typed_full: None,
    });
    let mut edge = frame.live_edge.expect("the tape has a live edge");
    edge.now_ms = now_ms;
    edge.window_ms = config.lane_window_ms(edge.reference_ms);
    let geometry = TapeDotGeometry {
        left_x: 1.0 - 1.0 / frame.slot_count.max(1) as f64,
        right_x: 1.0,
        width_px: 800.0,
        height_px: 600.0,
    };
    let prices = PriceWindow::new(Decimal::from(80), Decimal::from(130));
    let drawn = |projection: &HeatmapProjection| -> Vec<_> {
        projection
            .aggressions
            .iter()
            .filter(|mark| draws_bubble(&style, mark))
            .cloned()
            .collect()
    };
    let actual = project_tape_frame_with_overlay(
        drawn(&frame.projection),
        Some(sealed),
        &style,
        geometry,
        Some((edge, 100)),
        prices,
        frame.projection.tape_facts.as_deref(),
        frame.tape_overlay.as_deref(),
    )
    .expect("the sealed painter draws the tape");
    let whole = frame.tape_projection();
    let unsealed = whole.tape_facts.as_deref().map(|facts| TapeFacts {
        seal: None,
        ..facts.clone()
    });
    let expected = project_tape_frame(
        drawn(whole),
        Some(complete),
        &style,
        geometry,
        Some((edge, 100)),
        prices,
        unsealed.as_ref(),
    )
    .expect("the complete painter draws the tape");
    (actual, expected)
}

#[test]
fn a_wide_tape_window_reconciles_no_more_cells_per_frame_than_a_narrow_one() {
    let prints = tape(150, None);
    let narrow = play(&prints, &config(10_000, 100_000));
    let wide = play(&prints, &config(140_000, 100_000));
    assert!(
        wide.widest_tape > 10 * narrow.widest_tape,
        "the wide window holds far more cells: {} against {}",
        wide.widest_tape,
        narrow.widest_tape
    );
    assert!(wide.overlaid > 100, "most frames carry pending prints");
    // The first frame reads everything once; after it, a frame reconciles
    // the cells after the seal and the pending prints, whatever the window.
    let steady = |run: &Run| run.reconciled[1..].iter().copied().max().unwrap_or(0);
    eprintln!(
        "TAPE_SEAL wide_cells={} narrow_cells={} wide_steady={} narrow_steady={} overlaid={}",
        wide.widest_tape,
        narrow.widest_tape,
        steady(&wide),
        steady(&narrow),
        wide.overlaid
    );
    let bound = (PRINTS_PER_SECOND as usize) * 2;
    assert!(
        steady(&wide) <= bound,
        "a 140 s tape reconciled {} cells in one frame",
        steady(&wide)
    );
    assert!(
        steady(&wide) <= steady(&narrow).max(bound / 2),
        "wide {} against narrow {}",
        steady(&wide),
        steady(&narrow)
    );
    assert!(
        steady(&wide) * 20 < wide.widest_tape,
        "{} of {} cells",
        steady(&wide),
        wide.widest_tape
    );
}

#[test]
fn the_reconciled_cells_do_not_grow_with_played_history() {
    let prints = tape(150, None);
    let run = play(&prints, &config(140_000, 100_000));
    let third = run.reconciled.len() / 3;
    let early = run.reconciled[1..third].iter().copied().max().unwrap();
    let late = run.reconciled[2 * third..].iter().copied().max().unwrap();
    assert!(
        late <= early.max(PRINTS_PER_SECOND as usize),
        "the last third reconciled {late}, the first {early}"
    );
}

#[test]
fn a_late_print_rereads_the_sealed_tape_and_draws_what_the_complete_path_draws() {
    // Twelve seconds behind the newest print: far inside the sealed cells.
    let prints = tape(60, Some((1_500, 12_000)));
    let run = play(&prints, &config(90_000, 100_000));
    assert!(
        run.reconciled[1..]
            .iter()
            .any(|cells| *cells > PRINTS_PER_SECOND as usize * 10),
        "the late print sends the painter back over the sealed cells once"
    );
}

#[test]
fn evictions_inside_the_window_draw_what_the_complete_path_draws() {
    // A retention cap far below the window: the pending tape evicts before
    // the worker does, so the horizon moves back and forth between frames.
    let prints = tape(60, None);
    let run = play(&prints, &config(90_000, 900));
    assert!(run.overlaid > 50);
}

/// The worker folds only the prints after its last seal, and its live pass is
/// the one a complete fold builds: every cell, every mark, every floored
/// contract, publication after publication, a late print included.
#[test]
fn the_worker_continues_its_sealed_tape_and_builds_the_complete_live_pass() {
    use crate::projection::{
        SealInputs, TapeReuse, VolumeDots, project_live, project_live_after, project_settled,
        reusable_through, seal_tape,
    };
    let mut config = config(40_000, 100_000);
    config.bubbles.min_quantity = 3.0;
    let mut history = crate::LiquidityHistory::new(config.clone());
    let prints = tape(45, Some((1_200, 9_000)));
    let zoom = DotZoom {
        native_tape: true,
        tape_window_ms: 100,
        tape_level_ticks: 1,
        candle_level_ticks: 1,
        lane_bars: Vec::new(),
    };
    let prices = PriceWindow::new(Decimal::from(80), Decimal::from(130)).unwrap();
    let mut bars = TickBarBuilder::new(200);
    let mut closed: Vec<Bar> = Vec::new();
    let mut previous: Option<TapeFacts> = None;
    let (mut continued, mut builds) = (0, 0);
    let mut next = 0;
    let mut now_ms = prints[0].timestamp_ms;
    while next < prints.len() {
        now_ms += 250;
        while next < prints.len() && prints[next].timestamp_ms <= now_ms {
            history.record_aggression(&prints[next]);
            if let Some(bar) = bars.push(&prints[next]) {
                closed.push(bar);
            }
            next += 1;
        }
        let edge = LiveEdge {
            now_ms,
            window_ms: config.lane_window_ms(6_000),
            reference_ms: 6_000,
            on_newest_bar: true,
        };
        let timeline = BarTimeline::from_bars(0, &closed, bars.partial(), Some(edge))
            .with_full_lane_coverage();
        let dots = VolumeDots::resolve(&zoom, &closed, bars.partial());
        let settled = project_settled(&history, &timeline, prices, Some(&dots));
        let reuse = previous.as_ref().map(|previous| TapeReuse {
            previous,
            revision: 0,
        });
        let complete = project_live(&history, &timeline, prices, &settled, Some(&dots));
        let live = project_live_after(&history, &timeline, prices, &settled, Some(&dots), reuse);
        assert_eq!(live, complete, "the live pass at {now_ms}");
        let coverage = Vec::new();
        continued += usize::from(reuse.is_some_and(|reuse| {
            let live_from_ms = settled.live_from_ms.unwrap_or(i64::MAX);
            let lane_from_ms = timeline.lane_start_ms();
            reusable_through(
                reuse,
                &history,
                lane_from_ms,
                &settled,
                Some(&dots),
                &coverage,
                live_from_ms,
            )
            .is_some()
        }));
        builds += 1;
        let mut facts = live
            .tape_facts
            .as_deref()
            .cloned()
            .expect("the tape has facts");
        seal_tape(
            &mut facts,
            previous.as_ref(),
            SealInputs {
                revision: 0,
                window_ms: 100,
                seal_from_ms: history.latest_ms(),
                lane_from_ms: timeline.lane_start_ms(),
                recorded: history.counters().aggressions_recorded,
            },
        );
        previous = Some(facts);
    }
    assert!(
        continued * 10 > builds * 9,
        "{continued} of {builds} passes continued the sealed tape"
    );
    assert!(
        continued < builds,
        "the late print folded the whole tape again"
    );
}

/// Prints a quarter apart, on the cent grid a chart opens on and off the
/// whole-number grid the tape later names.
fn quarter_prints() -> Vec<Trade> {
    (0..400_i64)
        .map(|index| Trade {
            agg_id: index as u64 + 1,
            timestamp_ms: 1_000 + index * 25,
            price: Decimal::new(10_000 + index % 7 * 25, 2),
            quantity: Decimal::from(1 + index % 5),
            side: if index % 3 == 0 {
                Side::Sell
            } else {
                Side::Buy
            },
        })
        .collect()
}

fn grid_request(bars: &TickBarBuilder, closed: &[Bar], now_ms: i64) -> ProjectionRequest {
    ProjectionRequest {
        timeline_revision: closed.len() as u64,
        first_bar_index: 0,
        closed: closed.to_vec(),
        partial: bars.partial().cloned(),
        lane: true,
        on_newest_bar: true,
        lane_reference_ms: Some(6_000),
        lane_now_ms: Some(now_ms),
        price_range: (90.0, 110.0),
        dot_zoom: Some(DotZoom {
            native_tape: true,
            tape_window_ms: 100,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: Vec::new(),
        }),
    }
}

/// The worker's tape after [`quarter_prints`], its capture grid sized from
/// the tape (cent to whole number) at print 210 — and, when `sealed_first`,
/// a publication at print 200 that sealed the cent-grid cells. Returns that
/// first tape, if any, and the last one.
fn tape_across_a_grid_change(sealed_first: bool) -> (Option<TapeFacts>, TapeFacts) {
    let mut worker = BookEngine::new("WINV26");
    worker.apply_visual_config(config(60_000, 100_000));
    let prints = quarter_prints();
    let mut bars = TickBarBuilder::new(50);
    let mut closed: Vec<Bar> = Vec::new();
    let started = std::time::Instant::now();
    let mut first = None;
    for (index, trade) in prints.iter().enumerate() {
        worker.record_trade(trade);
        if let Some(bar) = bars.push(trade) {
            closed.push(bar);
        }
        if index == 200 && sealed_first {
            let frame = worker
                .project_at(&grid_request(&bars, &closed, trade.timestamp_ms), started)
                .expect("the first publication");
            first = frame.projection.tape_facts.as_deref().cloned();
        }
        if index == 210 {
            worker.size_from_tape(Decimal::ONE, Some(Decimal::from(100)));
        }
    }
    let now_ms = prints.last().expect("prints").timestamp_ms;
    let last = worker
        .project_at(
            &grid_request(&bars, &closed, now_ms),
            started + std::time::Duration::from_secs(10),
        )
        .expect("the last publication");
    let facts = last.projection.tape_facts.as_deref().cloned();
    (first, facts.expect("the last publication has a tape"))
}

/// A grid the tape names after a sealed publication rekeys every native
/// cell. The worker kept copying the cells below its seal, folded on the old
/// grid: the tape mixed two grids, what it drew depended on when the worker
/// had last published, and the painter kept the old cells as sealed. After
/// the change the worker folds the whole tape again, exactly as a worker
/// that never published before it, and the painter rereads it.
#[test]
fn a_grid_change_after_a_sealed_publication_folds_the_whole_tape_again() {
    let (first, continued) = tape_across_a_grid_change(true);
    let (_, complete) = tape_across_a_grid_change(false);
    let first = first.expect("the first publication has a tape");
    let seal = first.seal.as_ref().expect("the first publication sealed");
    let cent = Decimal::new(1, 2);
    assert!(
        first
            .clusters
            .iter()
            .any(|cell| cell.price_span == cent && cell.first_timestamp_ms < seal.through_ms),
        "the first publication sealed cells on the cent grid"
    );
    assert!(
        complete
            .clusters
            .iter()
            .all(|cell| cell.price_span == Decimal::ONE)
    );
    let spans: std::collections::BTreeSet<Decimal> = continued
        .clusters
        .iter()
        .map(|cell| cell.price_span)
        .collect();
    assert_eq!(spans, [Decimal::ONE].into(), "one grid on the whole tape");
    assert_eq!(
        continued.clusters, complete.clusters,
        "the tape is the full fold on the new grid"
    );
    let now = continued
        .seal
        .as_ref()
        .expect("the last publication sealed");
    assert!(
        !Arc::ptr_eq(&seal.lineage, &now.lineage),
        "cells on another grid start a new lineage"
    );
}

/// The lineage is the seal's promise that no sealed cell changed. Cells on
/// another grid are other cells even where the sealed stretch the two
/// publications share holds none of them.
#[test]
fn a_seal_on_another_native_grid_starts_a_new_lineage() {
    use crate::projection::{SealInputs, seal_tape};
    let inputs = |native_width: Decimal| SealInputs {
        revision: 0,
        window_ms: 100,
        native_width,
        seal_from_ms: Some(5_000),
        lane_from_ms: Some(0),
        recorded: 0,
    };
    let empty = TapeFacts {
        clusters: Vec::new(),
        evicted_through_ms: None,
        floored_quantity: Decimal::ZERO,
        opening_bursts: Vec::new(),
        floor: Decimal::ZERO,
        seal: None,
    };
    let cent = Decimal::new(1, 2);
    let mut previous = empty.clone();
    seal_tape(&mut previous, None, inputs(cent));
    let (mut same, mut regridded) = (empty.clone(), empty);
    seal_tape(&mut same, Some(&previous), inputs(cent));
    seal_tape(&mut regridded, Some(&previous), inputs(Decimal::ONE));
    let lineage = |facts: &TapeFacts| Arc::clone(&facts.seal.as_ref().expect("sealed").lineage);
    assert!(Arc::ptr_eq(&lineage(&previous), &lineage(&same)));
    assert!(
        !Arc::ptr_eq(&lineage(&previous), &lineage(&regridded)),
        "a new grid is a new lineage"
    );
    assert_eq!(
        regridded.seal.as_ref().map(|seal| seal.native_width),
        Some(Decimal::ONE)
    );
}
