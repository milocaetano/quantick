//! quantick-app — desktop chart rendering alternative bars.
//!
//! A consumer of `quantick-engine`, never the other way around. For now it
//! renders deterministic demo bars built by the engine; the live feed is wired
//! in #33.

use eframe::egui;

mod app;
mod chart;
mod demo;

fn main() -> eframe::Result {
    let (bars, partial) = demo::demo_bars();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 650.0])
            .with_title("quantick"),
        ..Default::default()
    };

    eframe::run_native(
        "quantick",
        options,
        Box::new(move |_cc| Ok(Box::new(app::QuantickApp::new(bars, partial)))),
    )
}
