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
mod audio;
mod avwap;
mod bands;
mod bubble_presets;
mod candle_view;
mod canvas_layout;
mod chart;
mod chart_layers;
mod config;
mod control;
mod dock;
mod drawings;
mod feed;
mod feed_notice;
mod footprint_config;
mod footprint_panel;
mod footprint_presets;
mod footprint_render;
mod footprint_series;
mod frvp;
mod history_reach;
mod indicator_legend;
mod indicator_panel;
mod indicator_render;
mod indicator_style;
mod indicator_worker;
mod indicators;
mod layout_picker;
mod layout_strip;
mod layouts;
mod live_strip;
mod loading;
mod metrics;
mod order_strategies;
mod orderflow;
mod orderflow_engine;
mod orderflow_render;
mod orderflow_view;
mod orderflow_worker;
mod pane;
mod paper_calendar;
mod paper_home;
mod paper_hud;
mod paper_state;
mod paper_trading;
mod pointer_compass;
mod popup;
mod price_view;
mod replay_download;
mod replay_get_data;
mod replay_home;
mod replay_view;
mod resample;
mod state;
mod statusbar;
mod store_home;
mod strategy_anchors;
mod strategy_presets;
mod style;
mod surfaces;
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
mod window_scale;
mod workspace_bundle;

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
    // Before the first store is read: bring a cockpit left in this launch
    // directory into the durable home, once. Every launch before this change
    // wrote its arrangement beside wherever the app was started from, so the
    // trader's real cockpit may still be sitting in one of them — see
    // `store_home`. Copies only, and never over a home file that already
    // exists, so it is safe to run and safe to have run.
    if let Some(rescue) = store_home::consolidate_once()
        && rescue.copied > 0
    {
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "COCKPIT_HOME_READY",
            copied = rescue.copied,
            action = "opened_on_rescued_cockpit",
            "brought the cockpit into its durable home"
        );
    }

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
        // No `with_min_inner_size`: the window has no floor. Below roughly
        // 900x560 the chrome stops collapsing and starts clipping — the
        // drawing rail falls past its Minimal stage
        // (docs/drawing-toolbar-ux.md §2.8) — but that is a layout that reads
        // badly, not one that breaks, and a trader parking the chart in a
        // sliver beside another window is a real thing to want. The one place
        // a floor is still kept is what the app *reopens* at; see
        // [`REOPEN_FLOOR_PX`].
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size(workspace.window))
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
            let mut app = app::QuantickApp::new_with_workspace(
                config, feed_id, symbol, spec, feed, workspace,
            );
            // The window itself, which only this closure is handed: the app
            // measures its real client area through it (see
            // `crate::window_scale`).
            app.attach_surface(cc);
            Ok(Box::new(app))
        }),
    )
}

/// Size the window opens at when nothing asks for another.
const DEFAULT_WINDOW_PX: [f32; 2] = [1100.0, 650.0];
/// Smallest window the app will *reopen* at, whatever the last session left
/// behind.
///
/// The window itself has no minimum — it drags down to nothing, which is the
/// point. But a size is remembered across launches, and a chart squeezed to a
/// sliver and then closed would come back as a sliver: a window with no title
/// bar to grab and no edge to find. That is a trap the trader cannot get out
/// of from inside the app, so the *restore* path floors what it reads.
///
/// Small enough to be a deliberately tiny window, large enough to have an edge
/// and a title bar to drag. It is a recovery floor, not a layout one: nothing
/// about the chrome is promised at this size.
///
/// `QUANTICK_WINDOW_SIZE` is not floored — it is an explicit request for this
/// one run, made by someone who can unset it, and reaching a degenerate layout
/// on purpose is exactly what that hook is for.
const REOPEN_FLOOR_PX: [f32; 2] = [320.0, 240.0];

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
/// Not clamped: the hook can ask for a window of any positive size, including
/// one far too small to lay anything out in, because a validation run proving
/// the app survives a degenerate window has to be able to *ask* for one. A
/// value that does not parse is ignored with a warning rather than failing
/// the launch: a malformed env var must not stand between the user and their
/// chart.
fn window_size(saved: Option<[f32; 2]>) -> [f32; 2] {
    let fallback = restore_size(saved);
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

/// The size a saved workspace reopens at, floored at [`REOPEN_FLOOR_PX`].
///
/// Not to protect the layout, which is free to be cramped, but so a session
/// closed on a sliver of a window reopens on something the trader can grab.
/// Split out from [`window_size`] because it is the whole of the restore
/// policy and reads no environment, so a test can state it without touching a
/// process-wide variable other tests are reading at the same time.
fn restore_size(saved: Option<[f32; 2]>) -> [f32; 2] {
    saved.map_or(DEFAULT_WINDOW_PX, |[width, height]| {
        [
            width.max(REOPEN_FLOOR_PX[0]),
            height.max(REOPEN_FLOOR_PX[1]),
        ]
    })
}

/// `WIDTHxHEIGHT` in pixels, as asked for. `None` when the text is not two
/// positive numbers.
fn parse_window_size(raw: &str) -> Option<[f32; 2]> {
    let (width, height) = raw.split_once(['x', 'X'])?;
    let width: f32 = width.trim().parse().ok()?;
    let height: f32 = height.trim().parse().ok()?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some([width, height])
}

#[cfg(test)]
mod window_size_tests {
    use super::*;

    #[test]
    fn a_requested_size_is_honoured_however_small_it_is() {
        assert_eq!(parse_window_size("1280x720"), Some([1280.0, 720.0]));
        assert_eq!(parse_window_size(" 1280 X 720 "), Some([1280.0, 720.0]));
        assert_eq!(
            parse_window_size("200x100"),
            Some([200.0, 100.0]),
            "the hook reaches a degenerate layout on purpose"
        );
        assert_eq!(
            parse_window_size("1x1"),
            Some([1.0, 1.0]),
            "there is no floor left to hit"
        );
    }

    /// The window drags to nothing, but a session closed on nothing must not
    /// reopen on nothing: the restore path is the one place a floor survives,
    /// and it is a recovery floor, not a layout one.
    #[test]
    fn a_saved_sliver_reopens_on_something_the_trader_can_grab() {
        assert_eq!(restore_size(Some([0.0, 0.0])), REOPEN_FLOOR_PX);
        assert_eq!(
            restore_size(Some([4.0, 900.0])),
            [REOPEN_FLOOR_PX[0], 900.0]
        );
        assert_eq!(restore_size(Some([1280.0, 720.0])), [1280.0, 720.0]);
        assert_eq!(restore_size(None), DEFAULT_WINDOW_PX);
    }

    #[test]
    fn a_malformed_size_is_rejected_rather_than_guessed_at() {
        for raw in ["", "1280", "1280x", "x720", "wide x tall", "0x0", "-5x10"] {
            assert_eq!(parse_window_size(raw), None, "{raw:?}");
        }
    }
}
