//! The egui application: drains the live feed, renders bars, surfaces metrics,
//! and lets the user switch bar type live.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! and the bar-type dispatch in [`crate::state`] (pure, tested), and metric math
//! in [`crate::metrics`] (pure, tested). This layer owns the clocks, the tracing
//! and the widgets, drains the feed each frame, and turns everything into egui
//! shapes.
//!
//! What is on the canvas lives one layer down, in [`crate::pane`]: the bar
//! series, the viewport, the drawings and the indicator slots belong to a
//! [`ChartPane`], not to the window. The market feeding that pane — channels,
//! connection state, notices, history counters — is still owned here.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::candle_view::draw_style_window;
use crate::chart::PriceScale;
use crate::config::AppConfig;
use crate::dock::{Dock, DockEnv, DockTab};
use crate::drawings::{
    self, DeleteOutcome, MAX_DRAWING_FILL_ALPHA, MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX,
};
use crate::feed::{self, FeedCommand, FeedHandle};
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::library::ScriptLibrary;
use crate::indicators::state_file::{self, SavedIndicator, SavedInput, SavedKind};
use crate::loading::{self, LoadingTask};
use crate::metrics::{self, FrameStats};
use crate::notice_card;
use crate::pane::{self, ChartPane, DRAWING_ANCHOR_RADIUS_PX, PaneSide};
use crate::replay_view::{ReplayAction, ReplayView};
use crate::state::BarSpec;
use crate::statusbar;
use crate::style::{CandlePreset, ChartStyle};
use crate::tab::{CanvasChrome, CanvasLayout, Tab};
use crate::tabstrip::{self, PickerOutcome, SourcePicker, TabAction};
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolbar::{self, ToolbarAction};
use crate::toolrail::{Tool, ToolRail};

/// Width of the right-hand price-axis gutter, in pixels (§5 zone 9).
const AXIS_GUTTER: f32 = 64.0;
/// Height of the bottom time-axis strip, in pixels (§5 zone 6).
const TIME_STRIP: f32 = 24.0;
/// Id of the tab the window opens with.
const FIRST_TAB_ID: u64 = 0;

/// The (flow, time) pane ids for tab `id`.
///
/// Pane ids namespace every egui interaction a pane registers, so they have to
/// be unique across the whole window, not just within a tab — two tabs sharing
/// them would share a drag the moment both had been on screen.
const fn pane_ids(tab: u64) -> (u64, u64) {
    (tab * 2, tab * 2 + 1)
}
/// Initial position of the selected-drawing inspector.
const DRAWING_INSPECTOR_DEFAULT_POSITION: egui::Pos2 = egui::pos2(90.0, 120.0);
/// Length of the EMA the toolbar's hardcoded M1 entry adds (the settings UI
/// generated from `InputSpec` replaces this in M4).
const DEFAULT_EMA_LEN: usize = 9;
/// How often the hot-reload poll checks script files for changes.
const SCRIPT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1_000);
/// How long after the last indicator change the state file is written.
const INDICATOR_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(1_000);
/// Inspector width bounds (UX spec: resizable between 300 and 440 px).
const INSPECTOR_MIN_WIDTH_PX: f32 = 300.0;
/// See [`INSPECTOR_MIN_WIDTH_PX`].
const INSPECTOR_MAX_WIDTH_PX: f32 = 440.0;
/// Default inspector width for the shipped tools (the spec reserves 360 px
/// for the Fib level editor).
const INSPECTOR_DEFAULT_WIDTH_PX: f32 = 320.0;
/// Default inspector width for tools that mount a level editor tab.
const INSPECTOR_LEVELS_WIDTH_PX: f32 = 360.0;
/// Gap kept between the inspector and the selected object's bounding box.
const INSPECTOR_OBJECT_GAP_PX: f32 = 12.0;
/// Assumed inspector height for placement before its first frame reports one.
const INSPECTOR_FALLBACK_HEIGHT_PX: f32 = 280.0;
/// Dragging the price field across the whole visible range takes this many
/// steps, whatever the symbol's price magnitude.
const PRICE_DRAG_STEPS: f64 = 200.0;
/// DragValue speed of bar-index coordinates, in bars per drag point.
const BAR_DRAG_SPEED: f64 = 0.25;
/// Where the object manager first opens: under the toolbox's home corner.
const DRAWING_MANAGER_DEFAULT_POSITION: egui::Pos2 = egui::pos2(70.0, 140.0);
/// How long the delete toast keeps its Undo affordance on screen (UX spec).
const TOAST_UNDO_MS: u64 = 8_000;
/// Horizontal offset of a duplicated drawing, so the copy is visibly a copy.
const DUPLICATE_OFFSET_BARS: f32 = 2.0;
/// Vertical clearance between the toast and the bottom chrome.
const TOAST_BOTTOM_MARGIN_PX: f32 = 44.0;

/// Transient confirmation of a destructive drawing command, with its escape
/// hatch. Undo works from the button for [`TOAST_UNDO_MS`] and from Ctrl+Z
/// for as long as the history holds.
#[derive(Debug)]
struct DrawingToast {
    message: &'static str,
    shown_at: Instant,
    /// Whether the toast offers Undo. A delete does; the honest clear after
    /// a bar rebuild does not — its history is gone with the drawings, and
    /// a dead Undo button would lie.
    offers_undo: bool,
}

/// Which inspector tab is open. Tabs exist per capability: every tool gets
/// Style and Coordinates; a tool that brings its own tab (the Fib level
/// editor) mounts it as Extra without the central code knowing its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InspectorTab {
    #[default]
    Style,
    Extra,
    Coordinates,
}

/// What the inspector body asked for this frame. The caller owns every
/// mutation, so the pinned panel and the floating window share one rule set.
#[derive(Debug, Default, Clone, Copy)]
struct InspectorActions {
    toggle_hidden: bool,
    toggle_lock: bool,
    toggle_pin: bool,
    delete: bool,
    cancel_delete: bool,
    force_delete: bool,
    close: bool,
    edited: bool,
}

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);
/// Coalesce slider drags into one diagnostic event after the value settles.
const STYLE_LOG_DEBOUNCE: Duration = Duration::from_millis(350);

/// Split the padded plot area into the candle chart, the indicator panes, the
/// optional live strip, the right price gutter and the bottom time strip, so
/// the input handler and the renderer agree on the boundaries.
/// `live_strip_width` of zero means the strip is off and the chart runs
/// straight into the gutter, exactly as it did before the strip existed.
///
/// `pane_count` is the number of *visible* pane indicators: the band they
/// claim is carved here, once, rather than by each caller — a chart rect that
/// two call sites disagree about is two price scales for the same pixels.
pub fn plot_split(area: egui::Rect, live_strip_width: f32, pane_count: usize) -> PlotAreas {
    let plot = area.shrink(16.0);
    let strip_width = live_strip_width.max(0.0);
    let gutter_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_x = (gutter_x - strip_width).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    let body = egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y));
    let (chart, indicator_panes) = crate::indicators::split_panes(body, pane_count);
    PlotAreas {
        chart,
        indicator_panes,
        live_strip: (strip_width > 0.0).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(split_x, plot.top()),
                egui::pos2(gutter_x, split_y),
            )
        }),
        price_gutter: egui::Rect::from_min_max(
            egui::pos2(gutter_x, plot.top()),
            egui::pos2(plot.right(), split_y),
        ),
        time_strip: egui::Rect::from_min_max(
            egui::pos2(plot.left(), split_y),
            egui::pos2(split_x, plot.bottom()),
        ),
    }
}

/// Whether a pointer at `x` is over the live lane rather than the candles.
///
/// The divider itself counts as the lane, so the gesture that resizes it and
/// the gesture that pans the candles can never both fire on the same pixel.
/// Without a lane every pixel belongs to the candles, exactly as before.
pub fn gesture_hits_lane(divider_x: Option<f32>, x: f32) -> bool {
    divider_x.is_some_and(|divider| x >= divider)
}

/// Split the bottom time strip at the lane's divider: the candles' own time
/// axis on the left, the lane's on the right.
///
/// Each pane zooms from the strip under it, which is the only place a zoom
/// gesture can say *which* time axis it means. Without a divider the whole
/// strip belongs to the candles, exactly as it did before the lane had a zoom.
pub fn split_time_strip(
    strip: egui::Rect,
    divider_x: Option<f32>,
) -> (egui::Rect, Option<egui::Rect>) {
    let Some(divider) = divider_x.filter(|x| strip.x_range().contains(*x)) else {
        return (strip, None);
    };
    (
        egui::Rect::from_min_max(strip.min, egui::pos2(divider, strip.bottom())),
        Some(egui::Rect::from_min_max(
            egui::pos2(divider, strip.top()),
            strip.max,
        )),
    )
}

/// The interactive regions of the plot, plus the optional live strip.
pub struct PlotAreas {
    /// The candle body, with the indicator pane band already taken out of it.
    /// Every consumer — renderer and input handler alike — reads the chart
    /// rect from here, which is what keeps the price scale a drawing is
    /// placed against identical to the one it is hit-tested against.
    pub chart: egui::Rect,
    /// Stacked indicator panes below the candles, top to bottom. Empty when
    /// no pane indicator is visible.
    pub indicator_panes: Vec<egui::Rect>,
    /// Present only while the strip is shown; sits between `chart` and
    /// `price_gutter` and is not an input region.
    pub live_strip: Option<egui::Rect>,
    pub price_gutter: egui::Rect,
    pub time_strip: egui::Rect,
}

/// Format the forming bar's countdown, e.g. `37/50 ticks`.
///
/// Trailing zeros are trimmed on both figures: a volume bar's accumulator
/// carries the feed's own scale, and `1.20000000/5 vol` reads as noise.
fn fmt_progress(progress: &quantick_engine::BarProgress, unit: &str) -> String {
    format!(
        "{}/{} {unit}",
        progress.done.normalize(),
        progress.target.normalize()
    )
}

/// Format a duration in milliseconds for the lane's time axis, in the unit a
/// human would read it in.
pub fn fmt_window(milliseconds: i64) -> String {
    let milliseconds = milliseconds.max(0);
    if milliseconds < 1_000 {
        format!("{milliseconds} ms")
    } else if milliseconds < 60_000 {
        format!("{:.1} s", milliseconds as f64 / 1_000.0)
    } else {
        format!("{:.1} min", milliseconds as f64 / 60_000.0)
    }
}

/// Format a UTC epoch-millisecond timestamp as `HH:MM:SS` in the display
/// timezone `tz`, for the time axis.
pub fn fmt_time(ms: i64, tz: TzOffset) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let secs = local.div_euclid(1000).rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// The quantick chart window.
///
/// One workspace: N open markets ([`Tab`]) and the chrome around them. The
/// chrome is what is single-instance by nature — one menu bar, one toolbox,
/// one dock, one appearance, one status line — plus the indicator persistence
/// layer, which describes a workspace rather than a market.
pub struct QuantickApp {
    /// The open markets, left to right as the strip shows them (§11).
    ///
    /// Retained trades are O(trades × panes × open tabs): every tab keeps its
    /// own history and a split tab keeps it twice. Nothing caps the count —
    /// the strip is as long as the user makes it.
    tabs: Vec<Tab>,
    /// Which of them is on screen. Every tab drains every frame; only this one
    /// renders, and the chrome speaks for it.
    active_tab: usize,
    /// Handed out to new tabs and never reused, so a closed tab's ids can
    /// never be mistaken for a living one's.
    next_tab_id: u64,
    /// The tab the indicator state file describes — the one opened from the
    /// config defaults at startup, which is the workspace the file was written
    /// for. Cleared when that tab closes: the set it recorded is gone, and
    /// silently retargeting the file at another market would be a lie. Per-tab
    /// persistence is §14's `ui-state.toml` question and lands with the layout.
    persisted_tab: Option<u64>,
    /// The `+` dialog, while it is open.
    source_picker: Option<SourcePicker>,

    config: AppConfig,

    /// Loadable `.pine` scripts (embedded + indicators dir), scanned at
    /// startup. A file-backed script then follows its file: `poll_script_files`
    /// checks mtimes on a debounce and reloads on a save.
    script_library: ScriptLibrary,
    /// The open indicator-settings dialog, if any (one at a time).
    indicator_settings: Option<SettingsDialog>,
    /// The slot the open dialog edits. Held apart from the dialog so a tab or
    /// pane changing under it cannot retarget its Apply.
    indicator_settings_target: TabSlot,
    /// File-backed script slots: (slot, library index, last seen mtime) —
    /// what the hot-reload poll walks.
    script_files: Vec<(TabSlot, usize, std::time::SystemTime)>,
    /// How each live slot restores (the persistence identity per slot).
    ///
    /// Stays beside the library and the state file rather than moving into the
    /// panes with the slots themselves: one file records what the window had
    /// open, so one list records what is in it.
    slot_kinds: Vec<(TabSlot, SavedKind)>,
    /// Slots restored as hidden, applied when their Rebuilt lands. The
    /// persisted tab's flow pane only, because restoring is (see
    /// [`Self::maintain_indicator_state`]).
    pending_hidden: Vec<SlotId>,
    /// Where the indicator set persists.
    indicator_state_path: std::path::PathBuf,
    /// Set by any add/remove/hide/inputs change; drained by the debounced
    /// save.
    indicator_state_dirty: bool,
    /// When the last indicator change happened (the debounce clock).
    last_indicator_change: Option<Instant>,
    /// Last hot-reload poll instant (the poll runs about once a second;
    /// file metadata every frame would be waste).
    last_script_poll: Instant,

    /// The browser window and, while the active tab replays, the transport.
    replay_view: ReplayView,

    // External chart chrome: the tabbed right dock and the corner-docked
    // drawing toolbox. Neither is painted over the chart canvas.
    dock: Dock,
    toolrail: ToolRail,

    // Delete confirmation for a locked drawing, shown next to the trigger.
    drawing_delete_confirm: bool,
    // Pre-edit copy of the selected drawing while an inspector edit gesture
    // (slider/color/coordinate drag) is in flight; committed as one undo
    // entry once pointer and keyboard let go.
    inspector_edit_baseline: Option<InspectorEdit>,
    drawing_toast: Option<DrawingToast>,
    // Inspector chrome state: open tab, dock pin, whether the user moved the
    // floating window this session (manual position wins over placement),
    // and the selection the last placement was computed for.
    inspector_tab: InspectorTab,
    inspector_pinned: bool,
    inspector_moved: bool,
    inspector_last_selection: Option<usize>,
    drawing_manager_open: bool,
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,
    #[cfg(test)]
    inspector_pin_rect: Option<egui::Rect>,
    #[cfg(test)]
    toast_undo_rect: Option<egui::Rect>,
    #[cfg(test)]
    manager_action_rects: Vec<(usize, &'static str, egui::Rect)>,

    // Candle appearance + whether the style panel is open.
    style: ChartStyle,
    show_style: bool,
    style_revision: u64,
    style_log_pending: bool,
    last_style_change: Option<Instant>,
    // Whether the status bar shows the perf readings (View → perf readings).
    show_perf: bool,

    // Fixed UTC offset the time axis is displayed in (default UTC−03:00).
    tz: TzOffset,

    frames: FrameStats,
    /// CPU time per frame (update + tessellation + paint, no vsync wait), from
    /// eframe. Separates "we are slow" from "we are waiting for the display".
    cpu_frames: FrameStats,
    last_frame: Option<Instant>,
    /// Live trades taken in since the last perf summary, across every tab —
    /// what the window is ingesting, not what one market prints.
    trades_since_summary: u64,
    last_summary: Instant,
}

/// A drawing edit in flight, with the pane it was captured on.
///
/// The commit has to land on *that* pane: focus legitimately moves when the
/// user clicks the other chart or another tab, and the index alone addresses
/// a different object there. Pairing the baseline with its owner is what
/// keeps one gesture one undo entry on one drawing.
struct InspectorEdit {
    tab: u64,
    side: PaneSide,
    index: usize,
    before: drawings::Drawing,
}

/// An indicator slot together with the tab and pane that own it.
///
/// Slot ids are allocated per pane, so the id alone identifies nothing once
/// there are two panes, let alone two tabs: without the rest, removing one
/// tab's slot 0 would drop another's bookkeeping for its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabSlot {
    tab: u64,
    side: PaneSide,
    slot: SlotId,
}

impl QuantickApp {
    /// Create the app on `config`, opening one tab on `feed_id`/`symbol`
    /// (already streaming through `feed`) and bar `spec`.
    #[must_use]
    pub fn new(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
    ) -> Self {
        let tab = Tab::new(
            FIRST_TAB_ID,
            pane_ids(FIRST_TAB_ID),
            feed_id.into(),
            symbol.into(),
            spec,
            feed,
        );
        let mut app = Self {
            tabs: vec![tab],
            active_tab: 0,
            next_tab_id: FIRST_TAB_ID + 1,
            persisted_tab: Some(FIRST_TAB_ID),
            source_picker: None,
            config,
            script_library: ScriptLibrary::scan(),
            indicator_settings: None,
            indicator_settings_target: TabSlot {
                tab: FIRST_TAB_ID,
                side: PaneSide::Flow,
                slot: SlotId(0),
            },
            script_files: Vec::new(),
            slot_kinds: Vec::new(),
            pending_hidden: Vec::new(),
            indicator_state_path: state_file::default_path(),
            indicator_state_dirty: false,
            last_indicator_change: None,
            last_script_poll: Instant::now(),
            replay_view: ReplayView::new(),
            dock: Dock::new(),
            toolrail: ToolRail::new(),
            drawing_delete_confirm: false,
            inspector_edit_baseline: None,
            drawing_toast: None,
            inspector_tab: InspectorTab::default(),
            inspector_pinned: false,
            inspector_moved: false,
            inspector_last_selection: None,
            drawing_manager_open: false,
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            #[cfg(test)]
            inspector_pin_rect: None,
            #[cfg(test)]
            toast_undo_rect: None,
            #[cfg(test)]
            manager_action_rects: Vec::new(),
            style: ChartStyle::default(),
            show_style: false,
            style_revision: 0,
            style_log_pending: false,
            last_style_change: None,
            show_perf: true,
            tz: TzOffset::default(),
            frames: FrameStats::new(120),
            cpu_frames: FrameStats::new(120),
            last_frame: None,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        };
        // Recording is not a display choice: it starts with the feed, so
        // hiding the map later never leaves a hole in what was captured.
        let config = app.config.clone();
        app.active_tab_mut().refresh_chip_label(&config);
        app.active_tab_mut().ensure_book_capture(&config);
        // A feed that declares its own look opens wearing it.
        app.active_tab_mut().apply_feed_bubble_preset(&config);
        // The map itself stays hidden until asked for — a layer nobody
        // requested must cost no projection. Dev/ops can open it without a
        // click; capture is already running either way.
        app.active_tab_mut()
            .tape_mut()
            .set_depth_visible(std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|v| v == "1"));
        // Same convenience for the live strip; its pixels stay
        // capability-gated either way (see live_strip_width).
        if std::env::var("QUANTICK_LIVE_STRIP_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().flow_pane.live_strip_visible = true;
        }
        // Same convenience for the aggression layer (bubbles + the live
        // column's footprint). Same code path as the toolbar toggle.
        if std::env::var("QUANTICK_BUBBLES_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().tape_mut().set_bubbles_enabled(true);
        }
        // Same convenience for indicators: open with the two M1 natives on
        // (EMA overlay + CVD pane), through the same code path the toolbar
        // menu takes, so a scripted validation run needs no clicks.
        if std::env::var("QUANTICK_INDICATORS_AUTOSTART").is_ok_and(|value| value == "1") {
            let pane = &mut app.active_tab_mut().flow_pane;
            pane.add_indicator(IndicatorSource::NativeEma {
                len: DEFAULT_EMA_LEN,
                source: quantick_indicators::SourceId::Close,
            });
            pane.add_indicator(IndicatorSource::NativeCvd);
        }
        // Restore the persisted indicator set before any autostart hook:
        // the file is what the user actually had open.
        app.restore_indicator_state();
        // Scripted validation runs can open with library scripts loaded:
        // a comma-separated list of script names, each through the same
        // code path the INDICATORS menu takes.
        if let Ok(names) = std::env::var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match app
                    .script_library
                    .entries()
                    .iter()
                    .position(|entry| entry.name == name)
                {
                    Some(index) => {
                        app.add_script_indicator(index);
                        // An env var is not a user edit. Without this, a
                        // scripted validation run appended its own scripts to
                        // the saved set and they opened by themselves on the
                        // next plain launch — config presence activating
                        // something, which the rules forbid. The natives hook
                        // above never registers a kind, so it is already inert.
                        app.forget_last_indicator_state_change();
                    }
                    None => tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "INDICATOR_SCRIPT_UNKNOWN",
                        script = %name,
                        action = "autostart_entry_skipped",
                        "autostart names a script the library does not have"
                    ),
                }
            }
        }
        // Same convenience for Market Replay: open the folder named by
        // QUANTICK_REPLAY_DIR and play its first session. One env var, the same
        // code path a click takes, so a scripted run and a person get the same
        // behaviour.
        if std::env::var("QUANTICK_REPLAY_AUTOSTART").is_ok_and(|value| value == "1") {
            let speed = std::env::var("QUANTICK_REPLAY_SPEED")
                .ok()
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|speed| *speed > 0.0)
                .unwrap_or(1.0);
            let started = app.replay_view.autostart(speed);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_AUTOSTART",
                folder = std::env::var(crate::replay_view::REPLAY_DIR_ENV).unwrap_or_default(),
                speed,
                started,
                action = if started { "load_first_session" } else { "open_browser" },
                "market replay autostart"
            );
        }
        app
    }

    /// The active tab beside the config it reads.
    ///
    /// Split here, once, because almost every tab operation needs both and
    /// `self.tabs[i].f(&self.config)` is a borrow error at every call site.
    fn active_with_config(&mut self) -> (&mut Tab, &AppConfig) {
        (&mut self.tabs[self.active_tab], &self.config)
    }

    /// The tab on screen.
    fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// See [`Self::active_tab`].
    fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    /// The pane the chrome speaks for: the active tab's focused pane (§11).
    fn focused_pane(&self) -> &ChartPane {
        self.active_tab().focused_pane()
    }

    /// See [`Self::focused_pane`].
    fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().focused_pane_mut()
    }

    /// The slot a command from the chrome addresses: the active tab, its
    /// focused pane, that slot.
    fn target_slot(&self, slot: SlotId) -> TabSlot {
        TabSlot {
            tab: self.active_tab().id,
            side: self.active_tab().focused_side(),
            slot,
        }
    }

    /// Open `feed_id`/`symbol` in a new tab and make it active.
    ///
    /// Opening a market a tab already holds is allowed — two views of one
    /// book are a legitimate thing to want. For MetaTrader that means two
    /// listeners on one port, and the second one loses the bind: that tab
    /// shows the bridge's own bind-failure notice, which is the honest answer
    /// and the reason `[metatrader.ports]` maps a port per symbol.
    fn open_tab(&mut self, feed_id: String, symbol: String) {
        let Some(provider) = self.config.provider_of(&feed_id) else {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "TAB_OPEN_UNKNOWN_FEED",
                feed = %feed_id,
                action = "ignore_request",
                "asked to open a feed the config does not have"
            );
            return;
        };
        // One feed per tab, resolved per symbol: a MetaTrader tab binds the
        // port `[metatrader.ports]` maps its symbol to (`endpoint_for`), so two
        // MT5 tabs on different symbols listen on different ports and each
        // finds its own bridge. Two tabs on the *same* MT5 symbol is allowed
        // and means one port for two listeners: the second loses the bind and
        // shows the feed's own MT5_BIND_FAILED notice, which is the honest
        // answer rather than a silently dead chart.
        let handle = feed::spawn_live(provider, &symbol, &self.config);
        self.adopt_tab(feed_id, symbol, handle);
    }

    /// Take a market that is already streaming as a new tab, and make it the
    /// active one.
    ///
    /// The bar spec is inherited from the tab you were on: opening a second
    /// market to compare it against the first is the reason to do this, and
    /// landing on a different aggregation would defeat that.
    fn adopt_tab(&mut self, feed_id: String, symbol: String, feed: FeedHandle) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_OPENED",
            tab = id,
            feed = %feed_id,
            symbol = %symbol,
            tabs = self.tabs.len() + 1,
            action = "activate_new_tab",
            "opening a market in a new tab"
        );
        let spec = self.active_tab().flow_pane.state.spec().clone();
        self.tabs
            .push(Tab::new(id, pane_ids(id), feed_id, symbol, spec, feed));
        self.active_tab = self.tabs.len() - 1;
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        self.active_tab_mut().ensure_book_capture(&config);
        self.active_tab_mut().apply_feed_bubble_preset(&config);
    }

    /// Close the tab at `index`, activating a neighbour.
    ///
    /// The last tab stays: a window with no market has nothing to draw. What
    /// the closed tab owned goes with it — dropping its `FeedHandle` closes
    /// the receivers its feed thread sends into, and dropping its panes drops
    /// the indicator worker and book worker handles, whose run loops end when
    /// their command channels disconnect. No joins, no shutdown protocol.
    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        let closed = self.tabs.remove(index);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_CLOSED",
            tab = closed.id,
            feed = %closed.feed_id,
            symbol = %closed.symbol,
            tabs = self.tabs.len(),
            action = "drop_feed_and_workers",
            "closing a market tab"
        );
        // Its slots are gone with its panes; the bookkeeping must not outlive
        // them or a later tab reusing a slot number would inherit its kind.
        self.slot_kinds.retain(|(owner, _)| owner.tab != closed.id);
        self.script_files
            .retain(|(owner, ..)| owner.tab != closed.id);
        if self.persisted_tab == Some(closed.id) {
            // The set the file describes no longer exists. Silently writing
            // some other tab's indicators over it would be a lie about what
            // the workspace was.
            self.persisted_tab = None;
            self.indicator_state_dirty = false;
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_STATE_UNTRACKED",
                tab = closed.id,
                action = "stop_saving_until_restart",
                "the tab the indicator state file describes was closed"
            );
        }
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        drop(closed);
    }

    /// Move `delta` tabs along the strip, wrapping (§10: Ctrl+Tab).
    fn cycle_tab(&mut self, delta: isize) {
        if self.tabs.len() < 2 {
            return;
        }
        let count = self.tabs.len() as isize;
        let next = (self.active_tab as isize + delta).rem_euclid(count);
        self.active_tab = next as usize;
    }

    /// Build the toolbar's model from the app's state, draw it, and carry
    /// out whatever it asked (§6 — the toolbar module owns grouping and the
    /// overflow rule; this method owns the side effects).
    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        // Pre-collect owned option lists so the toolbar's combos don't borrow
        // `self.config` while they mutate `self.feed_id` / `self.active_tab().symbol`.
        // Providers that aren't streaming yet are labelled "(soon)" so the
        // menu is honest about what actually connects.
        let feeds: Vec<(String, String)> = self
            .config
            .feeds
            .iter()
            .map(|f| {
                let label = if f.provider.is_implemented() {
                    f.name.clone()
                } else {
                    format!("{} (soon)", f.name)
                };
                (f.id.clone(), label)
            })
            .collect();
        let symbols: Vec<String> = self
            .config
            .feed(&self.active_tab().feed_id)
            .map(|f| f.symbols.clone())
            .unwrap_or_default();
        // During a replay the SOURCE group gives way to what is actually
        // playing: a live venue cannot be picked without leaving the
        // recording first, and a combo that silently did so would throw away
        // the session mid-run.
        let replay = self
            .active_tab()
            .replay
            .as_ref()
            .map(|link| toolbar::ReplaySource {
                label: link.label(),
                hover: format!(
                    "Replaying {}\nSide source: {}",
                    link.session.path.display(),
                    link.session
                        .header
                        .side_source
                        .as_deref()
                        .unwrap_or("not recorded"),
                ),
            });
        let capabilities = self.active_tab().capabilities(&self.config);
        let feed_display_name = self.active_tab().feed_display_name(&self.config).to_owned();
        let heatmap_on = self.active_tab().tape().depth_visible();
        let bubbles_on = self.active_tab().tape().bubbles_enabled();
        // The focused pane's slots (§11): the menu lists what a command from
        // it would act on, and never the pane beside it.
        let indicators: Vec<toolbar::IndicatorMenuEntry> = self
            .focused_pane()
            .indicators
            .all()
            .iter()
            .map(|view| toolbar::IndicatorMenuEntry {
                slot: view.slot.0,
                label: view.label().to_owned(),
                hidden: view.hidden,
                errored: view.error.is_some(),
                stale: view.stale.is_some(),
            })
            .collect();
        let scripts: Vec<String> = self
            .script_library
            .entries()
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        let dock_visible = self.dock.visible();
        let show_style = self.show_style;
        // The SOURCE and BARS groups write straight into the active tab: a
        // feed or symbol change is that tab's market switch, and the bar spec
        // is its flow pane's.
        let tab = self.active_tab_mut();
        let mut model = toolbar::ToolbarModel {
            feeds,
            feed_id: &mut tab.feed_id,
            feed_display_name,
            symbols,
            symbol: &mut tab.symbol,
            replay,
            kind: &mut tab.flow_pane.kind,
            tick_n: &mut tab.flow_pane.tick_n,
            volume_units: &mut tab.flow_pane.volume_units,
            dollar_notional: &mut tab.flow_pane.dollar_notional,
            time_interval_ms: &mut tab.flow_pane.time_interval_ms,
            imbalance_target: &mut tab.flow_pane.imbalance_target,
            history_step: &mut tab.history_step,
            history_trades: tab.history_trades,
            capabilities,
            heatmap_on,
            bubbles_on,
            live_strip_on: tab.flow_pane.live_strip_visible,
            dock_visible,
            appearance_open: show_style,
            indicators,
            scripts,
        };
        let actions = toolbar::draw(ctx, &mut model);
        // A newly picked feed may not offer the current symbol. Never during
        // a replay: the recorded instrument belongs to no live feed's menu,
        // and snapping it away would relabel the whole session — the status
        // bar and the logs must keep naming what is actually playing.
        if self.active_tab().replay.is_none() {
            let (tab, config) = self.active_with_config();
            tab.ensure_symbol_valid(config);
            tab.refresh_chip_label(config);
        }
        for action in actions {
            self.apply_toolbar_action(action);
        }
    }

    /// One toolbar side effect. Layer toggles reuse the same code paths the
    /// old checkboxes took, so provider gating and command acknowledgement
    /// rules are unchanged.
    fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadOlder => self.active_tab_mut().request_older_history(),
            ToolbarAction::SetHeatmap(shown) => {
                self.active_tab_mut().tape_mut().set_depth_visible(shown);
            }
            ToolbarAction::SetBubbles(enabled) => {
                self.active_tab_mut()
                    .tape_mut()
                    .set_bubbles_enabled(enabled);
            }
            ToolbarAction::SetLiveStrip(shown) => {
                self.active_tab_mut().flow_pane.live_strip_visible = shown;
            }
            ToolbarAction::OpenDockTab(tab) => self.dock.open_tab(tab),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.show_style = !self.show_style,
            // Every indicator command lands on the focused pane (§11), which
            // is the flow pane whenever the canvas is not split.
            ToolbarAction::AddEmaIndicator => {
                self.add_native_indicator(SavedKind::NativeEma);
            }
            ToolbarAction::AddCvdIndicator => {
                self.add_native_indicator(SavedKind::NativeCvd);
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                self.focused_pane_mut()
                    .indicators
                    .toggle_hidden(SlotId(slot));
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let target = self.target_slot(SlotId(slot));
                // UI first (the entry vanishes this frame), worker second;
                // events already in flight for the slot are dropped on apply.
                let pane = self.focused_pane_mut();
                pane.indicators.remove(target.slot);
                pane.indicator_worker
                    .send(IndicatorCommand::Remove(target.slot));
                self.slot_kinds.retain(|(owner, _)| *owner != target);
                self.script_files.retain(|(owner, ..)| *owner != target);
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::AddScriptIndicator(index) => {
                self.add_script_indicator(index);
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let target = self.target_slot(SlotId(slot));
                if let Some(view) = self
                    .focused_pane()
                    .indicators
                    .all()
                    .iter()
                    .find(|v| v.slot == target.slot)
                {
                    self.indicator_settings = Some(SettingsDialog {
                        slot: target.slot,
                        title: view.label().to_owned(),
                        draft: view.input_values.clone(),
                    });
                    self.indicator_settings_target = target;
                }
            }
        }
    }

    /// Draw the settings dialog and execute its outcome. Apply goes through
    /// the worker (construct anew, replace, replay) — the same path every
    /// input change takes, UI or not.
    fn draw_indicator_settings(&mut self, ctx: &egui::Context) {
        let target = self.indicator_settings_target;
        let outcome = {
            let Self {
                indicator_settings,
                tabs,
                ..
            } = self;
            let Some(dialog) = indicator_settings.as_mut() else {
                return;
            };
            // The tab the dialog was opened on may have been closed under it.
            let Some(view) = tabs
                .iter()
                .find(|tab| tab.id == target.tab)
                .map(|tab| tab.pane(target.side))
                .and_then(|pane| {
                    pane.indicators
                        .all()
                        .iter()
                        .find(|view| view.slot == dialog.slot)
                })
            else {
                // The indicator was removed under the dialog.
                *indicator_settings = None;
                return;
            };
            indicator_panel::draw(ctx, dialog, &view.descriptor.inputs)
        };
        match outcome {
            SettingsOutcome::Open => {}
            SettingsOutcome::Cancel => self.indicator_settings = None,
            SettingsOutcome::Apply => {
                let dialog = self.indicator_settings.take().expect("dialog is open");
                // The slot the dialog was opened on, not whatever has focus
                // now: clicking Apply must not retarget the edit.
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
                    tab.pane_mut(target.side)
                        .indicator_worker
                        .send(IndicatorCommand::SetInputs {
                            slot: dialog.slot,
                            values: dialog.draft,
                        });
                }
                self.mark_indicator_state_dirty();
            }
        }
    }

    /// Load a library script behind a fresh slot. A file that no longer
    /// reads or a script that no longer compiles becomes the slot's error —
    /// shown with lines and codes, never silently dropped.
    ///
    /// Returns the slot it claimed, so a caller that needs to address the new
    /// indicator (restoring saved inputs, say) does not have to guess which
    /// one it is.
    fn add_script_indicator(&mut self, index: usize) -> Option<SlotId> {
        let entry = self.script_library.entries().get(index)?;
        let name = entry.name.clone();
        match self.script_library.read(index) {
            Some(Ok(text)) => {
                let slot = self
                    .focused_pane_mut()
                    .add_indicator(IndicatorSource::Script {
                        name: name.clone(),
                        text,
                    });
                let owner = self.target_slot(slot);
                // Watch the file so a save reloads it. Registered here, with
                // the add, so the two cannot drift apart.
                if let Some((_, mtime)) = self.script_library.file_info(index) {
                    self.script_files.push((owner, index, mtime));
                }
                self.slot_kinds.push((owner, SavedKind::Script { name }));
                self.mark_indicator_state_dirty();
                Some(slot)
            }
            Some(Err(message)) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %name,
                    error = %message,
                    action = "error_slot_shown",
                    "cannot read an indicator script"
                );
                // A click that produces nothing at all is the failure this
                // function's own doc comment rules out. The compile half of
                // that promise runs worker-side; the read half never leaves
                // the UI thread, so the error slot is built here, from the
                // same two events the worker would have sent.
                let pane = self.focused_pane_mut();
                let slot = pane.indicators.allocate_slot();
                pane.indicators.apply(IndicatorEvent::Rebuilt {
                    slot,
                    descriptor: quantick_indicators::IndicatorDescriptor {
                        title: name,
                        short_title: None,
                        overlay: false,
                        plots: Vec::new(),
                        fills: Vec::new(),
                        inputs: Vec::new(),
                    },
                    columns: Vec::new(),
                    inputs: Vec::new(),
                    stale: None,
                });
                pane.indicators.apply(IndicatorEvent::Error {
                    slot,
                    error: quantick_indicators::EvalError {
                        bar_index: 0,
                        message,
                    },
                });
                Some(slot)
            }
            None => None,
        }
    }

    /// Draw the modular candle-appearance panel and debounce its diagnostic
    /// event so dragging a slider cannot flood logs at frame rate.
    fn draw_style_panel(&mut self, ctx: &egui::Context, now: Instant) {
        let response = draw_style_window(ctx, &mut self.show_style, &mut self.style);
        if response.changed {
            self.style_revision = self.style_revision.saturating_add(1);
            self.style_log_pending = true;
            self.last_style_change = Some(now);
        }

        let settled = self
            .last_style_change
            .is_some_and(|changed| now.saturating_duration_since(changed) >= STYLE_LOG_DEBOUNCE);
        if self.style_log_pending && (settled || !self.show_style) {
            self.emit_style_changed(response.applied_preset);
        }
    }

    fn emit_style_changed(&mut self, applied_preset: Option<CandlePreset>) {
        let candles = &self.style.candles;
        let preset = applied_preset
            .or_else(|| CandlePreset::detect(candles))
            .map_or("custom", CandlePreset::log_value);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANDLE_STYLE_CHANGED",
            revision = self.style_revision,
            preset,
            body_mode = ?candles.body_mode,
            fill_opacity = candles.fill_opacity,
            outline_opacity = candles.outline_opacity,
            outline_width_px = candles.outline_width,
            body_width_fraction = candles.body_width_frac,
            wick_mode = ?candles.wick_color_mode,
            wick_width_px = candles.wick_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            action = "redraw_only",
            "candle appearance changed"
        );
        self.style_log_pending = false;
    }

    /// Hot reload: about once a second, compare each file-backed script's
    /// mtime; a changed file is re-read and sent as a Reload — recompiled
    /// and replayed on success, or flagged stale (the last good version
    /// keeps running) on errors. The mtime updates even when the compile
    /// fails, so a broken save does not re-fire every second.
    fn poll_script_files(&mut self) {
        if self.script_files.is_empty()
            || self.last_script_poll.elapsed() < SCRIPT_RELOAD_POLL_INTERVAL
        {
            return;
        }
        self.last_script_poll = Instant::now();
        let mut reloads: Vec<(TabSlot, String, String)> = Vec::new();
        for (owner, index, seen_mtime) in &mut self.script_files {
            let Some((path, mtime)) = self.script_library.file_info(*index) else {
                continue;
            };
            if mtime == *seen_mtime {
                continue;
            }
            *seen_mtime = mtime;
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    reloads.push((*owner, name, text));
                }
                Err(error) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %path.display(),
                    error = %error,
                    action = "reload_skipped",
                    "cannot re-read a changed indicator script"
                ),
            }
        }
        for (owner, name, text) in reloads {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_SCRIPT_RELOAD",
                script = %name,
                tab = owner.tab,
                pane = ?owner.side,
                action = "recompile_and_replay",
                "indicator script changed on disk"
            );
            // To the worker that owns the slot: the same script loaded on two
            // panes is two slots, and a Reload sent to the wrong one addresses
            // whatever indicator happens to share its number there.
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == owner.tab) {
                tab.pane_mut(owner.side)
                    .indicator_worker
                    .send(IndicatorCommand::Reload {
                        slot: owner.slot,
                        source: IndicatorSource::Script { name, text },
                    });
            }
        }
    }

    /// Add one of the built-in indicators to the focused pane and register how
    /// it restores.
    fn add_native_indicator(&mut self, kind: SavedKind) -> SlotId {
        let source = match kind {
            SavedKind::NativeCvd => IndicatorSource::NativeCvd,
            // Every other kind is a script, which comes through
            // `add_script_indicator`; EMA is the remaining native.
            _ => IndicatorSource::NativeEma {
                len: DEFAULT_EMA_LEN,
                source: quantick_indicators::SourceId::Close,
            },
        };
        let slot = self.focused_pane_mut().add_indicator(source);
        let owner = self.target_slot(slot);
        self.slot_kinds.push((owner, kind));
        self.mark_indicator_state_dirty();
        slot
    }

    fn mark_indicator_state_dirty(&mut self) {
        self.indicator_state_dirty = true;
        self.last_indicator_change = Some(Instant::now());
    }

    /// Undo the dirty mark an add just set — for indicators an env var asked
    /// for rather than the user. The slot still exists and still works; it
    /// simply does not enter the persisted set.
    fn forget_last_indicator_state_change(&mut self) {
        self.slot_kinds.pop();
        self.indicator_state_dirty = false;
        self.last_indicator_change = None;
    }

    /// Rebuild the persisted set at startup, through the same commands the
    /// menu sends (add, then bind saved inputs, then hide once the view
    /// lands). A script the library no longer has is skipped with a log
    /// line, never a phantom entry.
    ///
    /// The flow pane only, and the file holds only its slots — see
    /// [`Self::maintain_indicator_state`] for why.
    fn restore_indicator_state(&mut self) {
        let saved = state_file::load(&self.indicator_state_path);
        for entry in saved {
            let slot = match &entry.kind {
                SavedKind::NativeEma => Some(self.add_native_indicator(SavedKind::NativeEma)),
                SavedKind::NativeCvd => Some(self.add_native_indicator(SavedKind::NativeCvd)),
                SavedKind::Script { name } => {
                    match self
                        .script_library
                        .entries()
                        .iter()
                        .position(|candidate| candidate.name == *name)
                    {
                        // The returned slot, not `slot_kinds.last()`: that
                        // assumed the add always pushes an entry as its last
                        // act, and it does not when the file no longer reads
                        // — the saved inputs would then bind to whatever
                        // indicator happened to be added before this one.
                        Some(index) => self.add_script_indicator(index),
                        None => {
                            tracing::warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "INDICATOR_STATE_SCRIPT_MISSING",
                                script = %name,
                                action = "entry_skipped",
                                "the saved state references a script the library no longer has"
                            );
                            None
                        }
                    }
                }
            };
            let Some(slot) = slot else { continue };
            let values: Vec<_> = entry
                .inputs
                .iter()
                .filter_map(SavedInput::to_value)
                .collect();
            if !values.is_empty() && values.len() == entry.inputs.len() {
                self.active_tab_mut()
                    .flow_pane
                    .indicator_worker
                    .send(IndicatorCommand::SetInputs { slot, values });
            } else if !entry.inputs.is_empty() {
                // One unreadable cell dropped every input of the entry, in
                // silence — a hand-edited or stale file lost the whole
                // parameter set without a word.
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_STATE_INPUTS_DROPPED",
                    kind = ?entry.kind,
                    saved = entry.inputs.len(),
                    readable = values.len(),
                    action = "declared_defaults_used",
                    "saved indicator inputs could not be read; using the declared defaults"
                );
            }
            if entry.hidden {
                self.pending_hidden.push(slot);
            }
        }
        // Restoring is not a change; only user edits dirty the file.
        self.indicator_state_dirty = false;
        self.last_indicator_change = None;
    }

    /// Apply restored-hidden flags once their views exist, then write the
    /// state file when a change has settled (debounced off the frame path).
    ///
    /// The file records one workspace: the flow pane of the tab opened from
    /// the config defaults at startup, which is what the app opens with and
    /// therefore all it can restore into. Slots on a time pane, or on a tab
    /// the user opened later, are in-session — a restored entry for either
    /// would have nowhere to land, and would then be quietly dropped by the
    /// next save. Persisting the tab strip and the layout (§14,
    /// `ui-state.toml`) is what unlocks persisting their indicators, and they
    /// land together or not at all.
    fn maintain_indicator_state(&mut self) {
        if !self.pending_hidden.is_empty()
            && let Some(index) = self
                .persisted_tab
                .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
        {
            let pane = &mut self.tabs[index].flow_pane;
            let existing: Vec<SlotId> = self
                .pending_hidden
                .iter()
                .copied()
                .filter(|slot| pane.indicators.all().iter().any(|v| v.slot == *slot))
                .collect();
            for slot in &existing {
                pane.indicators.toggle_hidden(*slot);
            }
            self.pending_hidden.retain(|slot| !existing.contains(slot));
        }
        let settled = self
            .last_indicator_change
            .is_some_and(|changed| changed.elapsed() >= INDICATOR_STATE_SAVE_DEBOUNCE);
        let Some(persisted) = self
            .persisted_tab
            .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
        else {
            // Nothing to describe: the tab the file was written for is gone.
            return;
        };
        if self.indicator_state_dirty && settled {
            self.indicator_state_dirty = false;
            // The change has been written; the clock starts again with the
            // next edit rather than ticking on every frame from here on.
            self.last_indicator_change = None;
            // What is on disk today, so a slot that failed to build does not
            // overwrite its own saved parameters with an empty list.
            let previous = state_file::load(&self.indicator_state_path);
            let tab_id = self.tabs[persisted].id;
            let saved: Vec<SavedIndicator> = self.tabs[persisted]
                .flow_pane
                .indicators
                .all()
                .iter()
                .filter_map(|view| {
                    let owner = TabSlot {
                        tab: tab_id,
                        side: PaneSide::Flow,
                        slot: view.slot,
                    };
                    let kind_ref = self
                        .slot_kinds
                        .iter()
                        .find(|(candidate, _)| *candidate == owner)
                        .map(|(_, kind)| kind)?;
                    let kind = kind_ref.clone();
                    // A slot whose build failed has an empty view: the
                    // worker's error path sends `Rebuilt { inputs: [] }`.
                    // Rewriting its entry from that would erase the user's
                    // saved parameters before they had a chance to fix the
                    // script, so a broken slot keeps what is already on disk.
                    let inputs = if view.error.is_some() {
                        previous
                            .iter()
                            .find(|entry| entry.kind == *kind_ref)
                            .map_or_else(Vec::new, |entry| entry.inputs.clone())
                    } else {
                        view.input_values
                            .iter()
                            .map(SavedInput::from_value)
                            .collect()
                    };
                    Some(SavedIndicator {
                        kind,
                        hidden: view.hidden,
                        inputs,
                    })
                })
                .collect();
            state_file::save(&self.indicator_state_path, &saved);
        }
    }

    /// Every pane's overlay at once, for a change that invalidates them all —
    /// a feed switch or a source reset re-cuts both charts.
    /// Bar-index anchors are meaningful only for the market/spec that created
    /// them. Clear them on a source or aggregation rebuild rather than
    /// silently attaching a mark to different market data — and say so.
    ///
    /// Scoped to one pane: the panes cut the same trades into different bars,
    /// so re-cutting one of them leaves the other's anchors exactly as valid
    /// as they were.
    fn note_overlay_cleared(&mut self, had_drawings: bool) {
        self.toolrail.arm(Tool::Pointer);
        self.drawing_delete_confirm = false;
        self.inspector_edit_baseline = None;
        self.inspector_last_selection = None;
        // The cleared history cannot resurrect anything, so this toast
        // offers no Undo — a dead button would lie. But losing the marks is
        // never silent.
        self.drawing_toast = had_drawings.then(|| DrawingToast {
            message: "Drawings cleared - the bars were rebuilt under them.",
            shown_at: Instant::now(),
            offers_undo: false,
        });
    }

    /// Periodically log a perf summary and warn on threshold breaches.
    fn maybe_emit_summary(&mut self, now: Instant) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        let rate = self.trades_since_summary as f64 / elapsed.as_secs_f64();
        let lag = self.active_tab().trade_arrival_ms();
        let avg = self.frames.avg_ms().unwrap_or(0.0);
        let cpu_avg = self.cpu_frames.avg_ms().unwrap_or(0.0);
        let worst = self.frames.worst_ms().unwrap_or(0.0);
        let fps = self.frames.fps().unwrap_or(0.0);
        let book = self.active_tab_mut().tape_mut().health();
        let book_lag = book.arrival_latency_ms;
        let book_rate = book.depth_updates_since_summary as f64 / elapsed.as_secs_f64();
        let book_queue_len = self.active_tab().book_events.len();
        let candle_preset =
            CandlePreset::detect(&self.style.candles).map_or("custom", CandlePreset::log_value);

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "APP_HEALTH_SUMMARY",
            // Frames and the trade rate are the window's; every market figure
            // below is the *active* tab's, which is what is on screen.
            tabs = self.tabs.len(),
            tab = self.active_tab().id,
            fps = fps as i64,
            frame_avg_ms = avg,
            frame_cpu_ms = cpu_avg,
            frame_worst_ms = worst,
            feed_arrival_ms = lag,
            trades_per_s = rate,
            live_trades = self.active_tab().live_trades,
            bar_spec = self.active_tab().flow_pane.state.spec().summary(),
            canvas_layout = ?self.active_tab().layout,
            time_pane_spec = self.active_tab().time_pane.as_ref().map(|pane| pane.state.spec().summary()),
            book_enabled = book.enabled,
            book_status = book.status,
            book_generation = book.generation,
            book_last_update_id = book.last_update_id,
            book_last_event_ms = book.last_event_ms,
            book_snapshot_observed_ms = book.last_snapshot_observed_ms,
            book_arrival_ms = book_lag,
            book_updates_per_s = book_rate,
            book_updates_total = book.depth_updates,
            book_queue_len,
            book_channel_closed = self.active_tab().book_channel_closed_reported,
            book_bid_levels = book.bid_levels,
            book_ask_levels = book.ask_levels,
            heatmap_active_levels = book.active_levels,
            heatmap_archived_runs = book.archived_runs,
            aggression_count = book.aggression_count,
            heatmap_history_bytes = book.history_bytes,
            heatmap_cells = book.projection_cells,
            heatmap_aggressions = book.projection_aggressions,
            heatmap_liquidity_events = book.projection_liquidity_events,
            heatmap_effective_grouping = %book.effective_grouping,
            heatmap_effective_grouping_multiple = book.effective_grouping_multiple,
            heatmap_dropped_cells = book.dropped_cells,
            heatmap_dropped_aggressions = book.dropped_aggressions,
            heatmap_dropped_liquidity_events = book.dropped_liquidity_events,
            heatmap_projection_ms = book.projection_ms,
            heatmap_live_ms = book.live_ms,
            heatmap_projection_builds = book.projection_builds,
            heatmap_projection_cache_hits = book.projection_cache_hits,
            heatmap_config_revision = book.config_revision,
            heatmap_snapshots = book.snapshots,
            heatmap_gaps = book.gaps,
            candle_style_revision = self.style_revision,
            candle_preset,
            candle_body_mode = ?self.style.candles.body_mode,
            candle_fill_opacity = self.style.candles.fill_opacity,
            candle_outline_opacity = self.style.candles.outline_opacity,
            candle_outline_width_px = self.style.candles.outline_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            replay_active = self.active_tab().replay.is_some(),
            replay_speed = self.active_tab().replay.as_ref().map(|r| r.status.speed()),
            replay_playing = self.active_tab().replay.as_ref().map(|r| r.status.is_playing()),
            replay_progress = self.active_tab().replay.as_ref().map(|r| r.status.progress()),
            replay_played = self.active_tab().replay.as_ref().map(|r| r.status.played()),
            replay_total = self.active_tab().replay.as_ref().map(|r| r.status.total()),
            action = "observe",
            "application health summary"
        );
        if avg > metrics::SLOW_FRAME_MS {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_SLOW_FRAMES",
                frame_avg_ms = avg,
                threshold_ms = metrics::SLOW_FRAME_MS,
                heatmap_enabled = book.enabled,
                heatmap_projection_ms = book.projection_ms,
                heatmap_cells = book.projection_cells,
                action = "inspect_render_budget",
                "slow frames: the chart is not keeping up"
            );
        }
        if let Some(l) = lag
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_HIGH_TRADE_LAG",
                feed_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                action = "inspect_trade_connection",
                "high feed lag: trades are arriving well behind their timestamps"
            );
        }
        if let Some(l) = book_lag
            && book.enabled
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_HIGH_ARRIVAL",
                symbol = self.active_tab().symbol.as_str(),
                book_arrival_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                book_status = book.status,
                action = "inspect_depth_connection",
                // Arrival, not age: this is how late the newest accepted
                // depth event was when it reached us, an observation frozen
                // at that moment. A book that stops updating keeps its last
                // figure — the tape-age readout is what catches that.
                "order-book events are arriving late"
            );
        }
        if book.dropped_cells > 0
            || book.dropped_aggressions > 0
            || book.dropped_liquidity_events > 0
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROJECTION_CAPPED",
                symbol = self.active_tab().symbol.as_str(),
                dropped_cells = book.dropped_cells,
                dropped_aggressions = book.dropped_aggressions,
                dropped_liquidity_events = book.dropped_liquidity_events,
                action = "increase_grouping_or_reduce_retention",
                "heatmap primitive cap was reached"
            );
        }

        self.trades_since_summary = 0;
        self.active_tab_mut().tape_mut().reset_summary_counters();
        self.last_summary = now;
    }

    /// Everything the status bar reports this frame.
    ///
    /// Provenance (venue, symbol, transport, side honesty) is the market's and
    /// reads from the window; the content section — spec, bar counts, forming
    /// bar, whether the view follows live — is the *focused pane's* (§11), so
    /// the bar always describes the chart the user is working in.
    fn status_model(&self) -> statusbar::StatusModel {
        let pane = self.focused_pane();
        let bars = pane.state.bars();
        let (backfilled, live) = match pane.state.backfill_boundary() {
            Some(boundary) => (boundary, bars.len().saturating_sub(boundary)),
            None => (0, bars.len()),
        };
        let note = self.active_tab().side_note(&self.config);
        statusbar::StatusModel {
            venue: if self.active_tab().replay.is_some() {
                "recording".to_owned()
            } else {
                self.active_tab().feed_display_name(&self.config).to_owned()
            },
            symbol: self.active_tab().symbol.clone(),
            replay: self
                .active_tab()
                .replay
                .as_ref()
                .map(|link| statusbar::ReplayFigures {
                    speed: link.status.speed(),
                    progress: link.status.progress(),
                }),
            connection: self.active_tab().feed_connection,
            feed_arrival_ms: self.active_tab().trade_arrival_ms(),
            tape_age_ms: self.active_tab().tape_age_at(metrics::wall_clock_ms()),
            spec_summary: pane.state.spec().summary(),
            bar_progress: pane
                .state
                .progress()
                .map(|(progress, unit)| fmt_progress(&progress, unit)),
            backfilled_bars: backfilled,
            live_bars: live,
            side_note: note.clone().map(|(label, _)| label),
            side_detail: note.and_then(|(_, detail)| detail),
            follows_live: pane.viewport.follows_live(),
            price_auto: pane.price_view.is_auto(),
            live_trades: self.active_tab().live_trades,
            fps: self.frames.fps(),
            frame_avg_ms: self.frames.avg_ms(),
            frame_cpu_ms: self.cpu_frames.avg_ms(),
            show_perf: self.show_perf,
        }
    }
}

/// Opens the Market Replay browser (§10).
const REPLAY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);
/// Shows/hides the panels dock (§10).
const DOCK_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B);
/// Opens the source picker for a new tab (§10).
const NEW_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T);
/// Closes the active tab (§10). Free of any other binding: the chart has no
/// text inputs and no document to "write".
const CLOSE_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W);
/// Cycles forward through the strip (§10).
const NEXT_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Tab);
/// See [`NEXT_TAB_SHORTCUT`].
const PREVIOUS_TAB_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::Tab,
);
/// Height of the menu bar, in pixels (§5 zone 1).
const MENU_BAR_HEIGHT: f32 = 28.0;

impl QuantickApp {
    /// The window's menu bar (§10): shallow menus for discoverability and
    /// shortcuts, never the only path to anything.
    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&REPLAY_SHORTCUT)) {
            self.replay_view.open_browser();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&DOCK_SHORTCUT)) {
            self.dock.toggle_visible();
        }

        let mut tab_action = None;
        egui::TopBottomPanel::top("menu_bar")
            .exact_height(MENU_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui
                            .add(
                                egui::Button::new("New Tab…")
                                    .shortcut_text(ui.ctx().format_shortcut(&NEW_TAB_SHORTCUT)),
                            )
                            .clicked()
                        {
                            tab_action = Some(TabAction::New);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.tabs.len() > 1,
                                egui::Button::new("Close Tab").shortcut_text(
                                    ui.ctx().format_shortcut(&CLOSE_TAB_SHORTCUT),
                                ),
                            )
                            .on_disabled_hover_text(
                                "The last tab stays open — a window with no market has nothing to show",
                            )
                            .clicked()
                        {
                            tab_action = Some(TabAction::Close(self.active_tab));
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.menu_button("Layout", |ui| {
                            for (layout, label) in [
                                (CanvasLayout::Single, "Single"),
                                (CanvasLayout::TimeAndFlow, "Time + Flow"),
                            ] {
                                if ui
                                    .selectable_label(self.active_tab().layout == layout, label)
                                    .clicked()
                                {
                                    self.active_tab_mut().set_layout(layout);
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new("Market Replay…")
                                    .shortcut_text(ui.ctx().format_shortcut(&REPLAY_SHORTCUT)),
                            )
                            .clicked()
                        {
                            self.replay_view.open_browser();
                            ui.close_menu();
                        }
                        if self.active_tab().replay.is_some() && ui.button("Close Replay").clicked()
                        {
                            let (tab, config) = self.active_with_config();
                            let cleared = tab.close_replay(config);
                            self.note_overlay_cleared(cleared);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("View", |ui| {
                        let panels_label = if self.dock.visible() {
                            "Hide panels"
                        } else {
                            "Show panels"
                        };
                        if ui
                            .add(
                                egui::Button::new(panels_label)
                                    .shortcut_text(ui.ctx().format_shortcut(&DOCK_SHORTCUT)),
                            )
                            .clicked()
                        {
                            self.dock.toggle_visible();
                            ui.close_menu();
                        }
                        let toolbox_label = if self.toolrail.visible() {
                            "Hide drawing toolbox"
                        } else {
                            "Show drawing toolbox"
                        };
                        if ui.button(toolbox_label).clicked() {
                            self.toolrail.toggle_visible();
                            ui.close_menu();
                        }
                        for (tab, label) in [
                            (DockTab::L2, "L2 settings"),
                            (DockTab::Bubbles, "Bubble settings"),
                            (DockTab::Session, "Session"),
                        ] {
                            if ui.button(label).clicked() {
                                self.dock.open_tab(tab);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_perf, "Perf readings")
                            .on_hover_text("fps, frame time and trade count on the status bar");
                        ui.separator();
                        ui.menu_button("Timezone", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for tz in TzOffset::ALL {
                                        if ui.selectable_label(self.tz == tz, tz.label()).clicked()
                                        {
                                            self.tz = tz;
                                            ui.close_menu();
                                        }
                                    }
                                });
                        });
                    });
                    ui.menu_button("Tools", |ui| {
                        if ui.button("Appearance…").clicked() {
                            self.show_style = true;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("Replay file format…").clicked() {
                            self.replay_view.open_format_help();
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    // The tab strip shares the menu row: zone 1 already had
                    // the horizontal room, so tabs cost no chrome budget.
                    tab_action = self.draw_tab_strip(ui);
                });
            });
        if let Some(action) = tab_action {
            self.apply_tab_action(action);
        }
    }

    /// The chips, built from what each tab actually is right now.
    fn draw_tab_strip(&self, ui: &mut egui::Ui) -> Option<TabAction> {
        let chips: Vec<tabstrip::TabChip<'_>> = self
            .tabs
            .iter()
            .map(|tab| tabstrip::TabChip {
                label: tab.chip_label(),
                replaying: tab.replay.is_some(),
                needs_attention: tab.needs_attention(),
            })
            .collect();
        tabstrip::draw(ui, &chips, self.active_tab)
    }

    /// One delete command for every trigger (inspector button, keyboard,
    /// manager). A locked object raises the confirmation next to the trigger
    /// instead of deleting; a landed delete raises the Undo toast.
    fn request_delete_selected(&mut self, now: Instant) {
        match self.focused_pane_mut().drawings.delete_selected(false) {
            DeleteOutcome::Deleted => {
                self.drawing_delete_confirm = false;
                self.drawing_toast = Some(DrawingToast {
                    message: "Drawing deleted.",
                    shown_at: now,
                    offers_undo: true,
                });
            }
            DeleteOutcome::NeedsConfirmation => self.drawing_delete_confirm = true,
            DeleteOutcome::NothingSelected => {}
        }
    }

    /// Keyboard grammar for drawings. Any focused widget wins: while an input
    /// owns the keyboard, chart shortcuts stay suspended.
    fn handle_drawing_keys(&mut self, ctx: &egui::Context, now: Instant) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        struct DrawingKeys {
            escape: bool,
            delete: bool,
            backspace: bool,
            undo: bool,
            redo: bool,
            lock: bool,
            hide: bool,
            duplicate: bool,
            nudge_bars: f32,
            nudge_px: f32,
        }
        let keys = ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            let alt = input.modifiers.alt;
            // Shift turns a nudge into ten steps (UX spec).
            let step = if shift { 10.0 } else { 1.0 };
            let horizontal = f32::from(input.key_pressed(egui::Key::ArrowRight))
                - f32::from(input.key_pressed(egui::Key::ArrowLeft));
            let vertical = f32::from(input.key_pressed(egui::Key::ArrowUp))
                - f32::from(input.key_pressed(egui::Key::ArrowDown));
            DrawingKeys {
                escape: input.key_pressed(egui::Key::Escape),
                delete: input.key_pressed(egui::Key::Delete),
                backspace: input.key_pressed(egui::Key::Backspace),
                undo: command && !shift && input.key_pressed(egui::Key::Z),
                redo: (command && input.key_pressed(egui::Key::Y))
                    || (command && shift && input.key_pressed(egui::Key::Z)),
                lock: alt && input.key_pressed(egui::Key::L),
                hide: alt && input.key_pressed(egui::Key::H),
                duplicate: command && input.key_pressed(egui::Key::D),
                nudge_bars: horizontal * step,
                nudge_px: vertical * step,
            }
        });
        // The escape stack: pending confirmation → draft → selection →
        // Pointer, one layer per press.
        if keys.escape {
            if self.drawing_delete_confirm {
                self.drawing_delete_confirm = false;
            } else if self.focused_pane().drawings.draft().is_some() {
                self.focused_pane_mut().drawings.cancel_draft();
                self.toolrail.arm(Tool::Pointer);
            } else if self.focused_pane().drawings.selected().is_some() {
                self.focused_pane_mut().drawings.select(None);
            } else {
                self.toolrail.arm(Tool::Pointer);
            }
        }
        if self.focused_pane().drawings.draft().is_some() {
            // During placement the delete keys belong to the draft workflow:
            // Backspace steps back one anchor.
            if keys.backspace {
                self.focused_pane_mut().drawings.remove_last_draft_anchor();
            }
        } else if keys.delete || keys.backspace {
            self.request_delete_selected(now);
        }
        if keys.undo {
            self.focused_pane_mut().drawings.undo();
        }
        if keys.redo {
            self.focused_pane_mut().drawings.redo();
        }
        if keys.lock
            && let Some(index) = self.focused_pane().drawings.selected()
        {
            let locked = self.focused_pane().drawings.items()[index].locked;
            self.focused_pane_mut()
                .drawings
                .set_selected_locked(!locked);
        }
        if keys.hide
            && let Some(index) = self.focused_pane().drawings.selected()
        {
            let hidden = self.focused_pane().drawings.items()[index].hidden;
            self.focused_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if keys.duplicate {
            self.focused_pane_mut()
                .drawings
                .duplicate_selected(DUPLICATE_OFFSET_BARS);
        }
        if (keys.nudge_bars != 0.0 || keys.nudge_px != 0.0)
            && self.focused_pane().drawings.selected().is_some()
        {
            // Arrows write the same honest chart coordinates a drag does:
            // one bar per horizontal step, one pixel's worth of price per
            // vertical step. Each press lands as one undo entry.
            let price_per_px = self.focused_pane().last_auto_range.map_or(0.0, |auto| {
                let (lo, hi) = self.focused_pane().price_view.resolve(auto);
                (hi - lo) / f64::from(self.focused_pane().last_chart_height.max(1.0))
            });
            self.focused_pane_mut().drawings.begin_gesture();
            self.focused_pane_mut()
                .drawings
                .translate_selected(keys.nudge_bars, f64::from(keys.nudge_px) * price_per_px);
            self.focused_pane_mut().drawings.commit_gesture();
        }
    }

    /// Commit a pending inspector edit gesture as one undo entry.
    fn commit_inspector_gesture(&mut self) {
        let Some(edit) = self.inspector_edit_baseline.take() else {
            return;
        };
        // The pane the edit started on. Its tab may have been closed under the
        // gesture, in which case the object it described is gone with it.
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == edit.tab) {
            tab.pane_mut(edit.side)
                .drawings
                .record_edit_of(edit.index, edit.before);
        }
    }

    /// The delete toast: visible for [`TOAST_UNDO_MS`], with an Undo button
    /// driving the same history as Ctrl+Z.
    fn draw_drawing_toast(&mut self, ctx: &egui::Context, now: Instant) {
        let Some(toast) = &self.drawing_toast else {
            return;
        };
        if now.saturating_duration_since(toast.shown_at) >= Duration::from_millis(TOAST_UNDO_MS) {
            self.drawing_toast = None;
            return;
        }
        let message = toast.message;
        let offers_undo = toast.offers_undo;
        let mut undo_clicked = false;
        #[cfg(test)]
        let mut undo_rect = None;
        egui::Area::new(egui::Id::new("drawing_toast"))
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, -TOAST_BOTTOM_MARGIN_PX),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::TAG_BG)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(6.0_f32)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(message);
                            if offers_undo {
                                let undo = ui.button("Undo");
                                #[cfg(test)]
                                {
                                    undo_rect = Some(undo.rect);
                                }
                                undo_clicked = undo.clicked();
                            }
                        });
                    });
            });
        #[cfg(test)]
        {
            self.toast_undo_rect = undo_rect;
        }
        if undo_clicked {
            self.focused_pane_mut().drawings.undo();
            self.drawing_toast = None;
        }
    }

    /// Everything the inspector shows for the selected object, shared by the
    /// floating window and the pinned dock panel. Sections are driven by the
    /// tool's capabilities — an unsupported property is absent, not disabled.
    fn drawing_inspector_body(&mut self, ui: &mut egui::Ui, index: usize) -> InspectorActions {
        let mut actions = InspectorActions::default();
        let drawing = &self.focused_pane().drawings.items()[index];
        let tool = drawing.tool;
        let locked = drawing.locked;
        let hidden = drawing.hidden;
        let show_confirm = self.drawing_delete_confirm && locked;

        // Header: object identity plus the view controls.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tool.name()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Close").clicked() {
                    actions.close = true;
                }
                let pin_label = if self.inspector_pinned {
                    "Unpin"
                } else {
                    "Pin"
                };
                let pin = ui
                    .small_button(pin_label)
                    .on_hover_text("Dock the inspector at the side of the chart");
                #[cfg(test)]
                {
                    self.inspector_pin_rect = Some(pin.rect);
                }
                if pin.clicked() {
                    actions.toggle_pin = true;
                }
                let eye_label = if hidden { "Show" } else { "Hide" };
                if ui.small_button(eye_label).clicked() {
                    actions.toggle_hidden = true;
                }
            });
        });

        // The always-visible textual actions (UX spec: never glyph-only,
        // never behind a scroll).
        let intent = drawings::action_bar::draw(ui, locked);
        actions.toggle_lock |= intent.toggle_lock;
        actions.delete |= intent.delete;

        if locked {
            ui.label(
                egui::RichText::new(
                    "Locked - protected from accidental moves. Style stays editable.",
                )
                .small(),
            );
        }
        if hidden {
            ui.label(egui::RichText::new("Hidden - Show brings it back.").small());
        }
        if show_confirm {
            ui.separator();
            ui.label("Delete locked drawing?");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    actions.cancel_delete = true;
                }
                if ui.button("Delete anyway").clicked() {
                    actions.force_delete = true;
                }
            });
        }
        ui.separator();

        if self.inspector_tab == InspectorTab::Extra && tool.extra_tab().is_none() {
            // The previous selection had an extra tab; this tool brings none.
            self.inspector_tab = InspectorTab::Style;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.inspector_tab, InspectorTab::Style, "Style");
            // A tool that brings its own tab (the Fib level editor) mounts it
            // here by name; the central code never learns what is inside.
            if let Some(extra) = tool.extra_tab() {
                ui.selectable_value(&mut self.inspector_tab, InspectorTab::Extra, extra);
            }
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::Coordinates,
                "Coordinates",
            );
        });
        ui.separator();

        let tab = self.inspector_tab;
        let price_speed = self.focused_pane().last_auto_range.map_or(1.0, |(lo, hi)| {
            ((hi - lo) / PRICE_DRAG_STEPS).abs().max(1e-9)
        });
        let side = self.active_tab().focused_side();
        let Self {
            tabs,
            active_tab,
            drawing_presets,
            ..
        } = self;
        let drawings = &mut tabs[*active_tab].pane_mut(side).drawings;
        let Some(drawing) = drawings.selected_mut() else {
            return actions;
        };
        match tab {
            InspectorTab::Extra => {
                actions.edited |= tool.draw_extra_tab(ui, drawing, drawing_presets);
            }
            InspectorTab::Style => {
                ui.label("Style");
                actions.edited |= ui
                    .color_edit_button_srgba(&mut drawing.style.color)
                    .changed();
                actions.edited |= ui
                    .add(
                        egui::Slider::new(
                            &mut drawing.style.width_px,
                            MIN_DRAWING_WIDTH_PX..=MAX_DRAWING_WIDTH_PX,
                        )
                        .text("line width (px)"),
                    )
                    .changed();
                if tool.supports_fill() {
                    actions.edited |= ui
                        .add(
                            egui::Slider::new(
                                &mut drawing.style.fill_alpha,
                                0..=MAX_DRAWING_FILL_ALPHA,
                            )
                            .text("fill opacity"),
                        )
                        .changed();
                }
            }
            InspectorTab::Coordinates => {
                // Geometry through numbers: bar index and price per anchor,
                // the same canonical coordinates drags write. Locked blocks
                // geometry here exactly as it does on the canvas.
                const ANCHOR_LABELS: [&str; 4] = ["A", "B", "C", "D"];
                ui.add_enabled_ui(!locked, |ui| {
                    for (point_index, point) in drawing.points.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(ANCHOR_LABELS.get(point_index).copied().unwrap_or("?"));
                            actions.edited |= ui
                                .add(
                                    egui::DragValue::new(&mut point.bar)
                                        .speed(BAR_DRAG_SPEED)
                                        .prefix("bar "),
                                )
                                .changed();
                            actions.edited |= ui
                                .add(egui::DragValue::new(&mut point.price).speed(price_speed))
                                .changed();
                        });
                    }
                });
                if locked {
                    ui.label(
                        egui::RichText::new("Unlock the drawing to edit its coordinates.").small(),
                    );
                }
            }
        }
        actions
    }

    /// Apply what the inspector body asked for, with the shared undo
    /// coalescing: the pre-edit object is captured at the first change and
    /// committed once pointer and keyboard let go.
    fn apply_inspector_actions(
        &mut self,
        ctx: &egui::Context,
        actions: InspectorActions,
        index: usize,
        before: drawings::Drawing,
        now: Instant,
    ) {
        if actions.edited && self.inspector_edit_baseline.is_none() {
            self.inspector_edit_baseline = Some(InspectorEdit {
                tab: self.active_tab().id,
                side: self.active_tab().focused_side(),
                index,
                before,
            });
        }
        let gesture_settled = ctx.input(|input| !input.pointer.any_down())
            && ctx.memory(|memory| memory.focused().is_none());
        if gesture_settled {
            self.commit_inspector_gesture();
        }
        if actions.toggle_hidden {
            let hidden = self.focused_pane().drawings.items()[index].hidden;
            self.focused_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if actions.toggle_lock {
            let locked = self.focused_pane().drawings.items()[index].locked;
            self.focused_pane_mut()
                .drawings
                .set_selected_locked(!locked);
            self.drawing_delete_confirm = false;
        }
        if actions.toggle_pin {
            self.inspector_pinned = !self.inspector_pinned;
        }
        if actions.delete {
            self.request_delete_selected(now);
        }
        if actions.cancel_delete {
            self.drawing_delete_confirm = false;
        }
        if actions.force_delete {
            self.drawing_delete_confirm = false;
            if self.focused_pane_mut().drawings.delete_selected(true) == DeleteOutcome::Deleted {
                self.drawing_toast = Some(DrawingToast {
                    message: "Drawing deleted.",
                    shown_at: now,
                    offers_undo: true,
                });
            }
        }
        if actions.close {
            self.focused_pane_mut().drawings.select(None);
            self.drawing_delete_confirm = false;
        }
    }

    /// Where a freshly opened floating inspector should sit: 12 px right of
    /// the object's bounding box, falling back to left, below and above, and
    /// always clamped into the chart pane — which already excludes the price
    /// and time axes, so the popup can never cover either or leave the view.
    fn inspector_target_position(&self, ctx: &egui::Context, index: usize) -> Option<egui::Pos2> {
        let chart = self.focused_pane().last_chart_area?;
        let total = self.focused_pane().slots();
        let (auto_lo, auto_hi) = self.focused_pane().last_auto_range?;
        let (lo, hi) = self.focused_pane().price_view.resolve((auto_lo, auto_hi));
        let scale = PriceScale::from_range(
            lo,
            hi,
            self.focused_pane().last_chart_top,
            self.focused_pane().last_chart_top + self.focused_pane().last_chart_height,
        );
        let history_right = self
            .focused_pane()
            .last_lane_divider_x
            .unwrap_or(chart.right());
        let drawing = self.focused_pane().drawings.items().get(index)?;
        let points =
            self.focused_pane()
                .projected_drawing_points(drawing, history_right, total, &scale);
        let first = points.first()?;
        let mut bbox = egui::Rect::from_min_max(*first, *first);
        for point in &points {
            bbox.extend_with(*point);
        }
        let bbox = bbox.expand(DRAWING_ANCHOR_RADIUS_PX);
        let size = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .map_or(
                egui::vec2(INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX),
                |rect| rect.size(),
            );
        let gap = INSPECTOR_OBJECT_GAP_PX;
        let candidates = [
            egui::pos2(bbox.right() + gap, bbox.top()),
            egui::pos2(bbox.left() - gap - size.x, bbox.top()),
            egui::pos2(bbox.left(), bbox.bottom() + gap),
            egui::pos2(bbox.left(), bbox.top() - gap - size.y),
        ];
        let fits = |position: egui::Pos2| {
            let rect = egui::Rect::from_min_size(position, size);
            chart.contains_rect(rect) && !rect.intersects(bbox)
        };
        let chosen = candidates
            .into_iter()
            .find(|position| fits(*position))
            .unwrap_or(candidates[0]);
        let max_x = (chart.right() - size.x).max(chart.left());
        let max_y = (chart.bottom() - size.y).max(chart.top());
        Some(egui::pos2(
            chosen.x.clamp(chart.left(), max_x),
            chosen.y.clamp(chart.top(), max_y),
        ))
    }

    /// Shared prologue of both inspector hosts. Returns the selection and its
    /// pre-frame copy, or cleans up when nothing is selected.
    fn inspector_selection(&mut self) -> Option<(usize, drawings::Drawing)> {
        let Some(index) = self.focused_pane().drawings.selected() else {
            self.drawing_delete_confirm = false;
            self.commit_inspector_gesture();
            self.inspector_last_selection = None;
            return None;
        };
        // An edit gesture that outlived its object's selection commits now.
        if self
            .inspector_edit_baseline
            .as_ref()
            .is_some_and(|edit| edit.index != index)
        {
            self.commit_inspector_gesture();
        }
        Some((index, self.focused_pane().drawings.items()[index].clone()))
    }

    /// The pinned inspector: a dock panel at the chart's side. Declared with
    /// the chrome, before the central canvas, so the canvas pays its width.
    fn draw_drawing_inspector_panel(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.inspector_pinned {
            return;
        }
        let Some((index, before)) = self.inspector_selection() else {
            return;
        };
        let mut actions = InspectorActions::default();
        egui::SidePanel::right("drawing_inspector_panel")
            .resizable(true)
            .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
            .width_range(INSPECTOR_MIN_WIDTH_PX..=INSPECTOR_MAX_WIDTH_PX)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    actions = self.drawing_inspector_body(ui, index);
                });
            });
        self.apply_inspector_actions(ctx, actions, index, before, now);
    }

    /// The selected object's floating inspector. Non-modal by contract: it
    /// never captures the whole canvas and never blocks moving the geometry
    /// underneath it. Opens beside the selection; once the user moves it, the
    /// manual position wins for the rest of the session.
    fn draw_drawing_inspector(&mut self, ctx: &egui::Context, now: Instant) {
        if self.inspector_pinned {
            // The pinned panel already drew (and cleaned up) this frame.
            return;
        }
        let Some((index, before)) = self.inspector_selection() else {
            return;
        };
        let selection_changed = self.inspector_last_selection != Some(index);
        self.inspector_last_selection = Some(index);
        let mut actions = InspectorActions::default();
        let mut open = true;
        let inspector_interactable = !self.focused_pane().drawing_drag.is_active();
        // The level editor earns the wider default the spec reserves for it.
        let default_width = if before.tool.extra_tab().is_some() {
            INSPECTOR_LEVELS_WIDTH_PX
        } else {
            INSPECTOR_DEFAULT_WIDTH_PX
        };
        let mut window = egui::Window::new(before.tool.settings_title())
            .id(egui::Id::new("drawing_inspector"))
            .open(&mut open)
            .default_pos(DRAWING_INSPECTOR_DEFAULT_POSITION)
            .default_width(default_width)
            .min_width(INSPECTOR_MIN_WIDTH_PX)
            .max_width(INSPECTOR_MAX_WIDTH_PX)
            .collapsible(false)
            .movable(true)
            .interactable(inspector_interactable)
            .resizable(true);
        if selection_changed
            && !self.inspector_moved
            && let Some(position) = self.inspector_target_position(ctx, index)
        {
            window = window.current_pos(position);
        }
        let response = window.show(ctx, |ui| self.drawing_inspector_body(ui, index));
        if let Some(response) = &response {
            if response.response.dragged() {
                self.inspector_moved = true;
            }
            if let Some(inner) = response.inner {
                actions = inner;
            }
        }
        if !open {
            actions.close = true;
        }
        self.apply_inspector_actions(ctx, actions, index, before, now);
    }

    /// The object manager: a non-modal list of every drawing with the named
    /// per-object actions. It sends the same store commands as the inspector
    /// and the keyboard — nothing here re-implements lock or delete rules.
    fn draw_drawing_manager(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.drawing_manager_open {
            return;
        }
        #[cfg(test)]
        self.manager_action_rects.clear();
        let mut open = true;
        let mut select_row: Option<usize> = None;
        let mut eye_row: Option<usize> = None;
        let mut lock_row: Option<usize> = None;
        let mut front_row: Option<usize> = None;
        let mut delete_row: Option<usize> = None;
        let mut show_all = false;
        let mut unlock_all = false;
        egui::Window::new("Drawn objects")
            .id(egui::Id::new("drawing_manager"))
            .open(&mut open)
            .default_pos(DRAWING_MANAGER_DEFAULT_POSITION)
            .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let count = self.focused_pane().drawings.items().len();
                if count == 0 {
                    ui.label("No drawings yet.");
                }
                // Walked in reverse: the manager lists top-most first, the
                // same order hit-testing resolves overlap.
                for index in (0..count).rev() {
                    let drawing = &self.focused_pane().drawings.items()[index];
                    let selected = self.focused_pane().drawings.selected() == Some(index);
                    let locked = drawing.locked;
                    let hidden = drawing.hidden;
                    let name = drawing.tool.name();
                    ui.horizontal(|ui| {
                        let mut label = egui::RichText::new(format!("{} {}", name, index + 1));
                        if hidden {
                            label = label.weak();
                        }
                        if ui.selectable_label(selected, label).clicked() {
                            select_row = Some(index);
                        }
                        if locked {
                            ui.label(egui::RichText::new("locked").small());
                        }
                        if hidden {
                            ui.label(egui::RichText::new("hidden").small());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let delete = ui.small_button("Delete");
                            #[cfg(test)]
                            self.manager_action_rects
                                .push((index, "Delete", delete.rect));
                            if delete.clicked() {
                                delete_row = Some(index);
                            }
                            let front = ui.small_button("Front");
                            #[cfg(test)]
                            self.manager_action_rects.push((index, "Front", front.rect));
                            if front.clicked() {
                                front_row = Some(index);
                            }
                            let lock = ui.small_button(if locked { "Unlock" } else { "Lock" });
                            #[cfg(test)]
                            self.manager_action_rects.push((index, "Lock", lock.rect));
                            if lock.clicked() {
                                lock_row = Some(index);
                            }
                            let eye = ui.small_button(if hidden { "Show" } else { "Hide" });
                            #[cfg(test)]
                            self.manager_action_rects.push((index, "Eye", eye.rect));
                            if eye.clicked() {
                                eye_row = Some(index);
                            }
                        });
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Show all").clicked() {
                        show_all = true;
                    }
                    if ui.button("Unlock all").clicked() {
                        unlock_all = true;
                    }
                });
            });
        self.drawing_manager_open = open;
        if let Some(index) = select_row {
            self.focused_pane_mut().drawings.select(Some(index));
            // Centre the viewport on the object's bar span.
            let slots = self.focused_pane().slots();
            if let Some(chart) = self.focused_pane().last_chart_area {
                let points = &self.focused_pane().drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    self.focused_pane_mut()
                        .viewport
                        .center_on_bar(mid, chart.width(), slots);
                }
            }
        }
        if let Some(index) = eye_row {
            let hidden = self.focused_pane().drawings.items()[index].hidden;
            self.focused_pane_mut()
                .drawings
                .set_hidden_at(index, !hidden);
        }
        if let Some(index) = lock_row {
            let locked = self.focused_pane().drawings.items()[index].locked;
            self.focused_pane_mut()
                .drawings
                .set_locked_at(index, !locked);
        }
        if let Some(index) = front_row {
            self.focused_pane_mut().drawings.bring_to_front(index);
        }
        if let Some(index) = delete_row {
            // The exact same command path as the inspector button and the
            // keyboard: select, then request. Locked rows raise the same
            // confirmation in the inspector.
            self.focused_pane_mut().drawings.select(Some(index));
            self.request_delete_selected(now);
        }
        if show_all {
            self.focused_pane_mut().drawings.set_all_hidden(false);
        }
        if unlock_all {
            self.focused_pane_mut().drawings.set_all_locked(false);
        }
    }

    /// Carry out what the replay interface asked for.
    fn apply_replay_action(&mut self, action: ReplayAction) {
        match action {
            ReplayAction::Open(request) => {
                let (tab, config) = self.active_with_config();
                let cleared = tab.open_replay(config, *request);
                self.note_overlay_cleared(cleared);
            }
            ReplayAction::Close => {
                let (tab, config) = self.active_with_config();
                let cleared = tab.close_replay(config);
                self.note_overlay_cleared(cleared);
            }
            ReplayAction::Control(control) => {
                // A dropped transport click is not worth a retry queue: the
                // worker drains commands every 8 ms, so a full channel means
                // the click was already superseded.
                if let Err(e) = self
                    .active_tab()
                    .commands
                    .try_send(FeedCommand::Replay(control))
                {
                    tracing::debug!(
                        target: "quantick::app",
                        event_code = "REPLAY_COMMAND_DROPPED",
                        reason = %e,
                        "transport command not queued"
                    );
                }
            }
        }
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(cpu) = frame.info().cpu_usage {
            self.cpu_frames.record(cpu * 1000.0);
        }
        self.draw_frame(ctx, Instant::now());
    }
}

impl QuantickApp {}

impl QuantickApp {
    /// One frame of the application: drain, lay out the chrome, draw the
    /// chart.
    ///
    /// Everything `update` does that is not eframe's own bookkeeping, so a
    /// test can run a real frame against a headless [`egui::Context`] and read
    /// what was painted — the only honest way to assert that a chart is on
    /// screen rather than a blank rectangle.
    fn draw_frame(&mut self, ctx: &egui::Context, now: Instant) {
        if let Some(last) = self.last_frame {
            self.frames.record((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);

        self.drain_tabs();
        self.maybe_emit_summary(now);

        let bg = pane::background_color(&self.style);
        // Rail shortcuts first: Esc/1/2 must be read before any widget can
        // claim the keyboard this frame.
        self.toolrail.handle_keys(ctx);
        self.handle_tab_keys(ctx);
        self.handle_drawing_keys(ctx, now);
        // Chrome panels claim their zones outside-in (§5): menu and toolbar
        // on top, the status line at the very bottom with the replay
        // transport directly above it, then the corner toolbox and right dock.
        // The chart keeps whatever remains.
        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        self.draw_source_picker(ctx);
        self.draw_indicator_settings(ctx);
        self.poll_script_files();
        self.maintain_indicator_state();
        let status = self.status_model();
        statusbar::draw(ctx, &status, &mut self.tz);
        // The browser window and, while the *active* tab plays a session, its
        // transport bar. A background tab's recording keeps advancing on its
        // own feed thread; what it does not get is the strip, which speaks for
        // one tab at a time (§11).
        let replay_action = {
            let Self {
                replay_view,
                tabs,
                active_tab,
                ..
            } = self;
            replay_view.draw(ctx, tabs[*active_tab].replay.as_ref())
        };
        if let Some(action) = replay_action {
            self.apply_replay_action(action);
        }
        {
            // The focused pane's objects: the toolbox lists and manages what a
            // click on the canvas would act on.
            let side = self.active_tab().focused_side();
            let Self {
                toolrail,
                tabs,
                active_tab,
                drawing_manager_open,
                ..
            } = self;
            let tab = &mut tabs[*active_tab];
            toolrail.draw(ctx, &mut tab.pane_mut(side).drawings, drawing_manager_open);
        }
        let dock_response = {
            let Self {
                dock,
                tabs,
                active_tab,
                replay_view,
                ..
            } = self;
            let tab = &mut tabs[*active_tab];
            let orderflow = tab
                .flow_pane
                .orderflow
                .as_mut()
                .expect("the flow pane is built with a tape and never drops it");
            dock.draw(
                ctx,
                &mut DockEnv {
                    orderflow,
                    replay_view,
                    replay: tab.replay.as_ref(),
                },
            )
        };
        if dock_response.restart_book_capture {
            self.active_tab_mut().restart_book_capture();
        }
        if let Some(action) = dock_response.replay_action {
            self.apply_replay_action(action);
        }
        // The pinned inspector is chrome: declared before the central canvas
        // so the chart pays its width, exactly like the dock.
        self.draw_drawing_inspector_panel(ctx, now);
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        let (tab, config) = self.active_with_config();
        let mut cleared = tab.maybe_switch_feed(config);
        // Both deferrals settle here, a frame after the click that armed
        // them, so the frame carrying the change paints its overlay first.
        for tab in self.tabs.iter_mut() {
            tab.apply_pending_layout();
        }
        cleared |= self.active_tab_mut().apply_spec_changes();
        if cleared {
            self.note_overlay_cleared(true);
        }
        self.draw_style_panel(ctx, now);
        // Waits owned by other components, mirrored level-style each frame so
        // the overlay needs no push notifications from either.
        let replay_loading = self.replay_view.is_loading();
        let book_syncing = self.active_tab().tape().is_syncing();
        let tab = self.active_tab_mut();
        tab.loading
            .set_active(LoadingTask::ReplaySession, replay_loading);
        tab.loading.set_active(LoadingTask::BookSync, book_syncing);

        let mut notice_action = notice_card::NoticeAction::None;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                {
                    let Self {
                        tabs,
                        active_tab,
                        toolrail,
                        drawing_presets,
                        style,
                        tz,
                        ..
                    } = self;
                    let mut chrome = CanvasChrome {
                        toolrail,
                        presets: drawing_presets,
                        style,
                        tz: *tz,
                    };
                    tabs[*active_tab].draw_canvas(ui, area, &mut chrome);
                }
                loading::overlay(ui, area, &self.active_tab().loading);
                let tab = self.active_tab();
                if notice_card::should_draw(&tab.notice, tab.flow_pane.state.bars().len()) {
                    notice_action = notice_card::draw(ui, area, &tab.notice);
                }
            });
        // Floating drawing controls must be registered after the opaque
        // central canvas so they stay in front of the chart.
        self.draw_drawing_inspector(ctx, now);
        self.draw_drawing_manager(ctx, now);
        self.draw_drawing_toast(ctx, now);
        if notice_action == notice_card::NoticeAction::Retry {
            let (tab, config) = self.active_with_config();
            let cleared = tab.restart_feed(config);
            self.note_overlay_cleared(cleared);
        }
        // Live feed: keep polling the channel ~60×/s without busy-spinning.
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// Every tab takes in what its feed sent this frame, on screen or not.
    ///
    /// §11: switching tabs never tears a feed down, so a background tab has to
    /// keep draining — its channels are bounded, and one left full backs its
    /// feed thread up until the market it is showing is hours behind. The
    /// indicator workers are fed on the same pass, so a tab brought forward is
    /// already current rather than rebuilding on the frame it appears.
    fn drain_tabs(&mut self) {
        let mut cleared_active = false;
        let config = &self.config;
        let mut trades = 0_u64;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let before = tab.live_trades;
            let cleared = tab.drain_feed();
            for pane in tab.panes_mut() {
                pane.apply_indicator_events();
            }
            tab.drain_book_feed();
            tab.drain_notices();
            // Heartbeat for the recorder. The lifecycle calls elsewhere already
            // start it at every point that knows the market changed; this one
            // makes "always recording" true by construction, so a start command
            // lost to a momentarily full channel heals on the next frame
            // instead of leaving the session silently unrecorded. Free while it
            // is running: one bool read and an early return.
            tab.ensure_book_capture(config);
            trades += tab.live_trades - before;
            if cleared && index == self.active_tab {
                cleared_active = true;
            }
        }
        // What the window ingested, across every market it is holding.
        self.trades_since_summary += trades;
        // Only the active tab's overlay chrome is on screen to react; a
        // background tab that lost its marks says so when it comes forward,
        // through the same empty overlay.
        if cleared_active {
            self.note_overlay_cleared(true);
        }
    }

    /// Tab shortcuts (§10): `Ctrl+T` new, `Ctrl+W` close, `Ctrl+Tab` cycle.
    fn handle_tab_keys(&mut self, ctx: &egui::Context) {
        let (new_tab, close_tab, next, previous) = ctx.input_mut(|input| {
            (
                input.consume_shortcut(&NEW_TAB_SHORTCUT),
                input.consume_shortcut(&CLOSE_TAB_SHORTCUT),
                input.consume_shortcut(&NEXT_TAB_SHORTCUT),
                input.consume_shortcut(&PREVIOUS_TAB_SHORTCUT),
            )
        });
        if new_tab {
            self.source_picker = Some(SourcePicker::new(&self.config));
        }
        if close_tab {
            self.close_tab(self.active_tab);
        }
        if next {
            self.cycle_tab(1);
        }
        if previous {
            self.cycle_tab(-1);
        }
    }

    /// The `+` dialog, while it is open.
    fn draw_source_picker(&mut self, ctx: &egui::Context) {
        let Some(picker) = self.source_picker.as_mut() else {
            return;
        };
        match picker.draw(ctx, &self.config) {
            PickerOutcome::Open => {}
            PickerOutcome::Cancel => self.source_picker = None,
            PickerOutcome::Chosen(feed_id, symbol) => {
                self.source_picker = None;
                self.open_tab(feed_id, symbol);
            }
        }
    }

    /// Carry out what the tab strip asked for.
    fn apply_tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::Activate(index) => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                }
            }
            TabAction::Close(index) => self.close_tab(index),
            TabAction::New => self.source_picker = Some(SourcePicker::new(&self.config)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_decimal::Decimal;
    use tokio::sync::mpsc;

    use quantick_feed_binance::depth::DepthEvent;

    use crate::config::{FeedCapabilities, FeedConfig, ProviderKind};
    use crate::drawings::{ChartPoint, PresetHost};
    use crate::feed::{FeedConnectionState, FeedEvent, FeedNotice};
    use crate::pane::DrawingDrag;
    use crate::pane::{CANVAS_DIVIDER_PX, DEFAULT_PANE_FRACTION, MIN_PANE_FRACTION};
    use crate::tab::BOOK_GENERATION_STRIDE;
    use crate::time_header;

    /// Run a tab operation that needs the config, splitting the borrow the
    /// way the frame loop does.
    fn with_config<R>(app: &mut QuantickApp, f: impl FnOnce(&mut Tab, &AppConfig) -> R) -> R {
        let (tab, config) = app.active_with_config();
        f(tab, config)
    }

    #[test]
    fn the_live_strip_carves_between_chart_and_gutter_only_when_shown() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let off = plot_split(area, 0.0, 0);
        assert!(off.live_strip.is_none());
        assert_eq!(off.chart.right(), off.price_gutter.left());

        let on = plot_split(area, crate::live_strip::LIVE_STRIP_WIDTH_PX, 0);
        let strip = on.live_strip.expect("strip rect");
        assert_eq!(on.chart.right(), strip.left());
        assert_eq!(strip.right(), on.price_gutter.left());
        assert_eq!(strip.width(), crate::live_strip::LIVE_STRIP_WIDTH_PX);
        // The strip pays with the chart's pixels: the gutter stays put, and
        // the time axis keeps spanning exactly the chart body.
        assert_eq!(on.price_gutter, off.price_gutter);
        assert_eq!(
            on.chart.width(),
            off.chart.width() - crate::live_strip::LIVE_STRIP_WIDTH_PX
        );
        assert_eq!(on.time_strip.right(), on.chart.right());
    }

    /// The pane band is carved once, inside `plot_split`, so the rect the
    /// renderer scales prices to is the rect the input handler hit-tests
    /// against. When the two disagreed, a drawing was placed where you
    /// clicked and then selected somewhere else — by 20% of the chart height
    /// per visible pane.
    #[test]
    fn the_pane_band_comes_out_of_every_callers_chart_rect() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let none = plot_split(area, 0.0, 0);
        assert!(none.indicator_panes.is_empty());

        let one = plot_split(area, 0.0, 1);
        let pane = *one
            .indicator_panes
            .first()
            .expect("one visible pane claims one rect");
        assert!(
            one.chart.height() < none.chart.height(),
            "the band is paid for out of the candles' pixels"
        );
        assert_eq!(one.chart.bottom(), pane.top(), "no gap, no overlap");
        assert_eq!(pane.bottom(), none.chart.bottom());
        assert_eq!(one.chart.width(), none.chart.width());
        // The axes stay where they were: only the candle body shrinks.
        assert_eq!(one.price_gutter, none.price_gutter);
        assert_eq!(one.time_strip, none.time_strip);

        let three = plot_split(area, 0.0, 3);
        assert_eq!(three.indicator_panes.len(), 3);
        assert!(three.chart.height() < one.chart.height());
    }

    /// Each pane zooms from the strip under it, and the split is exactly the
    /// divider — so a drag can never mean both time axes at once.
    #[test]
    fn the_time_strip_splits_at_the_lane_divider() {
        let strip = egui::Rect::from_min_max(egui::pos2(0.0, 580.0), egui::pos2(1000.0, 600.0));

        let (history, lane) = split_time_strip(strip, Some(700.0));
        let lane = lane.expect("the lane owns the strip under it");
        assert_eq!(history.left(), strip.left());
        assert_eq!(history.right(), 700.0);
        assert_eq!(lane.left(), 700.0);
        assert_eq!(lane.right(), strip.right());

        // Without a lane the candles keep the whole strip, exactly as before.
        assert_eq!(split_time_strip(strip, None), (strip, None));
        // A divider off the strip is not a split either.
        assert_eq!(split_time_strip(strip, Some(-5.0)), (strip, None));
    }

    /// The tape is inert: a gesture that lands on it must not reach the
    /// candles, and the divider belongs to the tape so the resize handle and
    /// the pan can never both fire on one pixel.
    #[test]
    fn a_gesture_on_the_tape_never_belongs_to_the_candles() {
        assert!(!gesture_hits_lane(Some(700.0), 699.9));
        assert!(gesture_hits_lane(Some(700.0), 700.0));
        assert!(gesture_hits_lane(Some(700.0), 1_200.0));
        // No lane: every pixel is the candles', exactly as before it existed.
        assert!(!gesture_hits_lane(None, 1_200.0));
    }

    #[test]
    fn the_lane_axis_reads_its_window_in_a_human_unit() {
        assert_eq!(fmt_window(800), "800 ms");
        assert_eq!(fmt_window(8_000), "8.0 s");
        assert_eq!(fmt_window(90_000), "1.5 min");
        assert_eq!(fmt_window(-1), "0 ms");
    }

    /// A minimal one-feed, two-symbol config for the app tests.
    fn test_config() -> AppConfig {
        AppConfig {
            default_feed: "binance".to_string(),
            default_symbol: "TESTUSDT".to_string(),
            feeds: vec![FeedConfig {
                id: "binance".to_string(),
                name: "Binance".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["TESTUSDT".to_string(), "ETHUSDT".to_string()],
                bubble_preset: None,
            }],
            metatrader: Default::default(),
        }
    }

    #[test]
    fn a_quote_driven_feed_says_so_where_the_side_note_goes() {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                // What a live Tickmill US500 session publishes once its bridge
                // says hello.
                capabilities: feed::fixed_capabilities(FeedCapabilities {
                    book_capture: false,
                    history_paging: false,
                    traded_volume: false,
                }),
                commands: cmd_tx,
                replay: None,
            },
        );
        let _ends = (evt_tx, book_tx);

        let (label, detail) = app
            .active_tab()
            .side_note(&app.config)
            .expect("a quote-driven feed discloses itself");
        assert_eq!(
            label, "prints: quote-derived",
            "a chart of one-unit prints must not read as a real tape"
        );
        // Short label, full story on hover: this row shares its space with the
        // machinery readouts, and a long label paints over them.
        assert!(label.len() < 25, "the label has to fit beside the readouts");
        assert!(
            detail.is_some_and(|text| text.contains("one synthetic print per tick")),
            "the hover has to explain what a quote-derived print is"
        );
        // And the affordances that would need a size are off with it.
        assert!(!app.active_tab().capabilities(&app.config).traded_volume);
        assert!(!app.active_tab().capabilities(&app.config).book_capture);
    }

    /// An app wired to in-memory channels, plus the test's ends of them: send
    /// feed events in, observe feed commands out. No egui, no network.
    fn test_app() -> (
        QuantickApp,
        mpsc::Sender<FeedEvent>,
        mpsc::Receiver<FeedCommand>,
        mpsc::Sender<DepthEvent>,
    ) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );
        (app, evt_tx, cmd_rx, book_tx)
    }

    /// The same app, plus the notice sender its feed would hold. The other
    /// ends come back so the caller keeps the channels open, exactly as a live
    /// feed thread would.
    #[allow(clippy::type_complexity)]
    /// A library entry whose file is gone must still produce something the
    /// user can see: the click used to log a warning and leave the chart
    /// unchanged, while this function's doc promised an error slot.
    #[test]
    fn a_script_that_no_longer_reads_becomes_a_visible_error_slot() {
        let (mut app, _events, _commands, _book) = test_app();
        let dir = std::env::temp_dir().join(format!("quantick-app-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("vanishing.pine");
        std::fs::write(
            &path,
            "//@version=5
plot(close)
",
        )
        .expect("write");

        app.script_library = crate::indicators::library::ScriptLibrary::scan_dir(&dir);
        let index = app
            .script_library
            .entries()
            .iter()
            .position(|e| e.name == "vanishing.pine")
            .expect("the file was scanned");
        std::fs::remove_file(&path).expect("remove");

        let before = app.active_tab().flow_pane.indicators.all().len();
        let slot = app
            .add_script_indicator(index)
            .expect("a click on a known entry claims a slot");
        assert_eq!(
            app.active_tab().flow_pane.indicators.all().len(),
            before + 1,
            "a slot appeared"
        );
        let view = app
            .active_tab()
            .flow_pane
            .indicators
            .all()
            .iter()
            .find(|v| v.slot == slot)
            .expect("the slot has a view");
        assert!(view.error.is_some(), "and it carries the read failure");
        assert_eq!(view.label(), "vanishing.pine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The PR's headline behaviour had no test: everything sat on the serde
    /// layer, and the wiring is where the defects were.
    #[test]
    fn the_indicator_set_restores_from_disk_and_saves_back() {
        use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind};

        let (mut app, _events, _commands, _book) = test_app();
        let path = std::env::temp_dir().join(format!(
            "quantick-indicator-state-app-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        app.indicator_state_path = path.clone();

        // A saved set: one native with bound inputs, one hidden native, and
        // a script the library does not have.
        crate::indicators::state_file::save(
            &path,
            &[
                SavedIndicator {
                    kind: SavedKind::NativeEma,
                    hidden: false,
                    inputs: vec![SavedInput::Int(21), SavedInput::Source("close".to_owned())],
                },
                SavedIndicator {
                    kind: SavedKind::NativeCvd,
                    hidden: true,
                    inputs: Vec::new(),
                },
                SavedIndicator {
                    kind: SavedKind::Script {
                        name: "not-in-the-library.pine".to_owned(),
                    },
                    hidden: false,
                    inputs: Vec::new(),
                },
            ],
        );

        app.restore_indicator_state();
        assert_eq!(
            app.slot_kinds.len(),
            2,
            "a script the library lacks adds nothing, not a phantom slot"
        );
        assert_eq!(app.slot_kinds[0].1, SavedKind::NativeEma);
        assert_eq!(app.slot_kinds[1].1, SavedKind::NativeCvd);
        assert_eq!(app.pending_hidden.len(), 1, "the hidden flag survived");
        assert!(
            !app.indicator_state_dirty,
            "restoring is not a user edit and must not rewrite the file"
        );

        // A user edit, settled: the file must match the live set.
        app.mark_indicator_state_dirty();
        app.last_indicator_change =
            Some(Instant::now() - INDICATOR_STATE_SAVE_DEBOUNCE - Duration::from_millis(10));
        for event in app
            .active_tab_mut()
            .flow_pane
            .indicator_worker
            .drain_events()
        {
            app.active_tab_mut().flow_pane.indicators.apply(event);
        }
        app.maintain_indicator_state();
        let written = crate::indicators::state_file::load(&path);
        assert_eq!(
            written.len(),
            {
                let kinds = app.slot_kinds.clone();
                app.active_tab()
                    .flow_pane
                    .indicators
                    .all()
                    .iter()
                    .filter(|view| kinds.iter().any(|(owner, _)| owner.slot == view.slot))
                    .count()
            },
            "every slot with a known kind is written, and only those"
        );
        assert!(
            !app.indicator_state_dirty,
            "the debounce fired, so the change is written"
        );
        let _ = std::fs::remove_file(&path);
    }

    fn test_app_with_notices() -> (
        QuantickApp,
        mpsc::Sender<FeedNotice>,
        (mpsc::Sender<FeedEvent>, mpsc::Sender<DepthEvent>),
    ) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        let (notice_tx, notice_rx) = mpsc::channel(8);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: notice_rx,
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );
        (app, notice_tx, (evt_tx, book_tx))
    }

    #[test]
    fn the_newest_notice_wins_and_clear_puts_the_chart_back() {
        let (mut app, notices, _feed_ends) = test_app_with_notices();
        assert_eq!(
            app.active_tab().notice,
            FeedNotice::Clear,
            "nothing to report at birth"
        );

        // A burst arriving between two frames must leave the latest state, not
        // a queue of cards to page through.
        notices
            .blocking_send(FeedNotice::working("starting the MetaTrader bridge"))
            .unwrap();
        notices
            .blocking_send(FeedNotice::attention(
                "MetaTrader 5 is not running",
                "Open the terminal and log in.",
            ))
            .unwrap();
        app.active_tab_mut().drain_notices();
        assert!(
            matches!(app.active_tab().notice, FeedNotice::Attention { .. }),
            "the newest notice is what the user sees, got {:?}",
            app.active_tab().notice
        );

        // And a feed that recovers takes its own instruction back down.
        notices.blocking_send(FeedNotice::Clear).unwrap();
        app.active_tab_mut().drain_notices();
        assert_eq!(app.active_tab().notice, FeedNotice::Clear);
    }

    #[test]
    fn only_explicit_connection_notices_drive_transport_state() {
        let (mut app, notices, _feed_ends) = test_app_with_notices();
        app.active_tab_mut().latest_trade_latency_ms = Some(42);
        assert_eq!(
            app.active_tab().feed_connection,
            FeedConnectionState::Connecting
        );

        notices.blocking_send(FeedNotice::Connected).unwrap();
        app.active_tab_mut().drain_notices();
        assert_eq!(
            app.active_tab().feed_connection,
            FeedConnectionState::Connected
        );
        assert_eq!(app.active_tab().notice, FeedNotice::Clear);

        // The MetaTrader bridge supervisor and bridge server share this
        // channel. Progress or attention from either can arrive after the
        // server has reported Connected, so neither is a transport transition.
        notices
            .blocking_send(FeedNotice::working("late supervisor progress"))
            .unwrap();
        app.active_tab_mut().drain_notices();
        assert_eq!(
            app.active_tab().feed_connection,
            FeedConnectionState::Connected
        );
        assert_eq!(
            statusbar::feed_state(false, app.active_tab().feed_connection),
            statusbar::FeedState::Live
        );

        notices
            .blocking_send(FeedNotice::attention(
                "late supervisor warning",
                "No transport action.",
            ))
            .unwrap();
        app.active_tab_mut().drain_notices();
        assert_eq!(
            app.active_tab().feed_connection,
            FeedConnectionState::Connected
        );
        assert_eq!(
            statusbar::feed_state(false, app.active_tab().feed_connection),
            statusbar::FeedState::Live
        );

        notices
            .blocking_send(FeedNotice::reconnecting(
                "Hyperliquid disconnected — reconnecting",
            ))
            .unwrap();
        app.active_tab_mut().drain_notices();
        assert_eq!(
            app.active_tab().feed_connection,
            FeedConnectionState::Reconnecting
        );
        assert_eq!(
            statusbar::feed_state(false, app.active_tab().feed_connection),
            statusbar::FeedState::Reconnecting,
            "a previous latency observation must not keep a disconnected socket green"
        );

        notices.blocking_send(FeedNotice::Connected).unwrap();
        app.active_tab_mut().drain_notices();
        assert_eq!(
            app.active_tab().feed_connection,
            FeedConnectionState::Connected
        );
        assert_eq!(
            statusbar::feed_state(false, app.active_tab().feed_connection),
            statusbar::FeedState::Live
        );
    }

    #[test]
    fn a_feed_with_nothing_to_report_leaves_the_chart_alone() {
        // Binance and replay hand over a closed channel; draining it must be a
        // no-op rather than an error the app has to special-case.
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().drain_notices();
        assert_eq!(app.active_tab().notice, FeedNotice::Clear);
    }

    /// Take the `SetBookCapture` command every test app queues at
    /// construction and return its generation. Capture follows the feed, so
    /// there is exactly one of these in flight before any test acts.
    fn take_capture_start(commands: &mut mpsc::Receiver<FeedCommand>) -> u64 {
        match commands
            .try_recv()
            .expect("capture starts with the feed, not with the toggle")
        {
            FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation,
            } => initial_generation,
            _ => panic!("unexpected command"),
        }
    }

    fn enable_heatmap_with_snapshot(
        app: &mut QuantickApp,
        commands: &mut mpsc::Receiver<FeedCommand>,
    ) {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        let generation = take_capture_start(commands);
        app.active_tab_mut().tape_mut().set_depth_visible(true);
        app.active_tab_mut()
            .tape_mut()
            .handle_depth_event(DepthEvent::Snapshot {
                symbol: "TESTUSDT".to_owned(),
                generation,
                observed_at_ms: 1_100,
                effective_at_ms: 999,
                price_step: None,
                snapshot: BookSnapshot::new(
                    10,
                    vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                    vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                    BookCoverage::Limited {
                        levels_per_side: 1_000,
                    },
                ),
            });
        app.active_tab_mut().tape_mut().flush_for_test();
        assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
    }

    #[test]
    fn loader_survives_until_every_pending_load_is_answered() {
        // Two "load older" clicks land while the initial backfill is still in
        // flight: three loads pending. The first reply must NOT hide the
        // indicator - only the last one may.
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        assert_eq!(
            app.active_tab().loading.count(LoadingTask::History),
            1,
            "backfill in flight at start"
        );

        app.active_tab_mut().request_older_history();
        app.active_tab_mut().request_older_history();
        assert_eq!(app.active_tab().loading.count(LoadingTask::History), 3);

        evt_tx.try_send(FeedEvent::Backfilled(Vec::new())).unwrap();
        app.active_tab_mut().drain_feed();
        assert_eq!(
            app.active_tab().loading.count(LoadingTask::History),
            2,
            "older loads still pending"
        );

        evt_tx
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.active_tab_mut().drain_feed();
        assert_eq!(
            app.active_tab().loading.count(LoadingTask::History),
            1,
            "one reply answers one load"
        );

        evt_tx
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.active_tab_mut().drain_feed();
        assert_eq!(
            app.active_tab().loading.count(LoadingTask::History),
            0,
            "last reply hides the loader"
        );
    }

    #[test]
    fn rejected_request_does_not_arm_the_loader() {
        // With the command channel closed the request never reaches the feed,
        // so no reply will ever come - the count must not grow.
        let (mut app, _evt_tx, cmd_rx, _book_tx) = test_app();
        drop(cmd_rx);
        app.active_tab_mut().request_older_history();
        assert_eq!(
            app.active_tab().loading.count(LoadingTask::History),
            1,
            "only the initial backfill"
        );
    }

    #[test]
    fn a_source_reset_restarts_the_history_wait() {
        // Loads queued before a reset will never be answered; the refill after
        // the reset is the one load left in flight.
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().request_older_history();
        app.active_tab_mut().request_older_history();
        assert_eq!(app.active_tab().loading.count(LoadingTask::History), 3);
        app.active_tab_mut().flow_pane.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        app.active_tab_mut().drain_feed();
        assert_eq!(app.active_tab().loading.count(LoadingTask::History), 1);
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "bar-index drawings cannot survive a source reset honestly"
        );
    }

    #[test]
    fn bar_spec_change_defers_one_frame_and_shows_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().flow_pane.tick_n = 100;
        app.active_tab_mut().flow_pane.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        app.active_tab_mut().apply_spec_changes();
        assert!(app.active_tab().loading.is_active(LoadingTask::BarRebuild));
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &BarSpec::Tick(50),
            "the arming frame must paint the overlay before the rebuild runs"
        );

        app.active_tab_mut().apply_spec_changes();
        assert_eq!(app.active_tab().flow_pane.state.spec(), &BarSpec::Tick(100));
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "a new bar partition must not inherit old bar-index anchors"
        );
        assert!(!app.active_tab().loading.is_active(LoadingTask::BarRebuild));
    }

    #[test]
    fn a_still_moving_selector_keeps_deferring_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().flow_pane.tick_n = 100;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().flow_pane.tick_n = 200; // the drag continues
        app.active_tab_mut().apply_spec_changes();
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &BarSpec::Tick(50),
            "no rebuild mid-gesture"
        );
        assert!(app.active_tab().loading.is_active(LoadingTask::BarRebuild));

        app.active_tab_mut().apply_spec_changes();
        assert_eq!(app.active_tab().flow_pane.state.spec(), &BarSpec::Tick(200));
        assert!(!app.active_tab().loading.is_active(LoadingTask::BarRebuild));
    }

    #[test]
    fn an_unchanged_spec_never_arms_the_rebuild_indicator() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().apply_spec_changes();
        assert!(!app.active_tab().loading.is_active(LoadingTask::BarRebuild));
        assert!(app.active_tab().flow_pane.pending_spec.is_none());
    }

    /// A trade a tenth of a second after the last, one unit at a walking
    /// price, so bars carry distinct times and a readable price range.
    fn trade(agg_id: u64) -> quantick_engine::Trade {
        quantick_engine::Trade {
            agg_id,
            timestamp_ms: 1_000 + agg_id as i64 * 100,
            price: Decimal::from(100) + Decimal::new(agg_id as i64 % 20, 1),
            quantity: Decimal::ONE,
            side: if agg_id.is_multiple_of(2) {
                quantick_engine::Side::Buy
            } else {
                quantick_engine::Side::Sell
            },
        }
    }

    #[test]
    fn quiet_market_keeps_the_observed_arrival_latency_live() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().feed_connection = FeedConnectionState::Connected;
        let trade = trade(1);
        let received_at_ms = trade.timestamp_ms + 42;

        app.active_tab_mut()
            .ingest_live_trade_at(&trade, received_at_ms);

        assert_eq!(app.active_tab().trade_arrival_ms(), Some(42));
        assert_eq!(
            statusbar::feed_state(false, app.active_tab().feed_connection),
            statusbar::FeedState::Live
        );
        assert_eq!(
            app.active_tab().trade_arrival_ms(),
            Some(42),
            "reading the status again without another print must not age latency"
        );
    }

    /// The gap the removed `Stalled` state used to cover: a transport that
    /// stays open and stops delivering. No error, no disconnect, and the
    /// stored arrival figure never ages — only the tape's own age does.
    #[test]
    fn a_quiet_tape_reads_as_stale_while_arrival_stays_frozen() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().feed_connection = FeedConnectionState::Connected;
        let trade = trade(1);
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms + 42);

        // A moment later: fresh.
        let age = app
            .active_tab()
            .tape_age_at(trade.timestamp_ms + 500)
            .expect("a live tape has an age");
        assert!(age < metrics::STALE_TAPE_MS);
        assert_eq!(
            statusbar::tape_text(None, app.active_tab().trade_arrival_ms(), Some(age)),
            "arrival 42 ms"
        );

        // A minute of silence on the same open socket.
        let age = app
            .active_tab()
            .tape_age_at(trade.timestamp_ms + 60_000)
            .expect("still a tape, just an old one");
        assert!(age > metrics::STALE_TAPE_MS, "{age} ms");
        assert_eq!(
            app.active_tab().trade_arrival_ms(),
            Some(42),
            "the arrival observation is frozen, which is why it cannot report this"
        );
        assert_eq!(
            statusbar::tape_text(None, app.active_tab().trade_arrival_ms(), Some(age)),
            "stale 60 s"
        );
    }

    #[test]
    fn backfill_does_not_claim_a_live_transport_latency() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![trade(1)]))
            .unwrap();

        app.active_tab_mut().drain_feed();

        assert_eq!(app.active_tab().trade_arrival_ms(), None);
        assert_eq!(
            statusbar::feed_state(false, app.active_tab().feed_connection),
            statusbar::FeedState::Connecting
        );
    }

    #[test]
    fn one_ui_drain_uses_one_observation_for_single_and_batched_trades() {
        use std::cell::Cell;

        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let last_trade = trade(3);
        let received_at_ms = last_trade.timestamp_ms + 75;
        evt_tx.try_send(FeedEvent::Live(trade(1))).unwrap();
        evt_tx
            .try_send(FeedEvent::LiveBatch(vec![trade(2), last_trade]))
            .unwrap();
        let clock_calls = Cell::new(0_u32);

        app.active_tab_mut().drain_feed_with_clock(|| {
            clock_calls.set(clock_calls.get() + 1);
            received_at_ms
        });

        assert_eq!(clock_calls.get(), 1, "one wall-clock read per UI drain");
        // Per market. The window's own summary counter adds these up across
        // every tab, in `drain_tabs`.
        assert_eq!(app.active_tab().live_trades, 3);
        assert_eq!(app.active_tab().trade_arrival_ms(), Some(75));
        assert_eq!(app.active_tab().flow_pane.state.timeline_revision(), 3);
        assert_eq!(
            app.active_tab()
                .flow_pane
                .state
                .partial()
                .map(|bar| bar.trade_count),
            Some(3)
        );
    }

    #[test]
    fn one_book_drain_uses_one_clock_observation() {
        use std::cell::Cell;

        let (mut app, _evt_tx, _cmd_rx, book_tx) = test_app();
        for _ in 0..2 {
            book_tx
                .try_send(DepthEvent::Status {
                    symbol: "TESTUSDT".to_owned(),
                    generation: 1,
                    status: quantick_orderbook::DepthStatus::Connecting,
                })
                .unwrap();
        }
        let clock_calls = Cell::new(0_u32);

        app.active_tab_mut().drain_book_feed_with_clock(|| {
            clock_calls.set(clock_calls.get() + 1);
            10_000
        });

        assert_eq!(clock_calls.get(), 1, "one wall-clock read per UI drain");
    }

    /// An app holding `count` backfilled trades, built into tick(1) bars — one
    /// bar per trade, the finest series a spec change can coarsen.
    fn app_with_history(count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
        let (mut app, evt_tx, cmd_rx, _book_tx) = test_app();
        app.active_tab_mut().flow_pane.tick_n = 1;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        let trades: Vec<_> = (1..=count).map(trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.active_tab_mut().drain_feed();
        assert_eq!(app.active_tab().flow_pane.state.bars().len() as u64, count);
        (app, cmd_rx)
    }

    /// The dark chart: with the view panned into history, a coarser spec cuts
    /// the same trades into far fewer bars, and the old right-edge index falls
    /// off the end of the series — leaving the window over empty space, where
    /// nothing is drawn at all.
    #[test]
    fn a_rebuild_keeps_the_view_on_the_market_time_it_was_showing() {
        let (mut app, _cmd_rx) = app_with_history(400);
        // Pan back to bar 200 of 400 and remember what the edge was showing.
        let slots = app.active_tab().flow_pane.slots();
        app.active_tab_mut()
            .flow_pane
            .viewport
            .pan_pixels(200.0 * 8.0, slots);
        assert!(!app.active_tab().flow_pane.viewport.follows_live());
        let was_showing = app
            .active_tab()
            .flow_pane
            .right_edge_time()
            .expect("a bar under the edge");

        // Coarsen: 400 trades become 10 bars, so index 200 no longer exists.
        app.active_tab_mut().flow_pane.tick_n = 40;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        assert_eq!(app.active_tab().flow_pane.state.bars().len(), 10);

        let slots = app.active_tab().flow_pane.slots();
        let (start, end) = app
            .active_tab()
            .flow_pane
            .viewport
            .visible_range(800.0, slots);
        assert!(
            start < end,
            "the window must still hold bars, got {start}..{end} of {slots}"
        );
        let now_showing = app
            .active_tab()
            .flow_pane
            .right_edge_time()
            .expect("still on a bar");
        let bar = &app.active_tab().flow_pane.state.bars()
            [app.active_tab().flow_pane.viewport.right_edge_bar(slots) as usize];
        assert!(
            bar.open_time <= was_showing && was_showing <= bar.close_time,
            "the edge bar ({}..{}) must span the time it was showing ({was_showing})",
            bar.open_time,
            bar.close_time
        );
        assert!(now_showing <= was_showing, "never jumps into the future");
    }

    /// Finer, not coarser: the series grows and the same market time moves to
    /// a much higher index. Following that is what keeps the user's place.
    #[test]
    fn a_finer_spec_follows_the_same_market_time_forward() {
        let (mut app, _cmd_rx) = app_with_history(400);
        app.active_tab_mut().flow_pane.tick_n = 40;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        let slots = app.active_tab().flow_pane.slots();
        app.active_tab_mut()
            .flow_pane
            .viewport
            .pan_pixels(5.0 * 8.0, slots); // back to bar 4 of 10
        let was_showing = app
            .active_tab()
            .flow_pane
            .right_edge_time()
            .expect("a bar under the edge");

        app.active_tab_mut().flow_pane.tick_n = 1;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        assert_eq!(app.active_tab().flow_pane.state.bars().len(), 400);
        let edge = app
            .active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots());
        assert_eq!(
            edge, 160.0,
            "bar 4 of tick(40) opens on trade 161 — bar 160 of tick(1)"
        );
        assert_eq!(
            app.active_tab().flow_pane.right_edge_time(),
            Some(was_showing)
        );
    }

    /// A view following the live edge is already anchored to the newest bar,
    /// whatever the rebuild does to the ones behind it.
    #[test]
    fn a_rebuild_leaves_a_live_view_at_the_live_edge() {
        let (mut app, _cmd_rx) = app_with_history(400);
        assert!(app.active_tab().flow_pane.viewport.follows_live());
        app.active_tab_mut().flow_pane.tick_n = 40;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        assert!(app.active_tab().flow_pane.viewport.follows_live());
        assert_eq!(
            app.active_tab()
                .flow_pane
                .viewport
                .right_edge_bar(app.active_tab().flow_pane.slots()),
            9.0
        );
    }

    /// Every string the frame painted, panels and chart alike.
    fn painted_text(output: &egui::FullOutput) -> Vec<String> {
        fn walk(shape: &egui::Shape, found: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => found.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// Whether the frame drew the price axis over the test's price range —
    /// the labels only exist once the chart really scaled and painted itself.
    fn has_price_axis(texts: &[String]) -> bool {
        texts.iter().any(|text| {
            text.parse::<f64>()
                .is_ok_and(|price| (95.0..=115.0).contains(&price))
        })
    }

    fn run_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
        run_frame_with_events(app, ctx, Vec::new())
    }

    fn run_frame_with_events(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        run_frame_with_modifiers(app, ctx, events, egui::Modifiers::NONE)
    }

    fn run_frame_with_modifiers(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1400.0, 900.0),
            )),
            events,
            modifiers,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.draw_frame(ctx, Instant::now()))
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click_chart(app: &mut QuantickApp, ctx: &egui::Context, position: egui::Pos2) {
        run_frame_with_events(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
            ],
        );
        run_frame_with_events(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, false),
            ],
        );
    }

    fn drag_chart(app: &mut QuantickApp, ctx: &egui::Context, start: egui::Pos2, end: egui::Pos2) {
        run_frame_with_events(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(end)]);
        run_frame_with_events(
            app,
            ctx,
            vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        );
    }

    fn drawing_tool(id: &str) -> drawings::DrawingTool {
        drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == id)
            .expect("drawing tool is registered")
    }

    fn arm_drawing_from_toolbox(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        id: &str,
    ) -> drawings::DrawingTool {
        let drawing = drawing_tool(id);
        let tool = Tool::Drawing(drawing);
        let button = app
            .toolrail
            .button_rect(tool)
            .expect("drawing button was rendered");
        click_chart(app, ctx, button.center());
        assert_eq!(
            app.toolrail.tool(),
            tool,
            "clicking {id} must arm that drawing tool"
        );
        drawing
    }

    /// Full UI interaction proof: every registered drawing is placed through
    /// egui pointer events against the real chart frame. This catches the
    /// original regression where multi-point tools silently ignored drags.
    #[test]
    fn every_toolbox_drawing_can_be_plotted_on_the_chart() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        click_chart(&mut app, &ctx, egui::pos2(600.0, 250.0));

        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(620.0, 300.0),
            egui::pos2(800.0, 450.0),
        );

        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(650.0, 500.0),
            egui::pos2(820.0, 350.0),
        );
        click_chart(&mut app, &ctx, egui::pos2(900.0, 280.0));

        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(700.0, 620.0),
            egui::pos2(950.0, 300.0),
        );

        arm_drawing_from_toolbox(&mut app, &ctx, "fib-extension");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(620.0, 650.0),
            egui::pos2(820.0, 500.0),
        );
        click_chart(&mut app, &ctx, egui::pos2(1_000.0, 350.0));

        let tools: Vec<_> = app
            .active_tab()
            .flow_pane
            .drawings
            .items()
            .iter()
            .map(|drawing| drawing.tool)
            .collect();
        assert_eq!(tools, drawings::DRAWING_TOOLS);
        assert!(
            app.active_tab_mut()
                .flow_pane
                .drawings
                .items()
                .iter()
                .all(|drawing| drawing.points.len() == drawing.tool.required_points())
        );
        assert_eq!(
            app.toolrail.tool(),
            Tool::Pointer,
            "placing a complete drawing restores navigation"
        );
    }

    #[test]
    fn a_drawing_can_be_selected_from_its_stroke_and_moved_without_panning() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        let before = app.active_tab().flow_pane.drawings.items()[0].points[0];
        let viewport_before = app
            .active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots());
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(1_000.0, 300.0),
            egui::pos2(1_040.0, 340.0),
        );
        let after = app.active_tab().flow_pane.drawings.items()[0].points[0];

        assert!(
            after.bar > before.bar,
            "dragging right moves the anchor right"
        );
        assert!(
            after.price < before.price,
            "dragging down moves the anchor to a lower price"
        );
        assert_eq!(
            app.active_tab()
                .flow_pane
                .viewport
                .right_edge_bar(app.active_tab().flow_pane.slots()),
            viewport_before,
            "moving a drawing must not pan the market underneath it"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawing_drag,
            DrawingDrag::None,
            "release ends the move gesture"
        );
    }

    #[test]
    fn inspector_never_blocks_drawing_drag() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let anchor = egui::pos2(300.0, 250.0);
        click_chart(&mut app, &ctx, anchor);
        run_frame(&mut app, &ctx);

        // The horizontal stroke crosses the whole pane, so wherever placement
        // put the window, some stroke pixel sits underneath it. Drag exactly
        // that pixel: the gesture must stay with the drawing.
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("placing the selected line opens its inspector");
        let line_y = anchor.y;
        let start = egui::pos2(inspector.center().x, line_y);
        assert!(
            inspector.contains(start),
            "the regression requires the floating inspector to cover this stroke pixel"
        );
        let before = app.active_tab().flow_pane.drawings.items()[0].points[0];

        drag_chart(&mut app, &ctx, start, egui::pos2(start.x, line_y + 100.0));

        assert_ne!(
            app.active_tab().flow_pane.drawings.items()[0].points[0].price,
            before.price,
            "the open inspector must not trap the line underneath it"
        );
    }

    #[test]
    fn drawing_actions_visible_without_scroll_at_360px() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        let output = run_frame(&mut app, &ctx);
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        assert!(
            inspector.width() >= INSPECTOR_MIN_WIDTH_PX - 1.0,
            "the inspector must respect its minimum width; got {}",
            inspector.width()
        );
        let texts = painted_text(&output);
        for label in ["Lock drawing", "Delete drawing"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the named action {label:?} must be visible without scrolling; painted: {texts:?}"
            );
        }
    }

    #[test]
    fn inspector_opens_beside_the_selection_inside_the_chart() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(300.0, 300.0),
            egui::pos2(400.0, 380.0),
        );
        run_frame(&mut app, &ctx);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        let chart = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("the chart pane was laid out");
        let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 300.0), egui::pos2(400.0, 380.0))
            .expand(DRAWING_ANCHOR_RADIUS_PX);
        assert!(
            !inspector.intersects(bbox),
            "the inspector must open beside the selection, not on top of it: {inspector:?} vs {bbox:?}"
        );
        assert!(
            chart.contains_rect(inspector),
            "the inspector must stay inside the chart pane (never over the axes): {inspector:?} vs {chart:?}"
        );
    }

    #[test]
    fn pinning_the_inspector_docks_it_and_frees_the_canvas() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(300.0, 300.0),
            egui::pos2(400.0, 380.0),
        );
        // Let the window settle its size and position before reading rects.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let chart_before = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("chart laid out");

        let pin = app.inspector_pin_rect.expect("pin button rendered");
        click_chart(&mut app, &ctx, pin.center());
        assert!(app.inspector_pinned, "clicking Pin docks the inspector");
        run_frame(&mut app, &ctx);
        let chart_after = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("chart laid out");
        assert!(
            chart_after.width() < chart_before.width(),
            "the docked inspector must be paid for by the canvas, not float over it"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Delete drawing")),
            "the docked panel still shows the named actions"
        );
    }

    #[test]
    fn button_manager_and_keyboard_send_the_same_delete_command() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.drawing_manager_open = true;
        // Let the manager window settle its size before reading button rects.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let delete = app
            .manager_action_rects
            .iter()
            .find(|(index, action, _)| *index == 0 && *action == "Delete")
            .map(|(_, _, rect)| *rect)
            .expect("the manager lists the drawing with a Delete action");
        click_chart(&mut app, &ctx, delete.center());
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "the manager's Delete lands the same command"
        );
        assert!(
            app.drawing_toast.is_some(),
            "the manager delete raises the same Undo toast as the keyboard"
        );
        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            1,
            "one undo rewinds the manager delete, exactly like the keyboard one"
        );
    }

    #[test]
    fn the_object_manager_toggles_eye_lock_and_z_order_per_row() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        for price_y in [250.0, 350.0] {
            app.toolrail
                .arm(Tool::Drawing(drawing_tool("horizontal-line")));
            click_chart(&mut app, &ctx, egui::pos2(700.0, price_y));
        }
        let objects = app
            .toolrail
            .objects_button_rect()
            .expect("the toolbox shows the Objects entry");
        click_chart(&mut app, &ctx, objects.center());
        assert!(
            app.drawing_manager_open,
            "the Objects button opens the manager"
        );
        run_frame(&mut app, &ctx);

        let rect_of = |app: &QuantickApp, index: usize, action: &str| {
            app.manager_action_rects
                .iter()
                .find(|(row, name, _)| *row == index && *name == action)
                .map(|(_, _, rect)| *rect)
                .expect("manager action rendered")
        };

        let eye = rect_of(&app, 0, "Eye");
        click_chart(&mut app, &ctx, eye.center());
        assert!(
            app.active_tab().flow_pane.drawings.items()[0].hidden,
            "the row's eye hides it"
        );

        run_frame(&mut app, &ctx);
        let lock = rect_of(&app, 1, "Lock");
        click_chart(&mut app, &ctx, lock.center());
        assert!(
            app.active_tab().flow_pane.drawings.items()[1].locked,
            "the row's lock locks it"
        );

        run_frame(&mut app, &ctx);
        let front = rect_of(&app, 0, "Front");
        let hidden_line_price = app.active_tab().flow_pane.drawings.items()[0].points[0].price;
        click_chart(&mut app, &ctx, front.center());
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[1].points[0].price,
            hidden_line_price,
            "Front moves the object to the top of the z-order"
        );
    }

    #[test]
    fn a_selected_drawing_exposes_its_edit_and_delete_controls() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(620.0, 300.0),
            egui::pos2(800.0, 450.0),
        );
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
        assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));

        let output = run_frame(&mut app, &ctx);
        assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
        let texts = painted_text(&output);
        for label in [
            "Rectangle settings",
            "Style",
            "line width (px)",
            "fill opacity",
            "Delete drawing",
        ] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "selected drawing inspector omitted {label:?}; painted text: {texts:?}"
            );
        }
    }

    #[test]
    fn rectangle_anchor_resizes_while_the_settings_window_stays_non_modal() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        let first_anchor = egui::pos2(620.0, 300.0);
        let second_anchor = egui::pos2(800.0, 450.0);
        drag_chart(&mut app, &ctx, first_anchor, second_anchor);
        let before = app.active_tab().flow_pane.drawings.items()[0]
            .points
            .clone();
        let viewport_before = app
            .active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots());

        let inspector = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            inspector
                .iter()
                .any(|text| text.contains("Rectangle settings")),
            "the settings window must be visible before the resize gesture"
        );

        drag_chart(&mut app, &ctx, first_anchor, egui::pos2(560.0, 240.0));
        let after = &app.active_tab().flow_pane.drawings.items()[0].points;

        assert_ne!(after[0], before[0], "the dragged corner must move");
        assert_eq!(
            after[1], before[1],
            "resizing one corner must leave the opposite corner fixed"
        );
        assert_eq!(
            app.active_tab()
                .flow_pane
                .viewport
                .right_edge_bar(app.active_tab().flow_pane.slots()),
            viewport_before,
            "resizing a drawing must not pan the chart"
        );
        assert_eq!(app.active_tab().flow_pane.drawing_drag, DrawingDrag::None);
        assert!(
            painted_text(&run_frame(&mut app, &ctx))
                .iter()
                .any(|text| text.contains("Rectangle settings")),
            "the non-modal settings window remains usable after resizing"
        );
    }

    fn key_press(key: egui::Key) -> egui::Event {
        key_press_with(key, egui::Modifiers::NONE)
    }

    fn key_press_with(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Whether any painted line segment uses `color` — proof a stroke of that
    /// colour reached the screen (or, negated, that a hidden object did not).
    fn painted_line_with_color(output: &egui::FullOutput, color: egui::Color32) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::LineSegment { stroke, .. } => {
                stroke.color == egui::epaint::ColorMode::Solid(color)
            }
            _ => false,
        })
    }

    #[test]
    fn locked_drawing_rejects_geometry_and_keyboard_delete() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.active_tab_mut()
            .flow_pane
            .drawings
            .set_selected_locked(true);

        let before = app.active_tab().flow_pane.drawings.items()[0].points[0];
        let viewport_before = app
            .active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots());
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(1_000.0, 300.0),
            egui::pos2(1_040.0, 340.0),
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[0].points[0],
            before,
            "locked geometry must not move"
        );
        assert_eq!(
            app.active_tab()
                .flow_pane
                .viewport
                .right_edge_bar(app.active_tab().flow_pane.slots()),
            viewport_before,
            "the blocked gesture still belongs to the drawing - the chart must not pan"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            1,
            "keyboard delete must not remove a locked drawing"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Delete locked drawing?")),
            "the same confirmation appears next to the trigger; painted: {texts:?}"
        );
    }

    #[test]
    fn one_drag_creates_one_undo_entry() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(
            app.active_tab().flow_pane.drawings.undo_depth(),
            1,
            "creating the drawing is the first undo entry"
        );

        let before_drag = app.active_tab().flow_pane.drawings.items()[0].points[0];
        let start = egui::pos2(1_000.0, 300.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        for step in [egui::pos2(1_010.0, 312.0), egui::pos2(1_025.0, 326.0)] {
            run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(step)]);
        }
        let end = egui::pos2(1_040.0, 340.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        );
        assert_ne!(
            app.active_tab().flow_pane.drawings.items()[0].points[0],
            before_drag,
            "the drag really moved the line"
        );

        assert_eq!(
            app.active_tab().flow_pane.drawings.undo_depth(),
            2,
            "a multi-frame drag coalesces into exactly one undo entry"
        );
        assert!(
            app.active_tab_mut().flow_pane.drawings.undo(),
            "undo the drag"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[0].points[0],
            before_drag,
            "one undo rewinds the whole drag"
        );
        assert!(
            app.active_tab_mut().flow_pane.drawings.undo(),
            "undo the creation"
        );
        assert!(app.active_tab().flow_pane.drawings.items().is_empty());
    }

    #[test]
    fn delete_and_backspace_never_leak_out_of_focused_inputs() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("test-text-input")));
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                key_press(egui::Key::Delete),
                key_press(egui::Key::Backspace),
            ],
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            1,
            "while an input owns the keyboard the delete keys stay in it"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "with focus released the same key deletes the selection"
        );
    }

    #[test]
    fn keyboard_delete_offers_an_undo_toast_and_ctrl_z_restores() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(app.active_tab().flow_pane.drawings.items().is_empty());
        let texts = painted_text(&run_frame(&mut app, &ctx));
        for label in ["Drawing deleted.", "Undo"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the toast must offer {label:?}; painted: {texts:?}"
            );
        }

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            1,
            "Ctrl+Z drives the same history as the toast's Undo"
        );

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Y, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "Ctrl+Y redoes the undone delete"
        );
    }

    #[test]
    fn hidden_drawing_neither_paints_nor_hit_tests() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let stroke_position = egui::pos2(700.0, 300.0);
        click_chart(&mut app, &ctx, stroke_position);
        let marker = egui::Color32::from_rgb(1, 2, 3);
        app.active_tab_mut()
            .flow_pane
            .drawings
            .selected_mut()
            .expect("placement selects the line")
            .style
            .color = marker;
        assert!(
            painted_line_with_color(&run_frame(&mut app, &ctx), marker),
            "the visible line paints its stroke"
        );

        app.active_tab_mut()
            .flow_pane
            .drawings
            .set_selected_hidden(true);
        assert!(
            !painted_line_with_color(&run_frame(&mut app, &ctx), marker),
            "a hidden drawing must not paint"
        );

        app.active_tab_mut().flow_pane.drawings.select(None);
        click_chart(&mut app, &ctx, stroke_position);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            None,
            "a hidden drawing must not hit-test"
        );

        let viewport_before = app
            .active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots());
        drag_chart(&mut app, &ctx, stroke_position, egui::pos2(640.0, 260.0));
        assert_ne!(
            app.active_tab()
                .flow_pane
                .viewport
                .right_edge_bar(app.active_tab().flow_pane.slots()),
            viewport_before,
            "over a hidden drawing the gesture belongs to the chart again"
        );
    }

    #[test]
    fn a_bar_rebuild_clears_drawings_with_an_explicit_notice_and_no_dead_undo() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.active_tab_mut().flow_pane.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        // Through the window's own drain: the tab drops the marks, and the
        // window is what turns that into the toast.
        app.drain_tabs();
        assert!(app.active_tab().flow_pane.drawings.items().is_empty());
        assert!(
            app.drawing_toast.is_some(),
            "the clear must raise the notice toast"
        );

        // A fresh egui Area sizes itself on its first frame; the text is
        // on screen from the second one.
        run_frame(&mut app, &ctx);
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Drawings cleared")),
            "losing the marks is never silent; painted: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Undo"),
            "the clear toast must not offer an Undo it cannot honour"
        );
    }

    #[test]
    fn escape_walks_confirm_draft_selection_then_pointer() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // Draft first: Esc cancels it and returns to Pointer.
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 1);
        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert!(
            app.active_tab().flow_pane.drawings.draft().is_none(),
            "Esc cancels the draft"
        );
        assert_eq!(app.toolrail.tool(), Tool::Pointer);

        // A locked selection with a pending confirmation: Esc peels one
        // layer per press — confirm, then selection, then nothing new.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.active_tab_mut()
            .flow_pane
            .drawings
            .set_selected_locked(true);
        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(app.drawing_delete_confirm, "the confirmation is pending");

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert!(!app.drawing_delete_confirm, "first Esc cancels the confirm");
        assert!(
            app.active_tab().flow_pane.drawings.selected().is_some(),
            "the selection survives"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            None,
            "second Esc deselects"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            1,
            "nothing was deleted"
        );
    }

    #[test]
    fn backspace_steps_back_through_the_draft_anchors() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
        click_chart(&mut app, &ctx, egui::pos2(750.0, 300.0));
        assert_eq!(app.active_tab().flow_pane.drawings.draft_len(), 2);

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Backspace)]);
        assert_eq!(
            app.active_tab().flow_pane.drawings.draft_len(),
            1,
            "Backspace removes the last placed anchor"
        );
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "the draft workflow never deletes finished objects"
        );
    }

    #[test]
    fn alt_l_and_alt_h_protect_the_selection_from_the_keyboard() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::L, egui::Modifiers::ALT)],
            egui::Modifiers::ALT,
        );
        assert!(
            app.active_tab().flow_pane.drawings.items()[0].locked,
            "Alt+L locks the selection"
        );

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::H, egui::Modifiers::ALT)],
            egui::Modifiers::ALT,
        );
        assert!(
            app.active_tab().flow_pane.drawings.items()[0].hidden,
            "Alt+H hides the selection"
        );
        assert_eq!(
            app.toolrail.tool(),
            Tool::Pointer,
            "Alt+H must not arm the horizontal-line tool"
        );
    }

    #[test]
    fn ctrl_d_duplicates_the_selection_offset_and_selected() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        let original_bar = app.active_tab().flow_pane.drawings.items()[0].points[0].bar;

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::D, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 2);
        assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(1));
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[1].points[0].bar,
            original_bar + DUPLICATE_OFFSET_BARS
        );
    }

    #[test]
    fn arrow_nudges_move_the_selection_and_shift_multiplies_by_ten() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        let start = app.active_tab().flow_pane.drawings.items()[0].points[0];
        let depth = app.active_tab().flow_pane.drawings.undo_depth();

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowRight)]);
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[0].points[0].bar,
            start.bar + 1.0,
            "one press is one bar"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.undo_depth(),
            depth + 1,
            "one press, one entry"
        );

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(
                egui::Key::ArrowRight,
                egui::Modifiers::SHIFT,
            )],
            egui::Modifiers::SHIFT,
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[0].points[0].bar,
            start.bar + 11.0,
            "Shift multiplies the nudge by ten"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowUp)]);
        assert!(
            app.active_tab().flow_pane.drawings.items()[0].points[0].price > start.price,
            "ArrowUp raises the price"
        );
    }

    #[test]
    fn the_repeat_pin_keeps_the_drawing_tool_armed() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // Default: one-shot back to Pointer.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
        assert_eq!(app.toolrail.tool(), Tool::Pointer);

        // Pinned: the tool stays armed for the next object.
        app.toolrail.set_repeat(true);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 320.0));
        assert_eq!(
            app.toolrail.tool().drawing_tool().map(|tool| tool.id()),
            Some("horizontal-line"),
            "the repeat pin keeps the tool armed"
        );
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 2);
    }

    #[test]
    fn the_fib_inspector_mounts_its_level_editor_tab() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 250.0),
            egui::pos2(900.0, 400.0),
        );
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Levels")),
            "the tool-owned tab is offered by name; painted: {texts:?}"
        );

        app.inspector_tab = InspectorTab::Extra;
        let texts = painted_text(&run_frame(&mut app, &ctx));
        for label in ["Preset", "Standard", "band opacity", "log scale"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the level editor must show {label:?}; painted: {texts:?}"
            );
        }
    }

    #[test]
    fn a_placed_fib_paints_its_levels_and_labels() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 250.0),
            egui::pos2(900.0, 400.0),
        );

        let texts = painted_text(&run_frame(&mut app, &ctx));
        for label in ["61.8%", "38.2%", "50.0%"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the standard retracement labels paint on the chart; painted: {texts:?}"
            );
        }
    }

    #[test]
    fn a_default_preset_shapes_new_fibs_and_leaves_existing_ones_alone() {
        use crate::drawings::DrawingPayload as _;
        use crate::drawings::fib::FibPayload;

        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // First fib: the built-in standard start.
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 250.0),
            egui::pos2(900.0, 400.0),
        );
        let standard_levels = app.active_tab().flow_pane.drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(standard_levels, 7);

        // Save a compact custom preset and make it the default for new fibs.
        let mut store = drawings::presets::PresetStore::load_from(std::env::temp_dir().join(
            format!("quantick-default-preset-test-{}.toml", std::process::id()),
        ));
        let mut compact = FibPayload::new(drawings::fib::FibKind::Retracement);
        compact.apply_preset(&drawings::fib::RETRACEMENT_PRESETS[1]);
        let exported = compact.export_preset().expect("fib exports presets");
        assert!(store.save_custom_preset("fib-retracement", "mine", exported, false));
        store.set_default_preset("fib-retracement", Some("mine".into()));
        let preset_path = store.path().to_path_buf();
        app.drawing_presets = store;

        // Second fib starts from the default preset...
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(400.0, 250.0),
            egui::pos2(550.0, 400.0),
        );
        let new_levels = app.active_tab().flow_pane.drawings.items()[1]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(new_levels, 5, "a new fib starts from the default preset");

        // ...and the first one is untouched.
        let old_levels = app.active_tab().flow_pane.drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(old_levels, 7, "the default never rewrites existing objects");
        let _ = std::fs::remove_file(preset_path);
    }

    /// The whole point, on a real frame: after a spec change with the view
    /// panned into history, the chart is drawn — not a black rectangle.
    #[test]
    fn a_rebuilt_chart_still_paints_itself() {
        let (mut app, _cmd_rx) = app_with_history(400);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx); // one frame to settle the layout
        let slots = app.active_tab().flow_pane.slots();
        app.active_tab_mut()
            .flow_pane
            .viewport
            .pan_pixels(200.0 * 8.0, slots);

        app.active_tab_mut().flow_pane.tick_n = 40;
        let armed = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            armed.iter().any(|text| text.contains("rebuilding bars")),
            "the arming frame says what it is doing: {armed:?}"
        );

        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "the chart must be on screen after the rebuild: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("no bars in view")),
            "and it must be showing bars, not an empty window: {texts:?}"
        );
    }

    /// The other way to empty the window — panning into the space past the
    /// newest bar, zoomed in far enough that no bar is left on screen. The
    /// chart keeps its axis and says how to get back.
    #[test]
    fn an_empty_window_says_so_instead_of_going_dark() {
        let (mut app, _cmd_rx) = app_with_history(400);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        app.active_tab_mut().flow_pane.viewport.zoom(8.0); // 64 px candles: only a dozen fit
        let slots = app.active_tab().flow_pane.slots();
        app.active_tab_mut()
            .flow_pane
            .viewport
            .pan_pixels(-10_000.0, slots); // into the empty future
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("no bars in view")),
            "an empty window must explain itself: {texts:?}"
        );
        assert!(
            has_price_axis(&texts),
            "and keep the axis, so the chart never reads as hung: {texts:?}"
        );
    }

    #[test]
    fn changing_feed_falls_back_to_a_valid_symbol() {
        // Two feeds with disjoint symbol lists: switching to a feed that does
        // not offer the current symbol must snap to that feed's first symbol.
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let config = AppConfig {
            default_feed: "a".to_string(),
            default_symbol: "AAA".to_string(),
            feeds: vec![
                FeedConfig {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["AAA".to_string()],
                    bubble_preset: None,
                },
                FeedConfig {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["BBB".to_string()],
                    bubble_preset: None,
                },
            ],
            metatrader: Default::default(),
        };
        let mut app = QuantickApp::new(
            config,
            "a",
            "AAA",
            BarSpec::Tick(10),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );

        app.active_tab_mut().feed_id = "b".to_string();
        with_config(&mut app, |tab, config| tab.ensure_symbol_valid(config));
        assert_eq!(
            app.active_tab().symbol,
            "BBB",
            "symbol snaps to feed b's first symbol"
        );

        // A symbol already valid for the feed is left untouched.
        app.active_tab_mut().symbol = "BBB".to_string();
        with_config(&mut app, |tab, config| tab.ensure_symbol_valid(config));
        assert_eq!(app.active_tab().symbol, "BBB");
    }

    /// One app on `config`, opened on `feed_id`/symbol — the smallest harness
    /// that exercises the constructor path (where a feed's declared bubble
    /// preset is applied).
    fn app_on(config: AppConfig, feed_id: &str, symbol: &str) -> QuantickApp {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        QuantickApp::new(
            config,
            feed_id,
            symbol,
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        )
    }

    #[test]
    fn a_feed_declaring_a_bubble_preset_opens_wearing_it() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("live lane pie".to_string());
        let app = app_on(config, "binance", "TESTUSDT");
        assert_eq!(
            app.active_tab().tape().active_preset_for_test(),
            "live lane pie"
        );
        assert!(
            app.active_tab()
                .tape()
                .config_for_test()
                .bubble_candle_summary,
            "the pie preset folds closed bars into per-price summaries"
        );
    }

    #[test]
    fn a_feed_declaring_an_unknown_bubble_preset_changes_nothing() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("no such preset".to_string());
        let with_unknown = app_on(config, "binance", "TESTUSDT");
        let untouched = app_on(test_config(), "binance", "TESTUSDT");
        assert_eq!(
            with_unknown.active_tab().tape().active_preset_for_test(),
            untouched.active_tab().tape().active_preset_for_test(),
            "a typo in the config must not restyle the chart"
        );
        assert_eq!(
            with_unknown.active_tab().tape().config_for_test(),
            untouched.active_tab().tape().config_for_test()
        );
    }

    #[test]
    fn switching_to_a_feed_with_a_declared_preset_applies_it_then() {
        // Feed "binance" declares nothing; a second feed declares the pie
        // look. Opening on the first must not apply it — moving to the
        // second must.
        let mut config = test_config();
        config.feeds.push(FeedConfig {
            id: "mt".to_string(),
            name: "MetaTrader 5".to_string(),
            provider: ProviderKind::MetaTrader,
            symbols: vec!["WINQ26".to_string()],
            bubble_preset: Some("live lane pie".to_string()),
        });
        let mut app = app_on(config, "binance", "TESTUSDT");
        let opened_with = app.active_tab().tape().active_preset_for_test().to_string();
        assert_ne!(
            opened_with, "live lane pie",
            "nothing declared, nothing applied"
        );

        // The switch path runs this after installing the new feed handle.
        app.active_tab_mut().feed_id = "mt".to_string();
        with_config(&mut app, |tab, config| {
            tab.apply_feed_bubble_preset_after_switch(config, "binance")
        });
        assert_eq!(
            app.active_tab().tape().active_preset_for_test(),
            "live lane pie"
        );
    }

    #[test]
    fn a_symbol_hop_inside_one_feed_keeps_the_panel_look() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("live lane pie".to_string());
        let mut app = app_on(config, "binance", "TESTUSDT");
        assert_eq!(
            app.active_tab().tape().active_preset_for_test(),
            "live lane pie"
        );

        // The user picks a different look by hand mid-session...
        assert!(app.active_tab_mut().tape_mut().apply_preset("dense tape"));
        // ...then hops symbols inside the same feed: the hand-picked look
        // survives — the declared preset belongs to the feed, not the symbol.
        with_config(&mut app, |tab, config| {
            tab.apply_feed_bubble_preset_after_switch(config, "binance")
        });
        assert_eq!(
            app.active_tab().tape().active_preset_for_test(),
            "dense tape"
        );

        // Arriving from another feed is what re-applies the declared look.
        with_config(&mut app, |tab, config| {
            tab.apply_feed_bubble_preset_after_switch(config, "other-feed")
        });
        assert_eq!(
            app.active_tab().tape().active_preset_for_test(),
            "live lane pie"
        );
    }

    #[test]
    fn capture_starts_with_the_feed_and_commits_only_after_the_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();

        // Construction already asked the feed to record: capture follows the
        // market, not the toolbar.
        assert_eq!(take_capture_start(&mut cmd_rx), BOOK_GENERATION_STRIDE);
        assert!(app.active_tab().tape().enabled());
        with_config(&mut app, |tab, config| tab.ensure_book_capture(config));
        assert!(
            cmd_rx.try_recv().is_err(),
            "a recorder already running needs no second command"
        );

        drop(cmd_rx);
        with_config(&mut app, |tab, config| {
            tab.request_book_capture(config, false)
        });
        assert!(
            app.active_tab().tape().enabled(),
            "closed command channel must preserve current capture state"
        );
    }

    /// The user's complaint made executable: hiding the map is pixels only.
    /// The recorder keeps running, so reopening it finds the history whole
    /// instead of a hole where the map was closed.
    #[test]
    fn hiding_the_heatmap_never_stops_the_recorder() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        let gaps_before = app.active_tab_mut().tape_mut().health().gaps;

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        assert!(app.active_tab().tape().depth_visible());

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
        assert!(
            !app.active_tab().tape().depth_visible(),
            "the map is hidden"
        );
        assert!(
            app.active_tab().tape().enabled(),
            "the recorder is untouched"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "showing or hiding the map sends no feed command"
        );

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        app.active_tab_mut().tape_mut().flush_for_test();
        assert!(app.active_tab().tape().depth_visible());
        assert_eq!(
            app.active_tab_mut().tape_mut().health().gaps,
            gaps_before,
            "the toggle must not punch a coverage gap into the recording"
        );
    }

    /// The guard that keeps an always-on recorder honest: a source with no
    /// depth pipeline — a replay, or a feed missing from the config — gets no
    /// recorder and no command, however often the app asks.
    #[test]
    fn a_source_without_depth_starts_no_recorder() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);

        let generation = app.active_tab_mut().next_book_generation();
        app.active_tab_mut()
            .tape_mut()
            .set_enabled(false, generation);
        app.active_tab_mut().feed_id = "not-in-the-config".to_owned();
        assert!(!app.active_tab().capabilities(&app.config).book_capture);

        with_config(&mut app, |tab, config| tab.ensure_book_capture(config));
        assert!(!app.active_tab().tape().enabled());
        assert!(
            cmd_rx.try_recv().is_err(),
            "a source with no book is never asked to record"
        );
    }

    #[test]
    fn bubble_toggle_needs_no_feed_command_and_leaves_capture_alone() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        assert!(!app.active_tab().tape().bubbles_enabled());

        app.active_tab_mut().tape_mut().set_bubbles_enabled(true);
        assert!(app.active_tab().tape().bubbles_enabled());
        assert!(
            cmd_rx.try_recv().is_err(),
            "aggregate trades already flow; no feed command is needed"
        );

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
        assert!(
            app.active_tab().tape().bubbles_enabled(),
            "hiding the book must not stop the bubbles"
        );
    }

    #[test]
    fn grouping_restart_commits_only_after_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let grouping = Decimal::new(5, 2);

        assert!(
            app.active_tab_mut()
                .tape_mut()
                .stage_capture_grouping_for_test(grouping)
        );
        assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
        app.active_tab_mut().restart_book_capture();

        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::RestartBookCapture { .. })
        ));
        assert_eq!(
            app.active_tab_mut()
                .tape_mut()
                .base_capture_grouping_for_test(),
            grouping
        );
        assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 0);
        assert_eq!(
            app.active_tab_mut().tape_mut().health().status,
            "connecting"
        );
    }

    #[test]
    fn closed_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app
            .active_tab_mut()
            .tape_mut()
            .base_capture_grouping_for_test();

        assert!(
            app.active_tab_mut()
                .tape_mut()
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        drop(cmd_rx);
        app.active_tab_mut().restart_book_capture();

        assert_eq!(
            app.active_tab_mut()
                .tape_mut()
                .base_capture_grouping_for_test(),
            original
        );
        assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
    }

    #[test]
    fn full_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app
            .active_tab_mut()
            .tape_mut()
            .base_capture_grouping_for_test();
        let (full_tx, mut full_rx) = mpsc::channel(1);
        app.active_tab_mut().commands = full_tx;
        app.active_tab()
            .commands
            .try_send(FeedCommand::LoadOlder { count: 1 })
            .unwrap();

        assert!(
            app.active_tab_mut()
                .tape_mut()
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        app.active_tab_mut().restart_book_capture();

        assert!(matches!(
            full_rx.try_recv(),
            Ok(FeedCommand::LoadOlder { count: 1 })
        ));
        assert_eq!(
            app.active_tab_mut()
                .tape_mut()
                .base_capture_grouping_for_test(),
            original
        );
        assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
    }

    #[test]
    fn depth_channel_updates_heatmap_without_mutating_candles() {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        let generation = take_capture_start(&mut cmd_rx);
        let bars_before = app.active_tab().flow_pane.state.bars().len();
        book_tx
            .try_send(DepthEvent::Snapshot {
                symbol: "TESTUSDT".to_owned(),
                generation,
                observed_at_ms: 1_100,
                effective_at_ms: 999,
                price_step: None,
                snapshot: BookSnapshot::new(
                    10,
                    vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                    vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                    BookCoverage::Limited {
                        levels_per_side: 1_000,
                    },
                ),
            })
            .unwrap();

        app.active_tab_mut().drain_book_feed();
        app.active_tab_mut().tape_mut().flush_for_test();
        let book = app.active_tab_mut().tape_mut().health();
        assert_eq!(book.bid_levels, 1);
        assert_eq!(book.ask_levels, 1);
        assert_eq!(app.active_tab().flow_pane.state.bars().len(), bars_before);
    }

    #[test]
    fn candle_appearance_change_is_render_only() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        let capture_epoch = app.active_tab().book_capture_epoch;
        let bar_spec = app.active_tab().flow_pane.state.spec().clone();

        app.style.candles = CandlePreset::OutlineOnly.style();
        app.style_revision = app.style_revision.saturating_add(1);
        app.emit_style_changed(Some(CandlePreset::OutlineOnly));

        assert_eq!(app.active_tab().flow_pane.state.spec(), &bar_spec);
        assert!(app.active_tab().tape().enabled());
        assert_eq!(app.active_tab().book_capture_epoch, capture_epoch);
        assert!(
            matches!(cmd_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "appearance changes must not restart or reconfigure market data"
        );
    }

    #[test]
    fn closed_depth_channel_is_reported_once_per_feed_handle() {
        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        drop(book_tx);

        app.active_tab_mut().drain_book_feed();
        assert!(app.active_tab().book_channel_closed_reported);
        app.active_tab_mut().drain_book_feed();
        assert!(
            app.active_tab().book_channel_closed_reported,
            "subsequent frames keep the one-shot diagnostic latched"
        );
    }

    #[test]
    fn replay_keeps_the_recorded_symbol_out_of_the_live_feed_snap() {
        // A recorded instrument no configured live feed offers must survive
        // the toolbar frame untouched — snapping it away would relabel the
        // whole session on the status bar and in the logs. The live path,
        // drawn through the very same frame, must keep snapping an invalid
        // selection back to the feed's list.
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
        let session = quantick_replay::Session::from_text(
            std::path::Path::new("WINJ26_2026-03-16.csv"),
            text,
            quantick_replay::ParseOptions::default(),
        )
        .expect("fixture session parses");
        with_config(&mut app, |tab, config| {
            tab.open_replay(
                config,
                crate::feed::ReplayRequest {
                    session: std::sync::Arc::new(session),
                    options: crate::feed::ReplayOptions {
                        autoplay: false,
                        ..Default::default()
                    },
                },
            )
        });
        assert_eq!(app.active_tab().symbol, "WINJ26");

        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.draw_toolbar(ctx));
        assert_eq!(
            app.active_tab().symbol,
            "WINJ26",
            "a toolbar frame during replay must not relabel the session"
        );

        // The same frame path with the replay closed: validation still works.
        app.active_tab_mut().replay = None;
        app.active_tab_mut().symbol = "NOT-A-SYMBOL".to_owned();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.draw_toolbar(ctx));
        assert_eq!(
            app.active_tab().symbol,
            "TESTUSDT",
            "live selections keep snapping to the feed's symbol list"
        );
    }

    #[test]
    fn fmt_time_in_utc() {
        // Epoch: 1970-01-01 00:00:00 UTC, then +1h 2m 3s.
        assert_eq!(fmt_time(0, TzOffset::new(0)), "00:00:00");
        assert_eq!(fmt_time(3_723_000, TzOffset::new(0)), "01:02:03");
    }

    /// An app with `count` trades of history, split, and laid out by two real
    /// frames so both panes have reported their rects.
    fn split_app(ctx: &egui::Context, count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
        let (mut app, commands) = app_with_history(count);
        run_frame(&mut app, ctx);
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, ctx);
        run_frame(&mut app, ctx);
        (app, commands)
    }

    /// Let every pane's indicator worker finish what it was sent, then apply
    /// its events — the two steps the frame loop takes, made deterministic.
    fn settle_indicators(app: &mut QuantickApp) {
        for pane in app.active_tab_mut().panes_mut() {
            pane.indicator_worker.flush();
            pane.apply_indicator_events();
        }
    }

    /// A point inside the pane on `side`, for a click that focuses it.
    fn pane_point(app: &QuantickApp, side: PaneSide) -> egui::Pos2 {
        app.active_tab()
            .pane(side)
            .last_chart_area
            .expect("the pane reported its rect")
            .center()
    }

    /// (a) The split really puts two charts on the canvas: two panes with
    /// their own laid-out rects, side by side, and a divider between them.
    #[test]
    fn enabling_the_split_lays_out_two_panes_and_a_divider() {
        let ctx = egui::Context::default();
        let (app, _commands) = split_app(&ctx, 200);

        let time = app
            .active_tab()
            .time_pane
            .as_ref()
            .expect("Time + Flow builds the time pane")
            .last_chart_area
            .expect("the time pane was laid out");
        let flow = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("the flow pane was laid out");
        assert!(
            time.right() <= flow.left(),
            "time pane left, flow pane right: {time:?} vs {flow:?}"
        );
        assert!(
            flow.left() - time.right() >= CANVAS_DIVIDER_PX,
            "the divider owns the pixels between them"
        );
        assert!(time.width() > 0.0 && flow.width() > 0.0);
    }

    /// Both panes paint: each keeps its own price axis, and the time pane
    /// carries the timeframe selector §11 gives it.
    #[test]
    fn both_panes_paint_and_the_time_pane_carries_its_own_selector() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "the split canvas still paints a chart: {texts:?}"
        );
        for (label, _) in time_header::PRESETS {
            assert!(
                texts.iter().any(|text| text == label),
                "the time pane's header must offer {label:?}; painted: {texts:?}"
            );
        }
    }

    /// Seeding replays every retained trade, so it is armed on the frame that
    /// asks for the split and done on the next — the overlay gets painted
    /// before the work, exactly as a bar-spec change does.
    #[test]
    fn enabling_the_split_paints_before_it_seeds() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        run_frame(&mut app, &ctx);

        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        assert!(
            app.active_tab().time_pane.is_none(),
            "the frame carrying the change does no work"
        );
        assert!(
            app.active_tab().loading.is_active(LoadingTask::BarRebuild),
            "and arms the overlay that says what is coming — the same one
             `a_rebuilt_chart_still_paints_itself` proves reaches the screen"
        );

        run_frame(&mut app, &ctx);
        let time = app
            .active_tab()
            .time_pane
            .as_ref()
            .expect("the next frame builds it");
        assert!(
            !time.state.trades().is_empty(),
            "seeded from the market the flow pane already holds"
        );
        assert!(
            !app.active_tab().loading.is_active(LoadingTask::BarRebuild),
            "and the overlay comes down with the work"
        );
    }

    /// The forming bar changes with every print and only its latest value is
    /// ever read, so a batch of prints is one update per pane — not one per
    /// print per pane, which the worker then had to collapse again.
    #[test]
    fn a_batch_of_prints_publishes_one_forming_bar_per_pane() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        settle_indicators(&mut app);

        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        app.active_tab_mut().attach_for_test(FeedHandle {
            events: evt_rx,
            book_events: mpsc::channel(8).1,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            commands: cmd_tx,
            replay: None,
        });
        let batch: Vec<_> = (700..710).map(trade).collect();
        let before: Vec<usize> = [PaneSide::Flow, PaneSide::Time]
            .into_iter()
            .map(|side| {
                app.active_tab()
                    .pane(side)
                    .indicator_worker
                    .partial_updates_for_test()
            })
            .collect();
        evt_tx.try_send(FeedEvent::LiveBatch(batch)).unwrap();

        app.active_tab_mut().drain_feed();

        for (side, before) in [PaneSide::Flow, PaneSide::Time].into_iter().zip(before) {
            let sent = app
                .active_tab()
                .pane(side)
                .indicator_worker
                .partial_updates_for_test()
                - before;
            assert_eq!(
                sent, 1,
                "ten prints are one forming-bar update on the {side:?} pane"
            );
        }
    }

    /// (b) One tape, two panes: the same trades reach both `ChartState`s, and
    /// each cuts them by its own spec — which is the whole point of the split.
    #[test]
    fn one_tape_feeds_both_panes_and_each_cuts_it_its_own_way() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let time = app.active_tab().time_pane.as_ref().expect("time pane");
        assert_eq!(
            time.state.trades().len(),
            app.active_tab().flow_pane.state.trades().len(),
            "both panes hold the same tape"
        );
        assert_ne!(
            time.state.bars().len(),
            app.active_tab().flow_pane.state.bars().len(),
            "tick(1) and M1 cannot agree on a bar count over the same trades"
        );

        // And a live trade after the split reaches both of them.
        let flow_before = app.active_tab().flow_pane.state.trades().len();
        let time_before = app
            .active_tab()
            .time_pane
            .as_ref()
            .expect("time pane")
            .state
            .trades()
            .len();
        let trade = trade(500);
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms);
        assert_eq!(
            app.active_tab().flow_pane.state.trades().len(),
            flow_before + 1
        );
        assert_eq!(
            app.active_tab()
                .time_pane
                .as_ref()
                .expect("time pane")
                .state
                .trades()
                .len(),
            time_before + 1,
            "a pane off the drain path would silently fall behind the market"
        );
    }

    /// Seeding a pane opened mid-session must not relabel live prints as
    /// history: the backfill divider is a data-honesty mark, not a decoration.
    #[test]
    fn a_pane_opened_late_keeps_the_backfill_boundary_honest() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(100);
        // Everything so far is backfill; now three prints arrive live.
        for id in 101..=103 {
            let trade = trade(id);
            app.active_tab_mut()
                .ingest_live_trade_at(&trade, trade.timestamp_ms);
        }
        run_frame(&mut app, &ctx);
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        // The seed runs on the frame after the click; see
        // `enabling_the_split_paints_before_it_seeds`.
        run_frame(&mut app, &ctx);

        let time = app.active_tab().time_pane.as_ref().expect("time pane");
        assert_eq!(
            time.state.trades().len(),
            103,
            "the new pane opens showing the market, not an empty chart"
        );
        assert_eq!(
            time.state.backfill_trade_count(),
            app.active_tab().flow_pane.state.backfill_trade_count(),
            "the live prints must not become history in the second view"
        );
    }

    /// (c) The time pane's header governs the time pane and nothing else; the
    /// toolbar's BARS group keeps governing the flow pane (§11).
    #[test]
    fn a_timeframe_chip_moves_only_the_time_panes_spec() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let flow_spec = app.active_tab().flow_pane.state.spec().clone();
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).state.spec(),
            &BarSpec::Time(time_header::DEFAULT_INTERVAL_MS),
            "the time pane opens on M1, not on the flow selector's interval"
        );

        // The 15m chip, clicked where it was actually drawn.
        let (label, expected_ms) = time_header::PRESETS[2];
        let chip = app.active_tab().time_header_chip(2).expect("the 15m chip");
        assert!(chip.is_positive(), "the {label} chip was laid out");
        click_chart(&mut app, &ctx, chip.center());
        // The spec change is deferred one frame, exactly as the toolbar's is.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).state.spec(),
            &BarSpec::Time(expected_ms),
            "clicking {label} must re-cut the time pane"
        );
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &flow_spec,
            "and must leave the chart beside it alone"
        );
    }

    /// (d) `Insert → Indicator` targets the focused pane (§11): the slot lands
    /// on the pane the user is working in, and only there.
    #[test]
    fn an_indicator_lands_on_the_focused_pane_only() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let flow_before = app.active_tab().flow_pane.indicators.all().len();

        let point = pane_point(&app, PaneSide::Time);

        click_chart(&mut app, &ctx, point);
        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Time,
            "clicking a pane focuses it"
        );

        app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
        settle_indicators(&mut app);

        let time = app.active_tab().time_pane.as_ref().expect("time pane");
        assert_eq!(
            time.indicators.all().len(),
            1,
            "the EMA belongs to the pane that had focus"
        );
        assert!(
            time.indicators
                .all()
                .iter()
                .any(|view| view.label().contains("EMA")),
            "and it really built: {:?}",
            time.indicators
                .all()
                .iter()
                .map(|view| view.label().to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            app.active_tab().flow_pane.indicators.all().len(),
            flow_before,
            "the pane beside it gains nothing"
        );
        // The persisted set is the flow pane's; a time-pane slot must not
        // enter it (see maintain_indicator_state).
        assert_eq!(app.slot_kinds.len(), 1, "exactly one slot was registered");
        assert!(
            app.slot_kinds
                .iter()
                .all(|(owner, _)| owner.side == PaneSide::Time),
            "and it is the time pane's"
        );
    }

    /// Slot ids are per pane, so the same number means different indicators on
    /// the two of them. Removing one must not unregister the other.
    #[test]
    fn removing_a_slot_on_one_pane_leaves_the_same_number_on_the_other() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let point = pane_point(&app, PaneSide::Flow);

        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
        settle_indicators(&mut app);
        assert_eq!(app.slot_kinds.len(), 2);
        let time_slot = app
            .active_tab()
            .time_pane
            .as_ref()
            .expect("time pane")
            .indicators
            .all()[0]
            .slot;

        // Focused on the time pane: remove its slot.
        app.apply_toolbar_action(ToolbarAction::RemoveIndicator(time_slot.0));

        assert_eq!(
            app.slot_kinds.len(),
            1,
            "exactly one registration went with it"
        );
        assert_eq!(
            app.slot_kinds[0].0.side,
            PaneSide::Flow,
            "and the survivor is the flow pane's"
        );
        assert_eq!(app.active_tab().flow_pane.indicators.all().len(), 1);
    }

    /// §11: flow layers stay on the flow pane. A time pane must not run a book
    /// worker, draw a lane, or claim strip pixels.
    #[test]
    fn the_time_pane_has_no_tape_and_no_flow_layers() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        app.apply_toolbar_action(ToolbarAction::SetLiveStrip(true));
        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        app.apply_toolbar_action(ToolbarAction::SetBubbles(true));
        run_frame(&mut app, &ctx);

        let time = app.active_tab().time_pane.as_ref().expect("time pane");
        assert!(
            time.orderflow.is_none(),
            "no tape means no book worker behind it"
        );
        assert_eq!(
            time.live_strip_width(),
            0.0,
            "the strip is a flow layer and claims no pixels here"
        );
        assert!(
            time.last_lane_divider_x.is_none(),
            "and there is no live lane to divide"
        );
        // The toggles still reached the flow pane, which is what owns them.
        assert!(app.active_tab().tape().depth_visible());
        assert!(app.active_tab().tape().bubbles_enabled());
        assert!(app.active_tab().flow_pane.live_strip_visible);
    }

    /// (e) Dragging the divider moves it, and stops at the quarter §11
    /// promises each pane.
    #[test]
    fn dragging_the_divider_resizes_the_panes_and_stops_at_the_minimum() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let flow_before = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("laid out")
            .width();
        let divider = app
            .active_tab()
            .canvas_divider_rect()
            .expect("the divider was registered");
        let grab = divider.center();

        drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 120.0, grab.y));
        run_frame(&mut app, &ctx);
        assert!(
            app.active_tab().split_fraction > DEFAULT_PANE_FRACTION,
            "dragging right widens the time pane, got {}",
            app.active_tab().split_fraction
        );
        assert!(
            app.active_tab()
                .flow_pane
                .last_chart_area
                .expect("laid out")
                .width()
                < flow_before,
            "at the flow pane's expense"
        );

        // Now shove it far past the minimum: it stops, it does not collapse.
        for _ in 0..6 {
            let grab = app
                .active_tab()
                .canvas_divider_rect()
                .expect("registered")
                .center();
            drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 400.0, grab.y));
            run_frame(&mut app, &ctx);
        }
        assert!(
            (app.active_tab().split_fraction - (1.0 - MIN_PANE_FRACTION)).abs() < 1e-3,
            "the flow pane keeps its quarter, got {}",
            app.active_tab().split_fraction
        );
        assert!(
            app.active_tab()
                .flow_pane
                .last_chart_area
                .expect("laid out")
                .width()
                > 0.0
        );
    }

    /// The keyboard's drawing grammar follows focus as well: Delete removes
    /// the selection on the pane the user is in, never its opposite number on
    /// the chart beside it.
    #[test]
    fn the_keyboard_deletes_from_the_focused_pane() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        for side in [PaneSide::Flow, PaneSide::Time] {
            app.toolrail
                .arm(Tool::Drawing(drawing_tool("horizontal-line")));
            let point = pane_point(&app, side);
            click_chart(&mut app, &ctx, point);
        }
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1
        );
        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Time,
            "the last click was on the time pane"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);

        assert!(
            app.active_tab()
                .pane(PaneSide::Time)
                .drawings
                .items()
                .is_empty(),
            "Delete removes the focused pane's selection"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            1,
            "and leaves the pane beside it untouched"
        );
    }

    /// The status bar's content section speaks for the focused pane (§11), so
    /// it always describes the chart the user is working in.
    #[test]
    fn the_status_bar_follows_the_focused_pane() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let point = pane_point(&app, PaneSide::Flow);

        click_chart(&mut app, &ctx, point);
        let flow_status = app.status_model();
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        let time_status = app.status_model();

        assert_eq!(
            flow_status.spec_summary,
            app.active_tab().flow_pane.state.spec().summary()
        );
        assert_eq!(
            time_status.spec_summary,
            app.active_tab().pane(PaneSide::Time).state.spec().summary()
        );
        assert_ne!(
            flow_status.spec_summary, time_status.spec_summary,
            "the two panes report different specs, so the bar has to change"
        );
        // Provenance is the market's and never moves with focus.
        assert_eq!(flow_status.symbol, time_status.symbol);
        assert_eq!(flow_status.venue, time_status.venue);
    }

    /// Drawings are per pane, and the tool rail is one: an object lands on the
    /// pane under the cursor and stays out of the other's overlay.
    #[test]
    fn a_drawing_lands_on_the_pane_under_the_cursor() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1,
            "the click landed on the time pane"
        );
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "and nowhere else"
        );

        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Flow);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1,
            "placing on one pane must not add to the other"
        );
    }

    /// Going back to Single hides the context chart; it must not throw away
    /// what the user built on it, and it must keep following the market.
    #[test]
    fn leaving_the_split_keeps_the_time_panes_work_and_its_bars() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1
        );

        app.active_tab_mut().set_layout(CanvasLayout::Single);
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Flow,
            "a single canvas is the flow pane, whatever had focus"
        );

        let before = app.active_tab().pane(PaneSide::Time).state.trades().len();
        let trade = trade(700);
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms);
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).state.trades().len(),
            before + 1,
            "a hidden pane keeps draining, so showing it again never catches up"
        );

        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1,
            "and its drawings survived the round trip"
        );
    }

    /// The default layout is the one quantick opens on, and the split must not
    /// have quietly changed it: one pane, the whole canvas, no second worker.
    #[test]
    fn the_single_layout_still_gives_the_flow_pane_the_whole_canvas() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        run_frame(&mut app, &ctx);

        assert_eq!(app.active_tab().layout, CanvasLayout::Single);
        assert!(
            app.active_tab().time_pane.is_none(),
            "an unsplit canvas builds no second pane, and no worker behind it"
        );
        let chart = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("laid out");
        // The canvas the pane was given, reconstructed from the rect it kept:
        // wider than half the window, so nothing was carved off for a divider.
        assert!(
            chart.width() > 600.0,
            "the flow pane still owns the canvas, got {chart:?}"
        );
        assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
    }

    // ---- workspace tabs (§11) ----

    /// The ends of a tab's feed, kept alive by the test so its channels stay
    /// open exactly as a running feed thread would keep them.
    struct TabEnds {
        events: mpsc::Sender<FeedEvent>,
        #[expect(dead_code, reason = "held open so the tab's channel is not closed")]
        book: mpsc::Sender<DepthEvent>,
        #[expect(dead_code, reason = "held open so the tab's channel is not closed")]
        commands: mpsc::Receiver<FeedCommand>,
    }

    /// Open a second market the way the `+` does, minus the real feed spawn:
    /// the same bookkeeping runs, over channels the test drives.
    fn open_second_tab(app: &mut QuantickApp, ctx: &egui::Context, symbol: &str) -> TabEnds {
        app.apply_tab_action(TabAction::New);
        assert!(app.source_picker.is_some(), "the + opens the picker");
        app.source_picker = None;

        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        app.adopt_tab(
            "binance".to_owned(),
            symbol.to_owned(),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );
        run_frame(app, ctx);
        TabEnds {
            events: evt_tx,
            book: book_tx,
            commands: cmd_rx,
        }
    }

    /// The other half of the same root cause: an edit committed against
    /// whatever had focus when the gesture settled, not against the pane it
    /// was captured on. Focus moves legitimately — clicking the other chart —
    /// so the baseline has to carry its own owner.
    #[test]
    fn an_inspector_edit_commits_on_the_pane_it_started_on() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        // A mark on each pane, so an index means something on both.
        for side in [PaneSide::Time, PaneSide::Flow] {
            app.toolrail
                .arm(Tool::Drawing(drawing_tool("horizontal-line")));
            let point = pane_point(&app, side);
            click_chart(&mut app, &ctx, point);
        }
        let time_depth = app.active_tab().pane(PaneSide::Time).drawings.undo_depth();
        let flow_depth = app.active_tab().flow_pane.drawings.undo_depth();

        // An edit begun on the time pane: the baseline, then a real change to
        // the object (the store records an entry only if something moved).
        let before = app.active_tab().pane(PaneSide::Time).drawings.items()[0].clone();
        app.inspector_edit_baseline = Some(InspectorEdit {
            tab: app.active_tab().id,
            side: PaneSide::Time,
            index: 0,
            before,
        });
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .drawings
            .select(Some(0));
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .drawings
            .selected_mut()
            .expect("the time pane's mark")
            .style
            .width_px = MAX_DRAWING_WIDTH_PX;
        // ...that settles after focus has moved to the chart beside it.
        let point = pane_point(&app, PaneSide::Flow);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
        app.commit_inspector_gesture();

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.undo_depth(),
            time_depth + 1,
            "the entry lands on the pane the edit started on"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.undo_depth(),
            flow_depth,
            "and never on the one that happened to take focus"
        );
    }

    /// A press egui routed to a floating window is not a click on the pane
    /// underneath it.
    ///
    /// The toast, the object manager and the inspector all float over the
    /// canvas. Taking their presses as pane clicks made the focused pane
    /// follow whichever pane the *window* happened to overlap, so the toast's
    /// Undo — which acts on the focused pane — undid an edit on the other
    /// chart, and the manager's list flipped under the click that opened it.
    #[test]
    fn a_press_on_a_floating_window_does_not_move_pane_focus() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        // A mark on the time pane, then delete it: the toast comes up with an
        // Undo that acts on whatever pane has focus.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Time);
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(
            app.active_tab()
                .pane(PaneSide::Time)
                .drawings
                .items()
                .is_empty()
        );
        // A fresh egui Area sizes itself on its first frame.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let undo = app.toast_undo_rect.expect("the toast offers Undo");
        let divider = app
            .active_tab()
            .canvas_divider_rect()
            .expect("the split is on");
        assert!(
            undo.center().x > divider.right(),
            "the regression needs the toast's button to float over the *other* pane"
        );

        click_chart(&mut app, &ctx, undo.center());

        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Time,
            "a press routed to the toast is not a click on the pane behind it"
        );
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).drawings.items().len(),
            1,
            "so Undo puts back the mark it was offered for"
        );
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "and touches nothing on the pane the button happens to float over"
        );
    }

    /// §11's amber dot: a background tab says something is wrong with its
    /// feed without the user having to open it.
    ///
    /// It marks trouble, not activity — a tab still connecting has nothing to
    /// report yet, and a recording has no transport to lose.
    #[test]
    fn the_attention_dot_marks_lost_connections_and_nothing_else() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let tab = app.active_tab_mut();

        assert_eq!(tab.feed_connection, FeedConnectionState::Connecting);
        assert!(
            !tab.needs_attention(),
            "still connecting is not yet trouble"
        );

        tab.feed_connection = FeedConnectionState::Connected;
        assert!(!tab.needs_attention(), "nor is a healthy feed");

        tab.feed_connection = FeedConnectionState::Reconnecting;
        assert!(
            tab.needs_attention(),
            "a feed that had a connection and lost it is"
        );

        tab.feed_connection = FeedConnectionState::Connected;
        tab.notice = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        assert!(
            tab.needs_attention(),
            "so is one asking the user to fix something"
        );

        // A recording has no transport to lose, whatever it is holding.
        let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
        let session = quantick_replay::Session::from_text(
            std::path::Path::new("WINJ26_2026-03-16.csv"),
            text,
            quantick_replay::ParseOptions::default(),
        )
        .expect("fixture session parses");
        with_config(&mut app, |tab, config| {
            tab.open_replay(
                config,
                crate::feed::ReplayRequest {
                    session: std::sync::Arc::new(session),
                    options: crate::feed::ReplayOptions {
                        autoplay: false,
                        ..Default::default()
                    },
                },
            )
        });
        let tab = app.active_tab_mut();
        tab.feed_connection = FeedConnectionState::Reconnecting;
        assert!(
            !tab.needs_attention(),
            "a replaying tab has no transport to report on"
        );
        run_frame(&mut app, &ctx);
    }

    /// (a) The `+` opens the picker, and choosing a market adds a tab that
    /// becomes the active one.
    #[test]
    fn the_plus_opens_a_picker_and_its_choice_becomes_the_active_tab() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(100);
        assert_eq!(app.tabs.len(), 1, "quantick opens on one tab");
        assert_eq!(app.active_tab().symbol, "TESTUSDT");

        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");

        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1, "the new tab is the one you land on");
        assert_eq!(app.active_tab().symbol, "ETHUSDT");
        assert_ne!(
            app.tabs[0].id, app.tabs[1].id,
            "ids are handed out, never reused"
        );
        // Pane ids namespace egui state; two tabs sharing one would share a
        // drag the moment both had been on screen.
        assert_ne!(app.tabs[0].flow_pane.id, app.tabs[1].flow_pane.id);
        // The picker's choice is honoured, not the config default.
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("ETHUSDT")),
            "the strip names the market it opened; painted: {texts:?}"
        );
    }

    /// (b) A tab is a whole workspace: switching away and back finds its bars,
    /// its viewport, its focus and its drawings exactly as they were.
    #[test]
    fn switching_tabs_preserves_everything_each_one_owns() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(200);

        // Give tab 0 a distinctive state: a drawing, a panned viewport, and
        // the split open with the time pane focused.
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        // Clicking the time pane focuses it and lands the mark there — the
        // real gesture, not a poked field.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = app
            .active_tab()
            .pane(PaneSide::Time)
            .last_chart_area
            .expect("the time pane was laid out")
            .center();
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Time);
        let slots = app.active_tab().flow_pane.slots();
        app.active_tab_mut()
            .flow_pane
            .viewport
            .pan_pixels(120.0, slots);
        let first_bars = app.active_tab().flow_pane.state.bars().len();
        let first_edge = app.active_tab().flow_pane.viewport.right_edge_bar(slots);
        let first_drawings = app.active_tab().focused_pane().drawings.items().len();
        assert_eq!(first_drawings, 1, "the drawing landed on the focused pane");

        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        assert_eq!(
            app.active_tab().layout,
            CanvasLayout::Single,
            "a new tab opens on the default layout, not the previous tab's"
        );
        assert!(app.active_tab().flow_pane.drawings.items().is_empty());

        app.apply_tab_action(TabAction::Activate(0));
        run_frame(&mut app, &ctx);
        assert_eq!(app.active_tab().flow_pane.state.bars().len(), first_bars);
        assert_eq!(
            app.active_tab()
                .flow_pane
                .viewport
                .right_edge_bar(app.active_tab().flow_pane.slots()),
            first_edge,
            "the viewport came back where it was left"
        );
        assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Time);
        assert_eq!(
            app.active_tab().focused_pane().drawings.items().len(),
            first_drawings,
            "and its marks with it"
        );
    }

    /// (c) §11: switching never tears a feed down. A background tab keeps
    /// draining — its channels are bounded, and one left full backs its feed
    /// thread up until the market it shows is hours behind.
    #[test]
    fn a_background_tab_keeps_ingesting() {
        let ctx = egui::Context::default();
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        // Leave the new market in the background.
        app.apply_tab_action(TabAction::Activate(0));
        assert_eq!(app.active_tab, 0);

        let before = app.tabs[1].flow_pane.state.trades().len();
        // Push into the background tab's own channel, then run the window's
        // drain — not that tab's, which would prove nothing about the loop.
        for id in 900..905 {
            ends.events.try_send(FeedEvent::Live(trade(id))).unwrap();
        }
        app.drain_tabs();

        assert_eq!(
            app.tabs[1].flow_pane.state.trades().len(),
            before + 5,
            "a tab off screen still takes in what its feed sent"
        );
        assert_eq!(
            app.trades_since_summary, 5,
            "and the window counts them as its own ingest"
        );
    }

    /// (d) Closing the active tab activates a neighbour and takes the market
    /// with it. The last tab has no × to click.
    #[test]
    fn closing_a_tab_activates_a_neighbour_and_drops_its_market() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let first = app.active_tab().id;
        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        let second = app.active_tab().id;
        // Register a slot on the tab about to close, so the bookkeeping has
        // something to lose with it.
        app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
        assert!(app.slot_kinds.iter().any(|(owner, _)| owner.tab == second));

        app.apply_tab_action(TabAction::Close(1));

        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_tab().id, first, "a neighbour takes over");
        assert!(
            !app.slot_kinds.iter().any(|(owner, _)| owner.tab == second),
            "its indicator bookkeeping went with it"
        );
        // The last tab stays: a window with no market has nothing to draw.
        app.apply_tab_action(TabAction::Close(0));
        assert_eq!(app.tabs.len(), 1, "the last tab is not closable");
        run_frame(&mut app, &ctx);
    }

    /// The workers a closed tab owned end with it: their run loops exit when
    /// the command channels they hold disconnect, so dropping the tab is the
    /// whole shutdown protocol.
    #[test]
    fn closing_a_tab_ends_its_worker_threads() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        // A flush proves the worker is alive and answering right now.
        app.active_tab_mut().flow_pane.indicator_worker.flush();
        let doomed = app.tabs.pop().expect("the second tab");
        let worker = doomed.flow_pane.indicator_worker;
        drop(doomed.flow_pane.orderflow);
        app.active_tab = 0;

        // Dropping the handle disconnects the command channel; the run loop's
        // `recv` then fails and the thread returns. A send after that is
        // refused rather than queued into a thread nobody will ever join.
        drop(worker);
        // The window is still whole, and the surviving tab still draws.
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "the surviving tab keeps drawing: {texts:?}"
        );
    }

    /// (e) The SOURCE group writes into the active tab only — switching a
    /// market must not relabel the tab beside it.
    #[test]
    fn the_source_combo_changes_only_the_active_tab() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        let untouched = app.tabs[0].symbol.clone();

        // What the combo does: write the selection, then let the frame switch.
        app.active_tab_mut().symbol = "TESTUSDT".to_owned();
        let (tab, config) = app.active_with_config();
        tab.maybe_switch_feed(config);

        assert_eq!(app.active_tab().symbol, "TESTUSDT");
        assert_eq!(app.active_tab().active.1, "TESTUSDT", "its feed followed");
        assert_eq!(
            app.tabs[0].symbol, untouched,
            "the other tab kept its market"
        );
        assert_eq!(app.tabs[0].active.1, untouched);
    }

    /// (f) The transport speaks for one tab at a time (§11). A recording in a
    /// background tab keeps its own clock but claims none of the chrome.
    #[test]
    fn the_transport_shows_only_while_the_active_tab_replays() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
        let session = quantick_replay::Session::from_text(
            std::path::Path::new("WINJ26_2026-03-16.csv"),
            text,
            quantick_replay::ParseOptions::default(),
        )
        .expect("fixture session parses");
        with_config(&mut app, |tab, config| {
            tab.open_replay(
                config,
                crate::feed::ReplayRequest {
                    session: std::sync::Arc::new(session),
                    options: crate::feed::ReplayOptions {
                        autoplay: false,
                        ..Default::default()
                    },
                },
            )
        });
        assert!(app.active_tab().replay.is_some());
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("WINJ26")),
            "the active tab's session is named on screen: {texts:?}"
        );

        // Open a live tab beside it and switch: the recording keeps playing in
        // its own tab, but the transport belongs to whoever is on screen.
        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        assert!(
            app.tabs[0].replay.is_some(),
            "the background tab is still the one holding the recording"
        );
        assert!(
            app.active_tab().replay.is_none(),
            "and the active tab is streaming"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            !texts.iter().any(|text| text.contains("Speed")),
            "no transport for a tab that is not on screen: {texts:?}"
        );
    }

    /// (g) `Ctrl+Tab` / `Ctrl+Shift+Tab` walk the strip and wrap (§10).
    #[test]
    fn the_cycle_shortcuts_walk_the_strip_and_wrap() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        let _ends = open_second_tab(&mut app, &ctx, "TESTUSDT");
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.active_tab, 2);

        app.cycle_tab(1);
        assert_eq!(
            app.active_tab, 0,
            "forward from the last wraps to the first"
        );
        app.cycle_tab(-1);
        assert_eq!(app.active_tab, 2, "and back again");
        app.cycle_tab(-1);
        assert_eq!(app.active_tab, 1);

        // Through the real key path, so the shortcut itself is covered.
        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Tab, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.active_tab, 2, "Ctrl+Tab moves forward one");
    }

    /// Ctrl+W closes, Ctrl+T opens the picker — and neither collides with a
    /// binding the chart already had.
    #[test]
    fn the_tab_shortcuts_open_and_close() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        assert_eq!(app.tabs.len(), 2);

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::W, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.tabs.len(), 1, "Ctrl+W closes the active tab");

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::T, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
        );
        assert!(app.source_picker.is_some(), "Ctrl+T opens the picker");
    }

    /// Provenance follows the active tab (§11): the status bar names the
    /// market on screen, not the one that happens to be first.
    #[test]
    fn the_status_bar_follows_the_active_tab() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        assert_eq!(app.status_model().symbol, "TESTUSDT");

        let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        assert_eq!(app.status_model().symbol, "ETHUSDT");

        app.apply_tab_action(TabAction::Activate(0));
        assert_eq!(app.status_model().symbol, "TESTUSDT");
    }

    /// Two tabs on one market are allowed — two views of one book is a
    /// legitimate thing to want — and each still gets its own everything.
    #[test]
    fn the_same_market_can_be_open_twice() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let _ends = open_second_tab(&mut app, &ctx, "TESTUSDT");

        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].symbol, app.tabs[1].symbol);
        assert_ne!(app.tabs[0].id, app.tabs[1].id);
        assert_ne!(app.tabs[0].flow_pane.id, app.tabs[1].flow_pane.id);
        // Separate engines: what one holds says nothing about the other.
        assert!(!app.tabs[0].flow_pane.state.bars().is_empty());
        assert!(app.tabs[1].flow_pane.state.bars().is_empty());
    }

    #[test]
    fn fmt_time_applies_the_offset() {
        // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
        assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
        // UTC midnight in UTC+05:30 is 05:30.
        assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
    }
}
