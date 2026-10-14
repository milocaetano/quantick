//! Private geometry export for independent visibility analysis; no rendered evidence is bundled.
use super::*;
use quantick_chart::flow_execution::FlowPresentation;
use std::fmt::Write as _;

#[test]
#[ignore = "requires WIN_TICK_FIXTURE and a private WIN_GEOMETRY_DIR"]
fn recorded_visual_matrix() {
    let input = std::env::var_os("WIN_TICK_FIXTURE").expect("private canonical recording");
    let root =
        std::path::PathBuf::from(std::env::var_os("WIN_GEOMETRY_DIR").expect("private output"));
    std::fs::create_dir_all(&root).unwrap();
    let trades = parse_trades(&std::fs::read_to_string(input).unwrap()).unwrap();
    for (name, offset, count) in [
        ("opening", 0, 40_000),
        ("dense", trades.len() - 200_000, 200_000),
    ] {
        let sample = &trades[offset..offset + count];
        let slots = sample.len().div_ceil(2_000);
        for width in [500.0_f32, 840.0] {
            for candle_width in [8.0_f32, 32.0] {
                let right = slots as f32;
                let left = right - width / candle_width;
                let visible = &sample[(left.max(0.0).floor() as usize * 2_000)..];
                let prices = PriceWindow::new(
                    visible.iter().map(|t| t.price).min().unwrap() - Decimal::from(30),
                    visible.iter().map(|t| t.price).max().unwrap() + Decimal::from(30),
                )
                .unwrap();
                let mut frame = project(sample, width, 100);
                reconcile(&frame, sample);
                let initial: Vec<_> = frame.dots.iter().map(|dot| dot.radius).collect();
                for radius in [18.0_f32, 24.0, 32.0] {
                    for (dot, original) in frame.dots.iter_mut().zip(&initial) {
                        dot.radius = original * radius / 18.0;
                    }
                    let plan = FlowPresentation::new(&frame, [[0.0, 0.0], [width, 800.0]], |dot| {
                        Some([
                            (dot.candle_position.to_f32().unwrap() - left) * candle_width,
                            prices.y_unclamped(dot.mark.price).unwrap() as f32 * 800.0,
                        ])
                    });
                    let mut csv = String::from(
                        "index,order,x,y,radius,quantity,buy,members,first_ordinal,last_ordinal,unresolved,role\n",
                    );
                    for (order, region) in plan.regions.iter().enumerate() {
                        let dot = &frame.dots[region.dot_index];
                        writeln!(
                            &mut csv,
                            "{},{},{},{},{},{},{},{},{},{},{},{:?}",
                            region.dot_index,
                            order,
                            region.disc.center[0],
                            region.disc.center[1],
                            region.disc.radius,
                            dot.mark.quantity,
                            dot.mark.buy_quantity,
                            dot.members.len(),
                            dot.members.iter().map(|m| m.ordinal).min().unwrap(),
                            dot.members.iter().map(|m| m.ordinal).max().unwrap(),
                            region.unresolved_overlap,
                            region.role
                        )
                        .unwrap();
                    }
                    let stem = format!("{name}-w{width}-c{candle_width}-r{radius}");
                    std::fs::write(root.join(format!("{stem}.csv")), csv).unwrap();
                    std::fs::write(root.join(format!("{stem}.txt")), format!(
                        "offset={offset} count={count} width={width} height=800 candle_width={candle_width} left={left} right={right} radius={radius} reference=20000 total_groups={} visible_groups={}\n",
                        frame.dots.len(), plan.regions.len())).unwrap();
                }
            }
        }
    }
}
