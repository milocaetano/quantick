use super::*;
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
fn translucent_circles_preserve_side_areas_without_cutoffs_or_inherited_dressing() {
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
        let colors = [
            egui::Color32::GREEN.gamma_multiply(0.5),
            egui::Color32::RED.gamma_multiply(0.5),
        ];
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
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.pos.distance(egui::pos2(50.0, 50.0)) <= radius + 0.00001)
        );
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
fn footprint_backing_is_opaque_and_exactly_the_earned_disc_before_unchanged_sectors() {
    let background = egui::Color32::from_rgb(13, 17, 23);
    for (buy, sell) in [(1, 0), (51, 49), (4593, 4594)] {
        let (frame, plain) = painted_region(buy, sell);
        let (_, backed) = painted_region_with_backing(buy, sell, Some(background));
        assert_eq!(backed.len(), 2);
        let egui::Shape::Circle(disc) = &backed[0] else {
            panic!("one exact-radius neutral disc precedes the sectors")
        };
        assert_eq!(disc.center, egui::pos2(50.0, 50.0));
        assert_eq!(disc.radius, frame.dots[0].radius);
        assert_eq!(disc.fill, background);
        assert_eq!(disc.fill.a(), 255);
        assert_eq!(disc.stroke, egui::Stroke::NONE);
        assert_eq!(
            backed[1], plain[0],
            "backing cannot change quantitative geometry or colors"
        );
    }
}

#[test]
fn circle_edge_visibility_and_hit_testing_share_exact_geometry() {
    let (frame, _) = painted_region(4593, 4594);
    let history = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
    for (x, pointer_x) in [(-10.0, 1.0), (110.0, 99.0)] {
        let disc = flow_disc(&frame.dots[0], egui::pos2(x, 50.0)).unwrap();
        assert!(disc.visible(history));
        assert!(
            disc.hit_distance(history, egui::pos2(pointer_x, 50.0))
                .is_some()
        );
        assert!(
            disc.hit_distance(history, disc.center).is_none(),
            "outside pointer cannot inspect the clip"
        );
    }
    for center in [
        egui::pos2(-13.0, 50.0),
        egui::pos2(113.0, 50.0),
        egui::pos2(-10.0, -10.0),
    ] {
        let disc = flow_disc(&frame.dots[0], center).unwrap();
        assert!(
            !disc.visible(history),
            "a bounding-box corner is not a circle intersection"
        );
        assert!(disc.hit_distance(history, history.clamp(center)).is_none());
    }
}

#[test]
fn capped_opening_has_no_proportional_fill_or_footprint_backing() {
    let (mut frame, _) = painted_region(100, 0);
    frame.dots[0].opening_capped = true;
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
    assert!(output.shapes.is_empty());
}
