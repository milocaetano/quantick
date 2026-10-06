//! A zoom hands one tape frame the whole window to merge again, and on a
//! wide WIN window that reread held the trader's frames for seconds; a
//! rebuild that landed half a minute late was then caught up inside a frame
//! that held the UI for 28 s. [`TapeRebuilds`] prices every frame's work
//! ([`TapeWork::fits_frame`]) and runs what does not fit beside the frame,
//! drawing the retained groups until it lands. These tests drive the pane's
//! host through zoom bursts, late landings and a stalled worker, with a
//! runner that lands each rebuild some frames late as the rebuild thread
//! does, and pin two things: no frame reconciles more than its budget, and
//! every frame it does reconcile is exactly the tape the complete per-frame
//! path draws.
use super::*;
use crate::projection::{
    FRAME_WORK_BUDGET, RunnerStopped, TapeFrameInputs, TapeRebuild, TapeRebuildRunner, TapeRebuilds,
};
use std::collections::VecDeque;

/// Runs each rebuild at once and hands it back `delay` polls later, as the
/// rebuild thread would some frames on.
struct LateRunner {
    delay: usize,
    landing: VecDeque<(usize, u64, TapeDotMemory)>,
    started: usize,
}

impl LateRunner {
    fn new(delay: usize) -> Self {
        Self {
            delay,
            landing: VecDeque::new(),
            started: 0,
        }
    }
}

impl TapeRebuildRunner for LateRunner {
    fn start(&mut self, generation: u64, rebuild: TapeRebuild) -> Result<(), Box<TapeRebuild>> {
        self.started += 1;
        self.landing
            .push_back((self.delay, generation, rebuild.run()));
        Ok(())
    }

    fn finished(&mut self) -> Result<Option<(u64, TapeDotMemory)>, RunnerStopped> {
        for landing in &mut self.landing {
            landing.0 = landing.0.saturating_sub(1);
        }
        Ok(self
            .landing
            .pop_front_if(|landing| landing.0 == 0)
            .map(|(_, generation, memory)| (generation, memory)))
    }
}

/// How one run went.
#[derive(Default, Debug)]
struct Hosted {
    /// The most work, in units, any frame reconciled in the frame.
    worst_in_frame: usize,
    /// Frames reconciled in the frame, all equal to the complete path.
    reconciled: usize,
    /// Frames drawn by the stand-in.
    stood_in: usize,
    /// The frames a stand-in drew after the last display change, and the
    /// frames from it to the first reconciled frame.
    stood_in_after_last_change: usize,
    /// Rebuilds handed to the runner.
    rebuilds: usize,
}

/// A run's schedule: the configuration each frame from a given one on is
/// drawn with, and the frames the worker publishes nothing at.
struct Schedule<'a> {
    changes: &'a [(usize, HeatmapConfig)],
    stalled: std::ops::Range<usize>,
    delay: usize,
}

/// [`play`] through the pane's host: every display change starts the host
/// over as the order-flow view does, every frame is painted through it, and
/// every frame it reconciles is asserted equal to the complete per-frame
/// path and within the frame budget.
fn play_hosted(prints: &[Trade], first: &HeatmapConfig, schedule: &Schedule<'_>) -> Hosted {
    let mut worker = BookEngine::new("WINV26");
    worker.apply_visual_config(first.clone());
    let mut config = first.clone();
    let mut pending = PendingTape::default();
    let mut bars = TickBarBuilder::new(200);
    let mut closed: Vec<Bar> = Vec::new();
    let mut queued: Vec<(u64, Trade)> = Vec::new();
    let mut published: Option<Arc<VisibleOrderflow>> = None;
    let mut sealed = TapeDotMemory::default();
    let mut complete = TapeDotMemory::default();
    let mut host = TapeRebuilds::new(LateRunner::new(schedule.delay));
    let started = std::time::Instant::now();
    let mut run = Hosted::default();
    let last_change = schedule.changes.iter().map(|(at, _)| *at).max();
    let mut settled_since_change = false;
    let newest = prints.iter().map(|trade| trade.timestamp_ms).max().unwrap();
    let mut next = 0;
    let mut now_ms = prints[0].timestamp_ms;
    let mut frame_index = 0;
    while now_ms <= newest + FRAME_MS {
        for (_, next_config) in schedule.changes.iter().filter(|(at, _)| *at == frame_index) {
            config = next_config.clone();
            worker.apply_visual_config(config.clone());
            host.start_over(&mut sealed);
            complete.clear();
        }
        while next < prints.len() && prints[next].timestamp_ms <= now_ms {
            let ordinal = pending.record(&prints[next], &config);
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
        if frame_index % FRAMES_PER_PUBLICATION == 0
            && !queued.is_empty()
            && !schedule.stalled.contains(&frame_index)
        {
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
            VisibleOrderflow::with_pending_overlay(
                &pending,
                &config,
                &request,
                published.as_deref(),
            )
            .map(Arc::new)
        };
        if let Some(frame) = frame {
            let inputs = painter_inputs(&frame, &config, now_ms);
            let actual = host
                .project(
                    &mut sealed,
                    TapeFrameInputs {
                        projection: &frame.projection,
                        bubbles: &inputs.style,
                        style: &inputs.style,
                        geometry: inputs.geometry,
                        time: Some(inputs.time),
                        prices: inputs.prices,
                        overlay: frame.tape_overlay.as_ref(),
                    },
                )
                .expect("the host draws the tape");
            let expected = paint_complete(&frame, &config, now_ms, &mut complete);
            let after_change = last_change.is_some_and(|last| frame_index >= last);
            if host.is_running() {
                run.stood_in += 1;
                if after_change && !settled_since_change {
                    run.stood_in_after_last_change += 1;
                }
            } else {
                let work = host.last_work().expect("a reconciled frame is priced");
                assert!(
                    work.fits_frame(),
                    "frame {frame_index} reconciled {work:?} in the frame"
                );
                run.worst_in_frame = run.worst_in_frame.max(work.units());
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
                run.reconciled += 1;
                settled_since_change |= after_change;
            }
        }
        now_ms += FRAME_MS;
        frame_index += 1;
    }
    run.rebuilds = host.runner().started;
    run
}

/// Zoom out from `from_ms` by `steps` steps every `every` frames from frame
/// `at`, then back in the same way.
fn burst(at: usize, every: usize, from_ms: i64, steps: usize) -> Vec<(usize, HeatmapConfig)> {
    let windows: Vec<i64> = (1..=steps)
        .map(|step| from_ms * (1 << step))
        .chain((0..steps).rev().map(|step| from_ms * (1 << step)))
        .collect();
    windows
        .into_iter()
        .enumerate()
        .map(|(index, window_ms)| (at + index * every, config(window_ms, 100_000)))
        .collect()
}

#[test]
fn a_zoom_burst_reconciles_no_frame_past_its_budget_and_every_one_exactly() {
    let changes = burst(600, 4, 8_000, 4);
    let run = play_hosted(
        &tape(150, None),
        &config(8_000, 100_000),
        &Schedule {
            changes: &changes,
            stalled: 0..0,
            delay: 3,
        },
    );
    assert!(
        run.rebuilds >= 4,
        "the widest windows rebuild beside the frame: {run:?}"
    );
    assert!(run.stood_in > 0, "{run:?}");
    assert!(run.reconciled > 700, "{run:?}");
    assert!(run.worst_in_frame <= FRAME_WORK_BUDGET, "{run:?}");
    // Back at the narrow window, the tape is reconciled within a few frames
    // of the last step.
    assert!(run.stood_in_after_last_change <= 8, "{run:?}");
}

#[test]
fn a_rebuild_that_lands_behind_the_tape_is_caught_up_beside_the_frame_too() {
    // One zoom to a wide window, and a runner forty frames slow: the landed
    // memory is four seconds of windows behind, too many for a frame.
    let changes = [(900, config(128_000, 100_000))];
    let run = play_hosted(
        &tape(150, None),
        &config(8_000, 100_000),
        &Schedule {
            changes: &changes,
            stalled: 0..0,
            delay: 40,
        },
    );
    assert!(
        run.rebuilds >= 2,
        "the landing is priced again and caught up beside the frame: {run:?}"
    );
    assert!(run.worst_in_frame <= FRAME_WORK_BUDGET, "{run:?}");
    assert!(run.reconciled > 1_000, "{run:?}");
}

#[test]
fn a_stalled_worker_keeps_every_frame_within_its_budget() {
    // The worker publishes nothing for two minutes of tape: the pending
    // prints and the cells after its seal grow past a frame's budget.
    let run = play_hosted(
        &tape(150, None),
        &config(128_000, 100_000),
        &Schedule {
            changes: &[],
            stalled: 200..1_400,
            delay: 2,
        },
    );
    assert!(run.rebuilds >= 2, "{run:?}");
    assert!(run.stood_in > 0, "{run:?}");
    assert!(run.worst_in_frame <= FRAME_WORK_BUDGET, "{run:?}");
    assert!(run.reconciled > 500, "{run:?}");
}

#[test]
fn a_steady_wide_tape_reconciles_every_frame_in_the_frame() {
    let run = play_hosted(
        &tape(150, None),
        &config(128_000, 100_000),
        &Schedule {
            changes: &[],
            stalled: 0..0,
            delay: 2,
        },
    );
    // Only the opening frame, which reads the whole tape so far, may run
    // beside the frame.
    assert!(run.rebuilds <= 1, "{run:?}");
    assert!(run.stood_in <= 2, "{run:?}");
}

/// A pending tape that has folded its overlay frame after frame folds the
/// same cells as one that folds every pending print at once, through a
/// stalled worker, receipts, a wider window and back.
#[test]
fn the_overlay_folded_where_prints_land_is_the_overlay_folded_whole() {
    let prints = tape(90, Some((1_800, 7_000)));
    let first = config(8_000, 100_000);
    let changes = [
        (300, config(60_000, 100_000)),
        (700, config(8_000, 100_000)),
    ];
    let mut config = first.clone();
    let mut worker = BookEngine::new("WINV26");
    worker.apply_visual_config(config.clone());
    let mut pending = PendingTape::default();
    let mut bars = TickBarBuilder::new(200);
    let mut closed: Vec<Bar> = Vec::new();
    let mut queued: Vec<(u64, Trade)> = Vec::new();
    let mut acknowledged = 0;
    let mut published: Option<Arc<VisibleOrderflow>> = None;
    let started = std::time::Instant::now();
    let mut compared = 0;
    let mut next = 0;
    let mut now_ms = prints[0].timestamp_ms;
    let mut frame_index = 0;
    while next < prints.len() {
        for (_, next_config) in changes.iter().filter(|(at, _)| *at == frame_index) {
            config = next_config.clone();
            worker.apply_visual_config(config.clone());
        }
        while next < prints.len() && prints[next].timestamp_ms <= now_ms {
            let ordinal = pending.record(&prints[next], &config);
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
        // The worker stalls for twenty seconds twice.
        let stalled = (200..400).contains(&frame_index) || (500..700).contains(&frame_index);
        if frame_index % FRAMES_PER_PUBLICATION == 0 && !queued.is_empty() && !stalled {
            let through = queued.last().map(|(ordinal, _)| *ordinal).unwrap();
            for (_, trade) in queued.drain(..) {
                worker.record_trade(&trade);
            }
            let at = started + std::time::Duration::from_millis(frame_index as u64 * 16);
            published = worker.project_at(&request, at);
            pending.acknowledge(through);
            acknowledged = through;
        }
        let mut whole = PendingTape::default();
        for trade in &prints[..next] {
            whole.record(trade, &config);
        }
        whole.acknowledge(acknowledged);
        let overlay = |pending: &PendingTape| {
            VisibleOrderflow::with_pending_overlay(pending, &config, &request, published.as_deref())
                .and_then(|frame| frame.tape_overlay)
                .map(|overlay| format!("{:?}", overlay.cells))
        };
        let (kept, folded) = (overlay(&pending), overlay(&whole));
        assert_eq!(kept, folded, "frame {frame_index}");
        compared += usize::from(kept.is_some());
        now_ms += FRAME_MS;
        frame_index += 1;
    }
    assert!(compared > 500, "{compared} overlays compared");
}
