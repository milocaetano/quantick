//! quantick-app — desktop chart rendering alternative bars from live trades.
//!
//! A consumer of `quantick-engine`, never the other way around. On startup it
//! reads the feed/asset configuration (see [`config`]), recovers the factual
//! recent trades available from that source, then streams live trades on top,
//! forming bars in real time. The feed and symbol can be switched live from the
//! chart. Frame time and feed lag are surfaced on screen and in structured logs.

use eframe::egui;
use tracing_subscriber::EnvFilter;

use crate::state::BarSpec;

mod app;
mod bands;
mod bubble_presets;
mod candle_view;
mod chart;
mod chart_layers;
mod config;
mod dock;
mod drawings;
mod feed;
mod indicator_legend;
mod indicator_panel;
mod indicator_render;
mod indicator_worker;
mod indicators;
mod live_strip;
mod loading;
mod metrics;
mod notice_card;
mod orderflow;
mod orderflow_engine;
mod orderflow_render;
mod orderflow_view;
mod orderflow_worker;
mod pane;
mod paper_hud;
mod paper_state;
mod paper_trading;
mod price_view;
mod replay_view;
mod resample;
mod state;
mod statusbar;
mod style;
mod symbols_file;
mod tab;
mod tabstrip;
mod theme;
mod time_header;
mod timezone;
mod toolbar;
mod toolrail;
mod trade_paint;
mod ui_state;
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
    let (mut config, source) = match config::load() {
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
    if let Err(e) = config::apply_startup_selection_from_env(&mut config) {
        tracing::error!(
            target: "quantick::app",
            event_code = "STARTUP_SELECTION_ERROR",
            %e,
            "cannot apply startup feed/symbol selection; fix or unset QUANTICK_DEFAULT_FEED and QUANTICK_DEFAULT_SYMBOL"
        );
        std::process::exit(1);
    }

    // The saved workspace, read before anything opens: the market its first
    // tab was on is the market this window has to spawn, because a feed is
    // started here and handed to the app already streaming.
    //
    // Filtered against the config that was just loaded, so a feed or symbol
    // that has since left it cannot decide what opens. `QUANTICK_DEFAULT_FEED`
    // and `QUANTICK_DEFAULT_SYMBOL` were applied to the config above and win
    // over the file — an env var is an explicit request for this one run.
    let workspace = ui_state::load(&ui_state::default_path()).restore(&config);
    let env_chose_market = config::startup_selection_came_from_env();
    if !env_chose_market && let Some((feed, symbol)) = workspace.first_market() {
        config.default_feed = feed.to_owned();
        config.default_symbol = symbol.to_owned();
    }

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

    // The bar type the chart opens on: the rule the saved workspace last read
    // this market on, else the feed's declared `default_bars`, else the
    // factory tick spec. The workspace comes first because it is the user's
    // own answer; `default_bars` is what a feed suggests to a tab that has
    // none.
    let spec = workspace
        .tabs
        .first()
        .filter(|_| !env_chose_market)
        .and_then(|tab| BarSpec::parse(&tab.flow_bars).ok())
        .or_else(|| config.startup_spec_for(&feed_id))
        .unwrap_or(BarSpec::Tick(INITIAL_TICK_SIZE));

    let feed = feed::spawn_live(provider, &symbol, &config);

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("bundled assets/icon.png is a valid PNG");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size(workspace.window))
            .with_min_inner_size(MIN_WINDOW_PX)
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
            Ok(Box::new(app::QuantickApp::new_with_workspace(
                config, feed_id, symbol, spec, feed, workspace,
            )))
        }),
    )
}

/// Size the window opens at when nothing asks for another.
const DEFAULT_WINDOW_PX: [f32; 2] = [1100.0, 650.0];
/// Smallest the window may be. Below this the drawing rail would fall past its
/// Minimal stage (191 px of long axis, docs/drawing-toolbar-ux.md §2.8) and
/// clip chrome instead of collapsing it.
const MIN_WINDOW_PX: [f32; 2] = [900.0, 560.0];

/// Size the window opens at: `QUANTICK_WINDOW_SIZE=WxH` when it is set, else
/// the `saved` size from the workspace, else [`DEFAULT_WINDOW_PX`].
///
/// The env var wins over the saved size for the same reason it wins over the
/// saved market: it is an explicit request for this one run, and a validation
/// run asking for a small window must get one whatever the last session left
/// behind.
///
/// The hook exists because window size is not decoration here — it is what
/// decides whether the indicator band has room for its panes and whether the
/// time axis has room for its labels. Without a way to ask for a small window,
/// that entire class of defect is invisible to any validation that is not a
/// human dragging a corner.
///
/// Clamped to the same minimum the window itself enforces, so the hook can
/// reach the smallest real layout and no smaller. A value that does not parse
/// is ignored with a warning rather than failing the launch: a malformed
/// env var must not stand between the user and their chart.
fn window_size(saved: Option<[f32; 2]>) -> [f32; 2] {
    // A saved size is clamped to the same minimum the hook and the window
    // itself enforce: a workspace written on a larger screen must not open a
    // window below the layout floor on a smaller one.
    let fallback = saved.map_or(DEFAULT_WINDOW_PX, |[width, height]| {
        [width.max(MIN_WINDOW_PX[0]), height.max(MIN_WINDOW_PX[1])]
    });
    let Ok(raw) = std::env::var("QUANTICK_WINDOW_SIZE") else {
        return fallback;
    };
    match parse_window_size(&raw) {
        Some(size) => {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_WINDOW_SIZE_OVERRIDE",
                width = size[0],
                height = size[1],
                "opening at the requested window size"
            );
            size
        }
        None => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_WINDOW_SIZE_REJECTED",
                value = %raw,
                action = "using_default",
                "QUANTICK_WINDOW_SIZE is not WIDTHxHEIGHT"
            );
            fallback
        }
    }
}

/// `WIDTHxHEIGHT` in pixels, clamped to the window's own minimum. `None` when
/// the text is not two positive numbers.
fn parse_window_size(raw: &str) -> Option<[f32; 2]> {
    let (width, height) = raw.split_once(['x', 'X'])?;
    let width: f32 = width.trim().parse().ok()?;
    let height: f32 = height.trim().parse().ok()?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some([width.max(MIN_WINDOW_PX[0]), height.max(MIN_WINDOW_PX[1])])
}

#[cfg(test)]
mod window_size_tests {
    use super::*;

    #[test]
    fn a_requested_size_is_honoured_and_floored_at_the_windows_own_minimum() {
        assert_eq!(parse_window_size("1280x720"), Some([1280.0, 720.0]));
        assert_eq!(parse_window_size(" 1280 X 720 "), Some([1280.0, 720.0]));
        assert_eq!(
            parse_window_size("200x100"),
            Some(MIN_WINDOW_PX),
            "the hook reaches the smallest real layout and no smaller"
        );
    }

    #[test]
    fn a_malformed_size_is_rejected_rather_than_guessed_at() {
        for raw in ["", "1280", "1280x", "x720", "wide x tall", "0x0", "-5x10"] {
            assert_eq!(parse_window_size(raw), None, "{raw:?}");
        }
    }
}
