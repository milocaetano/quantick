//! quantick-app — desktop chart rendering alternative bars from live trades.
//!
//! A consumer of `quantick-engine`, never the other way around. On startup it
//! reads the feed/asset configuration (see [`config`]), recovers the factual
//! recent trades available from that source, then streams live trades on top,
//! forming bars in real time. The feed and symbol can be switched live from the
//! chart. Frame time and feed lag are surfaced on screen and in structured logs.

use eframe::egui;
use tracing_subscriber::EnvFilter;

use quantick_feed as feed;
// The kept exit ladders moved into the paper account's crate; the name stays
// at the crate root so every `crate::order_strategies::…` still resolves.
use quantick_paper::order_strategies;

use crate::state::BarSpec;

#[cfg(test)]
#[path = "../../engine/tests/support/seventh_bar.rs"]
mod bar_extension_fixture;

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
mod deal_recording;
mod deal_recording_tab;
mod deal_recording_ui;
mod dock;
mod drawings;
mod feed_notice;
mod footprint_config;
mod footprint_panel;
mod footprint_presets;
mod footprint_render;
mod footprint_series;
mod frvp;
mod harness;
mod hooks;
mod indicator_guide;
mod indicator_legend;
mod indicator_panel;
mod indicator_render;
mod indicator_style;
mod indicator_worker;
mod indicators;
mod launch;
mod layout_picker;
mod layout_strip;
mod layouts;
mod live_envelope;
#[cfg(test)]
mod live_envelope_tests;
mod live_strip;
mod loading;
mod metrics;
mod operability;
mod orderflow_render;
mod orderflow_view;
mod orderflow_worker;
mod pane;
mod paper_account;
mod paper_calendar;
mod paper_chrome;
mod paper_home;
mod paper_hud;
mod paper_report;
mod paper_state;
mod paper_trading;
mod plot_area;
mod pointer_compass;
mod popup;
mod price_view;
mod replay_get_data;
mod replay_home;
mod replay_view;
mod resample;
mod risk_sizing;
mod scratch;
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
mod worker_backlog;
mod worker_progress;
mod workspace_bundle;
mod workspace_store;

// The test binary counts heap work per thread (`work_meter`); production
// builds keep the system allocator.
#[cfg(test)]
mod work_meter;
#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: work_meter::Counting = work_meter::Counting;

/// The bar type the chart opens on. The type and its parameter are tunable live
/// from the controls bar; the feed and symbol come from the configuration.
const INITIAL_TICK_SIZE: u64 = 50;

/// Install the tracing subscriber. Feed and app events flow to stderr; the level
/// is controlled by `RUST_LOG` (default `quantick=info`). Set
/// `QUANTICK_LOG_FORMAT=json` for newline-delimited JSON that an operator or an
/// AI diagnostic tool can parse without scraping prose. Deterministic cores emit
/// nothing, so logging can never affect replay results.
fn init_tracing(format: launch::LogFormat) {
    if format == launch::LogFormat::Json {
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

/// Write a rendered dump, or fail loudly without writing a byte.
///
/// Never a panic and never a partial write. A caller redirects this over a
/// committed file, and the shell truncates that file before the process starts
/// — so half a document on stdout is a corrupted index the next `git diff`
/// reports as a real change. Either the whole thing lands, or nothing does and
/// the exit code says so.
fn emit(rendered: Result<String, String>) {
    use std::io::Write as _;

    let markdown = match rendered {
        Ok(markdown) => markdown,
        Err(error) => {
            eprintln!("quantick-app: cannot render the dump: {error}");
            std::process::exit(1);
        }
    };
    // `print!` swallows the write error and panics on a broken pipe, which
    // would leave a truncated committed index behind an exit code of 0 — the
    // opposite of what this function promises.
    let mut stdout = std::io::stdout();
    if let Err(error) = stdout.write_all(markdown.as_bytes()) {
        eprintln!("quantick-app: cannot write the dump: {error}");
        std::process::exit(1);
    }
    if let Err(error) = stdout.flush() {
        eprintln!("quantick-app: cannot flush the dump: {error}");
        std::process::exit(1);
    }
}

/// Offline dump paths, served before anything opens a window.
///
/// The generated indexes under `docs/` and `.claude/skills/` are produced from
/// here rather than from a separate tool, because the registries they describe
/// are private to this binary — `crates/app` has no library target, so no
/// `examples/` binary and no sibling crate can reach them the way
/// `crates/control/examples/export_schemas.rs` reaches the wire schemas.
///
/// Returns whether the argument named a dump, so `main` can exit before
/// touching a config file or a display.
fn run_dump_subcommand(argument: &str) -> bool {
    match argument {
        "--dump-capability-inventory" => {
            emit(control::inventory::capability_inventory_markdown());
            true
        }
        "--dump-retry-matrix" => {
            emit(control::retry_matrix::retry_matrix_markdown());
            true
        }
        "--dump-hook-registry" => {
            emit(hooks::hook_registry_markdown());
            true
        }
        "--dump-ui-behaviour-matrix" => {
            emit(Ok(operability::matrix::ui_behaviour_matrix_markdown()));
            true
        }
        _ => false,
    }
}

fn main() -> eframe::Result {
    // Before tracing, before the config read, before the window: a dump is a
    // pure function of the registries and must not depend on a loadable
    // configuration or a usable display.
    if let Some(argument) = std::env::args().nth(1)
        && run_dump_subcommand(&argument)
    {
        return Ok(());
    }

    #[cfg(feature = "drawing-harness")]
    let toolrail = toolrail::ToolRailLaunch::capture(|name| std::env::var_os(name));
    let startup = launch::StartupConfig::capture(|name| std::env::var_os(name));
    #[cfg(feature = "quick-range-harness")]
    let quick_range =
        surfaces::drawing_chrome::QuickRangeLaunch::capture(|name| std::env::var_os(name));
    #[cfg(feature = "drawing-harness")]
    let drawing_chrome =
        surfaces::drawing_chrome::DrawingChromeLaunch::capture(|name| std::env::var_os(name));
    init_tracing(startup.log_format);

    // Immediately after the subscriber exists, so a mistyped hook is the first
    // thing the run says rather than something inferred later from a surface
    // that never opened.
    hooks::log_unknown_hooks();

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
        .and_then(|tab| {
            quantick_engine::bar_registry::BUILTIN_BARS
                .parse(&tab.flow_bars)
                .ok()
        })
        .or_else(|| config.startup_spec_for(&feed_id))
        .unwrap_or_else(|| BarSpec::Tick(INITIAL_TICK_SIZE).into());

    let feed = feed::spawn_live(
        provider,
        &symbol,
        &config.metatrader,
        paper_home::shelf_dir(),
    );

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
            .with_inner_size(startup.window_size(workspace.window))
            .with_title("quantick")
            .with_icon(icon),
        ..Default::default()
    };

    let launch = app::AppLaunch {
        window: startup.into_window_state(),
        #[cfg(feature = "drawing-harness")]
        toolrail,
        #[cfg(all(test, not(feature = "drawing-harness")))]
        toolrail: Default::default(),
        #[cfg(feature = "quick-range-harness")]
        quick_range,
        #[cfg(feature = "drawing-harness")]
        drawing_chrome,
        #[cfg(all(test, not(feature = "drawing-harness")))]
        drawing_chrome: Default::default(),
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
                config, feed_id, symbol, spec, feed, workspace, launch,
            );
            // The window itself, which only this closure is handed: the app
            // measures its real client area through it (see
            // `crate::window_scale`).
            app.attach_surface(cc);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod worker_progress_bench;
#[cfg(test)]
mod worker_progress_bench_observer;
