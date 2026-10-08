use super::*;
#[path = "flow_stage_profile.rs"]
mod profile;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowExecution, FlowReference, FlowTapeView, project_flow_tape},
};

fn painted_region(buy: i64, sell: i64) -> (FlowTapeFrame, Vec<egui::Shape>) {
    painted_region_with_backing(buy, sell, None)
}

fn painted_region_with_backing(
    buy: i64,
    sell: i64,
    backing: Option<egui::Color32>,
) -> (FlowTapeFrame, Vec<egui::Shape>) {
    let trades = [(buy, Side::Buy), (sell, Side::Sell)].map(|(quantity, side)| Trade {
        agg_id: 1,
        timestamp_ms: 1000,
        price: 100.into(),
        quantity: quantity.into(),
        side,
    });
    let mut frame = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: 2.into(),
                trade,
                opening: false,
            }),
        1,
        2,
        2,
        FlowTapeView {
            first_slot: 0,
            end_slot: 1,
            clip_left: 0.into(),
            clip_right: 1.into(),
            width_px: 1.0,
            height_px: 100.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::Typed(9187.into()),
            radius_limit: 12.0,
            merge_support_radius: 12.0,
            exclude_opening: false,
        },
    );
    frame.dots[0].mark.buy_share = 0.0;
    let mut config = HeatmapConfig::default();
    config.bubbles.hollow_small_buys = true;
    config.bubbles.detail_min_radius = 32.0;
    config.bubbles.readable_min_radius = 32.0;
    config.bubbles.halo_strength = 1.0;
    config.bubbles.outline_width = 4.0;
    config.bubbles.opacity = 0.0;
    config.bubbles.buy_color = Some([0, 255, 0]);
    config.bubbles.sell_color = Some([255, 0, 0]);
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw_flow_executions(
            &ctx.layer_painter(egui::LayerId::background()),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
            0.0,
            &frame,
            &config,
            backing,
            |_| Some(egui::pos2(50.0, 50.0)),
        );
    });
    (
        frame,
        output.shapes.into_iter().map(|shape| shape.shape).collect(),
    )
}

#[test]
fn regional_circles_preserve_side_areas_without_cutoffs_or_inherited_dressing() {
    for (buy, sell) in [
        (51, 49),
        (100, 0),
        (0, 100),
        (1, 99),
        (25, 25),
        (4, 96),
        (1150, 6938),
        (5114, 4073),
        (900, 0),
        (90, 0),
    ] {
        let (frame, shapes) = painted_region(buy, sell);
        assert_eq!(shapes.len(), 1, "no backing, halo, rim or outline");
        let egui::Shape::Mesh(mesh) = &shapes[0] else {
            panic!("one centered sector mesh")
        };
        let colors = quantick_orderflow::config::dressing::flow::ordinary_colors(
            crate::theme::CANVAS.to_array(),
            frame.ordinary_region_opacity(&frame.dots[0]),
        )
        .map(super::super::premultiplied);
        let radius = frame.dots[0].radius;
        let mut areas = [0.0_f32; 2];
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let a = mesh.vertices[triangle[0] as usize];
            let b = mesh.vertices[triangle[1] as usize];
            let c = mesh.vertices[triangle[2] as usize];
            assert_eq!(a.color, b.color);
            assert_eq!(a.color, c.color);
            let side = colors.iter().position(|color| *color == a.color).unwrap();
            let b = b.pos - a.pos;
            let c = c.pos - a.pos;
            areas[side] += (b.x * c.y - b.y * c.x).abs() * 0.5;
        }
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.pos.distance(egui::pos2(50.0, 50.0) + FLOW_OFFSET) <= radius + 0.00001
        }));
        assert_eq!(areas[0] > 0.0, buy > 0);
        assert_eq!(areas[1] > 0.0, sell > 0);
        let total = areas.iter().sum::<f32>();
        assert!((areas[0] / total - buy as f32 / (buy + sell) as f32).abs() < 0.005);
        // A bounded polygon approximates the circle; factual radius has no floor.
        let circle_area = std::f32::consts::PI * radius.powi(2);
        assert!((total / circle_area - 1.0).abs() < 0.05);
        assert!((radius.powi(2) - 144.0 * (buy + sell) as f32 / 9187.0).abs() < 0.0001);
    }
    let (small, _) = painted_region(51, 49);
    assert!(
        small.dots[0].radius < 3.5,
        "both sides remain below the old cutoff"
    );
    let (large, _) = painted_region(900, 0);
    let (small, _) = painted_region(90, 0);
    assert!((large.dots[0].radius.powi(2) / small.dots[0].radius.powi(2) - 10.0).abs() < 0.00001);
}

#[test]
fn footprint_isolation_is_opaque_without_adding_to_the_earned_sector_geometry() {
    let background = egui::Color32::from_rgb(13, 17, 23);
    for (buy, sell) in [(1, 0), (51, 49), (4593, 4594)] {
        let (_, plain) = painted_region(buy, sell);
        let (_, backed) = painted_region_with_backing(buy, sell, Some(background));
        assert_eq!(backed.len(), 1);
        let egui::Shape::Mesh(sectors) = &plain[0] else {
            panic!("ordinary side geometry")
        };
        let egui::Shape::Mesh(isolated) = &backed[0] else {
            panic!("isolated side geometry")
        };
        assert_eq!(sectors.indices, isolated.indices);
        assert_eq!(sectors.vertices.len(), isolated.vertices.len());
        for (plain, isolated) in sectors.vertices.iter().zip(&isolated.vertices) {
            assert_eq!(plain.pos, isolated.pos);
            assert_eq!(plain.uv, isolated.uv);
            assert_eq!(isolated.color.a(), 255);
            assert_ne!(plain.color, isolated.color);
        }
    }
}

#[test]
fn circle_edge_visibility_uses_exact_geometry() {
    let (frame, _) = painted_region(4593, 4594);
    let history = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
    for x in [-10.0, 110.0] {
        let disc = flow_disc(&frame.dots[0], egui::pos2(x, 50.0) - FLOW_OFFSET).unwrap();
        assert!(disc.visible([history.min.into(), history.max.into()]));
    }
    for center in [
        egui::pos2(-13.0, 50.0),
        egui::pos2(113.0, 50.0),
        egui::pos2(-10.0, -10.0),
    ] {
        let disc = flow_disc(&frame.dots[0], center - FLOW_OFFSET).unwrap();
        assert!(
            !disc.visible([history.min.into(), history.max.into()]),
            "a bounding-box corner is not a circle intersection"
        );
    }
}

#[test]
fn oversized_opening_keeps_earned_area_with_faint_sectors_and_no_opaque_backing() {
    let (mut frame, _) = painted_region(75, 25);
    frame.dots[0].opening_anchor = true;
    frame.dots[0].opening_oversized = true;
    frame.dots[0].radius = 120.0;
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw_flow_executions(
            &ctx.layer_painter(egui::LayerId::background()),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0)),
            0.0,
            &frame,
            &HeatmapConfig::default(),
            Some(egui::Color32::BLACK),
            |_| Some(egui::pos2(50.0, 50.0)),
        );
    });
    assert_eq!(output.shapes.len(), 1, "no opaque footprint backing");
    let egui::Shape::Mesh(mesh) = &output.shapes[0].shape else {
        panic!("earned-area opening sectors")
    };
    let opacity = FLOW_FILL_OPACITY * quantick_orderflow::config::dressing::HOLLOW_FILL_ALPHA;
    let colors = [
        FLOW_BUY.gamma_multiply(opacity),
        FLOW_SELL.gamma_multiply(opacity),
    ];
    let center = egui::pos2(50.0, 50.0) + FLOW_OFFSET;
    assert!(mesh.vertices.iter().all(|vertex| {
        vertex.pos.distance(center) <= 120.0001 && colors.contains(&vertex.color)
    }));
    let mut areas = [0.0_f32; 2];
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let a = mesh.vertices[triangle[0] as usize];
        let b = mesh.vertices[triangle[1] as usize].pos - a.pos;
        let c = mesh.vertices[triangle[2] as usize].pos - a.pos;
        let side = colors.iter().position(|color| *color == a.color).unwrap();
        areas[side] += (b.x * c.y - b.y * c.x).abs() * 0.5;
    }
    let area = areas.iter().sum::<f32>();
    assert!((area / (std::f32::consts::PI * 120.0_f32.powi(2)) - 1.0).abs() < 0.01);
    assert!((areas[0] / area - 0.75).abs() < 0.001);
}

#[test]
fn common_offset_preserves_vectors_and_radii() {
    let (frame, _) = painted_region(90, 0);
    let a = egui::pos2(30.0, 40.0);
    let b = egui::pos2(60.0, 75.0);
    let da = flow_disc(&frame.dots[0], a).unwrap();
    let db = flow_disc(&frame.dots[0], b).unwrap();
    assert_eq!(egui::Pos2::from(da.center), a + FLOW_OFFSET);
    assert_eq!(
        egui::Pos2::from(db.center) - egui::Pos2::from(da.center),
        b - a
    );
    assert_eq!(da.radius, frame.dots[0].radius);
    let history = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0));
    let paint = |frame: &FlowTapeFrame| {
        let ctx = egui::Context::default();
        ctx.run(egui::RawInput::default(), |ctx| {
            super::super::flow_perimeter::draw_flow_perimeters(
                &ctx.layer_painter(egui::LayerId::background()),
                history,
                frame,
                &FlowPresentation::new(frame, [history.min.into(), history.max.into()], |_| {
                    Some(a.into())
                }),
            )
        })
        .shapes
    };
    assert!(
        paint(&frame).is_empty(),
        "ordinary bubbles have no foreground perimeter"
    );
    let mut opening = frame;
    opening.dots[0].opening_anchor = true;
    opening.dots[0].opening_oversized = true;
    let shapes = paint(&opening);
    assert_eq!(
        shapes.len(),
        3,
        "opening perimeter and shadowed direct value remain distinct"
    );
    let egui::Shape::Mesh(mesh) = &shapes[0].shape else {
        panic!("exception mesh")
    };
    assert!(
        mesh.vertices
            .iter()
            .all(|vertex| vertex.pos.distance(da.center.into()) <= da.radius + 0.00001)
    );
}

#[test]
fn hiding_flow_legend_keeps_offset_and_exception_facts_but_hides_colour_key() {
    let (mut frame, _) = painted_region(100, 0);
    frame.dots[0].opening_anchor = true;
    frame.dots[0].opening_oversized = true;
    frame.dots[0].opening_quantity = 90.into();
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        flow_caption(
            &ctx.layer_painter(egui::LayerId::background()),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 100.0)),
            0.0,
            Some(&frame),
            FlowProgress::default(),
            false,
        );
    });
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            Some(text.galley.job.text.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(texts.contains("offset"));
    assert!(texts.contains("100") && texts.contains("90"));
    assert!(!texts.contains("capped"));
    assert!(!texts.contains("Buy") && !texts.contains("Sell"));
}

#[test]
fn oversized_opening_value_stays_inside_the_pane_even_when_its_perimeter_is_clipped() {
    let (mut frame, _) = painted_region(74365, 0);
    frame.dots[0].opening_anchor = true;
    frame.dots[0].opening_oversized = true;
    frame.dots[0].radius = 1200.0;
    let history = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
    for center in [history.min, history.center(), history.max] {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            super::super::flow_perimeter::draw_flow_perimeters(
                &ctx.layer_painter(egui::LayerId::background()),
                history,
                &frame,
                &FlowPresentation::new(&frame, [history.min.into(), history.max.into()], |_| {
                    Some((center - FLOW_OFFSET).into())
                }),
            );
        });
        let text = output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                Some(text)
            })
            .expect("exact value survives even when the giant circumference is outside the pane");
        assert_eq!(text.galley.job.text, "First 74365 (clipped)");
        assert!(history.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())));
    }
}

#[test]
fn flat_centered_sectors_use_one_fan_without_changing_the_outer_arc() {
    use crate::orderflow_render::bubbles::{SPHERE_LIGHT_OFFSET, sphere_segments};

    let center = egui::pos2(30.0, 40.0);
    let shading = SphereShading::flat(egui::Color32::GRAY);
    for radius in [0.1, 0.5, 3.5, 7.0, 12.0, 120.0] {
        for share in [0.001, 0.01, 0.25, 0.5, 0.75, 1.0] {
            let sweep = share * std::f32::consts::TAU;
            let full = sphere_segments(radius).next_multiple_of(4);
            let segments = ((full as f32 * share).round() as usize).max(2);
            let mut flat = egui::Mesh::default();
            add_sector(
                &mut flat,
                center,
                radius,
                PIE_START_ANGLE,
                sweep,
                shading,
                0.0,
            );
            assert_eq!(flat.vertices.len(), segments + 2);
            assert_eq!(flat.indices.len(), segments * 3);
            assert_eq!(flat.vertices[0].pos, center);
            for (index, vertex) in flat.vertices[1..].iter().enumerate() {
                let angle = PIE_START_ANGLE + sweep * (index as f32 / segments as f32);
                assert_eq!(
                    vertex.pos,
                    center + egui::vec2(angle.cos(), angle.sin()) * radius
                );
                assert_eq!(vertex.color, egui::Color32::GRAY);
            }
            let mut native = egui::Mesh::default();
            add_sector(
                &mut native,
                center,
                radius,
                PIE_START_ANGLE,
                sweep,
                shading,
                SPHERE_LIGHT_OFFSET,
            );
            let native_segments = ((sphere_segments(radius) as f32 * share).ceil() as usize).max(2);
            assert_eq!(native.vertices.len(), 2 * native_segments + 3);
            assert_eq!(native.indices.len(), native_segments * 9);
        }
    }
    let mut shaded = egui::Mesh::default();
    add_sector(
        &mut shaded,
        center,
        12.0,
        PIE_START_ANGLE,
        std::f32::consts::TAU,
        SphereShading {
            core: egui::Color32::WHITE,
            body: egui::Color32::GRAY,
            edge: egui::Color32::BLACK,
        },
        0.0,
    );
    assert_eq!(shaded.vertices.len(), 2 * 24 + 3);
    assert_eq!(shaded.indices.len(), 24 * 9);
}

fn dense_flow_fixture(dot_count: u64) -> FlowTapeFrame {
    // Mid-bin quantiles from the 5,798-region recorded 1000 ms candidate.
    // They reproduce its size distribution; this is not a replay identity fixture.
    const QUANTITIES: [i64; 32] = [
        29, 48, 65, 80, 93, 106, 118, 132, 146, 160, 174, 191, 210, 226, 245, 269, 289, 313, 341,
        370, 402, 437, 479, 532, 589, 663, 757, 877, 1018, 1279, 1760, 2964,
    ];
    let slot_count = dot_count as usize / 40;
    let trades: Vec<_> = (0..dot_count)
        .flat_map(|index| {
            let quantity = if index == 0 {
                12780
            } else {
                QUANTITIES[index as usize % 32]
            };
            let buy = quantity * (1 + index as i64 % 3) / 4;
            [(buy, Side::Buy), (quantity - buy, Side::Sell)].map(|(quantity, side)| Trade {
                agg_id: index + 1,
                timestamp_ms: 1000 + index as i64 * 1000,
                price: (100 + index as i64 % 31).into(),
                quantity: quantity.into(),
                side,
            })
        })
        .collect();
    let frame = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / 80,
                accepted_ordinal: ordinal % 80,
                ticks_per_bar: 80.into(),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: slot_count,
            clip_left: 0.into(),
            clip_right: Decimal::from(slot_count),
            width_px: 1200.0,
            height_px: 795.0,
            prices: PriceWindow::new(90.into(), 140.into()).unwrap(),
            reference: FlowReference::Typed(12780.into()),
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    );
    assert_eq!(frame.dots.len(), dot_count as usize);
    frame
}

#[test]
#[ignore = "manual production FLOW cache cold, warm and invalidated frame timing"]
fn dense_production_flow_cache_benchmark() {
    use quantick_chart::{flow_execution::FlowExecutionGeometry, viewport::Viewport};
    use std::{sync::Arc, time::Instant};

    for dot_count in [6000_u64, 30000] {
        let frame = Arc::new(dense_flow_fixture(dot_count));
        let slot_count = dot_count as usize / 40;
        let mut viewport = Viewport::new();
        viewport.set_px_per_bar(1200.0 / slot_count as f32);
        let geometry = |price_shift: f64| {
            FlowExecutionGeometry::new(
                viewport,
                slot_count,
                0,
                (90.0 + price_shift, 140.0 + price_shift),
                [20.0, 20.0, 1220.0, 815.0],
                false,
            )
            .unwrap()
        };
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1240.0, 835.0));
        let history = egui::Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(1220.0, 815.0));
        let paint = |source: &Arc<FlowTapeFrame>, geometry| {
            let started = Instant::now();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    crate::orderflow_render::draw_cached_flow(
                        &ctx.layer_painter(egui::LayerId::background()),
                        history,
                        source,
                        geometry,
                        Some(egui::Color32::from_rgb(19, 23, 34)),
                    );
                },
            );
            let shapes = output.shapes.len();
            let paint_ms = started.elapsed().as_secs_f64() * 1000.0;
            let tessellation_started = Instant::now();
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            let tessellation_ms = tessellation_started.elapsed().as_secs_f64() * 1000.0;
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let counts =
                primitives
                    .iter()
                    .fold((shapes, 0, 0), |(shapes, vertices, indices), primitive| {
                        if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                            (
                                shapes,
                                vertices + mesh.vertices.len(),
                                indices + mesh.indices.len(),
                            )
                        } else {
                            (shapes, vertices, indices)
                        }
                    });
            (elapsed_ms, paint_ms, tessellation_ms, counts)
        };
        let cold = paint(&frame, geometry(0.0));
        println!(
            "DENSE_FLOW_PRODUCTION_CACHE_COLD {{\"dots\":{dot_count},\"total_ms\":{:.6},\"paint_ms\":{:.6},\"final_tessellation_ms\":{:.6},\"shapes\":{},\"vertices\":{},\"indices\":{}}}",
            cold.0, cold.1, cold.2, cold.3.0, cold.3.1, cold.3.2,
        );
        for mode in ["warm_same_arc", "new_source_arc", "changed_price_axis"] {
            let mut samples = Vec::new();
            let mut paint_total = 0.0;
            let mut tessellation_total = 0.0;
            let (warmup, measured) = if mode == "warm_same_arc" {
                (30, 100)
            } else {
                (2, 10)
            };
            for iteration in 0..warmup + measured {
                // Worker projection and camera-key creation are outside renderer timing.
                let source = if mode == "new_source_arc" {
                    let mut next = (*frame).clone();
                    next.source_revision += iteration as u64 + 1;
                    Arc::new(next)
                } else {
                    Arc::clone(&frame)
                };
                let shift = if mode == "changed_price_axis" {
                    (iteration + 1) as f64 * 0.001
                } else {
                    0.0
                };
                let result = paint(&source, geometry(shift));
                if mode == "warm_same_arc" {
                    assert_eq!(
                        result.3, cold.3,
                        "the same frame retains every cached triangle"
                    );
                }
                if iteration >= warmup {
                    samples.push(result.0);
                    paint_total += result.1;
                    tessellation_total += result.2;
                }
            }
            samples.sort_by(f64::total_cmp);
            let p95 = (samples.len() * 95).div_ceil(100) - 1;
            println!(
                "DENSE_FLOW_PRODUCTION_CACHE {{\"dots\":{dot_count},\"mode\":\"{mode}\",\"frames\":{},\"mean_ms\":{:.6},\"p95_ms\":{:.6},\"paint_mean_ms\":{:.6},\"final_tessellation_mean_ms\":{:.6}}}",
                samples.len(),
                samples.iter().sum::<f64>() / samples.len() as f64,
                samples[p95],
                paint_total / samples.len() as f64,
                tessellation_total / samples.len() as f64,
            );
        }
    }
}
