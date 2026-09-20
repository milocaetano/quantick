//! quantick-app — desktop chart rendering alternative bars from live trades.
//!
//! A consumer of `quantick-engine`, never the other way around. On startup it
//! reads the feed/asset configuration (see [`config`]), recovers the factual
//! recent trades available from that source, then streams live trades on top,
//! forming bars in real time. The feed and symbol can be switched live from the
//! chart. Frame time and feed lag are surfaced on screen and in structured logs.


use quantick_feed as feed;
// The kept exit ladders moved into the paper account's crate; the name stays
// at the crate root so every `crate::order_strategies::…` still resolves.
use quantick_paper::order_strategies;


#[cfg(test)]
#[path = "../../engine/tests/support/seventh_bar.rs"]
mod bar_extension_fixture;

mod app;
// The headless chart model, under its old module names so every path in
// this crate keeps its address.
use quantick_chart::geometry as chart;
#[cfg(test)]
use quantick_chart::work_meter;
use quantick_chart::{indicator_style, live_strip, price_view, state, style, viewport};
mod audio;
mod avwap;
mod bands;
mod bubble_presets;
mod candle_view;
mod canvas_layout;
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
mod frvp;
mod harness;
mod hooks;
mod indicator_guide;
mod indicator_legend;
mod indicator_panel;
mod indicator_render;
mod indicator_worker;
mod indicators;
mod launch;
mod layout_picker;
mod layout_strip;
mod layouts;
mod live_envelope;
#[cfg(test)]
mod live_envelope_tests;
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
mod replay_get_data;
mod replay_home;
mod replay_view;
mod resample;
mod risk_sizing;
mod scratch;
mod statusbar;
mod store_home;
mod strategy_anchors;
mod strategy_presets;
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
mod widgets;
mod window_scale;
mod worker_progress;
mod workspace_bundle;
mod workspace_picker;
mod workspace_store;

/// One line, so the crate root stays the module list it is: everything the
/// process does before a window exists lives in [`launch::boot`].
fn main() -> eframe::Result {
    launch::boot::run()
}

// The test binary counts heap work per thread (`work_meter`); production
// builds keep the system allocator.
#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: work_meter::Counting = work_meter::Counting;

// Test-only, and filed as tests so the sidecar rule can see they are:
// benchmarks the ordinary suite runs, never the binary.
#[cfg(test)]
#[path = "worker_progress/tests/bench.rs"]
mod worker_progress_bench;
#[cfg(test)]
#[path = "worker_progress/tests/bench_observer.rs"]
mod worker_progress_bench_observer;
