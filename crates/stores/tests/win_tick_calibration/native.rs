//! Optional native-camera geometry export for a private recorded audit.
use super::*;
use quantick_chart::flow_execution::FlowPresentation;
use std::fmt::Write as _;

#[test]
#[ignore = "requires a private fixture, native cameras and output directory"]
fn recorded_native_cameras() {
    let input = std::env::var_os("WIN_TICK_FIXTURE").expect("private canonical recording");
    let cameras = std::env::var_os("WIN_GEOMETRY_CAMERAS").expect("captured native cameras");
    let root = std::path::PathBuf::from(
        std::env::var_os("WIN_GEOMETRY_DIR").expect("private geometry output"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let trades = parse_trades(&std::fs::read_to_string(input).unwrap()).unwrap();
    let cameras: toml::Value = toml::from_str(&std::fs::read_to_string(cameras).unwrap()).unwrap();
    for camera in cameras["cameras"].as_array().unwrap() {
        let name = camera["name"].as_str().unwrap();
        let number = |key: &str| camera[key].as_float().unwrap() as f32;
        let offset = camera["offset"].as_integer().unwrap() as usize;
        let count = camera["count"].as_integer().unwrap() as usize;
        let sample = &trades[offset..offset + count];
        let width = number("width");
        let height = number("height");
        let left = number("left");
        let candle_width = number("candle_width");
        let prices = PriceWindow::from_f64_range((
            camera["low"].as_float().unwrap(),
            camera["high"].as_float().unwrap(),
        ))
        .unwrap();
        let frame = project(sample, width, 100);
        reconcile(&frame, sample);
        let plan = FlowPresentation::new(&frame, [[0.0, 0.0], [width, height]], |dot| {
            Some([
                (dot.candle_position.to_f32().unwrap() - left) * candle_width,
                prices.y_unclamped(dot.mark.price).unwrap() as f32 * height,
            ])
        });
        let mut csv = String::from(
            "index,order,x,y,radius,quantity,buy,members,first_ordinal,last_ordinal,unresolved,role,candle_position,price,source_x,source_y\n",
        );
        for (order, region) in plan.regions.iter().enumerate() {
            let dot = &frame.dots[region.dot_index];
            writeln!(
                &mut csv,
                "{},{},{},{},{},{},{},{},{},{},{},{:?},{},{},{},{}",
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
                region.role,
                dot.candle_position,
                dot.mark.price,
                region.source_disc.center[0],
                region.source_disc.center[1],
            )
            .unwrap();
        }
        std::fs::write(root.join(format!("{name}.csv")), csv).unwrap();
        std::fs::write(root.join(format!("{name}.toml")), camera.to_string()).unwrap();
        println!(
            "{name}: {} total groups, {} presented",
            frame.dots.len(),
            plan.regions.len()
        );
    }
}
