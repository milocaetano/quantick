//! Real gateway admission and per-viewport paint/image correlation.
use super::*;
use quantick_control::wire::ResponseOutcome;
use quantick_control_handlers::native_window::NativeWindowHost;
use quantick_workspace::arrangement::{
    TabId,
    windows::{TransferTarget, WindowId},
};
use serde_json::json;
use std::sync::Arc;

fn input(window: WindowId, events: Vec<egui::Event>) -> egui::RawInput {
    let viewport = super::super::native_windows::viewport(window);
    let mut input = egui::RawInput {
        viewport_id: viewport,
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        events,
        ..Default::default()
    };
    input
        .viewports
        .entry(viewport)
        .or_default()
        .native_pixels_per_point = Some(1.0);
    input
}
fn service(app: &mut QuantickApp, ctx: &egui::Context) {
    let mut access = app.control.control_access.take().unwrap();
    access.begin_frame(app, ctx);
    app.control.control_access = Some(access);
}
fn admit(app: &mut QuantickApp, ctx: &egui::Context, captures: usize) {
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    while app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .awaiting_screenshot_for_test()
        < captures
    {
        assert!(
            std::time::Instant::now() < deadline,
            "capture did not reach UI admission"
        );
        service(app, ctx);
    }
}
fn paint(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    window: WindowId,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let now = frame_instant(ctx);
    ctx.run(input(window, events), |ctx| {
        super::super::native_windows::paint(app, ctx, window, now)
    })
}
fn screenshot(window: WindowId, color: egui::Color32) -> egui::Event {
    egui::Event::Screenshot {
        viewport_id: super::super::native_windows::viewport(window),
        image: Arc::new(egui::ColorImage::new([32, 24], color)),
    }
}

#[test]
fn parked_captures_pin_focus_and_reject_interleaved_other_viewport_pixels() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let root_tab = app.runtime.tabs.id_at(0);
    let child_tab = app.runtime.tabs.id_at(1);
    let child = app
        .runtime
        .tabs
        .transfer_tab(TabId(child_tab), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    app.runtime.tabs.focus_window(WindowId::ROOT).unwrap();
    let directory = gateway_test_directory("pinned-window-capture");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let first = client
        .send(
            "evidence.capture",
            json!({"scopes":["scene.controls"],"screenshot":true}),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    admit(&mut app, &ctx, 1);
    app.runtime.tabs.focus_window(child).unwrap();
    let second = client
        .send(
            "evidence.capture",
            json!({"scopes":["scene.controls"],"screenshot":true,"window_id":child.0.to_string()}),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    admit(&mut app, &ctx, 2);
    paint(&mut app, &ctx, WindowId::ROOT, vec![]);
    let output = paint(&mut app, &ctx, child, vec![]);
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::Shape::Callback(_))),
        "a pending immediate child capture installs a real renderer callback"
    );
    paint(
        &mut app,
        &ctx,
        WindowId::ROOT,
        vec![screenshot(child, egui::Color32::GREEN)],
    );
    paint(
        &mut app,
        &ctx,
        child,
        vec![screenshot(WindowId::ROOT, egui::Color32::RED)],
    );
    assert_eq!(
        app.control
            .control_access
            .as_ref()
            .unwrap()
            .awaiting_screenshot_for_test(),
        2,
        "foreign viewport events cannot complete either capture"
    );
    // Correct images arrive in the opposite order to request admission.
    paint(
        &mut app,
        &ctx,
        child,
        vec![screenshot(child, egui::Color32::GREEN)],
    );
    paint(
        &mut app,
        &ctx,
        WindowId::ROOT,
        vec![screenshot(WindowId::ROOT, egui::Color32::RED)],
    );
    service(&mut app, &ctx);
    let replies = [client.read().unwrap(), client.read().unwrap()];
    for reply in replies {
        let (window, tab, color) = if reply.request_id == first {
            (0, root_tab, [255, 0, 0, 255])
        } else {
            assert_eq!(reply.request_id, second);
            (child.0, child_tab, [0, 255, 0, 255])
        };
        let manifest = success_result(&reply);
        assert_eq!(manifest["window_id"], window.to_string());
        assert_eq!(manifest["screenshot"]["window_id"], window.to_string());
        assert_eq!(
            manifest["screenshot"]["source_window_id"],
            window.to_string()
        );
        let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
        let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let scene = &document["snapshot"]["scopes"]["scene.controls"]["value"];
        assert_eq!(scene["window_id"], window.to_string());
        assert_eq!(scene["active_tab_id"], tab.to_string());
        let chips: Vec<_> = scene["controls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|control| {
                control["control_id"]
                    .as_str()
                    .unwrap()
                    .starts_with("tab_strip.tab.")
            })
            .collect();
        assert_eq!(
            chips.len(),
            1,
            "only the target window's market chip is listed"
        );
        let png = decode_wire_base64(document["screenshot"]["image_base64"].as_str().unwrap());
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        assert_eq!(&pixels[..4], &color);
    }
    assert_eq!(
        app.runtime.tabs.windows().focused(),
        child,
        "capture must not change native focus"
    );
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn missing_and_closed_pinned_windows_are_refused_without_retargeting() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let child_tab = app.runtime.tabs.id_at(1);
    let child = app
        .runtime
        .tabs
        .transfer_tab(TabId(child_tab), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    app.runtime.tabs.focus_window(WindowId::ROOT).unwrap();
    let directory = gateway_test_directory("closed-window-capture");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    client
        .send(
            "evidence.capture",
            json!({"scopes":["scene.controls"],"screenshot":true}),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    admit(&mut app, &ctx, 1);
    app.runtime.tabs.close_window(WindowId::ROOT).unwrap();
    app.present_capture_window(quantick_control::wire::WireU64::new(child.0))
        .unwrap();
    service(&mut app, &ctx);
    let refused = client.read().unwrap();
    assert!(matches!(refused.outcome, ResponseOutcome::Failure { .. }));
    assert!(
        serde_json::to_string(&refused)
            .unwrap()
            .contains("market_window.missing_window")
    );
    for capability in ["snapshot.read", "scene.read", "evidence.capture"] {
        let mut payload = json!({"window_id":"999999"});
        if capability != "scene.read" {
            payload["scopes"] = json!(["scene.controls"]);
        }
        let response = remote_call(&mut app, &ctx, &mut client, capability, payload);
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
    }
    assert_eq!(app.runtime.tabs.windows().focused(), child);
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn targeted_scene_and_snapshot_readback_observe_native_state_and_leave_home_on_select() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let tab = app.runtime.tabs.id_at(1);
    let child = app
        .runtime
        .tabs
        .transfer_tab(TabId(tab), TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    app.present_capture_window(quantick_control::wire::WireU64::new(child.0))
        .unwrap();
    quantick_control_handlers::home_tab::HomeTabHost::show_home_tab(&mut app).unwrap();
    app.operate_market_tabs(
        quantick_control_schema::market_windows::TabOperation::Select(
            quantick_control_schema::market_windows::SelectInput {
                tab_id: quantick_control::wire::WireU64::new(tab),
                window_id: quantick_control::wire::WireU64::new(child.0),
            },
        ),
    )
    .unwrap();
    app.present_capture_window(quantick_control::wire::WireU64::new(child.0))
        .unwrap();
    assert!(!app.chrome.title_bar.surface.is_home());
    let directory = gateway_test_directory("targeted-window-state-readback");
    grant_annotate_for_test(&mut app, "all-reads");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    for capability in ["scene.read", "snapshot.read"] {
        let mut payload = json!({"window_id":child.0.to_string()});
        if capability == "snapshot.read" {
            payload["scopes"] = json!(["scene.controls"]);
        }
        client.send(capability, payload).unwrap();
        wait_for_queued_gateway_requests(&app, 1);
        service(&mut app, &ctx);
        for _ in 0..REPLY_WAIT_FRAMES {
            service(&mut app, &ctx);
            let mut raw = input(child, vec![]);
            raw.viewports
                .get_mut(&super::super::native_windows::viewport(child))
                .unwrap()
                .maximized = Some(true);
            let now = frame_instant(&ctx);
            let _ = ctx.run(raw, |ctx| {
                super::super::native_windows::paint(&mut app, ctx, child, now)
            });
            if client.reply_pending(std::time::Duration::from_millis(5)) {
                break;
            }
        }
        let result = success_result(&client.read().unwrap());
        assert_eq!(result["window_id"], child.0.to_string());
        let scene = &result["scopes"]["scene.controls"]["value"];
        assert_eq!(scene["active_tab_id"], tab.to_string());
        assert_eq!(scene_control(scene, "tab_strip.maximize")["selected"], true);
    }
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn ordinary_painted_reads_keep_bounded_admission_without_using_image_slots() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let directory = gateway_test_directory("ordinary-paint-read-batch");
    enable_test_gateway(&mut app, &ctx, &directory, 16);
    let mut first =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut second =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    for _ in 0..4 {
        first.send("scene.read", json!({})).unwrap();
        second.send("scene.read", json!({})).unwrap();
    }
    wait_for_queued_gateway_requests(&app, 8);
    drain_gateway_requests(&mut app, &ctx);
    for client in [&mut first, &mut second] {
        for _ in 0..4 {
            let response = client.read().unwrap();
            assert!(
                matches!(response.outcome, ResponseOutcome::Success { .. }),
                "{response:?}"
            );
        }
    }
    assert_eq!(
        app.control
            .control_access
            .as_ref()
            .unwrap()
            .awaiting_screenshot_for_test(),
        0
    );
    disable_test_gateway(&mut app, &ctx);
}
