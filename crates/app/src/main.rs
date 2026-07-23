//! quantick-app — desktop chart rendering alternative bars from live trades.
//!
//! A consumer of `quantick-engine`, never the other way around. On startup it
//! backfills recent BTCUSDT aggTrades over REST so the chart opens populated,
//! then streams live trades on top, forming bars in real time. Frame time and
//! feed lag are surfaced on screen and in structured logs.

use eframe::egui;
use tracing_subscriber::EnvFilter;

use crate::state::BarSpec;

mod app;
mod chart;
mod feed;
mod metrics;
mod price_view;
mod state;
mod style;
mod viewport;

const SYMBOL: &str = "BTCUSDT";
const TICK_SIZE: u64 = 50;

/// Install the tracing subscriber. Feed and app events flow to stderr; the level
/// is controlled by `RUST_LOG` (default `quantick=info`). The engine emits
/// nothing, so logging can never affect its determinism.
fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("quantick=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

fn main() -> eframe::Result {
    init_tracing();
    tracing::info!(
        target: "quantick::app",
        symbol = SYMBOL,
        tick_size = TICK_SIZE,
        "starting quantick"
    );

    let feed = feed::spawn(SYMBOL);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 650.0])
            .with_title("quantick"),
        ..Default::default()
    };

    eframe::run_native(
        "quantick",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(app::QuantickApp::new(
                SYMBOL,
                BarSpec::Tick(TICK_SIZE),
                feed,
            )))
        }),
    )
}
