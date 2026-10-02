use super::*;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowKeep, FlowReference, FlowTapeView, OwnedFlowExecution},
};
use rust_decimal::Decimal;
use std::time::{Duration, Instant};
fn request(end: usize) -> FlowRequest {
    FlowRequest {
        epoch: 1,
        layout_revision: 1,
        source_count: end,
        requested: 0..end,
        keep: FlowKeep {
            slots: 0..4,
            ordinals: 0..end,
        },
        opening_windows: Vec::new(),
        opening_ordinals: Vec::new(),
        view: FlowTapeView {
            first_slot: 0,
            end_slot: 4,
            clip_left: 0.into(),
            clip_right: 4.into(),
            width_px: 400.0,
            height_px: 200.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::Typed(900.into()),
            radius_limit: 7.0,
            merge_support_radius: 12.0,
            exclude_opening: true,
        },
    }
}
fn chunk(ordinal: usize) -> FlowChunk {
    FlowChunk {
        epoch: 1,
        ticks_per_bar: 200.into(),
        executions: vec![OwnedFlowExecution {
            ordinal,
            slot: ordinal / 200,
            accepted_ordinal: ordinal % 200,
            trade: Trade {
                agg_id: 1,
                timestamp_ms: 1000,
                price: 100.into(),
                quantity: Decimal::ONE,
                side: Side::Buy,
            },
        }],
    }
}
#[test]
fn bounded_queue_returns_the_exact_unsent_source_packet() {
    let (input, inbox) = mpsc::sync_channel(QUEUE_CHUNKS);
    let (release, wait) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        wait.recv().unwrap();
        drop(inbox);
    });
    let runner = FlowThread {
        channels: Some(Channels {
            input,
            request: Arc::new(Mutex::new(request(3))),
            output: Arc::new(Mutex::new(None)),
            thread,
        }),
    };
    assert!(runner.submit(chunk(0)).is_ok());
    assert!(runner.submit(chunk(1)).is_ok());
    let returned = runner.submit(chunk(2)).unwrap_err();
    assert_eq!(returned.executions[0].ordinal, 2);
    assert_eq!(returned.executions[0].trade.quantity, Decimal::ONE);
    release.send(()).unwrap();
}
#[test]
fn ongoing_appends_publish_advancing_results_and_layout_only_needs_no_source_copy() {
    let mut runner = FlowThread::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut next = 0;
    let mut observed = 0;
    while observed < 200 {
        let request = request(next + 1);
        runner.request(&request, false);
        if next < 200 && runner.submit(chunk(next)).is_ok() {
            next += 1;
        }
        if let Some(frame) = runner.finished() {
            assert_eq!(frame.source_revision, 1);
            assert!(frame.loaded_executions >= observed);
            observed = frame.loaded_executions;
        }
        assert!(
            Instant::now() < deadline,
            "worker must publish under continuing input"
        );
        std::thread::yield_now();
    }
    let mut zoom = request(200);
    zoom.layout_revision = 2;
    zoom.view.width_px = 20.0;
    runner.request(&zoom, true);
    loop {
        if let Some(frame) = runner.finished()
            && frame.layout_revision == 2
        {
            assert_eq!(frame.loaded_executions, 200);
            assert_eq!(frame.omitted_executions, 0);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "layout-only request must publish"
        );
        std::thread::yield_now();
    }
}
#[test]
fn a_disconnected_worker_reports_reseed_instead_of_reusing_old_coverage() {
    let (input, inbox) = mpsc::sync_channel(QUEUE_CHUNKS);
    drop(inbox);
    let thread = std::thread::spawn(|| {});
    while !thread.is_finished() {
        std::thread::yield_now();
    }
    let mut runner = FlowThread {
        channels: Some(Channels {
            input,
            request: Arc::new(Mutex::new(request(0))),
            output: Arc::new(Mutex::new(None)),
            thread,
        }),
    };
    assert!(runner.request(&request(1), true));
    assert!(runner.submit(chunk(0)).is_ok());
}

fn await_frame(
    runner: &FlowThread,
    accepts: impl Fn(&FlowTapeFrame) -> bool,
) -> Arc<FlowTapeFrame> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(frame) = runner.finished()
            && accepts(&frame)
        {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "worker publication did not advance"
        );
        std::thread::yield_now();
    }
}
#[test]
fn an_unfinished_quiet_fill_and_partial_layout_change_are_eventually_published() {
    let mut runner = FlowThread::default();
    let mut view = request(5000);
    runner.request(&view, false);
    assert!(runner.submit(chunk(0)).is_ok());
    await_frame(&runner, |frame| frame.loaded_executions == 1);
    assert!(runner.submit(chunk(1)).is_ok());
    let partial = await_frame(&runner, |frame| frame.loaded_executions == 2);
    assert_eq!(partial.omitted_executions, 4998);
    view.layout_revision += 1;
    view.view.width_px = 20.0;
    runner.request(&view, true);
    let remapped = await_frame(&runner, |frame| {
        frame.layout_revision == view.layout_revision
    });
    assert_eq!(remapped.loaded_executions, 2);
    assert_eq!(remapped.view.width_px, 20.0);
}
#[test]
fn queued_old_epoch_packets_cannot_starve_or_contaminate_the_new_request() {
    let mut runner = FlowThread::default();
    runner.request(&request(3), false);
    let mut next = request(1);
    next.epoch = 2;
    {
        // Hold the request until old packets are queued, then change epochs.
        let channels = runner.channels.as_ref().unwrap();
        let mut shared = channels.request.lock().unwrap();
        assert!(runner.submit(chunk(0)).is_ok());
        assert!(runner.submit(chunk(1)).is_ok());
        *shared = next.clone();
    }
    runner.request(&next, true);
    let mut packet = chunk(0);
    packet.epoch = 2;
    packet.executions[0].trade.quantity = 7.into();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match runner.submit(packet) {
            Ok(()) => break,
            Err(returned) => packet = *returned,
        }
        assert!(
            Instant::now() < deadline,
            "new epoch source must be admitted"
        );
        std::thread::yield_now();
    }
    let frame = await_frame(&runner, |frame| {
        frame.source_revision == 2 && frame.loaded_executions == 1
    });
    assert_eq!(frame.omitted_executions, 0);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        7.into()
    );
}
