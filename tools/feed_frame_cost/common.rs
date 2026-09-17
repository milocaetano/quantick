use super::*;
include!("f2_frame_adapter.rs");
// Actual renderer correctness before sampling, without a stopwatch or verdict.
#[test]
fn f2_frame_geometry() {
    for size in [egui::vec2(1440.0, 900.0), egui::vec2(1000.0, 700.0)] {
        let (mut app, _events, _commands, _book) = setup();
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(1.5);
        let mut output = None;
        for _ in 0..2 {
            output = Some(run_frame_sized(
                &mut app,
                &ctx,
                size,
                Vec::new(),
                egui::Modifiers::NONE,
            ));
        }
        let output = output.unwrap();
        assert_paint(&app, &painted_text(&output));
        let whole = panel_geometry(&app, &ctx, &output);
        println!(
            "\nF2_FRAME_GEOMETRY {}",
            serde_json::json!({
                "mode":"whole", "size":[size.x,size.y], "scale":ctx.pixels_per_point(),
                "panel_geometry":whole, "delivery_text":delivery_text(&app)
            })
        );
        if delivery_text(&app).is_empty() {
            assert!(whole.is_null());
            continue;
        }
        assert!(!whole.is_null());
        let mut reference_geometry = None;
        for reference in [false, true] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(1.5);
            prime_panel(&app, &ctx, size);
            let output = panel_proof(&app, &ctx, size, reference);
            assert_paint(&app, &painted_text(&output));
            let geometry = panel_geometry(&app, &ctx, &output);
            if let Some(expected) = &reference_geometry {
                assert_eq!(expected, &geometry, "actual and reference paint geometry");
            } else {
                reference_geometry = Some(geometry.clone());
            }
            println!(
                "\nF2_FRAME_GEOMETRY {}",
                serde_json::json!({
                    "mode":if reference {"panel_reference"} else {"panel_actual"},
                    "size":[size.x,size.y], "scale":ctx.pixels_per_point(),
                    "panel_geometry":geometry, "delivery_text":delivery_text(&app)
                })
            );
        }
    }
}

const WARMUP: u64 = 30;
const FRAMES: u64 = 600;
const PRINTS: u64 = 64;
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037, |n, b| {
        (n ^ u64::from(*b)).wrapping_mul(1099511628211)
    })
}
fn dimensions() -> egui::Vec2 {
    match std::env::var("F2_FRAME_SIZE").unwrap().as_str() {
        "normal" => egui::vec2(1440.0, 900.0),
        "narrow" => egui::vec2(1000.0, 700.0),
        _ => panic!("unregistered frame size"),
    }
}
fn setup() -> (
    QuantickApp,
    mpsc::Sender<ProtocolEvent>,
    mpsc::Receiver<FeedCommand>,
    mpsc::Sender<DepthEvent>,
) {
    let (mut app, _old_events, _old_commands, _old_book) = test_app();
    let (events, receiver) = mpsc::channel(64);
    let (book, book_events) = mpsc::channel(64);
    let (commands, commands_rx) = mpsc::channel(16);
    attach(&mut app, receiver, book_events, commands);
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(16));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    events
        .try_send(wrap(FeedEvent::Backfilled(
            (1..=8_000).map(trade).collect(),
        )))
        .unwrap();
    for _ in 0..3 {
        events
            .try_send(wrap(FeedEvent::Continuity(quantick_feed::FeedContinuity {
                gap: None,
                missing_messages: None,
                non_monotonic: false,
            })))
            .unwrap();
    }
    send_exclusions(&events);
    app.active_tab_mut().drain_feed();
    assert_eq!(app.active_tab().feed_integrity.unknown_loss, 3);
    assert_eq!(app.active_tab().feed_integrity.missing_messages, 0);
    assert_eq!(app.active_tab().feed_integrity.non_monotonic, 0);
    assert_delivery(&app);
    (app, events, commands_rx, book)
}
#[test]
#[ignore = "external F2 fixed frame protocol; requires finite execution decision"]
fn f2_frame_sample() {
    let size = dimensions();
    let mode = std::env::var("F2_FRAME_MODE").unwrap();
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(1.5);
    let (mut app, events, _commands, _book) = setup();
    let mut samples = Vec::with_capacity(FRAMES as usize);
    let mut last_output = None;
    let mut next = 8_001;
    let mut throughput_started = Instant::now();
    for frame in 0..WARMUP + FRAMES {
        if frame == WARMUP {
            throughput_started = Instant::now();
        }
        let output;
        let elapsed;
        if mode == "whole" {
            events
                .try_send(wrap(FeedEvent::LiveBatch(
                    (next..next + PRINTS).map(trade).collect(),
                )))
                .unwrap();
            next += PRINTS;
            let started = Instant::now();
            output = run_frame_sized(&mut app, &ctx, size, Vec::new(), egui::Modifiers::NONE);
            elapsed = started.elapsed().as_nanos();
        } else {
            assert!(mode == "panel_actual" || mode == "panel_reference");
            // Both sides prime the real cache once before timing. This also keeps
            // font setup outside the fixed warmup/measured call sequence.
            if frame == 0 {
                prime_panel(&app, &ctx, size);
            }
            let (painted, ns) = panel_frame(&app, &ctx, size, mode == "panel_reference");
            output = painted;
            elapsed = ns;
        }
        if frame >= WARMUP {
            samples.push(elapsed);
        }
        // Retain only the final real output. Drop occurs after timed update.
        if frame + 1 == WARMUP + FRAMES {
            last_output = Some(output);
        }
    }
    let throughput_ns = throughput_started.elapsed().as_nanos();
    let output = last_output.unwrap();
    let texts = painted_text(&output);
    assert_paint(&app, &texts);
    assert!(!output.shapes.is_empty());
    let state = &app.active_tab().flow_pane.state;
    let expected_count = if mode == "whole" {
        8_000 + (WARMUP + FRAMES) * PRINTS
    } else {
        8_000
    };
    assert_eq!(state.trades().len(), expected_count as usize);
    for (index, actual) in state.trades().iter().enumerate() {
        assert_eq!(actual, &trade(index as u64 + 1));
    }
    assert_eq!(state.bars().len(), expected_count as usize / 16);
    assert!(state.partial().is_none());
    let rect = app.active_tab().flow_pane.frame.chart_rect;
    if mode == "whole" {
        assert!(rect.is_some_and(|r| r.width() > 0.0 && r.height() > 0.0));
    }
    let geometry = rect.map(|r| [r.min.x, r.min.y, r.max.x, r.max.y]);
    let facts = serde_json::json!({
        "trades":state.trades().len(),"bars":state.bars().len(),
        "tape_digest":digest(format!("{:?}",state.trades().iter().collect::<Vec<_>>()).as_bytes()),
        "bars_digest":digest(format!("{:?}",state.bars()).as_bytes()),
        "partial_digest":digest(format!("{:?}",state.partial()).as_bytes()),
        "unknown":app.active_tab().feed_integrity.unknown_loss,
    });
    println!(
        "\nF2_FRAME_SAMPLE {}",
        serde_json::json!({
            "mode":mode,"size":[size.x,size.y],"scale":ctx.pixels_per_point(),
            "samples_ns":samples,"elapsed_ns":samples.iter().sum::<u128>(),
            "throughput_elapsed_ns":throughput_ns,"facts":facts,"chart_rect":geometry,
            "panel_geometry":panel_geometry(&app,&ctx,&output),
            "shape_count":output.shapes.len(),"delivery_text":delivery_text(&app),
            "frames":FRAMES,"warmup_frames":WARMUP,"prints_per_frame":PRINTS,
        })
    );
}
