use super::*;

#[test]
#[ignore = "manual FLOW renderer stage profiling"]
fn dense_flow_stage_profile() {
    use quantick_chart::{flow_execution::FlowExecutionGeometry, viewport::Viewport};
    use std::{hint::black_box, time::Instant};
    fn measure(label: &str, mut run: impl FnMut()) {
        for _ in 0..3 {
            run();
        }
        let start = Instant::now();
        for _ in 0..20 {
            run();
        }
        println!(
            "FLOW_STAGE {label} mean_ms={:.6}",
            start.elapsed().as_secs_f64() * 50.0
        );
    }
    let frame = dense_flow_fixture(30000);
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(1200.0 / 750.0);
    let geometry = FlowExecutionGeometry::new(
        viewport,
        750,
        0,
        (90.0, 140.0),
        [20.0, 20.0, 1220.0, 815.0],
        false,
    )
    .unwrap();
    let dots = frame.dots_in_paint_order().collect::<Vec<_>>();
    let points = dots
        .iter()
        .map(|dot| geometry.point(dot).unwrap())
        .collect::<Vec<_>>();
    let facts = dots
        .iter()
        .zip(&points)
        .map(|(dot, &(x, y))| {
            (
                dot.radius,
                dot.side_shares(),
                egui::pos2(x - 18.0, y - 18.0),
            )
        })
        .collect::<Vec<_>>();
    measure("coordinate_transform", || {
        for dot in &dots {
            black_box(geometry.point(black_box(dot)));
        }
    });
    measure("side_shares", || {
        for dot in &dots {
            black_box(black_box(dot).side_shares());
        }
    });
    let history = egui::Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(1220.0, 815.0));
    measure("flow_mesh_full", || {
        black_box(flow_mesh(
            history,
            &frame,
            Some(egui::Color32::BLACK),
            |dot| geometry.point(dot).map(|(x, y)| egui::pos2(x, y)),
        ));
    });
    measure("flow_mesh_precomputed_points", || {
        let mut points = points.iter();
        black_box(flow_mesh(
            history,
            &frame,
            Some(egui::Color32::BLACK),
            |_| points.next().map(|&(x, y)| egui::pos2(x, y)),
        ));
    });
    measure("sector_mesh_precomputed_facts", || {
        let mut mesh = egui::Mesh::default();
        for &(radius, (buy, sell), center) in &facts {
            let mut angle = f64::from(PIE_START_ANGLE);
            for (share, color) in [(buy, FLOW_BUY), (sell, FLOW_SELL)] {
                if share <= 0.0 {
                    continue;
                }
                let sweep = share * std::f64::consts::TAU;
                add_sector(
                    &mut mesh,
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
        black_box(mesh);
    });
}
