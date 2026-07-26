//! quantick-app — desktop chart rendering alternative bars from live trades.
//!
//! A consumer of `quantick-engine`, never the other way around. On startup it
//! reads the feed/asset configuration (see [`config`]), backfills recent history
//! over REST so the chart opens populated, then streams live trades on top,
//! forming bars in real time. The feed and symbol can be switched live from the
//! chart. Frame time and feed lag are surfaced on screen and in structured logs.

use eframe::egui;
use tracing_subscriber::EnvFilter;

use crate::state::BarSpec;

mod app;
mod bubble_presets;
mod candle_view;
mod chart;
mod config;
mod dock;
mod feed;
mod loading;
mod metrics;
mod orderflow;
mod orderflow_engine;
mod orderflow_render;
mod orderflow_view;
mod orderflow_worker;
mod price_view;
mod replay_view;
mod state;
mod statusbar;
mod style;
mod theme;
mod timezone;
mod toolbar;
mod toolrail;
mod viewport;
mod widgets;

/// The bar type the chart opens on. The type and its parameter are tunable live
/// from the controls bar; the feed and symbol come from the configuration.
const INITIAL_TICK_SIZE: u64 = 50;

/// Install the tracing subscriber. Feed and app events flow to stderr; the level
/// is controlled by `RUST_LOG` (default `quantick=info`). Set
/// `QUANTICK_LOG_FORMAT=json` for newline-delimited JSON that an operator or an
/// AI diagnostic tool can parse without scraping prose. Deterministic cores emit
/// nothing, so logging can never affect replay results.
fn init_tracing() {
    let json =
        std::env::var("QUANTICK_LOG_FORMAT").is_ok_and(|value| value.eq_ignore_ascii_case("json"));

    if json {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("quantick=info"));
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(true)
            .with_env_filter(filter)
            .with_target(true)
            .init();
    } else {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("quantick=info"));
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .with_target(true)
            .init();
    }
}

fn main() -> eframe::Result {
    init_tracing();

    // Feed and asset are configuration, not constants. A malformed external
    // config is fatal and surfaced, never silently ignored.
    let (config, source) = match config::load() {
        Ok(loaded) => loaded,
        Err(e) => {
            tracing::error!(
                target: "quantick::app",
                event_code = "CONFIG_ERROR",
                %e,
                "cannot load configuration; fix it or unset QUANTICK_CONFIG"
            );
            std::process::exit(1);
        }
    };

    let feed_id = config.default_feed.clone();
    let symbol = config.default_symbol.clone();
    let provider = config
        .provider_of(&feed_id)
        .expect("default_feed validated to exist");

    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "APP_STARTING",
        config_source = %source,
        feed = %feed_id,
        symbol = %symbol,
        provider = ?provider,
        "starting quantick"
    );

    let feed = feed::spawn_live(provider, &symbol, &config);

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("bundled assets/icon.png is a valid PNG");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 650.0])
            .with_title("quantick")
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "quantick",
        options,
        Box::new(move |cc| {
            // Chrome glyphs come from the bundled Phosphor icon font; the
            // design tokens point egui's own widgets at the chrome palette.
            // Both are installed once, before the first frame.
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::QuantickApp::new(
                config,
                feed_id,
                symbol,
                BarSpec::Tick(INITIAL_TICK_SIZE),
                feed,
            )))
        }),
    )
}
