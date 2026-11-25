use super::*;

/// Measure the coherent capture of every initial scope in batches:
/// `(best median, best p99, worst of the best batch)` in microseconds,
/// where "best" is the batch with the lowest p99. A noisy neighbour can
/// only make a batch look slower, never faster, so the best batch is the
/// honest reading of the capture's own cost.
/// A workspace with something in every collection the snapshot scopes walk.
///
/// The benchmark below exists to answer "what does a capture cost", and an
/// empty workspace cannot answer it: every loop the analysis, order-flow
/// and session scopes add runs zero times over `app_with_history` alone —
/// no indicators, no drawings, no book ladder, no replay link, no paper
/// rows — so a measurement taken there reports the cost of nine empty
/// projections and calls it flat. This fills each of them through the path
/// the application itself uses, so the number below is the cost of a
/// capture over a working chart.
pub(super) fn loaded_observer_workspace(bars: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot, DepthEvent};

    let (mut app, commands) = app_with_history(bars);

    // Indicators: delivered the way the worker delivers them, with a full
    // committed column so the "latest reading" walk has rows to reach.
    for index in 0..BENCH_INDICATORS_PER_PANE {
        let pane = &mut app.active_tab_mut().flow_pane;
        let slot = pane.indicators.allocate_slot("native.ema");
        let descriptor = quantick_indicators::IndicatorDescriptor {
            title: format!("EMA {index}"),
            short_title: None,
            overlay: true,
            plots: vec![quantick_indicators::PlotSpec {
                id: quantick_indicators::PlotId::new(0),
                title: "EMA".to_owned(),
                style: quantick_indicators::PlotStyle::Line,
                base_color: quantick_indicators::Rgba8::opaque(1, 2, 3),
                width: 1.0,
                offset: 0,
                marker: None,
            }],
            fills: Vec::new(),
            inputs: vec![quantick_indicators::InputSpec::Int {
                name: "len".to_owned(),
                title: "Length".to_owned(),
                default: 9,
                min: Some(1),
                max: Some(500),
                step: Some(1),
                options: Vec::new(),
            }],
        };
        let column = (0..bars).map(|bar| bar as f64).collect::<Vec<_>>();
        pane.indicators
            .apply(crate::indicator_worker::event_fixture::rebuilt(
                slot,
                descriptor,
                vec![column],
            ));
    }

    // Drawings: placed through the same call the toolrail makes.
    for index in 0..BENCH_DRAWINGS_PER_PANE {
        let slot = index % (bars as usize).max(1);
        let (time, price) = {
            let pane = &app.active_tab().flow_pane;
            let Some(bar) = pane.closed_bar(slot) else {
                break;
            };
            (
                pane.slot_open_time(slot),
                rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap_or(0.0),
            )
        };
        let flow = &mut app.active_tab_mut().flow_pane;
        flow.drawings.place_with(
            drawing_tool("horizontal-line"),
            &drawings::DrawingBand::Price,
            ChartPoint::at_time(slot as f32 + 0.5, price, time),
            |tool| drawings::NewDrawing {
                style: drawings::DrawingStyle::default(),
                payload: tool.default_payload(),
                look: Default::default(),
            },
        );
    }

    // The book: driven in as a venue snapshot and published, so the L2
    // scope has a full ladder to render into exact decimal strings.
    // Priced well above the ladder depth so the bid side never walks a
    // price down through zero, which the book rightly refuses.
    const BENCH_MID_PRICE: i64 = 1_000;
    let level = |offset: i64, side: i64| {
        BookLevel::new(
            rust_decimal::Decimal::from(BENCH_MID_PRICE + side * (offset + 1)),
            rust_decimal::Decimal::from(offset + 1),
        )
        .expect("a positive price and quantity is a valid level")
    };
    let bids = (0..BENCH_BOOK_LEVELS_PER_SIDE)
        .map(|offset| level(offset, -1))
        .collect::<Vec<_>>();
    let asks = (0..BENCH_BOOK_LEVELS_PER_SIDE)
        .map(|offset| level(offset, 1))
        .collect::<Vec<_>>();
    app.active_tab_mut()
        .tape_mut()
        .handle_depth_event(DepthEvent::Snapshot {
            symbol: "TESTUSDT".to_owned(),
            generation: 1,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            price_step: None,
            snapshot: BookSnapshot::new(
                10,
                bids,
                asks,
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
    app.active_tab_mut().tape_mut().flush_for_test();

    // Paper: a closed trade ledger and an open position, through the
    // simulator's own entry path.
    let print = |agg_id: u64, price: i64| quantick_engine::Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small ids") * 1_000,
        price: rust_decimal::Decimal::from(price),
        quantity: rust_decimal::Decimal::ONE,
        side: quantick_engine::Side::Buy,
    };
    {
        let paper = &mut app.active_tab_mut().paper;
        paper.seed(&print(0, 100));
        let mut agg = 1;
        for round in 0..BENCH_CLOSED_TRADES {
            paper.market(quantick_engine::Side::Buy);
            paper.on_trade(&print(agg, 100));
            agg += 1;
            paper.close_position();
            paper.on_trade(&print(agg, 101 + (round % 3) as i64));
            agg += 1;
        }
        // Leave one open, so the position branch is walked too.
        paper.market(quantick_engine::Side::Buy);
        paper.on_trade(&print(agg, 100));
    }

    (app, commands)
}

/// The shape of the loaded benchmark workspace. Chosen to sit at or above
/// what a working chart carries, so the measured cost is an upper bound on
/// a real one rather than a best case.
pub(super) const BENCH_INDICATORS_PER_PANE: usize = 6;

pub(super) const BENCH_DRAWINGS_PER_PANE: usize = 40;

/// The host clips its published ladder to `LADDER_LEVELS_PER_SIDE`, so
/// asking for more than that measures the clip, not the wire.
pub(super) const BENCH_BOOK_LEVELS_PER_SIDE: i64 = 128;

pub(super) const BENCH_CLOSED_TRADES: usize = 120;

/// Count the core capture's heap work on the fixture its bench times.
pub(super) fn measure_core_capture_heap() -> CaptureHeap {
    const WARMUP_CAPTURES: usize = 25;
    const MEASURED_CAPTURES: usize = 50;

    let (app, _commands) = loaded_observer_workspace(2_000);
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = registry
        .descriptors()
        .map(|descriptor| descriptor.scope_id.clone())
        .collect::<Vec<_>>();
    let instance = observer_instance();
    for _ in 0..WARMUP_CAPTURES {
        drop(registry.capture(&app, &instance, &scopes).unwrap());
    }
    let heap = CaptureHeap::least_of(MEASURED_CAPTURES, || {
        drop(registry.capture(&app, &instance, &scopes).unwrap());
    });
    println!("CONTROL_CORE_CAPTURE_HEAP {heap:?}");
    heap
}

/// Measure the core capture in batches; see [`BestBatches`].
pub(super) fn measure_core_capture_us() -> BestBatches {
    const WARMUP_CAPTURES: usize = 25;
    const MEASURED_CAPTURES: usize = 500;
    const BATCHES: usize = 3;

    let (app, _commands) = loaded_observer_workspace(2_000);
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = registry
        .descriptors()
        .map(|descriptor| descriptor.scope_id.clone())
        .collect::<Vec<_>>();
    let instance = observer_instance();
    for _ in 0..WARMUP_CAPTURES {
        drop(registry.capture(&app, &instance, &scopes).unwrap());
    }
    let mut readings = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let mut elapsed_us = Vec::with_capacity(MEASURED_CAPTURES);
        for _ in 0..MEASURED_CAPTURES {
            drop(registry.capture(&app, &instance, &scopes).unwrap());
            elapsed_us.push(registry.performance().last_capture_us);
        }
        let BatchReading {
            median_us,
            p99_us,
            worst_us,
        } = BatchReading::of(elapsed_us);
        println!(
            "CONTROL_CORE_CAPTURE {{\"capture_median_us\":{median_us},\"capture_p99_us\":{p99_us},\"capture_worst_us\":{worst_us},\"captures\":{MEASURED_CAPTURES}}}"
        );
        readings.push(BatchReading {
            median_us,
            p99_us,
            worst_us,
        });
    }
    BestBatches::of(&readings)
}

/// Count the maximum chart-window capture's heap work on the fixture its
/// bench times.
pub(super) fn measure_max_chart_window_capture_heap() -> CaptureHeap {
    use crate::control::chart::chart_window;

    const WARMUP_CAPTURES: usize = 10;
    const MEASURED_CAPTURES: usize = 20;

    let (app, query) = max_chart_window_fixture();
    let instance = observer_instance();
    for _ in 0..WARMUP_CAPTURES {
        drop(chart_window(&app, &instance, &query, None).unwrap());
    }
    let heap = CaptureHeap::least_of(MEASURED_CAPTURES, || {
        drop(chart_window(&app, &instance, &query, None).unwrap());
    });
    println!("CONTROL_MAX_CHART_WINDOW_CAPTURE_HEAP {heap:?}");
    heap
}

/// The workspace and the query of the largest allowed chart-window page.
pub(super) fn max_chart_window_fixture() -> (QuantickApp, crate::control::chart::ChartWindowQuery) {
    use crate::control::chart::{ChartWindowQuery, ChartWindowRange};
    use quantick_control::{limits::CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS, wire::WireU64};

    let max_page_items = u64::try_from(CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS)
        .expect("the reviewed page limit fits in the wire integer");
    let (app, _commands) = app_with_history(max_page_items);
    let query = ChartWindowQuery {
        tab_id: WireU64::new(app.runtime.tabs.active_id()),
        pane_id: WireU64::new(app.active_tab().flow_pane.id),
        range: ChartWindowRange::Slots {
            start_slot: WireU64::new(0),
            end_slot_exclusive: WireU64::new(max_page_items),
        },
        page_size: CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
    };
    (app, query)
}

/// Measure the maximum chart-window capture (the reviewed 32-bar page) in
/// batches; see [`BestBatches`].
pub(super) fn measure_max_chart_window_capture_us() -> BestBatches {
    use crate::control::chart::chart_window;
    use quantick_control::limits::CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS;

    const WARMUP_CAPTURES: usize = 10;
    const MEASURED_CAPTURES: usize = 100;
    const BATCHES: usize = 3;

    let (app, query) = max_chart_window_fixture();
    let instance = observer_instance();
    for _ in 0..WARMUP_CAPTURES {
        drop(chart_window(&app, &instance, &query, None).unwrap());
    }
    let mut readings = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        let mut elapsed_us = Vec::with_capacity(MEASURED_CAPTURES);
        for _ in 0..MEASURED_CAPTURES {
            let started = std::time::Instant::now();
            drop(chart_window(&app, &instance, &query, None).unwrap());
            elapsed_us.push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
        }
        let BatchReading {
            median_us,
            p99_us,
            worst_us,
        } = BatchReading::of(elapsed_us);
        println!(
            "CONTROL_MAX_CHART_WINDOW_CAPTURE {{\"capture_median_us\":{median_us},\"capture_p99_us\":{p99_us},\"capture_worst_us\":{worst_us},\"captures\":{MEASURED_CAPTURES},\"bars\":{CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS}}}"
        );
        readings.push(BatchReading {
            median_us,
            p99_us,
            worst_us,
        });
    }
    BestBatches::of(&readings)
}

/// One capture of the scene scope, as a client would read it.
pub(super) fn observer_scene(app: &QuantickApp) -> serde_json::Value {
    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("scene.controls");
    let capture = registry
        .capture(app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    capture.scopes[&scope].value.clone()
}

/// The scene's control IDs, in the order it reports them.
pub(super) fn scene_control_ids(scene: &serde_json::Value) -> Vec<String> {
    scene["controls"]
        .as_array()
        .expect("the scene reports a control list")
        .iter()
        .map(|control| control["control_id"].as_str().unwrap().to_owned())
        .collect()
}

pub(super) fn scene_control<'a>(scene: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    scene["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|control| control["control_id"] == id)
        .unwrap_or_else(|| panic!("the scene names {id}"))
}

/// The scopes a bundle is captured over in these tests: enough to explain
/// a session, and including the scene, which is what a screenshot
/// correlates against.
pub(super) const EVIDENCE_TEST_SCOPES: [&str; 6] = [
    "system.info",
    "workspace.summary",
    "feed.status",
    "chart.summary",
    "health.summary",
    "scene.controls",
];

/// One run of resource bytes, decoded through the contract's own wire
/// type rather than a second base64 spelling in this test.
pub(super) fn decode_wire_base64(encoded: &str) -> Vec<u8> {
    quantick_control::wire::Base64Bytes::new(encoded)
        .expect("the gateway encodes valid base64")
        .decode()
        .expect("what encodes decodes")
}

/// Page a retained bundle back through its own cursor and return the
/// canonical bytes the chunks reassemble into, with the manifest that
/// described them.
pub(super) fn read_evidence_bundle(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    manifest: &serde_json::Value,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut cursor = serde_json::Value::Null;
    loop {
        let payload = if cursor.is_null() {
            serde_json::json!({ "evidence_id": manifest["evidence_id"] })
        } else {
            serde_json::json!({ "evidence_id": manifest["evidence_id"], "cursor": cursor })
        };
        let response = remote_call(app, ctx, client, "evidence.read", payload);
        let page = success_result(&response);
        for chunk in page["page"]["items"].as_array().unwrap() {
            bytes.extend(decode_wire_base64(chunk["data"].as_str().unwrap()));
        }
        match page["page"]["next_cursor"].clone() {
            serde_json::Value::Null => break,
            next => cursor = next,
        }
    }
    assert_eq!(
        bytes.len(),
        manifest["encoded_bytes"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        "the pages reassemble to exactly the bundle the manifest describes"
    );
    bytes
}

/// One capture that wants a picture, served the way the window serves one:
/// the request parks, the frame delivers the pixels, the capture runs.
///
/// There is no shortcut past the parking. An image is worth exactly one
/// frame — `begin_frame` drops whatever no capture claimed — so a fixture
/// published before the request was sent would be gone by the time it
/// arrived, which is the staleness rule doing its job.
pub(super) fn capture_with_screenshot(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    payload: serde_json::Value,
    image: crate::control::RawScreenshot,
) -> quantick_control::wire::ResponseEnvelope {
    let request_id = client
        .send("evidence.capture", payload)
        .expect("the request is sent");
    // Wait for the capture to actually park before handing over pixels.
    // Not a nicety: an image is worth one frame, so publishing before the
    // request has parked would have the frame drop it and the capture then
    // wait for one that never comes. Asserted rather than assumed, so a
    // machine slow enough to break the precondition says so here instead
    // of failing later on a confusing assertion about the manifest.
    run_frames_until_capture_parks(app, ctx);
    let mut access = app
        .control
        .control_access
        .take()
        .expect("control access is installed");
    access.publish_screenshot_for_test(app, image);
    app.control.control_access = Some(access);
    for _ in 0..REPLY_WAIT_FRAMES {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, request_id);
    response
}

/// Run application frames until a capture has parked on an image, and return
/// how many are parked.
///
/// Bounded by time, not by frames. The request crosses a socket and the
/// gateway's reader thread before any frame can park it, and on a loaded
/// machine a fixed count of back-to-back frames runs out before that thread
/// has run at all (#416). The sleep between frames hands it the core.
pub(super) fn run_frames_until_capture_parks(app: &mut QuantickApp, ctx: &egui::Context) -> usize {
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        run_frame(app, ctx);
        let parked = app
            .control
            .control_access
            .as_ref()
            .expect("control access is installed")
            .awaiting_screenshot_for_test();
        if parked > 0 {
            return parked;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the capture did not park for an image within {GATEWAY_TEST_WAIT:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Frames a test spends waiting for the gateway's reply.
pub(super) const REPLY_WAIT_FRAMES: usize = 600;

/// A window's worth of pixels a PNG encoder cannot shrink.
///
/// The ramp below deflates to a few kilobytes, which is the wrong fixture
/// for anything about size: it makes every bundle a single chunk and hides
/// the paging path entirely. This one is noise from a fixed generator —
/// reproducible, and it compresses to nothing.
pub(super) fn incompressible_screenshot(width: u32, height: u32) -> crate::control::RawScreenshot {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..(width as usize * height as usize) {
        // xorshift64*: no dependency, and the same bytes on every machine.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let sample = state.wrapping_mul(0x2545_F491_4F6C_DD1D).to_le_bytes();
        rgba.extend_from_slice(&[sample[0], sample[1], sample[2], 0xff]);
    }
    crate::control::RawScreenshot {
        window_id: None,
        source_window_id: None,
        width_px: width,
        height_px: height,
        pixels_per_point: 1.0,
        rgba: crate::control::ScreenshotPixels::new(move || rgba),
    }
}

/// A window's worth of opaque pixels, the shape the platform hands over.
pub(super) fn test_screenshot(width: u32, height: u32) -> crate::control::RawScreenshot {
    crate::control::RawScreenshot {
        window_id: None,
        source_window_id: None,
        width_px: width,
        height_px: height,
        pixels_per_point: 1.0,
        rgba: crate::control::ScreenshotPixels::new(move || {
            (0..(width as usize * height as usize))
                .flat_map(|index| {
                    let shade = u8::try_from(index % 251).unwrap_or(0);
                    [shade, 0x20, 0x30, 0xff]
                })
                .collect()
        }),
    }
}
