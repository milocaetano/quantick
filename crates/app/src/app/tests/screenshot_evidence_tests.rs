//! Synthetic geometry-contract evidence, not execution at an OS DPI setting.
//!
//! Only the raster and recorded pane rectangles are fixtures. Parking, serving,
//! projection, PNG encoding, retention and socket reads use the shipped path.

use super::*;

type Json = serde_json::Value;

fn capture_fixture(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    image: crate::control::RawScreenshot,
    rectangles: [Option<egui::Rect>; 3],
) -> Json {
    let request_id = client
        .send(
            "evidence.capture",
            serde_json::json!({ "scopes": EVIDENCE_TEST_SCOPES, "screenshot": true }),
        )
        .unwrap();
    run_frames_until_capture_parks(app, ctx);
    let tab = app.active_tab_mut();
    assert_eq!(tab.time_panes.len(), 2);
    tab.time_panes[0].frame.chart_area = rectangles[0];
    tab.time_panes[1].frame.chart_area = rectangles[1];
    tab.flow_pane.frame.chart_area = rectangles[2];
    let mut access = app.control.control_access.take().unwrap();
    access.publish_screenshot_for_test(app, image);
    // Serve before another draw overwrites the deliberately synthetic bounds.
    // This is the same production frame service that claims a real egui image.
    access.begin_frame(app, ctx);
    app.control.control_access = Some(access);
    for _ in 0..REPLY_WAIT_FRAMES {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().unwrap();
    assert_eq!(response.request_id, request_id);
    assert!(app.surfaces.toast.message().is_some());
    success_result(&response).clone()
}

fn rectangle(x: f32, y: f32, width: f32, height: f32) -> Option<egui::Rect> {
    Some(egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(width, height),
    ))
}

fn canvas_ids(app: &QuantickApp) -> [String; 3] {
    let tab = app.active_tab();
    assert_eq!(
        tab.time_panes.len(),
        2,
        "both fixture time panes must be built"
    );
    [tab.time_panes[0].id, tab.time_panes[1].id, tab.flow_pane.id]
        .map(|id| format!("pane.{id}.canvas"))
}

fn assert_bundle(manifest: &Json, bytes: &[u8]) -> (Json, Vec<u8>) {
    assert_eq!(
        quantick_control::canonical::raw_digest(bytes),
        manifest["content_digest"].as_str().unwrap()
    );
    let document: Json = serde_json::from_slice(bytes).unwrap();
    for field in [
        "evidence_id",
        "instance_id",
        "session_id",
        "capture_revision",
    ] {
        assert_eq!(document[field], manifest[field], "association: {field}");
    }
    for field in ["instance_id", "capture_revision"] {
        assert_eq!(document["snapshot"][field], manifest[field]);
    }
    let descriptor = &document["screenshot"]["descriptor"];
    assert_eq!(descriptor, &manifest["screenshot"]);
    assert_eq!(descriptor["capture_revision"], manifest["capture_revision"]);
    assert_eq!(descriptor["format"], "png");
    assert_eq!(document["coverage"], manifest["coverage"]);
    assert!(
        document["coverage"]["not_captured"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!({
                "subject": "screenshot.state_skew",
                "reason": "pixels_precede_projections_by_one_drain"
            }))
    );
    let png = decode_wire_base64(document["screenshot"]["image_base64"].as_str().unwrap());
    assert_eq!(
        quantick_control::canonical::raw_digest(&png),
        descriptor["image_digest"].as_str().unwrap()
    );
    assert_eq!(descriptor["image_bytes"], png.len().to_string());
    let mut reader = png::Decoder::new(std::io::Cursor::new(&png))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let decoded = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(descriptor["width_px"], decoded.width);
    assert_eq!(descriptor["height_px"], decoded.height);
    assert_eq!(decoded.color_type, png::ColorType::Rgba);
    assert_eq!(decoded.bit_depth, png::BitDepth::Eight);
    pixels.truncate(decoded.buffer_size());
    (document, pixels)
}

fn assert_id_partition(document: &Json) {
    use std::collections::BTreeSet;
    let controls = document["snapshot"]["scopes"]["scene.controls"]["value"]["controls"]
        .as_array()
        .unwrap();
    let descriptor = &document["screenshot"]["descriptor"];
    let regions = descriptor["control_regions"].as_array().unwrap();
    let gaps = descriptor["controls_without_region"].as_array().unwrap();
    let mut seen = BTreeSet::new();
    for control in controls {
        let id = control["control_id"].as_str().unwrap();
        assert!(seen.insert(id), "duplicate scene ID: {id}");
        if control["bounds"].is_object() {
            assert_eq!(regions.iter().filter(|r| r["control_id"] == id).count(), 1);
            assert!(!gaps.iter().any(|gap| gap["subject"] == id));
        } else {
            assert!(!regions.iter().any(|r| r["control_id"] == id));
            let expected = serde_json::json!({
                "subject": id,
                "reason": control["bounds_availability"]["reason"]
            });
            assert_eq!(gaps.iter().filter(|gap| **gap == expected).count(), 1);
        }
    }
    assert_eq!(regions.len() + gaps.len(), controls.len());
}

#[test]
fn synthetic_fractional_geometry_round_trips_original_png_and_clipped_regions() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(10);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    // Production intentionally builds one pending context pane per frame.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let ids = canvas_ids(&app);
    let directory = gateway_test_directory("fractional-evidence-geometry");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut image = test_screenshot(300, 180);
    image.pixels_per_point = 1.5;
    let manifest = capture_fixture(
        &mut app,
        &ctx,
        &mut client,
        image,
        [
            rectangle(10.0, 20.0, 80.0, 40.0),
            rectangle(180.0, 100.0, 40.0, 30.0),
            rectangle(-4.0, 8.0, 20.0, 10.0),
        ],
    );
    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    assert_eq!(
        bytes,
        read_evidence_bundle(&mut app, &ctx, &mut client, &manifest)
    );
    let (document, pixels) = assert_bundle(&manifest, &bytes);
    let expected_pixels: Vec<u8> = (0..300 * 180)
        .flat_map(|index| [(index % 251) as u8, 0x20, 0x30, 0xff])
        .collect();
    assert_eq!(
        pixels, expected_pixels,
        "production PNG preserves every fixture pixel"
    );
    assert_eq!(manifest["screenshot"]["pixels_per_point"], "1.5");
    assert_id_partition(&document);
    let controls = document["snapshot"]["scopes"]["scene.controls"]["value"]["controls"]
        .as_array()
        .unwrap();
    let regions = manifest["screenshot"]["control_regions"]
        .as_array()
        .unwrap();
    // These expected strings are literals, not results from application helpers.
    let expected = [
        (["10", "20", "80", "40"], ["15", "30", "120", "60"], true),
        (
            ["180", "100", "40", "30"],
            ["270", "150", "60", "45"],
            false,
        ),
        (["-4", "8", "20", "10"], ["-6", "12", "30", "15"], false),
    ];
    for (id, (points, pixels, within)) in ids.iter().zip(expected) {
        let control = controls.iter().find(|c| c["control_id"] == *id).unwrap();
        let region = regions.iter().find(|r| r["control_id"] == *id).unwrap();
        for (field, expected) in ["x_pt", "y_pt", "width_pt", "height_pt"]
            .into_iter()
            .zip(points)
        {
            assert_eq!(control["bounds"][field], expected);
        }
        for (field, expected) in ["x_px", "y_px", "width_px", "height_px"]
            .into_iter()
            .zip(pixels)
        {
            assert_eq!(region[field], expected);
        }
        assert_eq!(region["within_image"], within);
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn synthetic_precision_uses_published_scale_and_preserves_missing_bounds_codes() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(6);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    // Production intentionally builds one pending context pane per frame.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let ids = canvas_ids(&app);
    let directory = gateway_test_directory("fractional-evidence-precision");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut image = test_screenshot(300, 180);
    image.pixels_per_point = 1.504;
    let manifest = capture_fixture(
        &mut app,
        &ctx,
        &mut client,
        image,
        [
            None,
            rectangle(0.0, 0.0, -1.0, 10.0),
            rectangle(1000.0, 10.0, 20.0, 20.0),
        ],
    );
    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    let (document, _) = assert_bundle(&manifest, &bytes);
    assert_id_partition(&document);
    let descriptor = &manifest["screenshot"];
    assert_eq!(descriptor["pixels_per_point"], "1.5");
    let region = descriptor["control_regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["control_id"] == ids[2])
        .unwrap();
    assert_eq!(
        region,
        &serde_json::json!({
            "control_id": ids[2], "x_px": "1500", "y_px": "15",
            "width_px": "30", "height_px": "30", "within_image": false
        })
    );
    let controls = document["snapshot"]["scopes"]["scene.controls"]["value"]["controls"]
        .as_array()
        .unwrap();
    let control = controls.iter().find(|c| c["control_id"] == ids[2]).unwrap();
    assert_eq!(
        control["bounds"],
        serde_json::json!({
            "x_pt": "1000", "y_pt": "10", "width_pt": "20", "height_pt": "20"
        })
    );
    for (id, reason) in ids.iter().zip([
        "the_pane_has_not_been_drawn_yet",
        "the_panes_rectangle_is_not_a_reportable_number",
    ]) {
        assert!(
            descriptor["controls_without_region"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!({"subject": id, "reason": reason}))
        );
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn synthetic_fractional_image_retains_all_chunks_and_full_digest() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(6);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("fractional-evidence-chunks");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut image = incompressible_screenshot(512, 512);
    image.pixels_per_point = 1.5;
    let response = capture_with_screenshot(
        &mut app,
        &ctx,
        &mut client,
        serde_json::json!({"scopes": EVIDENCE_TEST_SCOPES, "screenshot": true}),
        image,
    );
    let manifest = success_result(&response).clone();
    assert!(manifest["chunk_count"].as_u64().unwrap() > 1);
    assert_eq!(manifest["screenshot"]["pixels_per_point"], "1.5");
    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    let (_, pixels) = assert_bundle(&manifest, &bytes);
    assert_eq!(pixels.len(), 512 * 512 * 4);
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}
