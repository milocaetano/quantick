//! quantick-app — desktop chart rendering alternative bars from live trades.
//!
//! A consumer of `quantick-engine`, never the other way around. On startup it
//! backfills recent BTCUSDT aggTrades over REST so the chart opens populated,
//! then streams live trades on top, forming bars in real time.

use eframe::egui;

mod app;
mod chart;
mod feed;
mod state;

const SYMBOL: &str = "BTCUSDT";
const TICK_SIZE: u64 = 50;

fn main() -> eframe::Result {
    let events = feed::spawn(SYMBOL);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 650.0])
            .with_title("quantick"),
        ..Default::default()
    };

    eframe::run_native(
        "quantick",
        options,
        Box::new(move |_cc| Ok(Box::new(app::QuantickApp::new(SYMBOL, TICK_SIZE, events)))),
    )
}
