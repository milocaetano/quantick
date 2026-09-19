use super::*;

fn payload_with_rows(rows: &[(i64, i64)]) -> FrvpPayload {
    FrvpPayload {
        width_frac: 0.5,
        show_labels: false,
        show_poc: false,
        show_value_area: false,
        cache: Some(refreshed_cache(rows, 19)),
        ..FrvpPayload::default()
    }
}

fn context<'a>(payload: &'a FrvpPayload, scale: &'a PriceScale) -> DrawContext<'a> {
    DrawContext {
        payload,
        anchors: &[],
        scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: false,
        halo: false,
        content_editing: false,
    }
}

fn hit(payload: &FrvpPayload, scale: &PriceScale, x: f32, price: f64) -> bool {
    TOOL.hit_test(
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 500.0)),
        &[egui::pos2(100.0, 0.0), egui::pos2(500.0, 400.0)],
        egui::pos2(x, scale.y(price)),
        10.0,
        &context(payload, scale),
    )
}

#[test]
fn precise_profile_hit_uses_row_widths_and_leaves_missing_rows_empty() {
    let payload = payload_with_rows(&[(100, 10), (102, 2), (104, 6)]);
    for inverted in [false, true] {
        let scale = PriceScale::from_range(99.0, 106.0, 0.0, 490.0).with_inverted(inverted);
        assert!(hit(&payload, &scale, 125.0, 102.5), "inside the short row");
        assert!(
            !hit(&payload, &scale, 180.0, 102.5),
            "beyond its 40 px width"
        );
        assert!(
            !hit(&payload, &scale, 150.0, 101.5),
            "missing bucket passes through"
        );
        assert!(!hit(&payload, &scale, 350.0, 102.5), "empty range middle");
        assert!(
            hit(&payload, &scale, 100.0, 101.5),
            "range border remains a target"
        );
        assert!(
            !hit(&payload, &scale, 94.0, 101.5),
            "six pixels away is too far"
        );
    }
}

#[test]
fn precise_profile_subpixel_rows_keep_their_full_painted_height() {
    let payload = payload_with_rows(&[(100, 10), (104, 2)]);
    for inverted in [false, true] {
        let scale = PriceScale::from_range(0.0, 1000.0, 0.0, 100.0).with_inverted(inverted);
        // One price unit is 0.1 px, but the wide row paints a full pixel.
        assert!(hit(&payload, &scale, 250.0, 107.0));
        assert!(!hit(&payload, &scale, 250.0, 112.0));
        assert!(!hit(&payload, &scale, 350.0, 107.0));
    }
}

#[test]
fn precise_profile_hit_tracks_visible_poc_and_value_area_lines() {
    let mut payload = payload_with_rows(&[(100, 10), (102, 2), (104, 6)]);
    let scale = PriceScale::from_range(99.0, 106.0, 0.0, 490.0);
    assert!(!hit(&payload, &scale, 400.0, 100.5));
    payload.show_poc = true;
    assert!(hit(&payload, &scale, 400.0, 100.5));
    assert!(!hit(&payload, &scale, 400.0, 100.7));
    let area = payload
        .cache
        .as_ref()
        .unwrap()
        .output().profile
        .as_ref()
        .unwrap()
        .1
        .unwrap();
    for price in [area.val as f64, (area.vah + 1) as f64] {
        payload.show_value_area = false;
        assert!(!hit(&payload, &scale, 400.0, price));
        payload.show_value_area = true;
        assert!(hit(&payload, &scale, 400.0, price));
    }
}

#[test]
fn precise_profile_hit_empty_cache_has_only_drawn_borders() {
    let payload = FrvpPayload::default();
    let scale = PriceScale::from_range(99.0, 106.0, 0.0, 490.0);
    assert!(!hit(&payload, &scale, 150.0, 102.5));
    assert!(hit(&payload, &scale, 100.0, 102.5));
}

#[test]
fn precise_profile_outline_interior_passes_through_but_its_staircase_hits() {
    let mut payload = payload_with_rows(&[(100, 10), (102, 2), (103, 6)]);
    assert!(payload.cache.is_some());
    payload.heat_first_slot = Some(15);
    let chart = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(800.0, 500.0));
    let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 104.0)];
    let points = [egui::pos2(100.0, 0.0), egui::pos2(500.0, 400.0)];
    for inverted in [false, true] {
        let scale = PriceScale::from_range(99.0, 106.0, 0.0, 490.0).with_inverted(inverted);
        let ctxt = DrawContext {
            anchors: &anchors,
            ..context(&payload, &scale)
        };
        let hit =
            |x, price| TOOL.hit_test(chart, &points, egui::pos2(x, scale.y(price)), 10.0, &ctxt);
        assert!(hit(150.0, 100.5), "filled part left of the map cut");
        assert!(!hit(250.0, 100.5), "transparent interior right of the cut");
        assert!(hit(300.0, 100.5), "long row outline");
        assert!(hit(250.0, 101.0), "closure at a missing bucket");
        assert!(!hit(250.0, 101.5), "gap stays empty");
        assert!(!hit(170.0, 102.5), "short row stops before the cut");
        assert!(hit(210.0, 103.0), "connector between contiguous tips");
        assert!(hit(220.0, 103.5), "next row outline");
    }
}

#[test]
fn precise_profile_wrapper_has_no_invisible_handle_targets() {
    let payload = payload_with_rows(&[(100, 10), (104, 6)]);
    let scale = PriceScale::from_range(99.0, 106.0, 0.0, 490.0);
    let mut ctxt = context(&payload, &scale);
    let chart = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(800.0, 500.0));
    let points = [egui::pos2(100.0, 0.0), egui::pos2(500.0, 400.0)];
    let tool = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == TOOL.id())
        .unwrap();
    let center = egui::pos2(100.0, scale.y(102.5));
    assert_eq!(tool.hit_handle(chart, &points, center, 12.0, &ctxt), None);
    assert!(!tool.hit_test(chart, &points, center + egui::vec2(8.0, 0.0), 10.0, &ctxt));
    ctxt.selected = true;
    assert_eq!(
        tool.hit_handle(chart, &points, center, 12.0, &ctxt),
        Some(0)
    );
    assert_eq!(
        tool.hit_handle(chart, &points, center + egui::vec2(8.0, 0.0), 12.0, &ctxt),
        None
    );
}

#[test]
fn precise_profile_reversed_anchors_developing_edge_and_clip_agree() {
    let mut payload = payload_with_rows(&[(100, 10), (104, 6)]);
    payload.extend_right = true;
    payload.cache = Some(refreshed_cache(&[(100, 10), (104, 6)], 40));
    let scale = PriceScale::from_range(99.0, 106.0, 0.0, 490.0);
    let anchors = [ChartPoint::at(30.0, 104.0), ChartPoint::at(10.0, 100.0)];
    let points = [egui::pos2(500.0, 0.0), egui::pos2(100.0, 400.0)];
    let ctxt = DrawContext {
        anchors: &anchors,
        ..context(&payload, &scale)
    };
    let chart = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(800.0, 500.0));
    assert!(TOOL.hit_test(
        chart,
        &points,
        egui::pos2(710.0, scale.y(102.0)),
        10.0,
        &ctxt
    ));
    assert!(!TOOL.hit_test(
        chart,
        &points,
        egui::pos2(500.0, scale.y(102.0)),
        10.0,
        &ctxt
    ));
    let clipped = egui::Rect::from_min_max(egui::pos2(120.0, 50.0), egui::pos2(700.0, 450.0));
    assert!(!TOOL.hit_test(
        clipped,
        &points,
        egui::pos2(100.0, scale.y(102.0)),
        10.0,
        &ctxt
    ));
    assert!(!TOOL.hit_test(
        clipped,
        &points,
        egui::pos2(710.0, scale.y(102.0)),
        10.0,
        &ctxt
    ));
}

/// Identical fixture is run on main's paint/hit implementation and the repair.
#[test]
#[ignore = "manual frame-cost comparison; prints measurements without timing assertions"]
fn precise_profile_frame_benchmark() {
    let rows: Vec<_> = (0..2048).map(|i| (i, 1 + i % 37)).collect();
    let payload = payload_with_rows(&rows);
    let scale = PriceScale::from_range(0.0, 2048.0, 0.0, 900.0);
    let ctxt = context(&payload, &scale);
    let points = [egui::pos2(100.0, 0.0), egui::pos2(1300.0, 900.0)];
    let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1400.0, 900.0));
    let ctx = egui::Context::default();
    for batch in 0..6 {
        let start = std::time::Instant::now();
        for _ in 0..200 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ctx| {
                    let painter = ctx.layer_painter(egui::LayerId::background());
                    TOOL.paint_under(&painter, rect, ctxt.style, &points, &ctxt);
                    TOOL.paint(&painter, rect, ctxt.style, &points, &ctxt);
                    std::hint::black_box(TOOL.hit_test(
                        rect,
                        &points,
                        egui::pos2(650.0, 450.0),
                        10.0,
                        &ctxt,
                    ));
                },
            );
        }
        eprintln!(
            "PROFILE_FRAME_BENCH batch={batch} rows=2048 frames=200 us_per_frame={:.3}",
            start.elapsed().as_secs_f64() * 1_000_000.0 / 200.0
        );
    }
}
