use super::*;
use quantick_chart::viewport::Viewport;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowExecution, FlowReference, FlowTapeView, project_flow_tape},
};

fn frame() -> Arc<FlowTapeFrame> {
    let trade = Trade {
        agg_id: 42,
        timestamp_ms: 1000,
        price: 100.into(),
        quantity: 4500.into(),
        side: Side::Buy,
    };
    Arc::new(project_flow_tape(
        [FlowExecution {
            ordinal: 0,
            slot: 0,
            accepted_ordinal: 0,
            ticks_per_bar: 1.into(),
            trade: &trade,
            opening: false,
        }],
        7,
        1,
        1,
        FlowTapeView {
            first_slot: 0,
            end_slot: 1,
            clip_left: 0.into(),
            clip_right: 1.into(),
            width_px: 200.0,
            height_px: 120.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    ))
}

fn history() -> egui::Rect {
    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(200.0, 120.0))
}

fn geometry() -> FlowExecutionGeometry {
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(64.0);
    FlowExecutionGeometry::new(
        viewport,
        1,
        0,
        (90.0, 110.0),
        [0.0, 0.0, 200.0, 120.0],
        false,
    )
    .unwrap()
}

fn key(frame: &Arc<FlowTapeFrame>) -> Key<'_> {
    Key {
        frame,
        geometry: geometry(),
        clip: history(),
        backing: Some(egui::Color32::from_rgb(19, 23, 34)),
        background: crate::theme::CANVAS,
    }
}

fn paint(ctx: &egui::Context, key: Key<'_>) -> (Arc<Entry>, Vec<egui::epaint::ClippedShape>) {
    paint_at_density(ctx, key, 1.0)
}

fn paint_at_density(
    ctx: &egui::Context,
    key: Key<'_>,
    pixels_per_point: f32,
) -> (Arc<Entry>, Vec<egui::epaint::ClippedShape>) {
    let mut input = egui::RawInput {
        screen_rect: Some(history()),
        ..Default::default()
    };
    // Monitor DPI keeps logical bounds fixed; changing egui's zoom would also
    // replace screen_rect at the next pass and change the painter's actual clip.
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .native_pixels_per_point = Some(pixels_per_point);
    let mut entry = None;
    let output = ctx.run(input, |ctx| {
        let painter = ctx
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(key.clip);
        let actual = Key {
            clip: history().intersect(painter.clip_rect()),
            ..key
        };
        assert_eq!(ctx.pixels_per_point(), pixels_per_point);
        let drawing = cached_flow(
            &painter,
            history(),
            key.frame,
            key.geometry,
            key.background,
            key.backing,
        );
        drawing.paint(&painter, history(), false);
        drawing.paint(&painter, history(), true);
        entry = Some(ctx.memory_mut(|memory| memory.caches.cache::<Cache>().get(actual)));
    });
    let entry = entry.unwrap();
    assert!(
        entry.mesh.get().is_some(),
        "the production painter filled this cache entry"
    );
    (entry, output.shapes)
}

#[test]
fn warm_paint_reuses_the_same_geometry_and_exact_mesh_output() {
    let frame = frame();
    let ctx = egui::Context::default();
    let (cold, first_shapes) = paint(&ctx, key(&frame));
    let (warm, second_shapes) = paint(&ctx, key(&frame));
    assert!(Arc::ptr_eq(&cold, &warm));
    assert_eq!(first_shapes, second_shapes);
    assert!(!first_shapes.is_empty());
    assert!(
        cold.mesh
            .get()
            .unwrap()
            .meshes
            .iter()
            .all(egui::Mesh::is_valid)
    );
}

#[test]
fn a_new_partial_frame_allocation_cannot_reuse_old_geometry_with_identical_revisions() {
    let original = frame();
    let mut changed = (*original).clone();
    changed.dots[0].radius = 6.0;
    let partial = Arc::new(changed);
    assert_eq!(partial.source_revision, original.source_revision);
    assert_eq!(partial.layout_revision, original.layout_revision);
    assert_eq!(partial.loaded_executions, original.loaded_executions);
    let ctx = egui::Context::default();
    let (first, first_shapes) = paint(&ctx, key(&original));
    let (second, second_shapes) = paint(&ctx, key(&partial));
    assert!(!Arc::ptr_eq(&first, &second));
    assert_ne!(first_shapes, second_shapes);
}

#[test]
fn camera_clip_and_backing_each_rebuild_the_cached_mesh() {
    let frame = frame();
    let original = key(&frame);
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(32.0);
    let moved = FlowExecutionGeometry::new(
        viewport,
        1,
        0,
        (90.0, 110.0),
        [0.0, 0.0, 200.0, 120.0],
        false,
    )
    .unwrap();
    for changed in [
        Key {
            geometry: moved,
            ..original
        },
        Key {
            clip: history().shrink(10.0),
            ..original
        },
        Key {
            backing: None,
            ..original
        },
        Key {
            backing: Some(egui::Color32::BLACK),
            ..original
        },
    ] {
        let ctx = egui::Context::default();
        let (before, before_shapes) = paint(&ctx, original);
        let (after, after_shapes) = paint(&ctx, changed);
        assert!(!Arc::ptr_eq(&before, &after));
        if changed.backing.is_none() {
            assert_eq!(before_shapes, after_shapes, "both compose over the canvas");
        } else {
            assert_ne!(before_shapes, after_shapes);
        }
    }
}

#[test]
fn logical_sector_mesh_is_reused_when_monitor_pixel_density_changes() {
    let frame = frame();
    let ctx = egui::Context::default();
    let (first, first_shapes) = paint_at_density(&ctx, key(&frame), 1.0);
    for density in [1.5, 2.0, 1.0] {
        let (reused, shapes) = paint_at_density(&ctx, key(&frame), density);
        assert!(Arc::ptr_eq(&first, &reused));
        assert_eq!(first_shapes, shapes);
    }
}

#[test]
fn cache_holds_only_weak_source_ownership_and_evicts_unused_geometry() {
    let frame = frame();
    let source = Arc::downgrade(&frame);
    let mut cache = Cache::default();
    let entry = cache.get(key(&frame));
    assert_eq!(Arc::strong_count(&frame), 1);
    let retained = Arc::downgrade(&entry);
    drop(frame);
    assert!(
        source.upgrade().is_none(),
        "cached geometry must not retain source members"
    );
    assert!(entry._source.upgrade().is_none());
    drop(entry);
    cache.evice_cache();
    assert!(
        retained.upgrade().is_some(),
        "used geometry survives the current frame"
    );
    cache.evice_cache();
    assert!(
        retained.upgrade().is_none(),
        "unused geometry leaves with the next frame"
    );
}

#[test]
fn two_flow_panes_keep_independent_cache_entries_within_one_frame() {
    let first = frame();
    let second = frame();
    let mut cache = Cache::default();
    let a = cache.get(key(&first));
    let b = cache.get(key(&second));
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(&a, &cache.get(key(&first))));
    assert!(Arc::ptr_eq(&b, &cache.get(key(&second))));
}

#[test]
fn sector_mesh_uses_only_white_uv_and_survives_font_atlas_growth() {
    let frame = frame();
    let ctx = egui::Context::default();
    let (entry, before_shapes) = paint(&ctx, key(&frame));
    let mesh = &entry.mesh.get().unwrap().meshes[1];
    assert!(!mesh.vertices.is_empty());
    assert_eq!(mesh.texture_id, egui::TextureId::default());
    assert!(
        mesh.vertices
            .iter()
            .all(|vertex| vertex.uv == egui::epaint::WHITE_UV)
    );
    let before_size = ctx.fonts(|fonts| fonts.font_image_size());
    ctx.fonts(|fonts| {
        let text: String = ('!'..='~').collect();
        let _ = fonts.layout_no_wrap(
            text,
            egui::FontId::proportional(256.0),
            egui::Color32::WHITE,
        );
    });
    assert_ne!(ctx.fonts(|fonts| fonts.font_image_size()), before_size);
    let (after, after_shapes) = paint(&ctx, key(&frame));
    assert!(Arc::ptr_eq(&entry, &after));
    assert_eq!(before_shapes, after_shapes);
}

#[test]
fn direct_mesh_separates_peaks_without_changing_sector_geometry_or_colours() {
    use super::super::{
        bubbles::{PIE_START_ANGLE, SphereShading, add_sector},
        flow_execution::flow_mesh,
    };

    let trades = [
        (1000, 750, Side::Buy),
        (1000, 250, Side::Sell),
        (2000, 1000, Side::Buy),
        (2000, 3000, Side::Sell),
    ]
    .map(|(timestamp_ms, quantity, side)| Trade {
        agg_id: timestamp_ms as u64,
        timestamp_ms,
        price: 100.into(),
        quantity: quantity.into(),
        side,
    });
    let frame = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: 4.into(),
                trade,
                opening: false,
            }),
        7,
        4,
        4,
        frame().view,
    );
    assert_eq!(frame.dots.len(), 2);
    for backing in [None, Some(egui::Color32::from_rgb(19, 23, 34))] {
        let mut expected = egui::Mesh::default();
        // The smaller circle first clears its neighbour at 16px above-left.
        // Radius, shares and absence of a second backing polygon remain exact.
        // At one quarter of the reference it retains context colour; the
        // reference-sized circle earns full emphasis. Both are canvas-opaque.
        for (center, radius, buy, colors) in [
            (
                egui::pos2(
                    100.0 - 16.0 / std::f32::consts::SQRT_2,
                    60.0 - 16.0 / std::f32::consts::SQRT_2,
                ),
                6.0,
                0.75_f64,
                [
                    egui::Color32::from_rgb(30, 42, 59),
                    egui::Color32::from_rgb(45, 41, 42),
                ],
            ),
            (
                egui::pos2(104.0, 60.0),
                12.0,
                0.25_f64,
                [
                    egui::Color32::from_rgb(80, 128, 171),
                    egui::Color32::from_rgb(158, 122, 76),
                ],
            ),
        ] {
            let mut angle = f64::from(PIE_START_ANGLE);
            for (share, color) in [(buy, colors[0]), (1.0 - buy, colors[1])] {
                let sweep = share * std::f64::consts::TAU;
                add_sector(
                    &mut expected,
                    center,
                    radius,
                    angle as f32,
                    sweep as f32,
                    SphereShading::flat(color),
                    0.0,
                );
                angle += sweep;
            }
        }
        let actual = flow_mesh(history(), &frame, backing, |dot| {
            Some(egui::pos2(
                if dot.mark.quantity == 1000.into() {
                    118.0
                } else {
                    122.0
                },
                78.0,
            ))
        });
        assert_eq!(actual, expected, "backing={backing:?}");
    }
}

#[test]
fn precomposed_colours_match_gamma_premultiplied_gpu_blending_over_footprint() {
    use super::super::flow_execution::flow_colors;

    let opaque = egui::Color32::from_rgb(19, 23, 34);
    let colours = flow_colors(Some(opaque), false);
    assert_eq!(colours[0].to_array(), [80, 128, 171, 255]);
    assert_eq!(colours[1].to_array(), [158, 122, 76, 255]);

    // Quantick uses egui_glow 0.29.1: gamma vertex bytes, disabled framebuffer
    // sRGB conversion, and RGB ONE / ONE_MINUS_SRC_ALPHA. Model both original
    // draws against several underlying footprint colours in normalized floats.
    // One precomposed byte introduces at most one final channel step; this does
    // not claim equivalence for the removed vector-antialiasing edge fringe.
    let over = |source: [f64; 4], destination: [f64; 4]| {
        std::array::from_fn::<_, 4, _>(|channel| {
            source[channel] + destination[channel] * (1.0 - source[3])
        })
    };
    let normalized = |color: egui::Color32| color.to_array().map(|v| f64::from(v) / 255.0);
    for backing in [
        opaque,
        egui::Color32::BLACK,
        egui::Color32::WHITE,
        egui::Color32::from_rgba_premultiplied(12, 18, 29, 128),
    ] {
        for underlying in [
            egui::Color32::from_rgb(13, 17, 23),
            egui::Color32::from_rgb(0, 180, 140),
            egui::Color32::from_rgb(220, 40, 80),
        ] {
            let old_background = over(normalized(backing), normalized(underlying));
            for (source, composed) in flow_colors(None, false)
                .into_iter()
                .zip(flow_colors(Some(backing), false))
            {
                let old = over(normalized(source), old_background);
                let new = over(normalized(composed), normalized(underlying));
                for channel in 0..4 {
                    assert!((old[channel] - new[channel]).abs() * 255.0 <= 1.0);
                }
            }
        }
        assert_eq!(
            flow_colors(Some(backing), true),
            flow_colors(None, true),
            "the oversized opening retains its translucent drawing"
        );
    }
}
