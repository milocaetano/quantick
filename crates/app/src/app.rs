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
use egui_phosphor::regular as icons;

use crate::candle_view::draw_style_window;
use crate::chart::PriceScale;
use crate::chart_layers::{self, ChartLayer};
use crate::config::AppConfig;
use crate::dock::{Dock, DockEnv, DockTab};
use crate::drawings::{
    self, DeleteOutcome, MAX_DRAWING_FILL_ALPHA, MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX,
    PresetHost as _,
};
use crate::feed::{self, FeedCommand, FeedHandle};
use crate::indicator_legend;
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::library::ScriptLibrary;
use crate::indicators::state_file::{self, SavedIndicator, SavedInput, SavedKind};
use crate::loading::{self, LoadingTask};
use crate::metrics::{self, FrameStats};
use crate::notice_card;
use crate::pane::{self, ChartPane, DRAWING_ANCHOR_RADIUS_PX, PaneSide};
use crate::paper_trading::PaperTrading;
use crate::replay_view::{ReplayAction, ReplayView};
use crate::state::BarSpec;
use crate::statusbar;
use crate::style::{CandlePreset, ChartStyle};
use crate::symbols_file::{self, AddedSymbols};
use crate::tab::{CanvasChrome, CanvasLayout, Tab};
use crate::tabstrip::{self, PickerOutcome, SourcePicker, TabAction};
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolbar::{self, ToolbarAction};
use crate::toolrail::{Tool, ToolRail, ToolboxDock};
use crate::ui_state;
use crate::widgets::{IconButton, TOOLBAR_ICON};

/// Width of the right-hand price-axis gutter, in pixels (§5 zone 9).
const AXIS_GUTTER: f32 = 64.0;
/// Height of the bottom time-axis strip, in pixels (§5 zone 6).
const TIME_STRIP: f32 = 24.0;
/// Id of the tab the window opens with.
const FIRST_TAB_ID: u64 = 0;
/// How far the indicator legend drops below the position HUD when both
/// claim the chart's top-left corner.
const LEGEND_BELOW_HUD_OFFSET_PX: f32 = 64.0;

/// The (flow, time) pane ids for tab `id`.
///
/// Pane ids namespace every egui interaction a pane registers, so they have to
/// be unique across the whole window, not just within a tab — two tabs sharing
/// them would share a drag the moment both had been on screen.
const fn pane_ids(tab: u64) -> (u64, u64) {
    (tab * 2, tab * 2 + 1)
}
/// How much of the newest chart the `QUANTICK_DRAWINGS_DEMO` hook spreads its
/// objects across. Close to what a default viewport shows, so every object
/// lands on screen — a demo the camera cannot see proves nothing.
const DEMO_VISIBLE_SLOTS: usize = 90;
/// How many demo objects wide the visible window is — the reciprocal of how
/// far a multi-anchor object reaches. Four keeps a rectangle big enough to
/// read while still leaving the tools distinguishable from each other.
const DEMO_SPANS_PER_WINDOW: usize = 4;
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
/// Below this chart width a fresh selection opens the inspector pinned —
/// there is no floating position that would not crowd the geometry. Stops
/// applying once the user touches the pin either way.
const INSPECTOR_AUTO_PIN_CHART_WIDTH_PX: f32 = 1180.0;
/// Height of the floating inspector's custom title bar.
const INSPECTOR_TITLE_HEIGHT_PX: f32 = 28.0;
/// Title-bar paint metrics: leading padding, the title column when the grip
/// glyph precedes it, and the two font sizes.
const INSPECTOR_TITLE_PAD_X_PX: f32 = 2.0;
const INSPECTOR_TITLE_TEXT_X_PX: f32 = 18.0;
const INSPECTOR_TITLE_GRIP_GLYPH_PX: f32 = 14.0;
const INSPECTOR_TITLE_TEXT_PX: f32 = 13.0;
/// Gap between the object manager and the rail edge it opens beside.
const DRAWING_MANAGER_GAP_PX: f32 = 12.0;
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
/// Width of the Save-as box, in pixels. Wide enough that a name at the
/// [`ui_state::MAX_WORKSPACE_NAME`] limit reads in one line.
const WORKSPACE_NAME_BOX_WIDTH_PX: f32 = 280.0;

/// Transient confirmation that the window did what was asked, with an escape
/// hatch when the act has one. Undo works from the button for
/// [`TOAST_UNDO_MS`] and from Ctrl+Z for as long as the history holds.
///
/// This is the window's one acknowledgement channel, not the drawings'. It
/// floats over the chart's bottom edge instead of taking a cell on the status
/// line, and that is the reason: the status bar's readings live at fixed
/// positions Rafa's eye returns to without looking, and a cell that appears
/// for eight seconds and then leaves would slide `bars` and `arrival`
/// sideways twice per acknowledgement (`statusbar.rs`: "the layout never
/// moves").
#[derive(Debug)]
struct Toast {
    /// Borrowed for the fixed messages, owned when the act has a count to
    /// report. Acknowledgements are event-driven and rare — never a frame
    /// path — so an allocation here costs nothing anyone can see.
    message: std::borrow::Cow<'static, str>,
    shown_at: Instant,
    /// Whether the toast offers Undo. A delete does; the honest clear after
    /// a bar rebuild does not — its history is gone with the drawings, and
    /// a dead Undo button would lie. Neither does a workspace save: the file
    /// it replaced is gone, and `Reset startup layout` is the real way back.
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
/// Which default-style button the Style tab was pressed on, so the caller can
/// say out loud that something was remembered — a silent save leaves the
/// trader wondering whether it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SavedDefault {
    OneTool,
    EveryTool,
    Forgotten,
}

impl SavedDefault {
    const fn message(self) -> &'static str {
        match self {
            Self::OneTool => "Saved - new drawings of this tool open with this look.",
            Self::EveryTool => "Saved - every new drawing opens with this look.",
            Self::Forgotten => "Forgotten - this tool goes back to the built-in look.",
        }
    }
}

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
    /// Which default-style button was pressed, if any.
    saved_default: Option<SavedDefault>,
}

impl InspectorActions {
    /// Fold another frame section's requests into this one — the title bar
    /// and the body each report intent, the host applies the union.
    fn merge(&mut self, other: Self) {
        self.toggle_hidden |= other.toggle_hidden;
        self.toggle_lock |= other.toggle_lock;
        self.toggle_pin |= other.toggle_pin;
        self.delete |= other.delete;
        self.cancel_delete |= other.cancel_delete;
        self.force_delete |= other.force_delete;
        self.close |= other.close;
        self.edited |= other.edited;
        self.saved_default = self.saved_default.or(other.saved_default);
    }
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
/// `pane_sizing` carries one entry per *visible* pane indicator, top to
/// bottom: the band they claim is carved here, once, rather than by each
/// caller — a chart rect that two call sites disagree about is two price
/// scales for the same pixels. Sizing rather than a count, because how tall a
/// pane is and whether it has room to be drawn at all is the same decision.
pub fn plot_split(
    area: egui::Rect,
    live_strip_width: f32,
    pane_sizing: &[crate::indicators::PaneSizing],
) -> PlotAreas {
    let plot = area.shrink(16.0);
    let strip_width = live_strip_width.max(0.0);
    let gutter_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_x = (gutter_x - strip_width).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    let body = egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y));
    let (chart, indicator_panes) = crate::indicators::split_panes(body, pane_sizing);
    // The gutter is banded exactly like the body it labels: the candles' price
    // scale owns the height of the candles and not a pixel more, so a drag
    // over a pane's numbers can only ever move that pane.
    let band = |top: f32, bottom: f32| {
        egui::Rect::from_min_max(egui::pos2(gutter_x, top), egui::pos2(plot.right(), bottom))
    };
    let pane_gutters = indicator_panes
        .iter()
        .map(|pane| band(pane.rect.top(), pane.rect.bottom()))
        .collect();
    PlotAreas {
        chart,
        indicator_panes,
        pane_gutters,
        live_strip: (strip_width > 0.0).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(split_x, plot.top()),
                egui::pos2(gutter_x, split_y),
            )
        }),
        price_gutter: band(plot.top(), chart.bottom()),
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

/// Clamp a window of `size` at `position` into `chart`, top-left biased when
/// the window is larger than the pane.
fn clamp_into_chart(position: egui::Pos2, size: egui::Vec2, chart: egui::Rect) -> egui::Pos2 {
    let max_x = (chart.right() - size.x).max(chart.left());
    let max_y = (chart.bottom() - size.y).max(chart.top());
    egui::pos2(
        position.x.clamp(chart.left(), max_x),
        position.y.clamp(chart.top(), max_y),
    )
}

/// The inspector placement rule (`docs/ux/drawing-tools-2026-08.md` §D3).
///
/// The old rule scored least overlap with the object's *bounding box*, and a
/// small object has a small box: "beside it with a 12 px gap" scored zero and
/// won, dropping the panel straight onto the price action the trader drew the
/// line to read. The read is the neighbourhood of the object, not its box.
///
/// So the corners come first, and the winner is the **farthest** clear one:
///
/// 1. the two **top** corners first, inset by the gap. Top before bottom is
///    structural, not taste: a panel is positioned by its top-left and grows
///    downwards, so a top corner always has the whole pane to grow into. A
///    bottom-anchored panel that turns out taller than the placement assumed
///    runs off the window and loses its last rows — and rows a trader cannot
///    reach read as rows that do not exist;
/// 2. of the two, the one that clears `bbox` and whose centre is farthest
///    from the object wins, so the panel walks away from the drawing across
///    the chart. An exact tie (a centred object) takes the left one, so the
///    panel appears in the same place every time;
/// 3. only if neither top corner is free, the bottom two on the same rule;
/// 4. and if every corner is fouled — a large object covering the chart —
///    the beside-the-object candidates, least overlap first.
fn inspector_placement(chart: egui::Rect, bbox: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
    let gap = INSPECTOR_OBJECT_GAP_PX;
    let top_corners = [
        egui::pos2(chart.left() + gap, chart.top() + gap),
        egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
    ];
    let bottom_corners = [
        egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
        egui::pos2(chart.right() - gap - size.x, chart.bottom() - gap - size.y),
    ];
    let farthest_clear = |candidates: [egui::Pos2; 2]| {
        let mut best: Option<(egui::Pos2, f32)> = None;
        for candidate in candidates {
            let position = clamp_into_chart(candidate, size, chart);
            let rect = egui::Rect::from_min_size(position, size);
            if rect.intersect(bbox).is_positive() {
                continue;
            }
            let distance = rect.center().distance(bbox.center());
            if best.is_none_or(|(_, best)| distance > best) {
                best = Some((position, distance));
            }
        }
        best.map(|(position, _)| position)
    };
    if let Some(position) = farthest_clear(top_corners).or_else(|| farthest_clear(bottom_corners)) {
        return position;
    }
    let corners = [top_corners, bottom_corners].concat();

    // Nothing clear anywhere: crowd the object as little as possible.
    let fallbacks = [
        egui::pos2(bbox.right() + gap, bbox.top()),
        egui::pos2(bbox.left() - gap - size.x, bbox.top()),
        egui::pos2(bbox.left(), bbox.bottom() + gap),
        egui::pos2(bbox.left(), bbox.top() - gap - size.y),
    ];
    let mut best: Option<(egui::Pos2, f32, f32)> = None;
    for candidate in fallbacks.into_iter().chain(corners.iter().copied()) {
        let position = clamp_into_chart(candidate, size, chart);
        let rect = egui::Rect::from_min_size(position, size);
        let overlap = rect.intersect(bbox);
        let overlap_area = if overlap.is_positive() {
            overlap.area()
        } else {
            0.0
        };
        let distance = rect.center().distance(bbox.center());
        let wins = match &best {
            None => true,
            Some((_, best_area, best_distance)) => {
                overlap_area < *best_area
                    || (overlap_area == *best_area && distance > *best_distance)
            }
        };
        if wins {
            best = Some((position, overlap_area, distance));
        }
    }
    best.map_or_else(|| chart.left_top(), |(position, _, _)| position)
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
    pub indicator_panes: Vec<crate::indicators::PaneSlot>,
    /// The gutter band beside each pane, in the same order: where that pane's
    /// value labels are drawn and where its own zoom gesture lives.
    pub pane_gutters: Vec<egui::Rect>,
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
    fmt_time_as(ms, tz, crate::chart::TimeLabelFormat::Full)
}

/// The same instant written in a chosen [`TimeLabelFormat`] — what the time
/// axis calls when the strip is too narrow for the full form.
pub fn fmt_time_as(ms: i64, tz: TzOffset, format: crate::chart::TimeLabelFormat) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let secs = local.div_euclid(1000).rem_euclid(86_400);
    format.write(secs / 3600, (secs % 3600) / 60, secs % 60)
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
    /// The instruments the user added from the picker, already folded into
    /// `config`'s catalog. Kept apart from it so the picker can tell an
    /// addition — which it may take back out — from a shipped entry, which is
    /// the config file's and not the app's to touch.
    added_symbols: AddedSymbols,
    /// Where those additions persist. See [`crate::symbols_file`].
    symbols_path: std::path::PathBuf,

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

    // External chart chrome: the tabbed right dock and the edge-docked
    // drawing rail. Neither is painted over the chart canvas.
    dock: Dock,
    toolrail: ToolRail,

    // Delete confirmation for a locked drawing, shown next to the trigger.
    drawing_delete_confirm: bool,
    // Pre-edit copy of the selected drawing while an inspector edit gesture
    // (slider/color/coordinate drag) is in flight; committed as one undo
    // entry once pointer and keyboard let go.
    inspector_edit_baseline: Option<InspectorEdit>,
    toast: Option<Toast>,
    // Inspector chrome state: open tab, dock pin, whether the user moved the
    // floating window this session (manual position wins over placement),
    // and the selection the last placement was computed for.
    inspector_tab: InspectorTab,
    inspector_pinned: bool,
    inspector_moved: bool,
    inspector_last_selection: Option<usize>,
    // The floating inspector's position: automatic placement until the user
    // drags the title bar, manual from then on (only ever re-clamped). The
    // chart rectangle it is placed against belongs to the focused
    // [`ChartPane`], so a split window places against the pane the selection
    // lives on, not the window.
    // Scripted-validation hook: place one of every registered drawing on the
    // flow pane as soon as it has bars to anchor them to. Consumed once.
    pending_drawing_demo: bool,
    inspector_pos: Option<egui::Pos2>,
    // The floating panel's size as it was last actually drawn. Read back from
    // the window response rather than from egui's area memory: the memory
    // lookup is empty on the frame the panel is being laid out, which is
    // exactly the frame the placement and the clamp need a size.
    inspector_size: Option<egui::Vec2>,
    // Whether the user ever toggled the pin — the auto-pin width rule stops
    // firing once they have expressed a preference.
    inspector_pin_touched: bool,
    // Set on the unpin frame: the side panel still occupies that frame's
    // layout, so the floating host waits one frame and places against the
    // settled chart instead of the pinned-era geometry.
    inspector_settle_frame: bool,
    drawing_manager_open: bool,
    // Last frame's open state: the manager places itself beside the rail on
    // the frame it opens, and only then.
    drawing_manager_was_open: bool,
    // The manager's delete-all confirmation row is showing (audit M7): the
    // one command that removes locked objects too, so it is never one click.
    drawing_manager_confirm_delete_all: bool,
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,
    #[cfg(test)]
    inspector_pin_rect: Option<egui::Rect>,
    #[cfg(test)]
    toast_undo_rect: Option<egui::Rect>,
    #[cfg(test)]
    manager_action_rects: Vec<(usize, &'static str, egui::Rect)>,

    // Layer visibility (the right-click menu on a pane's canvas). The layers
    // belong to the pane that draws them; what lives here is where the choices
    // are written down and the switches no pane owns.
    /// Where layer visibility persists.
    chart_layers_path: std::path::PathBuf,
    /// The visibility already on disk, as a bitmask over the active tab's flow
    /// pane. Compared with the live one once per frame so a switch is saved
    /// whoever flipped it — the menu, the toolbar, the dock or the appearance
    /// panel. A dozen bool reads and an integer compare: cheaper than teaching
    /// four call sites to remember.
    saved_layer_mask: u16,
    /// What the file said at startup, applied to every pane opened since.
    layer_defaults: std::collections::BTreeMap<ChartLayer, bool>,
    /// Where a pane's layer menu leaves the grid switch and the "an indicator
    /// was hidden" flag; drained right after the canvas is drawn.
    layer_actions: chart_layers::LayerActions,

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

    /// Where trades save this run — resolved once at boot (environment >
    /// the user's stored pick > config) and updated by the panel's folder
    /// picker; new tabs journal here too.
    trades_dir: std::path::PathBuf,
    /// The in-flight trades-folder dialog, if any. One at a time.
    trades_dir_picker: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,

    // The saved workspace (§14, `ui-state.toml`): what the window opens on.
    // See [`crate::ui_state`] for what this file owns and what it deliberately
    // leaves to the sibling stores.
    /// Where the workspace persists.
    ui_state_path: std::path::PathBuf,
    /// Whether closing the window writes it. Read from the file at startup and
    /// toggled from the Workspace menu.
    save_on_exit: bool,
    /// The arrangements the trader named and kept, in the order the file lists
    /// them.
    ///
    /// Held here because every write of the workspace file rewrites the whole
    /// file: capturing the live window and saving it would drop the bookmarks
    /// on the floor if the app did not carry them between load and save.
    bookmarks: Vec<ui_state::NamedArrangement>,
    /// The Save-as box, while it is open: what has been typed so far.
    workspace_name_entry: Option<String>,
    /// Whether a workspace is on disk, so the menu can disable Reset without
    /// asking the filesystem. The menu body runs every frame it is open, and a
    /// `Path::exists` there is a syscall at 60 Hz for an answer that changes
    /// only when this app saves or forgets — the two places that update it.
    workspace_saved: bool,
    /// The window's inner size as of the last frame, in points — captured here
    /// because the size a workspace records is the one the user last saw, and
    /// by exit time the viewport has already been asked to close.
    window_size: Option<[f32; 2]>,
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
    /// (already streaming through `feed`) and bar `spec`, with no saved
    /// workspace to restore.
    ///
    /// The window itself always has one (`main` reads the file before it
    /// spawns a feed), so this is the tests' entry point: a case about the
    /// chart is not a case about what the last session left on disk.
    #[cfg(test)]
    #[must_use]
    pub fn new(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
    ) -> Self {
        Self::new_with_workspace(
            config,
            feed_id,
            symbol,
            spec,
            feed,
            ui_state::Workspace::default(),
        )
    }

    /// The same, restoring `workspace` over the configured defaults.
    ///
    /// The first tab is already streaming when this is called — `main` spawns
    /// it, because a window with no feed has nothing to show while it waits —
    /// so the caller is expected to have picked its market from
    /// [`Workspace::first_market`]. Everything else the workspace remembers is
    /// applied here.
    ///
    /// `workspace` must already have been through
    /// [`Workspace::restore`](ui_state::Workspace::restore): this function
    /// opens what it is given, and a market the config no longer offers is not
    /// its to discover.
    #[must_use]
    pub fn new_with_workspace(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
        workspace: ui_state::Workspace,
    ) -> Self {
        let trades_dir = {
            let stored = crate::paper_state::load(&crate::paper_state::default_path());
            PaperTrading::resolve_trades_dir(&config.paper.trades_dir, stored.as_deref())
        };
        let tab = Tab::new(
            FIRST_TAB_ID,
            pane_ids(FIRST_TAB_ID),
            feed_id.into(),
            symbol.into(),
            spec,
            feed,
            trades_dir.clone(),
        );
        let mut app = Self {
            tabs: vec![tab],
            active_tab: 0,
            next_tab_id: FIRST_TAB_ID + 1,
            persisted_tab: Some(FIRST_TAB_ID),
            source_picker: None,
            added_symbols: symbols_file::load(&symbols_file::default_path()),
            symbols_path: symbols_file::default_path(),
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
            toast: None,
            inspector_tab: InspectorTab::default(),
            inspector_pinned: false,
            inspector_moved: false,
            inspector_last_selection: None,
            pending_drawing_demo: false,
            inspector_pos: None,
            inspector_size: None,
            inspector_pin_touched: false,
            inspector_settle_frame: false,
            drawing_manager_open: false,
            drawing_manager_was_open: false,
            drawing_manager_confirm_delete_all: false,
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            #[cfg(test)]
            inspector_pin_rect: None,
            #[cfg(test)]
            toast_undo_rect: None,
            #[cfg(test)]
            manager_action_rects: Vec::new(),
            chart_layers_path: chart_layers::default_path(),
            saved_layer_mask: 0,
            layer_defaults: std::collections::BTreeMap::new(),
            layer_actions: chart_layers::LayerActions::default(),
            style: ChartStyle::default(),
            show_style: false,
            style_revision: 0,
            style_log_pending: false,
            last_style_change: None,
            show_perf: true,
            tz: TzOffset::default(),
            trades_dir,
            trades_dir_picker: None,
            ui_state_path: ui_state::default_path(),
            save_on_exit: true,
            bookmarks: Vec::new(),
            workspace_name_entry: None,
            workspace_saved: false,
            window_size: None,
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
        // Same for a declared opening layout: a feed the user reads by
        // timeframe can open straight on the timeframe chart.
        app.active_tab_mut().apply_feed_declared_layout(&config);
        // The map itself stays hidden until asked for — a layer nobody
        // requested must cost no projection. Capture is already running either
        // way, so this is a display choice and nothing else.
        app.active_tab_mut().tape_mut().set_depth_visible(false);
        // What the user last had on the canvas, applied over those defaults and
        // under the autostart hooks below: an env var is an explicit request
        // for this run and must still win (see `restore_chart_layers`).
        app.restore_chart_layers();
        // And the workspace itself — the tab strip, each tab's canvas, and the
        // chrome around them. After the config defaults (a saved cockpit is
        // the user's own answer to what a feed declares) and before the
        // autostart hooks, which are explicit requests for this one run.
        app.restore_workspace(workspace);
        // Dev/ops can open the map without a click.
        if std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().tape_mut().set_depth_visible(true);
        }
        // Same convenience for the live strip; its pixels stay
        // capability-gated either way (see live_strip_width).
        if std::env::var("QUANTICK_LIVE_STRIP_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().flow_pane.live_strip_visible = true;
        }
        // Drawing-toolbar hooks, so a validation run reaches every new
        // surface without a click (`.claude/skills/ui-harness`).
        if let Ok(id) = std::env::var("QUANTICK_DRAWING_TOOL")
            && let Some(tool) = drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == id.trim())
        {
            app.toolrail.arm(Tool::Drawing(tool));
        }
        if std::env::var("QUANTICK_DRAWING_MAGNET").is_ok_and(|value| value == "1") {
            app.toolrail.set_magnet(true);
        }
        app.pending_drawing_demo = std::env::var("QUANTICK_DRAWINGS_DEMO")
            .is_ok_and(|value| matches!(value.as_str(), "1" | "bands"));

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
        // Same convenience for the dock: open a named tab, so a scripted
        // validation run shows a panel without a click.
        if let Ok(name) = std::env::var("QUANTICK_DOCK_TAB") {
            let tab = match name.trim() {
                "l2" => Some(DockTab::L2),
                "bubbles" => Some(DockTab::Bubbles),
                "session" => Some(DockTab::Session),
                "trading" => Some(DockTab::Trading),
                "trades" => Some(DockTab::Trades),
                _ => None,
            };
            match tab {
                Some(tab) => app.dock.open_tab(tab),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "DOCK_TAB_AUTOSTART_UNKNOWN",
                    tab = %name,
                    action = "dock_left_as_is",
                    "QUANTICK_DOCK_TAB names no dock tab"
                ),
            }
        }
        // And for the performance report window — the Report… button's own
        // path, so a scripted run can show it.
        if std::env::var("QUANTICK_PAPER_REPORT_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().paper.autostart_report();
        }
        // Open on a named canvas layout, through the same path the View menu
        // takes. An env var is an explicit request for this run, so it wins
        // over a feed's declared `default_layout`.
        if let Ok(name) = std::env::var("QUANTICK_LAYOUT") {
            let layout = crate::config::DeclaredLayout::parse(&name).map(CanvasLayout::from);
            match layout {
                Some(layout) => app.active_tab_mut().set_layout(layout),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUT_AUTOSTART_UNKNOWN",
                    layout = %name,
                    action = "layout_left_as_is",
                    "QUANTICK_LAYOUT names no canvas layout (flow, time, time+flow)"
                ),
            }
        }
        // The Workspace menu's own path, so a validation run can see the save
        // confirmation without a click. A menu entry cannot be reached by an
        // env var, but the state it produces has to be
        // (`.claude/skills/ui-harness`). This writes the file for real,
        // exactly as the entry does — a hook that fakes its surface proves
        // nothing — so point `QUANTICK_UI_STATE` at a scratchpad first.
        if std::env::var("QUANTICK_WORKSPACE_SAVE").is_ok_and(|value| value == "1") {
            app.save_workspace("autostart");
        }
        // An env var is not a user edit: what the autostart hooks switched on
        // must not be written back as though the user had asked for it every
        // launch from now on. Same rule the indicator state follows.
        app.saved_layer_mask = app.layer_mask();
        app
    }

    /// Ask the operating system for a trades folder, off the UI thread —
    /// the panel's "choose where trades are saved". One dialog at a time.
    fn open_trades_dir_picker(&mut self) {
        if self.trades_dir_picker.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        // Start where trades actually go right now — under an env override
        // that is the override's folder, not the stored base.
        let start = self.active_tab().paper.trades_dir().to_path_buf();
        std::thread::Builder::new()
            .name("quantick-trades-dir-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose where trades are saved");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_folder());
            })
            .expect("spawn trades-dir picker thread");
        self.trades_dir_picker = Some(receiver);
    }

    /// Land the picked folder: every tab journals there from now on, and
    /// the choice is remembered across restarts (`paper-state.toml`) —
    /// files already written stay where they are.
    fn poll_trades_dir_picker(&mut self) {
        let Some(receiver) = &self.trades_dir_picker else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.trades_dir_picker = None;
        let Some(dir) = choice else { return };
        crate::paper_state::save(
            &crate::paper_state::default_path(),
            &dir.display().to_string(),
        );
        self.trades_dir = dir;
        for tab in &mut self.tabs {
            tab.paper.set_trades_dir(self.trades_dir.clone());
        }
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
    fn open_tab(&mut self, feed_id: String, symbol: String, spec: Option<BarSpec>) {
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
        self.adopt_tab(feed_id, symbol, handle, spec);
    }

    /// Take a market that is already streaming as a new tab, and make it the
    /// active one.
    ///
    /// The bar spec is inherited from the tab you were on: opening a second
    /// market to compare it against the first is the reason to do this, and
    /// landing on a different aggregation would defeat that. A feed that
    /// declares its own `default_bars`/`default_layout` overrides the
    /// inheritance — the declaration exists because that market reads
    /// differently, which is exactly when inheriting would mislead.
    /// `spec` overrides both, and exists for the one caller that already knows
    /// the answer: a workspace restoring the bar rule this market was last
    /// read on. Inheriting there would quietly discard what the user saved.
    fn adopt_tab(
        &mut self,
        feed_id: String,
        symbol: String,
        feed: FeedHandle,
        spec: Option<BarSpec>,
    ) {
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
        let spec = spec.unwrap_or_else(|| {
            self.config
                .startup_spec_for(&feed_id)
                .unwrap_or_else(|| self.active_tab().flow_pane.state.spec().clone())
        });
        let trades_dir = self.trades_dir.clone();
        self.tabs.push(Tab::new(
            id,
            pane_ids(id),
            feed_id,
            symbol,
            spec,
            feed,
            trades_dir,
        ));
        self.active_tab = self.tabs.len() - 1;
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        self.active_tab_mut().ensure_book_capture(&config);
        self.active_tab_mut().apply_feed_bubble_preset(&config);
        self.active_tab_mut().apply_feed_declared_layout(&config);
        // The new tab opens on the layers the user left showing, over the
        // preset it just put on: opening a second market is not a request to
        // bring back the chrome they switched off.
        let defaults = self.layer_defaults.clone();
        self.active_tab_mut()
            .flow_pane
            .apply_layer_states(&defaults);
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
        let mut closed = self.tabs.remove(index);
        // The tab's session ends here. Everything else it owns can simply be
        // dropped — the feed thread and the workers stop when their channels
        // go — but a simulated position is state the user created, and the
        // paper-trading contract says it ends in a labeled, journaled flatten,
        // never by vanishing with its window.
        closed.close();
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
        // The SOURCE group writes straight into the active tab: a feed or
        // symbol change is that tab's market switch. The BARS group writes
        // into the *focused pane* — the pane the status bar reads and every
        // indicator command lands on (§11) — so the three chrome surfaces
        // can never disagree about which chart a command describes, and in
        // the Time layout the group governs the chart actually on screen.
        let tab = self.active_tab_mut();
        let focused = tab.focused_side();
        let live_strip_on = tab.flow_pane.live_strip_visible;
        let pane = match focused {
            PaneSide::Time => tab.time_pane.as_mut().unwrap_or(&mut tab.flow_pane),
            PaneSide::Flow => &mut tab.flow_pane,
        };
        let mut model = toolbar::ToolbarModel {
            feeds,
            feed_id: &mut tab.feed_id,
            feed_display_name,
            symbols,
            symbol: &mut tab.symbol,
            replay,
            kind: &mut pane.kind,
            tick_n: &mut pane.tick_n,
            volume_units: &mut pane.volume_units,
            dollar_notional: &mut pane.dollar_notional,
            time_interval_ms: &mut pane.time_interval_ms,
            imbalance_target: &mut pane.imbalance_target,
            history_step: &mut tab.history_step,
            history_trades: tab.history_trades,
            capabilities,
            heatmap_on,
            bubbles_on,
            live_strip_on,
            dock_visible,
            appearance_open: show_style,
            paper: toolbar::PaperTradeModel {
                ready: tab.paper.ready(),
                buy_label: tab.paper.entry_label(quantick_engine::Side::Buy),
                sell_label: tab.paper.entry_label(quantick_engine::Side::Sell),
                buy_hover: tab.paper.entry_hover(quantick_engine::Side::Buy),
                sell_hover: tab.paper.entry_hover(quantick_engine::Side::Sell),
                close_label: tab.paper.close_button_label(),
            },
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
                let target = self.target_slot(SlotId(slot));
                self.toggle_indicator_hidden_at(target);
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.remove_indicator_at(target);
            }
            ToolbarAction::AddScriptIndicator(index) => {
                self.add_script_indicator(index);
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.open_indicator_settings_at(target);
            }
            // The toolbar acts on the market it is showing: the active tab's
            // simulator, whose tape the buttons' price came from.
            ToolbarAction::PaperBuy => self
                .active_tab_mut()
                .paper
                .market(quantick_engine::Side::Buy),
            ToolbarAction::PaperSell => self
                .active_tab_mut()
                .paper
                .market(quantick_engine::Side::Sell),
            ToolbarAction::PaperClose => self.active_tab_mut().paper.close_position(),
        }
    }

    /// Flip a slot's render-side eye, wherever the slot lives. Addressed by
    /// [`TabSlot`], never by focus: the legend acts on the pane it is drawn
    /// on, and the toolbar path builds its target from focus before calling.
    fn toggle_indicator_hidden_at(&mut self, target: TabSlot) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) else {
            return;
        };
        tab.pane_mut(target.side)
            .indicators
            .toggle_hidden(target.slot);
        self.mark_indicator_state_dirty();
    }

    /// Remove a slot, wherever it lives. UI first (the entry vanishes this
    /// frame), worker second; events already in flight for the slot are
    /// dropped on apply.
    fn remove_indicator_at(&mut self, target: TabSlot) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) else {
            return;
        };
        let pane = tab.pane_mut(target.side);
        pane.indicators.remove(target.slot);
        pane.indicator_worker
            .send(IndicatorCommand::Remove(target.slot));
        self.slot_kinds.retain(|(owner, _)| *owner != target);
        self.script_files.retain(|(owner, ..)| *owner != target);
        self.mark_indicator_state_dirty();
    }

    /// Open the settings dialog for a slot, wherever it lives.
    fn open_indicator_settings_at(&mut self, target: TabSlot) {
        let Some(view) = self
            .tabs
            .iter()
            .find(|tab| tab.id == target.tab)
            .map(|tab| tab.pane(target.side))
            .and_then(|pane| {
                pane.indicators
                    .all()
                    .iter()
                    .find(|view| view.slot == target.slot)
            })
        else {
            return;
        };
        self.indicator_settings = Some(SettingsDialog {
            slot: target.slot,
            title: view.label().to_owned(),
            draft: view.input_values.clone(),
        });
        self.indicator_settings_target = target;
    }

    /// Draw each visible pane's indicator legend and run what its rows asked
    /// for. Actions resolve against the pane the legend was drawn on — the
    /// legend must never act on the chart beside it (the audit's MAJOR-4
    /// trap, avoided by construction).
    fn draw_indicator_legends(&mut self, ctx: &egui::Context) {
        let tab_id = self.active_tab().id;
        let split = self.active_tab().layout == CanvasLayout::TimeAndFlow
            && self.active_tab().time_pane.is_some();
        let mut pending: Vec<(PaneSide, indicator_legend::LegendAction)> = Vec::new();
        for side in [PaneSide::Flow, PaneSide::Time] {
            if side == PaneSide::Time && !split {
                continue;
            }
            let pane = self.active_tab().pane(side);
            // The rect is last frame's, like every anchor the input path
            // reads; a pane not yet drawn has none and draws no legend.
            let Some(mut rect) = pane.last_chart_area else {
                continue;
            };
            // The position HUD owns the very corner while a position is
            // open; the legend rides just below it.
            if side == PaneSide::Flow && self.active_tab().paper.position_summary().is_some() {
                rect.min.y += LEGEND_BELOW_HUD_OFFSET_PX;
            }
            for action in indicator_legend::draw(ctx, pane.id, rect, pane.indicators.all()) {
                pending.push((side, action));
            }
        }
        for (side, action) in pending {
            let at = |slot| TabSlot {
                tab: tab_id,
                side,
                slot,
            };
            match action {
                indicator_legend::LegendAction::ToggleHidden(slot) => {
                    self.toggle_indicator_hidden_at(at(slot));
                }
                indicator_legend::LegendAction::OpenSettings(slot) => {
                    self.open_indicator_settings_at(at(slot));
                }
                indicator_legend::LegendAction::Remove(slot) => {
                    self.remove_indicator_at(at(slot));
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
            // Follow the live label: an applied edit can retitle the
            // indicator (`EMA(9)` → `EMA(21)`), and a dialog that stays open
            // must not keep announcing the old one.
            dialog.title = view.label().to_owned();
            indicator_panel::draw(ctx, dialog, &view.descriptor.inputs)
        };
        match outcome {
            SettingsOutcome::Open => {}
            SettingsOutcome::Close => self.indicator_settings = None,
            SettingsOutcome::Apply => self.apply_indicator_settings_draft(),
        }
    }

    /// Send the open dialog's draft to the worker and keep the dialog open
    /// (audit M2): tuning is a nudge-and-look loop, and a dialog that dies
    /// on every Apply makes each attempt four clicks. The slot is the one
    /// the dialog was opened on, not whatever has focus now — clicking
    /// Apply must not retarget the edit.
    fn apply_indicator_settings_draft(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_ref() else {
            return;
        };
        let (slot, values) = (dialog.slot, dialog.draft.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs { slot, values });
        }
        self.mark_indicator_state_dirty();
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
                // The same kind string the healthy path derives from its
                // source, so an error slot the trader fixes and reloads keeps
                // whatever they had drawn on its pane.
                let slot = pane.indicators.allocate_slot(format!("script.{name}"));
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
    /// The file records the flow pane of the *first* tab — the one the window
    /// opens with, whether its market came from the config defaults or from
    /// the saved workspace ([`crate::ui_state`]). Slots on a time pane, or on
    /// a tab opened after it, stay in-session: a restored entry for either
    /// would have nowhere to land, and would then be quietly dropped by the
    /// next save.
    ///
    /// The tab strip now persists, which was the precondition this comment
    /// used to name — but the indicators of tabs 2..n did not follow it in the
    /// same change. That is the honest state: a restored workspace brings back
    /// every tab's *market and canvas*, and every tab but the first opens with
    /// no indicators. Extending the state file to key its entries by
    /// (tab, pane) is the increment that closes it.
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

    /// What a pane's layer menu could not switch itself.
    ///
    /// Drained right after the canvas, so the frame that clicked the entry is
    /// the frame that applies it. Both wishes reach the real owner — the shared
    /// style, the indicator state file — instead of a second copy on the pane.
    fn apply_layer_actions(&mut self) {
        let actions = std::mem::take(&mut self.layer_actions);
        if let Some(visible) = actions.grid {
            self.style.canvas.grid_enabled = visible;
            // The appearance panel's own edits bump this; the renderer and the
            // style log both read it to know something moved.
            self.style_revision = self.style_revision.saturating_add(1);
        }
        if actions.indicators_changed {
            self.mark_indicator_state_dirty();
        }
    }

    /// The visibility this app persists, as one bit per layer.
    ///
    /// Read off the active tab's flow pane: the file records the canvas
    /// quantick is built around, the same scope the indicator state file has
    /// (see [`Self::maintain_indicator_state`]). A tab's second pane opens
    /// matching it and is in-session from there.
    fn layer_mask(&self) -> u16 {
        self.active_tab().flow_pane.layer_mask(&self.style)
    }

    /// Save the layer visibility when it differs from what is on disk.
    ///
    /// Called once per frame instead of from each switch: the layers are owned
    /// by four different pieces of chrome, and a save hook on each is four
    /// chances to forget one.
    fn maintain_chart_layers(&mut self) {
        let mask = self.layer_mask();
        if mask == self.saved_layer_mask {
            return;
        }
        chart_layers::save(
            &self.chart_layers_path,
            &self.active_tab().flow_pane.layer_states(&self.style),
        );
        self.saved_layer_mask = mask;
    }

    /// Apply the saved layer visibility to the tab the app opened with.
    ///
    /// Runs before the autostart env vars so an explicit `QUANTICK_*_AUTOSTART`
    /// still wins for the run it was set on: a validation session asks for the
    /// heatmap on the command line and gets it, whatever the file remembers.
    fn restore_chart_layers(&mut self) {
        self.layer_defaults = chart_layers::load(&self.chart_layers_path);
        // Whatever the file said (including nothing at all) is now on screen;
        // only a change from here is worth another write.
        if self.layer_defaults.is_empty() {
            self.saved_layer_mask = self.layer_mask();
            return;
        }
        if let Some(grid) = self.layer_defaults.get(&ChartLayer::Grid) {
            self.style.canvas.grid_enabled = *grid;
        }
        let defaults = self.layer_defaults.clone();
        self.apply_layer_defaults(&defaults);
        self.saved_layer_mask = self.layer_mask();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CHART_LAYERS_RESTORED",
            path = %self.chart_layers_path.display(),
            hidden = defaults.values().filter(|visible| !**visible).count(),
            "chart layer visibility restored"
        );
    }

    /// Put every open pane on the saved visibility. Also run when a tab is
    /// opened later, so a new tab looks like the one beside it rather than
    /// bringing back layers the user switched off.
    fn apply_layer_defaults(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        for tab in &mut self.tabs {
            tab.flow_pane.apply_layer_states(states);
            if let Some(pane) = tab.time_pane.as_mut() {
                pane.apply_layer_states(states);
            }
        }
    }

    /// The window as it stands, in the form the workspace file records.
    ///
    /// Read off the live state rather than accumulated as it changes: the
    /// arrangement is a dozen fields spread over the tabs and the chrome, and
    /// a second copy maintained by every control that moves one of them would
    /// be a dozen chances to forget. Saving is rare and event-driven, so
    /// reading them all at once costs nothing anyone can see.
    fn capture_workspace(&self) -> ui_state::Workspace {
        let (tabs, chrome) = self.capture_arrangement();
        ui_state::Workspace::new(
            self.save_on_exit,
            self.window_size,
            self.active_tab,
            tabs,
            Some(chrome),
        )
        // Every write rewrites the whole file, so the bookmarks have to ride
        // along or saving the startup screen would silently delete them.
        .with_saved(self.bookmarks.clone())
    }

    /// The tabs and the chrome as they stand — the part a startup workspace
    /// and a named one describe identically, so both capture through here.
    fn capture_arrangement(&self) -> (Vec<ui_state::SavedTab>, ui_state::SavedChrome) {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| ui_state::SavedTab {
                feed: tab.feed_id.clone(),
                symbol: tab.symbol.clone(),
                layout: tab.layout.into(),
                split_fraction: Some(tab.split_fraction),
                focus: Some(tab.focused_side().into()),
                flow_bars: tab.flow_pane.state.spec().to_config_string(),
                // Only a pane that exists has an interval worth recording; a
                // tab that never showed the split restores on the default,
                // which is what it had.
                time_bars: tab
                    .time_pane
                    .as_ref()
                    .map(|pane| pane.state.spec().to_config_string()),
            })
            .collect();
        let chrome = ui_state::SavedChrome {
            timezone_minutes: self.tz.minutes(),
            dock_visible: self.dock.visible(),
            dock_tab: self.dock.tab().map(Into::into),
            rail_visible: self.toolrail.visible(),
            rail_dock: self.toolrail.dock().into(),
            perf_readings: self.show_perf,
        };
        (tabs, chrome)
    }

    /// Open the saved workspace over the configured defaults.
    ///
    /// The first tab already exists and is already streaming the market
    /// `main` picked from this same workspace, so it is *arranged* here rather
    /// than opened; the rest are opened outright, each on its own feed. A tab
    /// carries its bar rule explicitly (see [`Self::adopt_tab`]) — inheriting
    /// would replace what the user saved with what the tab beside it happens
    /// to show.
    ///
    /// `save_on_exit` is taken from the file even when the file has no tabs:
    /// a trader who switched autosave off and then reset their layout must not
    /// find it switched back on at the next launch.
    fn restore_workspace(&mut self, workspace: ui_state::Workspace) {
        self.save_on_exit = workspace.save_on_exit;
        self.bookmarks = workspace.saved.clone();
        // One stat at boot, so the Reset entry can gate on a field instead of
        // the filesystem for the rest of the session. A file with no tabs
        // still counts: it carries the autosave setting, and Reset is how the
        // trader gets rid of it.
        self.workspace_saved = self.ui_state_path.exists();
        if let Some(chrome) = &workspace.chrome {
            self.tz = TzOffset::new(chrome.timezone_minutes);
            self.dock
                .restore(chrome.dock_visible, chrome.dock_tab.map(Into::into));
            self.toolrail.set_dock(chrome.rail_dock.into());
            self.toolrail.set_visible(chrome.rail_visible);
            self.show_perf = chrome.perf_readings;
        }
        if workspace.is_empty() {
            return;
        }
        for (index, saved) in workspace.tabs.iter().enumerate() {
            // `restore` has already dropped anything unparseable, so a spec
            // reaching here is one a control could have produced.
            let flow = BarSpec::parse(&saved.flow_bars).ok();
            if index == 0 {
                // Tab zero is the one `main` spawned. Its market matches this
                // entry (that is where `main` read it from), so only its bar
                // rule can still differ — `main` prefers a feed's declared
                // `default_bars` when the workspace names none.
                if let Some(spec) = flow {
                    self.tabs[0].flow_pane.set_spec(spec);
                }
            } else {
                self.open_tab(saved.feed.clone(), saved.symbol.clone(), flow);
            }
            let time_interval =
                saved
                    .time_bars
                    .as_deref()
                    .and_then(|text| match BarSpec::parse(text) {
                        Ok(BarSpec::Time(ms)) => Some(ms),
                        _ => None,
                    });
            let focus = saved.focus.map(Into::into);
            // `open_tab` activates what it opened, so the tab just arranged is
            // always the last one — index zero on the first pass.
            let target = if index == 0 { 0 } else { self.tabs.len() - 1 };
            self.tabs[target].restore_canvas(
                CanvasLayout::from(saved.layout),
                saved.split_fraction,
                focus,
                time_interval,
            );
        }
        self.active_tab = workspace.active_tab.min(self.tabs.len() - 1);
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_RESTORED",
            path = %self.ui_state_path.display(),
            tabs = self.tabs.len(),
            active = self.active_tab,
            save_on_exit = self.save_on_exit,
            "workspace restored"
        );
    }

    /// Write the workspace and say so on the status bar.
    ///
    /// The notice is the point of the explicit action: a trader who arranges a
    /// cockpit and clicks Save wants to know it is kept, and "it looks the
    /// same" is not an answer. A failed write says *that* instead — being told
    /// "saved" and finding out at the next launch is the one outcome worth
    /// engineering against.
    fn save_workspace(&mut self, reason: &'static str) {
        let workspace = self.capture_workspace();
        let saved = ui_state::save(&self.ui_state_path, &workspace);
        self.workspace_saved |= saved;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_SAVED",
            path = %self.ui_state_path.display(),
            tabs = workspace.tabs.len(),
            saved,
            reason,
            action = if saved { "workspace_written" } else { "workspace_not_written" },
            "workspace save"
        );
        self.note_workspace(if saved {
            format!(
                "Workspace saved — quantick opens on {} {}",
                workspace.tabs.len(),
                if workspace.tabs.len() == 1 {
                    "chart tab"
                } else {
                    "chart tabs"
                }
            )
        } else {
            "Workspace could not be saved — see the log".to_owned()
        });
    }

    /// The Save-as box: one text field, Save and Cancel.
    ///
    /// A window rather than an inline menu field, because a menu closes the
    /// moment focus moves and a name is several keystrokes long. Enter saves,
    /// Escape cancels, and the field takes the keyboard on the frame it opens
    /// so the trader can type without clicking into it first.
    fn draw_workspace_name_box(&mut self, ctx: &egui::Context) {
        let Some(mut entry) = self.workspace_name_entry.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let mut open = true;
        egui::Window::new("Save workspace as")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(WORKSPACE_NAME_BOX_WIDTH_PX);
                ui.label("A name you will recognise later.");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut entry)
                        .hint_text("scalp WIN")
                        .char_limit(ui_state::MAX_WORKSPACE_NAME)
                        .desired_width(f32::INFINITY),
                );
                field.request_focus();
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                // A name already in use replaces that bookmark. Saying so
                // before the click is the difference between "save as" and
                // "lose the arrangement I meant to keep".
                if let Some(clean) = ui_state::clean_workspace_name(&entry)
                    && self.bookmarks.iter().any(|held| held.name == clean)
                {
                    ui.label(
                        egui::RichText::new(format!("Replaces the saved \"{clean}\"."))
                            .color(theme::AMBER),
                    );
                }
                ui.horizontal(|ui| {
                    let named = ui_state::clean_workspace_name(&entry).is_some();
                    if ui
                        .add_enabled(named, egui::Button::new("Save"))
                        .on_disabled_hover_text("Type a name first")
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if save {
            self.save_named_workspace(&entry);
        } else if !cancel && open {
            // Neither settled: keep what has been typed for the next frame.
            self.workspace_name_entry = Some(entry);
        }
    }

    /// Write the bookmarks without disturbing the startup arrangement.
    ///
    /// Reads the file back and swaps only the named entries, rather than
    /// capturing the live window: saving a bookmark must not redefine what the
    /// app opens on, and `capture_workspace` describes the screen *now*, which
    /// is exactly what the startup arrangement must not become.
    fn write_bookmarks(&mut self) -> bool {
        let mut file = ui_state::load(&self.ui_state_path);
        // The one live setting that belongs to the file rather than to either
        // arrangement.
        file.save_on_exit = self.save_on_exit;
        file.saved = self.bookmarks.clone();
        let written = ui_state::save(&self.ui_state_path, &file);
        self.workspace_saved |= written;
        written
    }

    /// Keep the window as it stands under `name`.
    ///
    /// A bookmark, not a startup setting: what the app opens on is untouched.
    /// The reason to name an arrangement is usually to have somewhere to come
    /// back *to*, and a "save this so I can return to it" that also redefined
    /// the opening screen would be the opposite of a safety net.
    ///
    /// An existing name is replaced rather than duplicated — that is what
    /// "save as" means everywhere else, and it spares the menu a list of five
    /// entries called "scalp".
    fn save_named_workspace(&mut self, name: &str) {
        let Some(name) = ui_state::clean_workspace_name(name) else {
            self.note_workspace("A workspace needs a name".to_owned());
            return;
        };
        let (tabs, chrome) = self.capture_arrangement();
        let entry = ui_state::NamedArrangement {
            name: name.clone(),
            window: self.window_size,
            active_tab: self.active_tab,
            tabs,
            chrome: Some(chrome),
        };
        let replaced = match self.bookmarks.iter_mut().find(|held| held.name == name) {
            Some(held) => {
                *held = entry;
                true
            }
            None => {
                self.bookmarks.push(entry);
                false
            }
        };
        let written = self.write_bookmarks();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_SAVED",
            name = %name,
            replaced,
            saved = self.bookmarks.len(),
            written,
            action = if written { "bookmark_written" } else { "bookmark_not_written" },
            "named workspace saved"
        );
        self.note_workspace(if written {
            let verb = if replaced { "replaced" } else { "saved" };
            format!("Workspace \"{name}\" {verb} — reopen it from Workspace → Open")
        } else {
            format!("\"{name}\" could not be saved — see the log")
        });
    }

    /// Put the window back the way the bookmark called `name` recorded it.
    ///
    /// The saved markets are opened as new tabs and the tabs that were on
    /// screen are closed afterwards, rather than the reverse: `close_tab`
    /// refuses to close the last tab — a window with no market has nothing to
    /// draw — so growing before shrinking is what lets the whole strip be
    /// replaced. Closing goes through the same path a `Ctrl+W` takes, so a
    /// simulated position ends in the labeled, journaled flatten the
    /// paper-trading contract promises instead of vanishing with its tab.
    ///
    /// The startup workspace is left alone. Opening a bookmark is a thing you
    /// do to *this session*; making it the opening screen is `Save workspace`,
    /// one entry above.
    fn open_named_workspace(&mut self, name: &str) {
        let Some(entry) = self
            .bookmarks
            .iter()
            .find(|held| held.name == name)
            .cloned()
        else {
            self.note_workspace(format!("No workspace called \"{name}\""));
            return;
        };
        if entry.tabs.is_empty() {
            // `restore` drops empty bookmarks at load, so this is only
            // reachable from a file edited under a running app.
            self.note_workspace(format!("\"{name}\" has no market left to open"));
            return;
        }
        let replaced = self.tabs.len();
        for saved in &entry.tabs {
            self.open_tab(
                saved.feed.clone(),
                saved.symbol.clone(),
                BarSpec::parse(&saved.flow_bars).ok(),
            );
            let time_interval =
                saved
                    .time_bars
                    .as_deref()
                    .and_then(|text| match BarSpec::parse(text) {
                        Ok(BarSpec::Time(ms)) => Some(ms),
                        _ => None,
                    });
            let opened = self.tabs.len() - 1;
            self.tabs[opened].restore_canvas(
                CanvasLayout::from(saved.layout),
                saved.split_fraction,
                saved.focus.map(Into::into),
                time_interval,
            );
        }
        for _ in 0..replaced {
            self.close_tab(0);
        }
        if let Some(chrome) = &entry.chrome {
            self.tz = TzOffset::new(chrome.timezone_minutes);
            self.dock
                .restore(chrome.dock_visible, chrome.dock_tab.map(Into::into));
            self.toolrail.set_dock(chrome.rail_dock.into());
            self.toolrail.set_visible(chrome.rail_visible);
            self.show_perf = chrome.perf_readings;
        }
        self.active_tab = entry.active_tab.min(self.tabs.len().saturating_sub(1));
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_OPENED",
            name = %name,
            tabs = self.tabs.len(),
            closed = replaced,
            active = self.active_tab,
            action = "replace_tab_strip",
            "named workspace opened"
        );
        self.note_workspace(format!(
            "Opened \"{name}\" — {} {}",
            self.tabs.len(),
            if self.tabs.len() == 1 {
                "chart tab"
            } else {
                "chart tabs"
            }
        ));
    }

    /// Forget the bookmark called `name`. The window on screen is untouched —
    /// deleting a bookmark throws away a way back, not the place you are.
    fn delete_named_workspace(&mut self, name: &str) {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|held| held.name != name);
        if self.bookmarks.len() == before {
            return;
        }
        let written = self.write_bookmarks();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_DELETED",
            name = %name,
            remaining = self.bookmarks.len(),
            written,
            action = if written { "bookmark_forgotten" } else { "file_not_written" },
            "named workspace deleted"
        );
        self.note_workspace(if written {
            format!("Workspace \"{name}\" deleted")
        } else {
            format!("\"{name}\" could not be deleted — see the log")
        });
    }

    /// Forget the saved workspace: the next launch opens on the configured
    /// defaults. The window on screen is deliberately left alone — a trader
    /// resetting their *startup* layout mid-session has not asked to have the
    /// charts they are reading rearranged under them.
    fn forget_workspace(&mut self) {
        // Reset clears the *startup* arrangement. The bookmarks survive it,
        // because coming back after a reset is the whole reason to name one:
        // deleting the safety net as part of the act it exists to undo would
        // be the single worst thing this menu could do.
        let kept = !self.bookmarks.is_empty();
        let forgotten = if kept {
            let mut file = ui_state::Workspace::default().with_saved(self.bookmarks.clone());
            file.save_on_exit = self.save_on_exit;
            ui_state::save(&self.ui_state_path, &file)
        } else {
            ui_state::forget(&self.ui_state_path)
        };
        // The file still exists while it holds bookmarks, so Reset stays
        // available — it is now a no-op for the startup screen and the entry
        // says as much.
        self.workspace_saved = kept && forgotten;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_FORGOTTEN",
            path = %self.ui_state_path.display(),
            forgotten,
            bookmarks_kept = self.bookmarks.len(),
            action = if forgotten { "open_on_config_defaults" } else { "workspace_kept" },
            "workspace reset"
        );
        self.note_workspace(match (forgotten, kept) {
            (true, true) => format!(
                "Startup layout reset — the next launch opens on the configured default. \
                 {} saved {} kept.",
                self.bookmarks.len(),
                if self.bookmarks.len() == 1 {
                    "workspace"
                } else {
                    "workspaces"
                }
            ),
            (true, false) => {
                "Startup layout reset — the next launch opens on the configured default".to_owned()
            }
            (false, _) => "Workspace could not be reset — see the log".to_owned(),
        });
    }

    /// Keep the window size the workspace would record, and take the exit
    /// save when the window is closing.
    ///
    /// **Per-frame cost**: two reads off the frame's own input state and a
    /// float compare. The save itself is not on this path — it happens on the
    /// one frame the close is requested, and the window is going away anyway.
    ///
    /// The size is tracked here rather than read at exit because by then the
    /// viewport has already been asked to close: what a workspace should
    /// remember is the window the trader was working in, not whatever the
    /// platform reports on the way out.
    fn maintain_workspace(&mut self, ctx: &egui::Context) {
        let (size, closing) = ctx.input(|input| {
            let viewport = input.viewport();
            (
                viewport
                    .inner_rect
                    .map(|rect| [rect.width(), rect.height()]),
                viewport.close_requested(),
            )
        });
        if let Some(size) = size
            && size[0] > 0.0
            && size[1] > 0.0
        {
            self.window_size = Some(size);
        }
        if closing && self.save_on_exit {
            self.save_workspace("exit");
        }
    }

    /// Post a Workspace-menu answer through the window's one acknowledgement
    /// channel ([`Toast`]).
    ///
    /// No Undo: the file it replaced is gone, and `Reset startup layout` is
    /// the honest way back rather than a button that pretends otherwise.
    fn note_workspace(&mut self, message: String) {
        self.toast = Some(Toast {
            message: message.into(),
            shown_at: Instant::now(),
            offers_undo: false,
        });
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
        self.toast = had_drawings.then(|| Toast {
            message: "Drawings cleared - the bars were rebuilt under them.".into(),
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
            // Drawings are a per-frame, O(objects) paint cost, and the shared
            // ones are additionally reprojected on every other pane of the
            // tab. Counting them here is what lets a frame-cost reading be
            // attributed instead of guessed — and it is the only way a
            // headless run can prove the drawing overlay is populated at all.
            drawings = self.active_tab().flow_pane.drawings.items().len()
                + self
                    .active_tab()
                    .time_pane
                    .as_ref()
                    .map_or(0, |pane| pane.drawings.items().len()),
            shared_drawings = self.active_tab().flow_pane.drawings.shared_count()
                + self
                    .active_tab()
                    .time_pane
                    .as_ref()
                    .map_or(0, |pane| pane.drawings.shared_count()),
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
        let venue_bars = pane.history_prefix.len();
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
            venue_bars,
            backfilled_bars: backfilled,
            live_bars: live,
            side_note: note.clone().map(|(label, _)| label),
            side_detail: note.and_then(|(_, detail)| detail),
            // Provenance follows the active tab (§11), and so does the
            // simulated P&L: the cell speaks for the market on screen, never
            // for a background tab's position.
            sim_pnl: self.active_tab().paper.status_cell(),
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
/// Saves the workspace — the arrangement the next launch opens on.
///
/// Ctrl+Shift+S rather than the Ctrl+S every editor uses, deliberately: a
/// chart has no document, and a trader who reaches for Ctrl+S out of habit
/// mid-session should hit nothing rather than silently redefine what their
/// platform opens on.
const SAVE_WORKSPACE_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::S,
);
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
/// Simulated buy at market (`docs/ux/paper-trading.md` §9). All the
/// trading hotkeys are Shift+letter and stand down while any text field
/// owns the keyboard — a capital letter typed into a symbol box must
/// never become an order.
const PAPER_BUY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::B);
/// Simulated sell at market.
const PAPER_SELL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::S);
/// Reverse the simulated position.
const PAPER_REVERSE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::R);
/// Flatten: close the position and cancel every working order.
const PAPER_FLATTEN_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::F);
/// Cancel every working order without trading.
const PAPER_CANCEL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::X);
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
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE_WORKSPACE_SHORTCUT)) {
            self.save_workspace("shortcut");
        }
        // Trading hotkeys, swallowed only while no text field owns the
        // keyboard. Market entries use the ticket's quantity and offsets,
        // exactly like the toolbar buttons they twin.
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_BUY_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .market(quantick_engine::Side::Buy);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_SELL_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .market(quantick_engine::Side::Sell);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_REVERSE_SHORTCUT)) {
                self.active_tab_mut().paper.reverse_position();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_FLATTEN_SHORTCUT)) {
                self.active_tab_mut().paper.flatten();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_CANCEL_SHORTCUT)) {
                self.active_tab_mut().paper.cancel_all_orders();
            }
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
                        // What the canvas shows is a view concern, so the
                        // switch lives here rather than under File, and each
                        // entry names the charts it shows — "Timeframe", not
                        // layout jargon (audit §3).
                        ui.menu_button("Layout", |ui| {
                            for (layout, label) in [
                                (CanvasLayout::Single, "Flow"),
                                (CanvasLayout::Time, "Timeframe"),
                                (CanvasLayout::TimeAndFlow, "Timeframe + Flow"),
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
                        ui.menu_button("Drawing toolbar", |ui| {
                            for (dock, label) in [
                                (ToolboxDock::Left, "Left"),
                                (ToolboxDock::Right, "Right"),
                                (ToolboxDock::Top, "Top"),
                                (ToolboxDock::Bottom, "Bottom"),
                            ] {
                                if ui
                                    .selectable_label(self.toolrail.dock() == dock, label)
                                    .clicked()
                                {
                                    self.toolrail.set_dock(dock);
                                    ui.close_menu();
                                }
                            }
                        });
                        let toolbox_label = if self.toolrail.visible() {
                            "Hide drawing toolbar"
                        } else {
                            "Show drawing toolbar"
                        };
                        if ui.button(toolbox_label).clicked() {
                            self.toolrail.toggle_visible();
                            ui.close_menu();
                        }
                        for (tab, label) in [
                            (DockTab::L2, "L2 settings"),
                            (DockTab::Bubbles, "Bubble settings"),
                            (DockTab::Session, "Session"),
                            (DockTab::Trading, "Paper trading"),
                            (DockTab::Trades, "Trades"),
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
                    // The workspace is its own menu, not a File entry: "what
                    // does quantick open on" is a question a trader asks about
                    // their cockpit, not about a document, and burying it
                    // under File is how a platform ends up with traders who
                    // rebuild their screen every morning without knowing they
                    // never had to (audit §6).
                    ui.menu_button("Workspace", |ui| {
                        if ui
                            .add(
                                egui::Button::new("Save workspace").shortcut_text(
                                    ui.ctx().format_shortcut(&SAVE_WORKSPACE_SHORTCUT),
                                ),
                            )
                            .on_hover_text(
                                "Remember this arrangement — the tabs, the charts on each, the \
                                 panels, the timezone and the window — as what quantick opens on",
                            )
                            .clicked()
                        {
                            self.save_workspace("menu");
                            ui.close_menu();
                        }
                        // Enabled only when there is something on disk to go
                        // back to: an entry that would forget nothing is a
                        // question the trader should not have to answer by
                        // clicking it.
                        if ui
                            .add_enabled(
                                self.workspace_saved,
                                egui::Button::new("Reset startup layout"),
                            )
                            .on_hover_text(
                                "Forget the saved workspace; the next launch opens on the \
                                 configured default. The charts on screen are left alone.",
                            )
                            .on_disabled_hover_text(
                                "Nothing saved yet — quantick already opens on the configured \
                                 default",
                            )
                            .clicked()
                        {
                            self.forget_workspace();
                            ui.close_menu();
                        }
                        ui.separator();
                        // Bookmarks. Named apart from the two entries above on
                        // purpose: those govern what the app *opens on*, these
                        // are places to come back to. The wording carries the
                        // distinction so the menu does not need a paragraph.
                        if ui
                            .button("Save as…")
                            .on_hover_text(
                                "Keep this arrangement under a name you can reopen later. It \
                                 does not change what quantick opens on.",
                            )
                            .clicked()
                        {
                            self.workspace_name_entry = Some(String::new());
                            ui.close_menu();
                        }
                        let mut open: Option<String> = None;
                        let mut delete: Option<String> = None;
                        ui.add_enabled_ui(!self.bookmarks.is_empty(), |ui| {
                            ui.menu_button("Open", |ui| {
                                for entry in &self.bookmarks {
                                    let tabs = entry.tabs.len();
                                    if ui
                                        .button(&entry.name)
                                        .on_hover_text(format!(
                                            "{tabs} chart {} — replaces what is on screen",
                                            if tabs == 1 { "tab" } else { "tabs" }
                                        ))
                                        .clicked()
                                    {
                                        open = Some(entry.name.clone());
                                        ui.close_menu();
                                    }
                                }
                            })
                            .response
                            .on_disabled_hover_text("Nothing saved under a name yet");
                            ui.menu_button("Delete", |ui| {
                                for entry in &self.bookmarks {
                                    if ui.button(&entry.name).clicked() {
                                        delete = Some(entry.name.clone());
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        if let Some(name) = open {
                            self.open_named_workspace(&name);
                        }
                        if let Some(name) = delete {
                            self.delete_named_workspace(&name);
                        }
                        ui.separator();
                        if ui
                            .checkbox(&mut self.save_on_exit, "Save on exit")
                            .on_hover_text(
                                "Keep the arrangement automatically when the window closes. Off, \
                                 only Save workspace changes what quantick opens on.",
                            )
                            .changed()
                        {
                            // The setting lives in the file it governs, so
                            // switching it has to reach the disk now — not at
                            // the next exit, which is exactly the exit it may
                            // have just switched off.
                            self.save_workspace("save_on_exit_toggled");
                        }
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
                self.toast = Some(Toast {
                    message: "Drawing deleted.".into(),
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
        // The escape stack: rail drag → paper interaction → pending
        // confirmation → draft → selection → Pointer, one layer per press.
        // Paper trading's armed placement / grabbed line reads Escape here,
        // in the single stack — what keeps one press from firing two
        // cancels at once.
        if keys.escape {
            if self.toolrail.drag_active() {
                // The rail consumes this Esc to abort its dock drag.
            } else if self.active_tab_mut().paper.cancel_interaction() {
                // An armed order placement or a grabbed order line was
                // dropped; nothing else loses state on this press. Only the
                // active tab can have one in flight — a background tab has
                // no pointer over it to arm or grab with.
            } else if self.drawing_delete_confirm {
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
    fn draw_toast(&mut self, ctx: &egui::Context, now: Instant) {
        // Expire first, so the borrow taken below is only ever of a toast that
        // is still on screen.
        if self.toast.as_ref().is_some_and(|toast| {
            now.saturating_duration_since(toast.shown_at) >= Duration::from_millis(TOAST_UNDO_MS)
        }) {
            self.toast = None;
        }
        let Some(toast) = &self.toast else {
            return;
        };
        // Borrowed, never cloned: the toast is painted on every frame of its
        // eight seconds, and an owned message copied per frame would be ~500
        // allocations for a string that never changes.
        let message: &str = &toast.message;
        let offers_undo = toast.offers_undo;
        let mut undo_clicked = false;
        #[cfg(test)]
        let mut undo_rect = None;
        egui::Area::new(egui::Id::new("toast"))
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
            self.toast = None;
        }
    }

    /// The inspector's title bar, shared by both hosts: grip + title, then
    /// the view controls (hide, pin, close) as icon buttons. In the floating
    /// host the whole bar is the drag surface — the body never is, so a
    /// slider drag can never move the window; double-click re-runs the
    /// automatic placement.
    fn draw_inspector_title_bar(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        floating: bool,
    ) -> InspectorActions {
        let mut actions = InspectorActions::default();
        let drawing = &self.focused_pane().drawings.items()[index];
        let hidden = drawing.hidden;
        // The band belongs in the title, because that is where "which of
        // these two trend lines am I editing" is actually asked. Nothing is
        // added on the price band: it is where drawings have always lived,
        // and a suffix on every object would be noise.
        let title = match self.focused_pane().band_label(drawing).chip() {
            Some(band) => format!("{} · {band}", drawing.tool.settings_title()),
            None => drawing.tool.settings_title().to_owned(),
        };
        let sense = if floating {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        };
        let (bar_rect, bar) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), INSPECTOR_TITLE_HEIGHT_PX),
            sense,
        );
        if ui.is_rect_visible(bar_rect) {
            let painter = ui.painter();
            if floating {
                painter.text(
                    egui::pos2(
                        bar_rect.left() + INSPECTOR_TITLE_PAD_X_PX,
                        bar_rect.center().y,
                    ),
                    egui::Align2::LEFT_CENTER,
                    icons::DOTS_SIX_VERTICAL,
                    egui::FontId::proportional(INSPECTOR_TITLE_GRIP_GLYPH_PX),
                    theme::TEXT_FAINT,
                );
            }
            let title_x = if floating {
                INSPECTOR_TITLE_TEXT_X_PX
            } else {
                INSPECTOR_TITLE_PAD_X_PX
            };
            painter.text(
                egui::pos2(bar_rect.left() + title_x, bar_rect.center().y),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(INSPECTOR_TITLE_TEXT_PX),
                theme::TEXT_PRIMARY,
            );
        }
        // The controls are registered after the bar, so they win its pointer.
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = IconButton::new(icons::X, TOOLBAR_ICON)
                    .hover_text("Close - keeps the drawing, clears the selection")
                    .show(ui);
                if close.clicked() {
                    actions.close = true;
                }
                let pin_hover = if self.inspector_pinned {
                    "Unpin - float the inspector over the chart"
                } else {
                    "Pin - dock the inspector at the side of the chart"
                };
                let pin = IconButton::new(icons::PUSH_PIN, TOOLBAR_ICON)
                    .active(self.inspector_pinned)
                    .hover_text(pin_hover)
                    .show(ui);
                #[cfg(test)]
                {
                    self.inspector_pin_rect = Some(pin.rect);
                }
                if pin.clicked() {
                    actions.toggle_pin = true;
                }
                let eye_icon = if hidden { icons::EYE_SLASH } else { icons::EYE };
                let eye_hover = if hidden {
                    "Show this drawing again"
                } else {
                    "Hide this drawing - the inspector keeps the way back"
                };
                let eye = IconButton::new(eye_icon, TOOLBAR_ICON)
                    .active(hidden)
                    .hover_text(eye_hover)
                    .show(ui);
                if eye.clicked() {
                    actions.toggle_hidden = true;
                }
            });
        });
        if floating {
            let bar = bar.on_hover_text("Drag to move · double-click to reposition automatically");
            if bar.double_clicked() {
                // The reset path: back to automatic placement.
                self.inspector_moved = false;
                self.inspector_pos = self.inspector_target_position(ui.ctx(), index);
            } else if bar.dragged() {
                self.inspector_moved = true;
                let position = self.inspector_pos.or_else(|| {
                    ui.ctx()
                        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
                        .map(|rect| rect.min)
                });
                if let Some(position) = position {
                    self.inspector_pos = Some(position + bar.drag_delta());
                }
            }
        }
        ui.separator();
        actions
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
        let shareable = drawing.shareable();
        let mut shared = drawing.scope == drawings::DrawingScope::AllCharts;
        let show_confirm = self.drawing_delete_confirm && locked;

        // The always-visible textual actions (UX spec: never glyph-only,
        // never behind a scroll). Identity and the view controls live in the
        // host's title bar, not here.
        let intent = drawings::action_bar::draw(ui, locked);
        actions.toggle_lock |= intent.toggle_lock;
        actions.delete |= intent.delete;

        if locked {
            ui.label(
                egui::RichText::new(
                    "Locked - protected from accidental moves. Style stays editable.",
                )
                .small()
                .color(theme::TEXT_SUPPORT),
            );
        }
        if hidden {
            ui.label(
                egui::RichText::new("Hidden - Show brings it back.")
                    .small()
                    .color(theme::TEXT_SUPPORT),
            );
        }
        // Where the object appears. Always visible, never behind a tab.
        //
        // It used to live on the Coordinates tab, because sharing is a
        // statement about the anchors — which is the implementer's mental
        // model, not the trader's. Nobody hunting for "also show this on the
        // other chart" opens a tab called Coordinates, and that is not even
        // the tab the panel opens on. Reported as unfindable, and it was.
        ui.separator();
        let sharing = ui.add_enabled(
            shareable,
            egui::Checkbox::new(&mut shared, "Show on all charts"),
        );
        if sharing.changed()
            && let Some(drawing) = self.focused_pane_mut().drawings.selected_mut()
        {
            drawing.scope = if shared {
                drawings::DrawingScope::AllCharts
            } else {
                drawings::DrawingScope::ThisChart
            };
            actions.edited = true;
        }
        // A disabled control with no reason reads as a bug.
        let sharing_hint = if shareable {
            "The other chart of this tab draws it at the same moment in market time"
        } else {
            "This drawing has an anchor past the newest bar, so there is no market time to place              it by on another chart"
        };
        sharing.on_hover_text(sharing_hint);
        if !shareable {
            ui.label(
                egui::RichText::new(sharing_hint)
                    .small()
                    .color(theme::TEXT_SUPPORT),
            );
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
                // Capability-driven, like the fill slider below: a tool with
                // no stroke has no line width, and the repo's rule is that an
                // unsupported property is *absent*, not present and inert.
                // Caught by the visual pass — the text note's Style tab was
                // offering a slider that moved nothing.
                if tool.supports_stroke_width() {
                    actions.edited |= ui
                        .add(
                            egui::Slider::new(
                                &mut drawing.style.width_px,
                                MIN_DRAWING_WIDTH_PX..=MAX_DRAWING_WIDTH_PX,
                            )
                            .text("line width (px)"),
                        )
                        .changed();
                }
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

                // Stop asking for the same look every single time. Every tool
                // has a Style tab, so every tool gets this — the named-preset
                // editor only ever existed on the Fib tab, which left fifteen
                // tools with no way to remember anything.
                //
                // New objects only: a default that repainted the marks already
                // on the chart would be a bulk edit nobody asked for.
                ui.separator();
                ui.label(egui::RichText::new("Default for new drawings").small());
                ui.horizontal(|ui| {
                    let style = drawing.style;
                    if ui
                        .button("This tool")
                        .on_hover_text(format!(
                            "New {} objects open with this colour, width and fill",
                            tool.name().to_lowercase()
                        ))
                        .clicked()
                    {
                        drawing_presets.set_default_style(tool.id(), Some(style));
                        actions.saved_default = Some(SavedDefault::OneTool);
                    }
                    if ui
                        .button("All tools")
                        .on_hover_text("Every new drawing opens with this colour, width and fill")
                        .clicked()
                    {
                        for other in drawings::DRAWING_TOOLS {
                            drawing_presets.set_default_style(other.id(), Some(style));
                        }
                        actions.saved_default = Some(SavedDefault::EveryTool);
                    }
                    if drawing_presets.default_style(tool.id()).is_some()
                        && ui
                            .button("Forget")
                            .on_hover_text(format!(
                                "New {} objects go back to the built-in look",
                                tool.name().to_lowercase()
                            ))
                            .clicked()
                    {
                        drawing_presets.set_default_style(tool.id(), None);
                        actions.saved_default = Some(SavedDefault::Forgotten);
                    }
                });
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
            // The user has expressed a preference: the auto-pin width rule
            // stops firing for the rest of the session.
            self.inspector_pin_touched = true;
            if !self.inspector_pinned {
                // Unpinning re-opens the floating window. The pinned host
                // has been claiming the selection each frame, so treat it
                // as fresh again — otherwise automatic placement never runs
                // and the window falls back to the fixed default corner.
                self.inspector_last_selection = None;
                self.inspector_settle_frame = true;
            }
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
                self.toast = Some(Toast {
                    message: "Drawing deleted.".into(),
                    shown_at: now,
                    offers_undo: true,
                });
            }
        }
        if actions.close {
            self.focused_pane_mut().drawings.select(None);
            self.drawing_delete_confirm = false;
        }
        if let Some(saved) = actions.saved_default {
            // Nothing to undo: this changed a preference, not the chart.
            self.toast = Some(Toast {
                message: saved.message().into(),
                shown_at: now,
                offers_undo: false,
            });
        }
    }

    /// Where a freshly opened floating inspector should sit: the farthest
    /// chart corner that clears the object, bottom-left preferred, falling
    /// back to beside-the-object only when no corner is free — see
    /// [`inspector_placement`]. The chart pane already excludes both axes and
    /// the live lane, so the popup can never cover them or leave the view.
    fn inspector_target_position(&self, ctx: &egui::Context, index: usize) -> Option<egui::Pos2> {
        let chart = self.focused_pane().last_chart_area?;
        let bbox = self.drawing_bbox_on_screen(chart, index)?;
        Some(inspector_placement(chart, bbox, self.inspector_size(ctx)))
    }

    /// How big the floating inspector is, best answer available: the size it
    /// was last drawn at, then egui's area memory, then the assumed default.
    fn inspector_size(&self, ctx: &egui::Context) -> egui::Vec2 {
        self.inspector_size
            .or_else(|| {
                ctx.memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
                    .map(|rect| rect.size())
            })
            .unwrap_or(egui::vec2(
                INSPECTOR_DEFAULT_WIDTH_PX,
                INSPECTOR_FALLBACK_HEIGHT_PX,
            ))
    }

    /// The selected object's screen bounding box, expanded by the anchor
    /// radius — the rectangle the inspector must not cover. Projected on the
    /// focused pane, which is where the selection lives.
    fn drawing_bbox_on_screen(&self, chart: egui::Rect, index: usize) -> Option<egui::Rect> {
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
        Some(bbox.expand(DRAWING_ANCHOR_RADIUS_PX))
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
        self.inspector_last_selection = Some(index);
        let mut actions = InspectorActions::default();
        egui::SidePanel::right("drawing_inspector_panel")
            .resizable(true)
            .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
            .width_range(INSPECTOR_MIN_WIDTH_PX..=INSPECTOR_MAX_WIDTH_PX)
            .show(ctx, |ui| {
                actions = self.draw_inspector_title_bar(ui, index, false);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let body = self.drawing_inspector_body(ui, index);
                    actions.merge(body);
                });
            });
        self.apply_inspector_actions(ctx, actions, index, before, now);
    }

    /// The selected object's floating inspector. Non-modal by contract: it
    /// never captures the whole canvas — but it is opaque to the pointer, so
    /// a press on it never falls through to the chart. Opens beside the
    /// selection; once the user drags the title bar, the manual position
    /// wins for the rest of the session (selection changes never snap it
    /// back; the only automatic move is the re-clamp when the pane shrinks).
    fn draw_drawing_inspector(&mut self, ctx: &egui::Context, now: Instant) {
        if self.inspector_pinned {
            // The pinned panel already drew (and cleaned up) this frame.
            return;
        }
        let Some((index, before)) = self.inspector_selection() else {
            return;
        };
        if std::mem::take(&mut self.inspector_settle_frame) {
            // The unpin happened this frame: the side panel still occupies
            // this frame's layout and the drawing projects against the
            // pinned-era chart. Wait one frame and place against the
            // settled geometry.
            return;
        }
        let selection_changed = self.inspector_last_selection != Some(index);
        // The auto-pin (§4.2): a fresh selection on a chart too narrow for a
        // floating window opens pinned instead — decided here because this
        // host is the one that would otherwise claim the selection. Stops
        // firing once the user touches the pin.
        if selection_changed
            && !self.inspector_pin_touched
            && self
                .focused_pane()
                .last_chart_area
                .is_some_and(|chart| chart.width() < INSPECTOR_AUTO_PIN_CHART_WIDTH_PX)
        {
            self.inspector_pinned = true;
            // The pinned panel draws from the next frame on.
            return;
        }
        self.inspector_last_selection = Some(index);
        // Automatic placement only while the window is untouched.
        if selection_changed
            && !self.inspector_moved
            && let Some(position) = self.inspector_target_position(ctx, index)
        {
            self.inspector_pos = Some(position);
        }
        // Repair, never override: a position that no longer fits the chart
        // pane is clamped back in, and `inspector_moved` survives.
        if let (Some(position), Some(chart)) =
            (self.inspector_pos, self.focused_pane().last_chart_area)
        {
            let clamped = clamp_into_chart(position, self.inspector_size(ctx), chart);
            if clamped != position {
                self.inspector_pos = Some(clamped);
            }
        }
        // The level editor earns the wider default the spec reserves for it.
        let default_width = if before.tool.extra_tab().is_some() {
            INSPECTOR_LEVELS_WIDTH_PX
        } else {
            INSPECTOR_DEFAULT_WIDTH_PX
        };
        // Bounded by the window and scrolled inside it. A tool's panel can be
        // taller than the screen — the Fib level editor is — and an unbounded
        // window simply gets cut at the edge with no way to reach the rest.
        // Rows a trader cannot reach read as rows that do not exist, and the
        // control that was out of reach here was the Fib's own "extend", the
        // one that decides whether its targets project forward at all.
        let max_height = (ctx.screen_rect().height()
            - self
                .focused_pane()
                .last_chart_area
                .map_or(0.0, |chart| chart.top())
            - 2.0 * INSPECTOR_OBJECT_GAP_PX)
            .max(INSPECTOR_FALLBACK_HEIGHT_PX);
        let mut window = egui::Window::new(before.tool.settings_title())
            .id(egui::Id::new("drawing_inspector"))
            .title_bar(false)
            .default_pos(DRAWING_INSPECTOR_DEFAULT_POSITION)
            .default_width(default_width)
            .min_width(INSPECTOR_MIN_WIDTH_PX)
            .max_width(INSPECTOR_MAX_WIDTH_PX)
            .max_height(max_height)
            .movable(false)
            .interactable(true)
            .resizable(true);
        if let Some(position) = self.inspector_pos {
            window = window.current_pos(position);
        }
        let response = window.show(ctx, |ui| {
            let mut actions = self.draw_inspector_title_bar(ui, index, true);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    actions.merge(self.drawing_inspector_body(ui, index));
                });
            actions
        });
        self.inspector_size = response
            .as_ref()
            .map(|response| response.response.rect.size());
        let actions = response
            .and_then(|response| response.inner)
            .unwrap_or_default();
        self.apply_inspector_actions(ctx, actions, index, before, now);
    }

    /// Where the object manager opens: one gap inboard of the rail's inner
    /// edge, aligned with the rail's leading end, clamped into the chart —
    /// beside the button that opened it in all four docks.
    fn manager_target_position(&self, ctx: &egui::Context) -> Option<egui::Pos2> {
        let chart = self.focused_pane().last_chart_area?;
        let size = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_manager")))
            .map_or(
                egui::vec2(INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX),
                |rect| rect.size(),
            );
        let gap = DRAWING_MANAGER_GAP_PX;
        let position = match self.toolrail.dock() {
            ToolboxDock::Left | ToolboxDock::Top => {
                egui::pos2(chart.left() + gap, chart.top() + gap)
            }
            ToolboxDock::Right => egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
            ToolboxDock::Bottom => egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
        };
        Some(clamp_into_chart(position, size, chart))
    }

    /// The object manager: a non-modal list of every drawing with the named
    /// per-object actions. It sends the same store commands as the inspector
    /// and the keyboard — nothing here re-implements lock or delete rules.
    fn draw_drawing_manager(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.drawing_manager_open {
            self.drawing_manager_was_open = false;
            return;
        }
        let just_opened = !self.drawing_manager_was_open;
        self.drawing_manager_was_open = true;
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
        let mut delete_all = false;
        let mut window = egui::Window::new("Drawn objects")
            .id(egui::Id::new("drawing_manager"))
            .open(&mut open)
            .default_pos(DRAWING_MANAGER_DEFAULT_POSITION)
            .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
            .collapsible(false)
            // Resizable, with the list scrolling below (audit M13): thirty
            // objects used to grow the window past the screen and put the
            // footer out of reach.
            .resizable(true);
        if just_opened && let Some(position) = self.manager_target_position(ctx) {
            window = window.current_pos(position);
        }
        window.show(ctx, |ui| {
            let count = self.focused_pane().drawings.items().len();
            if count == 0 {
                ui.label("No drawings yet.");
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Walked in reverse: the manager lists top-most first, the
                    // same order hit-testing resolves overlap.
                    for index in (0..count).rev() {
                        let drawing = &self.focused_pane().drawings.items()[index];
                        let selected = self.focused_pane().drawings.selected() == Some(index);
                        let locked = drawing.locked;
                        let hidden = drawing.hidden;
                        let shared = drawing.scope == drawings::DrawingScope::AllCharts;
                        let name = drawing.tool.name();
                        // Read out with the rest of the row's facts, so the
                        // row closure holds no borrow of the pane.
                        let band = self.focused_pane().band_label(drawing);
                        let parked = matches!(band, crate::pane::BandLabel::Parked(_));
                        let band_chip = band.chip();
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
                            // Which band an object is on, for the objects that
                            // are not on the candles. A parked one — its
                            // indicator no longer on the chart — is listed in
                            // amber: it still exists, it simply has nowhere to
                            // paint, and it comes back when that indicator
                            // does. Deleting it stays the trader's call.
                            if let Some(chip) = band_chip {
                                let text = egui::RichText::new(chip).small();
                                if parked {
                                    ui.label(text.color(theme::AMBER)).on_hover_text(
                                        "The indicator this was drawn on is not on the chart. \
                                         Add it back and the object returns at the same value.",
                                    );
                                } else {
                                    ui.label(text);
                                }
                            }
                            if shared {
                                // Which marks are global is a question the
                                // list must answer at a glance (Marina, §D7).
                                ui.label(egui::RichText::new("all charts").small())
                                    .on_hover_text(
                                        "Also drawn on the other chart of this tab, at the same \
                                         moment in market time",
                                    );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
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
                                    let lock =
                                        ui.small_button(if locked { "Unlock" } else { "Lock" });
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
                                },
                            );
                        });
                    }
                });
            ui.separator();
            if self.drawing_manager_confirm_delete_all && count > 0 {
                // The count-bearing gate (audit M7): deleting everything is
                // one command, but never one stray click — and locked
                // objects go too, which the question says out loud.
                ui.horizontal(|ui| {
                    ui.label(format!("Delete all {count} drawing(s), locked included?"));
                    if ui.button("Delete all").clicked() {
                        delete_all = true;
                        self.drawing_manager_confirm_delete_all = false;
                    }
                    if ui.button("Keep").clicked() {
                        self.drawing_manager_confirm_delete_all = false;
                    }
                });
            } else {
                self.drawing_manager_confirm_delete_all = false;
                ui.horizontal(|ui| {
                    if ui.button("Show all").clicked() {
                        show_all = true;
                    }
                    if ui.button("Unlock all").clicked() {
                        unlock_all = true;
                    }
                    if count > 0 && ui.button("Delete all…").clicked() {
                        self.drawing_manager_confirm_delete_all = true;
                    }
                });
            }
        });
        self.drawing_manager_open = open;
        if delete_all {
            let deleted = self.focused_pane_mut().drawings.delete_all();
            if deleted > 0 {
                self.toast = Some(Toast {
                    message: "All drawings deleted.".into(),
                    shown_at: now,
                    offers_undo: true,
                });
            }
        }
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

impl QuantickApp {
    /// The `QUANTICK_DRAWINGS_DEMO` hook: one of every registered drawing on
    /// the flow pane, spread across the visible bars, the last one selected
    /// so the inspector is on screen too.
    ///
    /// Waits for bars: anchors are placed on real slots, so every anchor
    /// carries a real market time and the shared-drawing path is exercised
    /// rather than faked. Consumed once, whether or not it placed anything on
    /// this attempt — an env var is a request for this run, and it must never
    /// keep re-placing objects the user then deletes.
    fn apply_drawing_demo(&mut self) {
        if !self.pending_drawing_demo {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        let slots = pane.slots();
        // Enough bars for every tool to get its own stretch of chart.
        if slots < 8 * drawings::DRAWING_TOOLS.len() {
            return;
        }
        self.pending_drawing_demo = false;
        let share = std::env::var("QUANTICK_DRAWINGS_DEMO_SHARED").is_ok_and(|v| v == "1");
        // `=bands` adds a set on every indicator pane; `=1` stays exactly
        // what it was, so every screenshot taken of the old hook still is.
        let bands = std::env::var("QUANTICK_DRAWINGS_DEMO").is_ok_and(|v| v == "bands");
        if share {
            // A shared drawing has nothing to be shared *with* on a single
            // pane, so the hook that asks for one opens the split too — the
            // surface under test is the projection onto the other chart.
            self.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        // Anchored inside the window the chart actually opens on, not at slot
        // zero: a demo whose objects sit 300 bars off the left edge shows a
        // screenshot of an empty chart, which is exactly the evidence this
        // hook exists to produce. `visible` is the newest stretch, and the
        // objects are laid across it left to right in registry order.
        let visible = DEMO_VISIBLE_SLOTS.min(slots);
        let first = slots - visible;
        // Two different spacings on purpose. `stride` walks the *starts*
        // apart so the objects are distinguishable; `span` sets how far a
        // multi-anchor object reaches, and it has to be wide or a rectangle
        // lands two bars across and photographs as a sliver. The objects
        // overlap, which is fine — a QA screen wants every tool legible, not
        // a tidy row.
        let stride = (visible / drawings::DRAWING_TOOLS.len()).max(1);
        let span = (visible / DEMO_SPANS_PER_WINDOW).max(2);
        let base = pane
            .closed_bar(slots.saturating_sub(1))
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
            for anchor in 0..tool.required_points() {
                let slot = (first + index * stride + anchor * span).min(slots.saturating_sub(1));
                let point = drawings::ChartPoint::at_time(
                    slot as f32 + 0.5,
                    base * (1.0 + f64::from(anchor as i32) * 0.001
                        - f64::from(index as i32 % 3) * 0.002),
                    pane.slot_open_time(slot),
                );
                let completed =
                    pane.drawings
                        .place_with(tool, &drawings::DrawingBand::Price, point, |tool| {
                            drawings::NewDrawing {
                                style: drawings::DrawingStyle::default(),
                                payload: tool.default_payload(),
                            }
                        });
                // Placement selects what it completed, so this reaches the
                // object just made — no separate index bookkeeping.
                if completed
                    && share
                    && let Some(drawing) = pane.drawings.selected_mut()
                    && drawing.shareable()
                {
                    drawing.scope = drawings::DrawingScope::AllCharts;
                }
            }
        }
        if bands {
            Self::seed_band_demo(pane, first, visible, slots);
        }
    }

    /// The `bands` half of the demo hook: on every indicator pane, a level on
    /// the band's own value and a diagonal across it.
    ///
    /// Two objects, not seventeen: a pane is a fifth of the chart's height,
    /// and a screenshot of every tool stacked in one would prove nothing
    /// about the projection it exists to check. The level is placed *at a
    /// value the series actually holds*, so a drawing that has drifted off
    /// its curve is visible at a glance.
    fn seed_band_demo(pane: &mut ChartPane, first: usize, visible: usize, slots: usize) {
        let level_slot = (first + visible / 2).min(slots.saturating_sub(1));
        let (left, right) = (
            (first + visible / 8).min(slots.saturating_sub(1)),
            (first + visible * 3 / 4).min(slots.saturating_sub(1)),
        );
        for (band, value) in pane.indicator_band_samples(level_slot) {
            for tool in drawings::DRAWING_TOOLS {
                let anchors: &[(usize, f64)] = match tool.id() {
                    "horizontal-line" => &[(0, 0.0)],
                    "trend-line" => &[(1, 0.0), (2, 0.0)],
                    _ => continue,
                };
                for (which, _) in anchors {
                    let (slot, value) = match which {
                        // The level sits on the sampled value itself.
                        0 => (level_slot, value),
                        // The diagonal spans the window around it, so its
                        // ends are inside the band without being on the curve.
                        1 => (left, value * 0.5),
                        _ => (right, value * 1.5),
                    };
                    let point = drawings::ChartPoint::at_time(
                        slot as f32 + 0.5,
                        value,
                        pane.slot_open_time(slot),
                    );
                    pane.drawings
                        .place_with(tool, &band, point, |tool| drawings::NewDrawing {
                            style: drawings::DrawingStyle::default(),
                            payload: tool.default_payload(),
                        });
                }
            }
        }
    }
}

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
        self.apply_drawing_demo();
        self.maybe_emit_summary(now);
        self.maintain_workspace(ctx);

        let bg = pane::background_color(&self.style);
        // Rail shortcuts first: Esc/1/2 must be read before any widget can
        // claim the keyboard this frame.
        self.toolrail.handle_keys(ctx);
        self.handle_tab_keys(ctx);
        self.handle_drawing_keys(ctx, now);
        // Chrome panels claim their zones outside-in (§5): menu and toolbar
        // on top, the status line at the very bottom with the replay
        // transport directly above it, then the edge-docked drawing rail and
        // the right dock. The chart keeps whatever remains.
        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        self.draw_source_picker(ctx);
        self.draw_workspace_name_box(ctx);
        self.draw_indicator_settings(ctx);
        self.draw_indicator_legends(ctx);
        self.poll_script_files();
        self.maintain_indicator_state();
        self.maintain_chart_layers();
        let status = self.status_model();
        let status_response = statusbar::draw(ctx, &status, &mut self.tz);
        if status_response.open_trading_tab {
            self.dock.open_tab(DockTab::Trading);
        }
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
                tz,
                ..
            } = self;
            // The Trading tab speaks for the market on screen: one tab, one
            // simulator, and the dock reads the active tab's — exactly like
            // the tape and the session panel beside it.
            let Tab {
                flow_pane,
                replay,
                paper,
                ..
            } = &mut tabs[*active_tab];
            let orderflow = flow_pane
                .orderflow
                .as_mut()
                .expect("the flow pane is built with a tape and never drops it");
            dock.draw(
                ctx,
                &mut DockEnv {
                    orderflow,
                    replay_view,
                    replay: replay.as_ref(),
                    paper,
                    tz: *tz,
                },
            )
        };
        if dock_response.restart_book_capture {
            self.active_tab_mut().restart_book_capture();
        }
        if let Some(action) = dock_response.replay_action {
            self.apply_replay_action(action);
        }
        // The ledger's jump-to-trade: center the flow pane on the round
        // trip's midpoint, the object manager's own "select and centre".
        if let Some((opened, closed)) = dock_response.navigate_to_trade {
            let pane = &mut self.active_tab_mut().flow_pane;
            if let (Some(entry), Some(exit), Some(area)) = (
                pane.slot_at_time(opened),
                pane.slot_at_time(closed),
                pane.last_chart_area,
            ) {
                let slots = pane.slots();
                let mid = (entry + exit) as f32 / 2.0;
                pane.viewport.center_on_bar(mid, area.width(), slots);
            }
        }
        if dock_response.pick_trades_dir {
            self.open_trades_dir_picker();
        }
        self.poll_trades_dir_picker();
        // The pinned inspector is chrome: declared before the central canvas
        // so the chart pays its width, exactly like the dock.
        self.draw_drawing_inspector_panel(ctx, now);
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        let (tab, config) = self.active_with_config();
        let mut cleared = tab.maybe_switch_feed(config);
        // Both deferrals settle here, a frame after the click that armed
        // them, so the frame carrying the change paints its overlay first.
        let Self { tabs, config, .. } = self;
        for tab in tabs.iter_mut() {
            tab.apply_pending_layout(config);
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
        // The layer menu offers what this source can produce; resolved once
        // here rather than per pane, per entry, inside the canvas.
        let capabilities = self.active_tab().capabilities(&self.config);
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
                        layer_actions,
                        ..
                    } = self;
                    let mut chrome = CanvasChrome {
                        toolrail,
                        presets: drawing_presets,
                        style,
                        tz: *tz,
                        capabilities,
                        layers: layer_actions,
                    };
                    tabs[*active_tab].draw_canvas(ui, area, &mut chrome);
                }
                // The grid and the indicator state belong to the window, not
                // to the pane whose menu switched them.
                self.apply_layer_actions();
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
        self.draw_toast(ctx, now);
        // Both are window chrome reading the active tab, like the notice card
        // and the transport strip: they speak for one market at a time.
        self.active_tab_mut().paper.draw_report_window(ctx);
        self.active_tab_mut().paper.draw_toast(ctx, now);
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
            // MetaTrader narrows its capabilities when the bridge says hello,
            // after the pane may already have asked and been told there was
            // nothing held. Watching the edge is what asks again once the
            // answer can be a real one.
            tab.poll_ohlcv_capability(config);
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
        // Focus-gated like `handle_drawing_keys` (audit MINOR-13): typing in
        // the source picker's field with Ctrl held must never close the tab
        // under it — closing is instant and currently irreversible.
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
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
        if self.source_picker.is_none() {
            return;
        }
        // The markets tabs are showing: the picker greys out removing one of
        // those, because a tab left on a symbol the catalog no longer offers
        // gets silently retargeted by the next SOURCE correction.
        let open_symbols: Vec<(String, String)> = self
            .tabs
            .iter()
            .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
            .collect();
        let outcome = {
            let Self {
                source_picker,
                config,
                added_symbols,
                ..
            } = self;
            let picker = source_picker.as_mut().expect("checked above");
            picker.draw(ctx, config, added_symbols, &open_symbols)
        };
        match outcome {
            PickerOutcome::Open => {}
            PickerOutcome::Cancel => self.source_picker = None,
            PickerOutcome::Chosen(feed_id, symbol) => {
                self.source_picker = None;
                self.open_tab(feed_id, symbol, None);
            }
            PickerOutcome::Added { feed_id, symbol } => match self.add_symbol(&feed_id, &symbol) {
                Ok(()) => {
                    self.source_picker = None;
                    self.open_tab(feed_id, symbol, None);
                }
                // The dialog stays open carrying the reason: the user is one
                // keystroke from a symbol that does fit, and closing would
                // make the refusal look like a crash.
                Err(reason) => {
                    if let Some(picker) = self.source_picker.as_mut() {
                        picker.refuse(reason);
                    }
                }
            },
            PickerOutcome::Removed { feed_id, symbol } => self.remove_symbol(&feed_id, &symbol),
        }
    }

    /// Put `symbol` in feed `feed_id`'s catalog and remember it across
    /// restarts. Reports whether the catalog took it.
    ///
    /// The config file itself is never written: it is hand-written, comments
    /// and all, and a program that rewrote it would eat them. The addition
    /// lives in its own sidecar, which the next launch folds back in before
    /// the config is validated (see [`crate::symbols_file`]).
    fn add_symbol(&mut self, feed_id: &str, symbol: &str) -> Result<(), String> {
        // Against the *whole* config, on a copy. A symbol is not just a name
        // in a list: it takes part in every cross-check the config has, and
        // the MetaTrader port map is one where a single mapped symbol offered
        // by two feeds is a configuration the app refuses to load. Persisting
        // one of those would write a file that kills the next launch — and the
        // error would name the config, which is not the file that broke.
        let mut candidate = self.config.clone();
        if !candidate.add_symbol(feed_id, symbol) {
            return Err(format!(
                "{} already offers {symbol}",
                self.config.feed_name(feed_id)
            ));
        }
        candidate.validate()?;
        self.config = candidate;
        self.added_symbols.add(feed_id, symbol);
        if let Err(error) = symbols_file::save(&self.symbols_path, &self.added_symbols) {
            // The catalog took it for this session either way; what is lost is
            // the next launch, and the user is told which file did not take it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.symbols_path.display(),
                error = %error,
                action = "addition_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_ADDED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.symbols_path.display(),
            action = "open_in_new_tab",
            "a symbol was added from the source picker"
        );
        Ok(())
    }

    /// Take a user-added `symbol` back out of feed `feed_id`'s catalog.
    ///
    /// Only ever a catalog edit: a tab already showing that market keeps
    /// streaming it. The picker will not offer this for a market a tab is on,
    /// which is what stops the selection correction from retargeting it.
    fn remove_symbol(&mut self, feed_id: &str, symbol: &str) {
        if !self.config.remove_symbol(feed_id, symbol) {
            return;
        }
        self.added_symbols.remove(feed_id, symbol);
        if let Err(error) = symbols_file::save(&self.symbols_path, &self.added_symbols) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.symbols_path.display(),
                error = %error,
                action = "removal_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_REMOVED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.symbols_path.display(),
            action = "leave_open_tabs_alone",
            "a user-added symbol left the catalog"
        );
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

    use crate::config::{AppConfig, FeedCapabilities, FeedConfig, ProviderKind};
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

        let off = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
        assert!(off.live_strip.is_none());
        assert_eq!(off.chart.right(), off.price_gutter.left());

        let on = plot_split(
            area,
            crate::live_strip::LIVE_STRIP_WIDTH_PX,
            &[crate::indicators::PaneSizing::Auto; 0],
        );
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

        let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
        assert!(none.indicator_panes.is_empty());

        let one = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 1]);
        let pane = *one
            .indicator_panes
            .first()
            .expect("one visible pane claims one rect");
        assert!(
            one.chart.height() < none.chart.height(),
            "the band is paid for out of the candles' pixels"
        );
        assert_eq!(one.chart.bottom(), pane.rect.top(), "no gap, no overlap");
        assert_eq!(pane.rect.bottom(), none.chart.bottom());
        assert_eq!(one.chart.width(), none.chart.width());
        // The axes keep their column; the time strip is untouched.
        assert_eq!(one.price_gutter.x_range(), none.price_gutter.x_range());
        assert_eq!(one.time_strip, none.time_strip);

        let three = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 3]);
        assert_eq!(three.indicator_panes.len(), 3);
        assert!(three.chart.height() < one.chart.height());
    }

    /// The gutter is banded like the body it labels. Before it was, the whole
    /// column belonged to the candles: dragging the numbers beside a CVD pane
    /// stretched the *price* scale, and the pane — which had no axis at all —
    /// did not move.
    #[test]
    fn every_pane_owns_the_gutter_band_beside_it() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
        assert!(none.pane_gutters.is_empty());
        assert_eq!(
            none.price_gutter.bottom(),
            none.chart.bottom(),
            "with no pane the gutter is the candles', top to bottom"
        );

        let two = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 2]);
        assert_eq!(two.pane_gutters.len(), two.indicator_panes.len());
        assert_eq!(
            two.price_gutter.bottom(),
            two.chart.bottom(),
            "the candles' scale stops where the candles do"
        );
        for (pane, gutter) in two.indicator_panes.iter().zip(&two.pane_gutters) {
            assert_eq!(
                gutter.y_range(),
                pane.rect.y_range(),
                "band beside its pane"
            );
            assert_eq!(gutter.x_range(), two.price_gutter.x_range(), "one column");
        }
        // No pixel answers to two scales: the bands tile the gutter exactly.
        assert_eq!(two.price_gutter.bottom(), two.pane_gutters[0].top());
        assert_eq!(two.pane_gutters[0].bottom(), two.pane_gutters[1].top());

        // The strip pays out of the candles, not the gutter: the pane bands
        // keep the same column when the tape is shown.
        let with_strip = plot_split(
            area,
            crate::live_strip::LIVE_STRIP_WIDTH_PX,
            &[crate::indicators::PaneSizing::Auto; 2],
        );
        assert_eq!(with_strip.pane_gutters, two.pane_gutters);
    }

    /// A pane indicator with `values` as its single plot, delivered the way
    /// the worker delivers one.
    fn add_pane_indicator(app: &mut QuantickApp, title: &str, values: Vec<f64>) -> SlotId {
        let slot = app
            .active_tab_mut()
            .flow_pane
            .indicators
            .allocate_slot("test.indicator".to_owned());
        rebuild_pane_indicator(app, slot, title, values);
        slot
    }

    /// The same indicator, recomputed — an edited input, a hot reload, older
    /// trades re-cutting the series.
    fn rebuild_pane_indicator(app: &mut QuantickApp, slot: SlotId, title: &str, values: Vec<f64>) {
        app.active_tab_mut()
            .flow_pane
            .indicators
            .apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: quantick_indicators::IndicatorDescriptor {
                    title: title.to_owned(),
                    short_title: None,
                    overlay: false,
                    plots: vec![quantick_indicators::PlotSpec {
                        id: quantick_indicators::PlotId::new(0),
                        title: title.to_owned(),
                        style: quantick_indicators::PlotStyle::Line,
                        base_color: quantick_indicators::Rgba8::opaque(255, 255, 255),
                        width: 1.0,
                        offset: 0,
                        marker: None,
                    }],
                    inputs: Vec::new(),
                    fills: Vec::new(),
                },
                columns: vec![values],
                inputs: Vec::new(),
                stale: None,
            });
    }

    /// The `(lo, hi)` a pane is drawing with right now: its manual range if the
    /// user has taken control of the axis, else the fit the last frame made.
    fn pane_range(app: &QuantickApp, slot: SlotId) -> (f64, f64) {
        let view = app
            .active_tab()
            .flow_pane
            .indicators
            .all()
            .iter()
            .find(|view| view.slot == slot)
            .expect("the pane is still there");
        let auto = view.last_auto.expect("a frame fitted this pane");
        view.scale.resolve(auto)
    }

    /// Whether a pane is still auto-fitting its values.
    fn pane_is_auto(app: &QuantickApp, slot: SlotId) -> bool {
        app.active_tab()
            .flow_pane
            .indicators
            .all()
            .iter()
            .find(|view| view.slot == slot)
            .expect("the pane is still there")
            .scale
            .is_auto()
    }

    /// The gutter band beside pane `index`, computed the way the frame does.
    fn pane_gutter(app: &QuantickApp, index: usize) -> egui::Rect {
        let pane = &app.active_tab().flow_pane;
        let areas = plot_split(
            pane.last_plot_area.expect("a frame has been drawn"),
            pane.live_strip_width(),
            pane.indicators.pane_sizing(
                &mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES],
            ),
        );
        areas.pane_gutters[index]
    }

    /// The plot band of pane `index` — where its curve is drawn, as opposed to
    /// the gutter where its numbers are.
    fn pane_body(app: &QuantickApp, index: usize) -> egui::Rect {
        let pane = &app.active_tab().flow_pane;
        let areas = plot_split(
            pane.last_plot_area.expect("a frame has been drawn"),
            pane.live_strip_width(),
            pane.indicators.pane_sizing(
                &mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES],
            ),
        );
        areas.indicator_panes[index].rect
    }

    /// The bar the viewport has at its right edge — how far the chart is
    /// panned along time.
    fn right_edge(app: &QuantickApp) -> f32 {
        let pane = &app.active_tab().flow_pane;
        pane.viewport.right_edge_bar(pane.slots())
    }

    /// The pane band as the last drawn frame carved it.
    fn pane_slots(app: &QuantickApp) -> Vec<crate::indicators::PaneSlot> {
        let pane = &app.active_tab().flow_pane;
        plot_split(
            pane.last_plot_area.expect("a frame has been drawn"),
            pane.live_strip_width(),
            pane.indicators.pane_sizing(
                &mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES],
            ),
        )
        .indicator_panes
    }

    /// An app showing every pane it allows, drawn once at `size`.
    fn app_with_full_pane_band(
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
        let (mut app, cmd_rx) = app_with_history(200);
        for index in 0..crate::indicators::MAX_PANES {
            add_pane_indicator(
                &mut app,
                &format!("pane{index}"),
                (0..200).map(f64::from).collect(),
            );
        }
        run_frame_at(&mut app, ctx, size);
        (app, cmd_rx)
    }

    /// The whole point of collapsing: a strip is not a dead band. One click on
    /// it brings the curve back, and it must survive the frame after — the
    /// automatic rule is what collapsed the pane, so handing the pane back to
    /// it would undo the click immediately.
    /// Three panes in the smallest window the app allows: the state the user
    /// reported as unusable. Something must collapse — that is the point — and
    /// nothing that stays open may be below the readable floor.
    #[test]
    fn the_smallest_window_collapses_rather_than_squeezing_every_pane() {
        let ctx = egui::Context::default();
        let (app, _cmd_rx) = app_with_full_pane_band(&ctx, MIN_WINDOW);

        let panes = pane_slots(&app);
        assert_eq!(panes.len(), crate::indicators::MAX_PANES);
        assert!(
            panes.iter().any(|pane| pane.collapsed),
            "the smallest window cannot hold three readable panes: {panes:?}"
        );
        for pane in &panes {
            assert!(
                pane.collapsed || pane.rect.height() >= crate::indicators::MIN_PANE_HEIGHT_PX,
                "an expanded pane below the readable floor: {pane:?}"
            );
        }
    }

    /// The same band in a roomy window: nothing collapses, so the floor never
    /// costs a user with a big screen anything.
    #[test]
    fn a_roomy_window_draws_every_pane() {
        let ctx = egui::Context::default();
        let (app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);
        assert!(
            pane_slots(&app).iter().all(|pane| !pane.collapsed),
            "a tall window has room for all of them: {:?}",
            pane_slots(&app)
        );
    }

    /// The whole point of collapsing: a strip is not a dead band. One click on
    /// it brings the curve back, and it must survive the frame after — the
    /// automatic rule is what collapsed the pane, so handing the pane back to
    /// it would undo the click immediately.
    #[test]
    fn clicking_a_collapsed_strip_opens_it_and_it_stays_open() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, MIN_WINDOW);

        let collapsed = pane_slots(&app)
            .iter()
            .position(|pane| pane.collapsed)
            .expect("the smallest window collapses at least one pane");

        let strip = pane_slots(&app)[collapsed].rect;
        click_sized(&mut app, &ctx, MIN_WINDOW, strip.center());
        run_frame_at(&mut app, &ctx, MIN_WINDOW);
        assert!(
            !pane_slots(&app)[collapsed].collapsed,
            "one click opens the strip"
        );

        run_frame_at(&mut app, &ctx, MIN_WINDOW);
        assert!(
            !pane_slots(&app)[collapsed].collapsed,
            "and it is still open a frame later, not re-collapsed by the layout"
        );
    }

    /// Drag a pane's top edge and that pane resizes — the grammar the canvas
    /// split and the live lane already use, on the one band in the app that
    /// did not have it. Dragging up grows the pane into the chart.
    #[test]
    fn dragging_a_panes_top_edge_resizes_that_pane() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);

        let before = pane_slots(&app)[0].rect.height();
        let edge = pane_slots(&app)[0].rect.center_top();
        drag_sized(
            &mut app,
            &ctx,
            TEST_WINDOW,
            edge,
            edge - egui::vec2(0.0, 40.0),
        );
        run_frame_at(&mut app, &ctx, TEST_WINDOW);

        let after = pane_slots(&app)[0].rect.height();
        assert!(
            after > before,
            "dragging the edge up grows the pane: {before} -> {after}"
        );
    }

    /// The floor holds during a drag too: a divider stops rather than
    /// producing a pane too short to read. Anything else would hand the user a
    /// way to recreate exactly the state this branch exists to prevent.
    #[test]
    fn a_divider_cannot_be_dragged_past_the_readable_floor() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);

        let edge = pane_slots(&app)[0].rect.center_top();
        drag_sized(
            &mut app,
            &ctx,
            TEST_WINDOW,
            edge,
            edge + egui::vec2(0.0, 400.0),
        );
        run_frame_at(&mut app, &ctx, TEST_WINDOW);

        let pane = pane_slots(&app)[0];
        assert!(
            pane.collapsed || pane.rect.height() >= crate::indicators::MIN_PANE_HEIGHT_PX,
            "a drag cannot squeeze a pane below the floor: {pane:?}"
        );
    }

    /// Double click on a divider gives the pane back to the automatic layout —
    /// the escape every other axis in the app offers on the same gesture.
    #[test]
    fn double_clicking_a_divider_returns_the_pane_to_automatic() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);

        let automatic = pane_slots(&app)[0].rect.height();
        let edge = pane_slots(&app)[0].rect.center_top();
        drag_sized(
            &mut app,
            &ctx,
            TEST_WINDOW,
            edge,
            edge - egui::vec2(0.0, 60.0),
        );
        run_frame_at(&mut app, &ctx, TEST_WINDOW);
        assert!(
            (pane_slots(&app)[0].rect.height() - automatic).abs() > 1.0,
            "the drag took manual control"
        );

        let edge = pane_slots(&app)[0].rect.center_top();
        click_sized(&mut app, &ctx, TEST_WINDOW, edge);
        click_sized(&mut app, &ctx, TEST_WINDOW, edge);
        run_frame_at(&mut app, &ctx, TEST_WINDOW);
        assert!(
            (pane_slots(&app)[0].rect.height() - automatic).abs() < 1.0,
            "and a double click hands it back: {} vs {automatic}",
            pane_slots(&app)[0].rect.height()
        );
    }

    /// The other half of the disclosure. A control that only opens is half a
    /// control: a trader who wants the candles back must be able to put a pane
    /// away without deleting the indicator, and the value must survive it.
    #[test]
    fn the_disclosure_closes_a_pane_as_well_as_opening_it() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);
        assert!(
            pane_slots(&app).iter().all(|pane| !pane.collapsed),
            "the roomy window starts with every pane open"
        );

        let corner = crate::indicator_render::pane_disclosure_rect(pane_slots(&app)[0].rect, false);
        click_sized(&mut app, &ctx, TEST_WINDOW, corner.center());
        run_frame_at(&mut app, &ctx, TEST_WINDOW);
        assert!(
            pane_slots(&app)[0].collapsed,
            "clicking the open disclosure puts the pane away"
        );
        assert!(
            !pane_slots(&app)[1].collapsed,
            "and only that pane: the click was over one corner"
        );

        // Room is not what is keeping it shut, so it stays shut.
        run_frame_at(&mut app, &ctx, TEST_WINDOW);
        assert!(
            pane_slots(&app)[0].collapsed,
            "a hand-closed pane stays shut"
        );

        let strip = pane_slots(&app)[0].rect;
        click_sized(&mut app, &ctx, TEST_WINDOW, strip.center());
        run_frame_at(&mut app, &ctx, TEST_WINDOW);
        assert!(
            !pane_slots(&app)[0].collapsed,
            "and the same control brings it back"
        );
    }

    /// A pane body is a piece of the chart, so the chart's own gestures have
    /// to work on it: drag to move, and the pane's own axis for the vertical
    /// half. Before this the body answered nothing at all — the only way to
    /// move a pane's curve out of its own way was to travel to the gutter on
    /// the far side of the tape and drag it there.
    #[test]
    fn dragging_a_pane_body_pans_that_pane_and_the_shared_time_axis() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        let other = add_pane_indicator(&mut app, "delta", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx); // the frame that fits each pane and records it

        let body = pane_body(&app, 0);
        let (lo, hi) = pane_range(&app, flow);
        let edge_before = right_edge(&app);

        drag_chart(
            &mut app,
            &ctx,
            body.center(),
            body.center() + egui::vec2(40.0, 30.0),
        );

        let (panned_lo, panned_hi) = pane_range(&app, flow);
        assert!(
            ((panned_hi - panned_lo) - (hi - lo)).abs() < 1e-6,
            "a pan moves the window without resizing it: {lo}..{hi} -> {panned_lo}..{panned_hi}"
        );
        assert!(
            panned_lo > lo,
            "the candles' direction: pull the content down and the window climbs ({lo} -> {panned_lo})"
        );
        // Not the neighbour's resolved range: panning time changes what is
        // visible, so every pane still on auto legitimately refits. What must
        // not happen is the neighbour being taken off auto by a drag that was
        // never over it.
        assert!(
            pane_is_auto(&app, other),
            "one pane, one scale: the neighbour still fits its own values"
        );
        assert!(
            !pane_is_auto(&app, flow),
            "and the dragged one took control"
        );
        assert!(
            app.active_tab().flow_pane.price_view.is_auto(),
            "and the candles' own price scale is not a pane's to move"
        );
        assert!(
            (right_edge(&app) - edge_before).abs() > f32::EPSILON,
            "time is shared: the sideways half of the drag moved the chart"
        );
    }

    /// Time is moved once per drag, not once per pane. Three stacked panes
    /// answering the same sideways drag would pan the chart three times, and
    /// the bars would run away from the pointer.
    #[test]
    fn a_pane_drag_pans_time_once_however_many_panes_are_stacked() {
        let ctx = egui::Context::default();

        let mut travelled = Vec::new();
        for panes in [1_usize, 3] {
            let (mut app, _cmd_rx) = app_with_history(200);
            for index in 0..panes {
                add_pane_indicator(
                    &mut app,
                    &format!("pane{index}"),
                    (0..200).map(f64::from).collect(),
                );
            }
            run_frame(&mut app, &ctx);

            let body = pane_body(&app, 0);
            let before = right_edge(&app);
            drag_chart(
                &mut app,
                &ctx,
                body.center(),
                body.center() + egui::vec2(40.0, 0.0),
            );
            travelled.push(right_edge(&app) - before);
        }

        assert!(
            (travelled[0] - travelled[1]).abs() < 1e-4,
            "one pane and three must pan time by the same amount: {travelled:?}"
        );
        assert!(travelled[0].abs() > f32::EPSILON, "and it did pan");
    }

    /// Double click inside a pane hands its scale back to auto-fit — the same
    /// escape its gutter offers, so a trader who panned a pane by mistake gets
    /// out of it wherever the pointer happens to be.
    #[test]
    fn double_clicking_a_pane_body_returns_it_to_auto_fit() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);

        let body = pane_body(&app, 0);
        drag_chart(
            &mut app,
            &ctx,
            body.center(),
            body.center() + egui::vec2(0.0, 30.0),
        );
        assert!(!pane_is_auto(&app, flow), "the drag took manual control");

        click_chart(&mut app, &ctx, body.center());
        click_chart(&mut app, &ctx, body.center());
        run_frame(&mut app, &ctx);

        assert!(
            pane_is_auto(&app, flow),
            "and a double click gives it back to the values"
        );
    }

    /// The headline of this feature: a pane's numbers are its own axis. A drag
    /// there stretches that pane and nothing else — before the gutter was
    /// banded, the same pixels moved the *candles'* price scale, and the pane
    /// had no axis to grab at all.
    #[test]
    fn dragging_a_pane_axis_zooms_that_pane_and_nothing_else() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        let other = add_pane_indicator(&mut app, "delta", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx); // the frame that fits each pane and records it

        let gutter = pane_gutter(&app, 0);
        let (lo, hi) = pane_range(&app, flow);
        let untouched = pane_range(&app, other);
        drag_chart(
            &mut app,
            &ctx,
            gutter.center(),
            gutter.center() - egui::vec2(0.0, 60.0),
        );

        let (zoomed_lo, zoomed_hi) = pane_range(&app, flow);
        assert!(
            zoomed_hi - zoomed_lo < hi - lo,
            "drag up compresses the span: {lo}..{hi} -> {zoomed_lo}..{zoomed_hi}"
        );
        assert!(
            (f64::midpoint(zoomed_lo, zoomed_hi) - f64::midpoint(lo, hi)).abs() < 1e-6,
            "and stretches around the middle rather than sliding the pane"
        );
        assert_eq!(pane_range(&app, other), untouched, "one pane, one scale");
        assert!(
            app.active_tab().flow_pane.price_view.is_auto(),
            "the candles never felt it: their gutter ends where they do"
        );
    }

    /// The other half of the isolation: the candles' own gutter must not reach
    /// down into a pane. Both gestures exist on the same column of pixels, and
    /// only the band decides which scale they mean.
    #[test]
    fn dragging_the_price_gutter_leaves_every_pane_alone() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);

        let untouched = pane_range(&app, flow);
        let gutter = {
            let pane = &app.active_tab().flow_pane;
            plot_split(
                pane.last_plot_area.expect("a frame has been drawn"),
                pane.live_strip_width(),
                pane.indicators.pane_sizing(
                    &mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES],
                ),
            )
            .price_gutter
        };
        drag_chart(
            &mut app,
            &ctx,
            gutter.center(),
            gutter.center() - egui::vec2(0.0, 60.0),
        );

        assert!(
            !app.active_tab().flow_pane.price_view.is_auto(),
            "the candles took the drag"
        );
        assert_eq!(
            pane_range(&app, flow),
            untouched,
            "and the pane never felt it"
        );
        assert!(pane_is_auto(&app, flow));
    }

    /// A drawing tool takes the *primary button*, never the chart.
    ///
    /// Arming one used to return early from the whole navigation pass, so the
    /// trader could not zoom, pan or resize anything while annotating (audit
    /// S2) — and carrying that shape into the panes would have multiplied it
    /// by every pane on screen.
    #[test]
    fn an_armed_tool_leaves_the_chart_navigable() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));

        // The wheel over the pane's own axis still zooms that axis.
        let (lo, hi) = pane_range(&app, flow);
        let over = pane_gutter(&app, 0).center();
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let (zoomed_lo, zoomed_hi) = pane_range(&app, flow);
        assert!(
            zoomed_hi - zoomed_lo < hi - lo,
            "an armed tool must not deafen the pane's axis: {lo}..{hi} -> \
             {zoomed_lo}..{zoomed_hi}"
        );

        // And the wheel over the candles still zooms time.
        let width = app.active_tab().flow_pane.viewport.candle_width();
        let over_candles = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("a frame has been drawn")
            .center();
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(over_candles),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(
            app.active_tab().flow_pane.viewport.candle_width() > width,
            "the candles still zoom while a tool is armed"
        );
    }

    /// An anchor dropped in an indicator pane belongs to that pane.
    #[test]
    fn a_click_in_an_indicator_pane_draws_on_that_band() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));

        let inside = pane_body(&app, 0).center();
        click_chart(&mut app, &ctx, inside);

        let placed = &app.active_tab().flow_pane.drawings;
        assert_eq!(placed.items().len(), 1, "the click placed one object");
        assert!(
            matches!(placed.items()[0].band, drawings::DrawingBand::Indicator(_)),
            "an anchor dropped in the CVD pane is a CVD level, not a price"
        );
    }

    /// The chevron that collapses a pane still beats an armed tool. egui hands
    /// an overlap to the last registrant and the chevron registers last — but
    /// the drawing path reads the raw pointer, so it has to honour that order
    /// itself or arming a tool silently kills the control.
    #[test]
    fn the_pane_chevron_still_wins_its_pixels_with_a_tool_armed() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));

        let body = pane_body(&app, 0);
        let chevron = crate::indicator_render::pane_disclosure_rect(body, false).center();
        click_chart(&mut app, &ctx, chevron);

        let sizing = app
            .active_tab()
            .flow_pane
            .indicators
            .all()
            .iter()
            .find(|view| view.slot == flow)
            .expect("the pane is still there")
            .sizing;
        assert_eq!(sizing, crate::indicators::PaneSizing::Collapsed);
        assert!(
            app.active_tab().flow_pane.drawings.items().is_empty(),
            "the chevron is chrome, not canvas"
        );
    }

    /// Scroll is the same gesture with a wheel: it zooms the pane under the
    /// pointer, and the candles keep auto-fitting.
    #[test]
    fn scrolling_a_pane_axis_zooms_that_pane() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);

        let (lo, hi) = pane_range(&app, flow);
        let over = pane_gutter(&app, 0).center();
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );

        let (zoomed_lo, zoomed_hi) = pane_range(&app, flow);
        assert!(
            zoomed_hi - zoomed_lo < hi - lo,
            "scrolling up zooms in: {lo}..{hi} -> {zoomed_lo}..{zoomed_hi}"
        );
        assert!(
            app.active_tab().flow_pane.price_view.is_auto(),
            "the candles kept auto-fitting"
        );
    }

    /// Manual control is manual: the range holds while values keep arriving,
    /// and a double-click on the axis hands the pane back to auto-fit.
    #[test]
    fn a_zoomed_pane_holds_its_range_until_the_axis_is_double_clicked() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
        run_frame(&mut app, &ctx);

        let gutter = pane_gutter(&app, 0);
        drag_chart(
            &mut app,
            &ctx,
            gutter.center(),
            gutter.center() - egui::vec2(0.0, 60.0),
        );
        let held = pane_range(&app, flow);
        assert!(!pane_is_auto(&app, flow), "the drag took manual control");

        // The indicator recomputes ten times bigger: auto-fit would jump, a
        // range the user set does not.
        rebuild_pane_indicator(
            &mut app,
            flow,
            "cvd",
            (0..200).map(|row| f64::from(row) * 10.0).collect(),
        );
        run_frame(&mut app, &ctx);
        assert_eq!(pane_range(&app, flow), held, "the range the user set holds");

        click_chart(&mut app, &ctx, gutter.center());
        click_chart(&mut app, &ctx, gutter.center());
        run_frame(&mut app, &ctx);
        assert!(
            pane_is_auto(&app, flow),
            "double-click returns the pane to auto-fit"
        );
        let refitted = pane_range(&app, flow);
        assert!(
            refitted.1 > held.1,
            "and auto-fit sees the values that arrived meanwhile: {refitted:?} vs {held:?}"
        );
    }

    /// The pane reads as a chart: its own round numbers, in the gutter beside
    /// it. The candles' axis never labels those pixels, and the pane's labels
    /// never appear over the candles.
    #[test]
    fn a_pane_prints_its_own_value_labels_in_the_gutter() {
        let (mut app, _cmd_rx) = app_with_history(200);
        let ctx = egui::Context::default();
        // Values well away from the test's 95..115 price range, so a label can
        // only have come from the pane's own axis.
        add_pane_indicator(
            &mut app,
            "cvd",
            (0..200).map(|row| f64::from(row) - 200.0).collect(),
        );
        let output = run_frame(&mut app, &ctx);
        let texts = painted_text(&output);

        let pane_labels: Vec<&String> = texts
            .iter()
            .filter(|text| {
                text.parse::<f64>()
                    .is_ok_and(|value| (-200.0..=-1.0).contains(&value))
            })
            .collect();
        assert!(
            pane_labels.len() >= 3,
            "the pane's own round numbers are drawn, and enough of them to \
             read as a scale: {texts:?}"
        );
        assert!(
            has_price_axis(&texts),
            "and the candles keep theirs: {texts:?}"
        );
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
                default_layout: None,
                default_bars: None,
            }],
            metatrader: Default::default(),
            paper: Default::default(),
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
                    ohlcv_history: false,
                    ohlcv_generation: 0,
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
        app.active_tab_mut()
            .flow_pane
            .drawings
            .place(drawing_tool("horizontal-line"), ChartPoint::at(1.0, 100.0));

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
        app.active_tab_mut()
            .flow_pane
            .drawings
            .place(drawing_tool("horizontal-line"), ChartPoint::at(1.0, 100.0));

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

    /// Paper trading through the app's own event path: backfill only seeds
    /// (never fills), the toolbar buy queues, the next live print fills, and
    /// the status-bar cell reports the simulated position.
    #[test]
    fn a_simulated_buy_fills_from_the_next_live_print_only() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![trade(2)]))
            .unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        assert!(app.active_tab().paper.ready(), "backfill seeds the mark");
        assert!(
            app.active_tab().paper.status_cell().is_none(),
            "an untouched simulator owes no status line"
        );

        app.apply_toolbar_action(ToolbarAction::PaperBuy);
        assert!(
            app.active_tab().paper.status_cell().is_some(),
            "a queued market order is visible state"
        );
        evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        let (text, _) = app
            .active_tab()
            .paper
            .status_cell()
            .expect("the fill opened a position");
        assert!(
            text.starts_with("SIM"),
            "the cell is labeled simulated: {text}"
        );
        assert!(
            app.status_model().sim_pnl.is_some(),
            "the status bar model carries the cell"
        );
    }

    /// The toolbar's exit control end to end: with a position open the model
    /// grows the ✕ button, the close action queues, the next print fills it,
    /// and the status cell switches from naming the position to `flat`.
    #[test]
    fn the_toolbar_close_action_exits_the_open_position() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        // The close journals; without this the test writes a real
        // `paper-trades/` folder into the crate's source tree.
        let dir =
            std::env::temp_dir().join(format!("quantick-paper-app-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.active_tab_mut().paper.redirect_history_dir(dir.clone());
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![trade(2)]))
            .unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        app.apply_toolbar_action(ToolbarAction::PaperBuy);
        evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        let (text, _) = app.active_tab().paper.status_cell().expect("open");
        assert!(text.contains("LONG"), "the cell names the side: {text}");
        assert!(
            app.active_tab().paper.close_button_label().is_some(),
            "an open position grows the toolbar exit"
        );

        app.apply_toolbar_action(ToolbarAction::PaperClose);
        evt_tx.try_send(FeedEvent::Live(trade(6))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        assert!(
            app.active_tab().paper.position_summary().is_none(),
            "the close filled at the next print"
        );
        let (text, _) = app.active_tab().paper.status_cell().expect("history");
        assert!(text.contains("flat"), "the cell says flat: {text}");
        assert!(
            app.active_tab().paper.close_button_label().is_none(),
            "flat removes the toolbar exit"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A source reset (replay seek, feed switch) flattens the simulated
    /// position at the last mark and journals the round trip — the same
    /// honesty contract the drawings' clear follows.
    #[test]
    fn a_source_reset_flattens_the_simulated_position_and_journals_it() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let dir =
            std::env::temp_dir().join(format!("quantick-paper-app-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.active_tab_mut().paper.redirect_history_dir(dir.clone());
        // No `set_symbol` here: the tab's own drain syncs the journal to its
        // symbol before reading a single event, which is what makes the
        // folder assertion below a proof of that wiring too.
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![trade(2)]))
            .unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        app.apply_toolbar_action(ToolbarAction::PaperBuy);
        evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        assert!(
            app.active_tab().paper.status_cell().is_some(),
            "the realized history keeps the cell alive"
        );
        let files: Vec<_> = std::fs::read_dir(dir.join("TESTUSDT"))
            .expect("the flatten was journaled under the symbol's folder")
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one session, one history file");
        let _ = std::fs::remove_dir_all(&dir);
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

    /// An app whose source quotes prices and nothing else: no book, no traded
    /// volume — what a live CFD bridge publishes.
    fn app_without_depth() -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
        let (_evt_tx, evt_rx) = mpsc::channel(64);
        let (_book_tx, book_rx) = mpsc::channel(64);
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
                capabilities: feed::fixed_capabilities(FeedCapabilities::none()),
                commands: cmd_tx,
                replay: None,
            },
        );
        (app, cmd_rx)
    }

    /// The active tab's flow pane beside the chrome a canvas frame hands it.
    fn with_flow_pane<R>(
        app: &mut QuantickApp,
        body: impl FnOnce(&mut ChartPane, &mut pane::PaneChrome<'_>) -> R,
    ) -> R {
        let capabilities = app.active_tab().capabilities(&app.config);
        let QuantickApp {
            tabs,
            active_tab,
            toolrail,
            drawing_presets,
            style,
            tz,
            layer_actions,
            ..
        } = app;
        let tab = &mut tabs[*active_tab];
        let mut chrome = pane::PaneChrome {
            toolrail,
            presets: drawing_presets,
            style,
            tz: *tz,
            symbol: &tab.symbol,
            paper: &mut tab.paper,
            paper_owns_input: true,
            capabilities,
            layers: layer_actions,
        };
        body(&mut tab.flow_pane, &mut chrome)
    }

    /// Switch a layer exactly as the menu does, including the hop the app makes
    /// for the ones a pane does not own.
    fn switch_layer(app: &mut QuantickApp, layer: ChartLayer, visible: bool) {
        with_flow_pane(app, |pane, chrome| {
            pane.set_layer_visible(layer, visible, chrome.layers);
        });
        app.apply_layer_actions();
    }

    /// Whether the active tab's flow pane is painting `layer`.
    fn layer_on(app: &QuantickApp, layer: ChartLayer) -> bool {
        app.active_tab().flow_pane.layer_visible(layer, &app.style)
    }

    /// The layer menu writes through to whoever owns the layer, and touches
    /// nothing else. A menu holding its own copy of "is the heatmap on" would
    /// disagree with the toolbar the moment either one was used.
    #[test]
    fn each_layer_switch_moves_exactly_one_owner() {
        let (mut app, _events, _commands, _book) = test_app();
        // What the chart opens with: the market layers are opt-in, the chart's
        // own chrome is on. This is the state the file's absence must preserve.
        for (layer, expected) in [
            (ChartLayer::Heatmap, false),
            (ChartLayer::Bubbles, false),
            (ChartLayer::LiveStrip, false),
            (ChartLayer::LaneMarks, true),
            (ChartLayer::DepthGaps, true),
            (ChartLayer::Grid, true),
            (ChartLayer::LastPrice, true),
            // A full-height rule across the candles for a boundary read once:
            // opt-in, like the market layers above it.
            (ChartLayer::BackfillDivider, false),
            (ChartLayer::SeamDivider, true),
            (ChartLayer::Crosshair, true),
            (ChartLayer::PaperTrading, true),
            (ChartLayer::Drawings, true),
        ] {
            assert_eq!(
                layer_on(&app, layer),
                expected,
                "{} opens in the wrong state",
                layer.id()
            );
        }

        for layer in ChartLayer::ALL {
            let before: Vec<bool> = ChartLayer::ALL
                .into_iter()
                .map(|other| layer_on(&app, other))
                .collect();
            let flipped = !layer_on(&app, layer);
            switch_layer(&mut app, layer, flipped);
            for (other, was) in ChartLayer::ALL.into_iter().zip(before) {
                let expected = if other == layer { flipped } else { was };
                assert_eq!(
                    layer_on(&app, other),
                    expected,
                    "switching {} moved {} too",
                    layer.id(),
                    other.id()
                );
            }
            switch_layer(&mut app, layer, !flipped);
        }
    }

    /// Hiding is a view state, never a kill switch: the recorder keeps running
    /// behind a hidden heatmap, so unhiding repaints the retained past instead
    /// of opening a hole in it.
    #[test]
    fn hiding_a_layer_never_stops_the_data_behind_it() {
        let (mut app, _events, _commands, _book) = test_app();
        let config = app.config.clone();
        app.active_tab_mut().ensure_book_capture(&config);
        assert!(
            app.active_tab().tape().enabled(),
            "capture is on before the test"
        );

        switch_layer(&mut app, ChartLayer::Heatmap, true);
        assert!(layer_on(&app, ChartLayer::Heatmap));
        switch_layer(&mut app, ChartLayer::Heatmap, false);
        assert!(!layer_on(&app, ChartLayer::Heatmap));
        assert!(
            app.active_tab().tape().enabled(),
            "the map went off screen; the recording must not stop with it"
        );

        // Same for the drawings: the layer switch hides them, it never removes
        // them, and the objects come back with their anchors intact.
        let before = app.active_tab().flow_pane.drawings.items().len();
        switch_layer(&mut app, ChartLayer::Drawings, false);
        assert_eq!(
            app.active_tab().flow_pane.drawings.items().len(),
            before,
            "hiding deletes nothing"
        );
        switch_layer(&mut app, ChartLayer::Drawings, true);
        assert!(layer_on(&app, ChartLayer::Drawings));
    }

    /// Close the app with layers hidden, open it again, and the canvas comes
    /// back the way it was left — through the same restore the constructor runs.
    #[test]
    fn layer_visibility_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("quantick-app-layers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("chart-layers.toml");
        let _ = std::fs::remove_file(&path);

        let (mut app, _events, _commands, _book) = test_app();
        app.chart_layers_path = path.clone();
        app.saved_layer_mask = app.layer_mask();
        switch_layer(&mut app, ChartLayer::Crosshair, false);
        switch_layer(&mut app, ChartLayer::PaperTrading, false);
        switch_layer(&mut app, ChartLayer::Grid, false);
        // Switched through the toolbar's own action rather than the menu: the
        // save must follow the state, not the widget that moved it.
        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        // A market layer switched *on* has to come back on, which is why the
        // file records each layer's state instead of a list of hidden ones.
        switch_layer(&mut app, ChartLayer::Bubbles, true);
        app.maintain_chart_layers();
        assert_eq!(
            app.saved_layer_mask,
            app.layer_mask(),
            "a settled canvas writes nothing further"
        );

        let (mut restored, _events, _commands, _book) = test_app();
        restored.chart_layers_path = path.clone();
        restored.restore_chart_layers();
        for (layer, expected) in [
            (ChartLayer::Crosshair, false),
            (ChartLayer::PaperTrading, false),
            (ChartLayer::Grid, false),
            (ChartLayer::Heatmap, true),
            (ChartLayer::Bubbles, true),
            (ChartLayer::LastPrice, true),
            (ChartLayer::Drawings, true),
        ] {
            assert_eq!(
                layer_on(&restored, layer),
                expected,
                "{} did not survive the restart",
                layer.id()
            );
        }
        // The lane marks belong to the order-flow preset; this file must not
        // have taken a second opinion on them.
        let text = std::fs::read_to_string(&path).expect("state file");
        assert!(!text.contains("lane_marks"), "{text}");
        std::fs::remove_file(&path).ok();
    }

    /// A tab opened later shows the same canvas as the one beside it: opening a
    /// second market is not a request to bring back hidden chrome.
    #[test]
    fn a_new_tab_opens_on_the_layers_the_user_left_showing() {
        let dir = std::env::temp_dir().join(format!("quantick-app-newtab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("chart-layers.toml");
        let _ = std::fs::remove_file(&path);

        let (mut app, _events, _commands, _book) = test_app();
        app.chart_layers_path = path.clone();
        app.saved_layer_mask = app.layer_mask();
        switch_layer(&mut app, ChartLayer::Crosshair, false);
        app.maintain_chart_layers();
        // A fresh app reads the file, then opens a second market.
        let (mut restored, _events, _commands, _book) = test_app();
        restored.chart_layers_path = path.clone();
        restored.restore_chart_layers();
        let (_evt_tx, evt_rx) = mpsc::channel(4);
        let (_book_tx, book_rx) = mpsc::channel(4);
        let (cmd_tx, _cmd_rx) = mpsc::channel(4);
        restored.adopt_tab(
            "binance".to_owned(),
            "OTHERUSDT".to_owned(),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
            None,
        );
        assert_eq!(restored.tabs.len(), 2, "the second market opened");
        assert!(
            !layer_on(&restored, ChartLayer::Crosshair),
            "the new tab brought back a layer the user had switched off"
        );
        std::fs::remove_file(&path).ok();
    }

    /// The menu itself: every layer gets a switch, a layer the feed cannot
    /// produce is offered disabled rather than as a lie, and clicking a real
    /// checkbox hides the layer behind it.
    #[test]
    fn the_layer_menu_offers_every_layer_and_its_switches_work() {
        let dir = std::env::temp_dir().join(format!("quantick-app-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("chart-layers.toml");
        let _ = std::fs::remove_file(&path);

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 700.0));
        let (mut app, _events, _commands, _book) = test_app();
        app.chart_layers_path = path.clone();

        let menu_frame = |app: &mut QuantickApp, events: Vec<egui::Event>| {
            with_flow_pane(app, |pane, chrome| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| pane.draw_layer_menu(ui, chrome));
                    },
                );
            });
        };

        menu_frame(&mut app, Vec::new());
        assert_eq!(
            app.active_tab().flow_pane.layer_menu_rects.len(),
            ChartLayer::ALL.len(),
            "every layer needs a switch, or it cannot be turned off at all"
        );

        let crosshair = app
            .active_tab()
            .flow_pane
            .layer_menu_rects
            .iter()
            .find(|(layer, _)| *layer == ChartLayer::Crosshair)
            .expect("the crosshair has a switch")
            .1
            .center();
        assert!(layer_on(&app, ChartLayer::Crosshair));
        menu_frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(crosshair),
                egui::Event::PointerButton {
                    pos: crosshair,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
        menu_frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: crosshair,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        assert!(
            !layer_on(&app, ChartLayer::Crosshair),
            "clicking the switch has to switch the layer"
        );

        // A capability the source lacks is disabled, not silently absent: a
        // recording has no book, and the entry says so instead of offering a
        // switch that would do nothing.
        let full = app.active_tab().capabilities(&app.config);
        let pane = &app.active_tab().flow_pane;
        assert!(pane.layer_blocked(ChartLayer::Heatmap, full).is_none());
        let (mut quote_only, _commands) = app_without_depth();
        let quotes = quote_only.active_tab().capabilities(&quote_only.config);
        let pane = &quote_only.active_tab().flow_pane;
        assert!(
            pane.layer_blocked(ChartLayer::Heatmap, quotes).is_some(),
            "a source with no book cannot promise a heatmap"
        );
        assert!(pane.layer_blocked(ChartLayer::Bubbles, quotes).is_some());
        assert!(pane.layer_blocked(ChartLayer::Grid, quotes).is_none());
        menu_frame(&mut quote_only, Vec::new());
        assert_eq!(
            quote_only.active_tab().flow_pane.layer_menu_rects.len(),
            ChartLayer::ALL.len(),
            "an unavailable layer is still listed, just not switchable"
        );
        std::fs::remove_file(&path).ok();
    }

    /// §11 keeps the tape on the flow pane, so the time pane's menu says the
    /// flow layers are drawn elsewhere instead of offering dead switches.
    #[test]
    fn the_time_pane_offers_the_flow_layers_as_drawn_elsewhere() {
        let (app, _events, _commands, _book) = test_app();
        let capabilities = app.active_tab().capabilities(&app.config);
        let time = ChartPane::time(99, 60_000);
        for layer in [
            ChartLayer::Heatmap,
            ChartLayer::Bubbles,
            ChartLayer::LiveStrip,
            ChartLayer::LaneMarks,
            ChartLayer::DepthGaps,
        ] {
            assert_eq!(
                time.layer_blocked(layer, capabilities),
                Some("the order-flow layers are drawn on the flow pane"),
                "{} has no machinery on a time pane",
                layer.id()
            );
            assert!(!time.layer_visible(layer, &app.style));
        }
        for layer in [
            ChartLayer::Grid,
            ChartLayer::LastPrice,
            ChartLayer::Crosshair,
            ChartLayer::Drawings,
        ] {
            assert!(time.layer_blocked(layer, capabilities).is_none());
        }
    }

    /// A switched-off layer really stops painting.
    ///
    /// The switches above only prove the *state* moved; this one draws a real
    /// chart twice and counts the shapes, so a gate someone forgets to place
    /// (or later deletes) shows up as a menu entry that changes nothing.
    #[test]
    fn a_hidden_layer_paints_nothing() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        let (mut app, _commands) = app_with_history(120);
        // The crosshair is a mode: it paints under a pointer, with its own tool
        // armed. Both are set here so the layer has something to switch off.
        app.toolrail.arm(Tool::Crosshair);

        let shapes = |app: &mut QuantickApp| -> usize {
            with_flow_pane(app, |pane, chrome| {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let area = ui.available_rect_before_wrap();
                            pane.hover_pos = Some(area.center());
                            pane.draw_chart(ui.painter(), area, chrome);
                        });
                    },
                );
                output.shapes.len()
            })
        };

        // Every layer under test on, whatever it opens as: the count below is
        // the "all on" chart, so an opt-in layer has to be switched on first.
        let under_test = [
            ChartLayer::LastPrice,
            ChartLayer::BackfillDivider,
            ChartLayer::Crosshair,
        ];
        for layer in under_test {
            switch_layer(&mut app, layer, true);
        }
        // One frame to settle: the live lane's divider and the price range are
        // computed by a draw and read by the next one, so the first frame is
        // not yet the chart this test is counting.
        let _ = shapes(&mut app);
        let all_on = shapes(&mut app);
        for layer in under_test {
            switch_layer(&mut app, layer, false);
            let off = shapes(&mut app);
            assert!(
                off < all_on,
                "{} kept painting after it was switched off ({off} shapes vs {all_on})",
                layer.id()
            );
            switch_layer(&mut app, layer, true);
            assert_eq!(
                shapes(&mut app),
                all_on,
                "{} did not come back exactly as it was",
                layer.id()
            );
        }
    }

    /// The closed-trade marks obey their own switch: a closed round trip
    /// paints marks and a connector, `closed trade marks` off erases them,
    /// and the live paper layer is untouched either way — hiding history
    /// must never hide the position machinery.
    #[test]
    fn the_trade_paint_layer_switch_stops_the_marks() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let dir =
            std::env::temp_dir().join(format!("quantick-trade-paint-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.active_tab_mut().paper.redirect_history_dir(dir.clone());
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![trade(2)]))
            .unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        app.apply_toolbar_action(ToolbarAction::PaperBuy);
        evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        app.apply_toolbar_action(ToolbarAction::PaperClose);
        evt_tx.try_send(FeedEvent::Live(trade(6))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        assert_eq!(
            app.active_tab().paper.session_trades().len(),
            1,
            "one closed round trip to paint"
        );

        let shapes = |app: &mut QuantickApp| -> usize {
            with_flow_pane(app, |pane, chrome| {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let area = ui.available_rect_before_wrap();
                            pane.draw_chart(ui.painter(), area, chrome);
                        });
                    },
                );
                output.shapes.len()
            })
        };
        // One frame to settle the ranges a draw computes for the next one.
        let _ = shapes(&mut app);
        let marks_on = shapes(&mut app);
        switch_layer(&mut app, ChartLayer::TradePaint, false);
        let marks_off = shapes(&mut app);
        assert!(
            marks_off < marks_on,
            "the marks kept painting with their layer off ({marks_off} vs {marks_on})"
        );
        assert!(
            app.active_tab()
                .flow_pane
                .layer_visible(ChartLayer::PaperTrading, &app.style),
            "hiding closed-trade history leaves the live paper layer alone"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Through the real frame pipeline — real pointer events, the pane's own
    /// scale — a click on the ✕ of a working order's chart tag cancels the
    /// order. It must never read as a drag on the order's line: the hit-test
    /// is geometric at press time, because a cached pixel rect goes stale
    /// the moment a live chart autoscales between paint and press.
    #[test]
    fn clicking_the_chart_tag_close_cancels_the_order() {
        let ctx = egui::Context::default();
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![
                trade(2),
                trade(6),
                trade(10),
                trade(14),
                trade(18),
            ]))
            .unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        // A resting buy limit in the middle of the backfilled price range,
        // so its line and tag are on screen.
        let price = Decimal::new(1005, 1);
        app.active_tab_mut()
            .paper
            .apply_sim_command_for_tests(quantick_sim::Command::PlaceLimit {
                side: quantick_engine::Side::Buy,
                quantity: Decimal::ONE,
                price,
                bracket: quantick_sim::Bracket::none(),
            });
        assert_eq!(app.active_tab().paper.working_orders().len(), 1);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let chart = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("the pane laid out");
        let tag_right = app
            .active_tab()
            .flow_pane
            .last_lane_divider_x
            .unwrap_or(chart.right());
        let y = price_y(&app, PaneSide::Flow, 100.5);
        let close = crate::paper_trading::close_button_rect(
            tag_right,
            crate::paper_trading::clamp_tag_center(y, chart.top(), chart.bottom()),
        );
        drag_chart(&mut app, &ctx, close.center(), close.center());
        assert!(
            app.active_tab().paper.working_orders().is_empty(),
            "the click cancelled the order instead of dragging it"
        );
    }

    /// The gesture itself: a right-click on the canvas opens the menu, and the
    /// primary button — which pans, zooms and places drawings — never does.
    ///
    /// The menu's contents are covered above; what this proves is the one thing
    /// between the user and all of it, the button it is bound to.
    #[test]
    fn only_the_secondary_button_opens_the_layer_menu() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let (mut app, _events, _commands, _book) = test_app();

        let click = |app: &mut QuantickApp, button: egui::PointerButton| -> usize {
            with_flow_pane(app, |pane, chrome| {
                let target = screen.center();
                for pressed in [true, false] {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events: vec![
                                egui::Event::PointerMoved(target),
                                egui::Event::PointerButton {
                                    pos: target,
                                    button,
                                    pressed,
                                    modifiers: egui::Modifiers::default(),
                                },
                            ],
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                let area = ui.available_rect_before_wrap();
                                pane.handle_navigation(ui, area, chrome);
                            });
                        },
                    );
                }
                pane.layer_menu_rects.len()
            })
        };

        assert_eq!(
            click(&mut app, egui::PointerButton::Primary),
            0,
            "a left click is a pan or a placement; it must not open the menu"
        );
        assert_eq!(
            click(&mut app, egui::PointerButton::Secondary),
            ChartLayer::ALL.len(),
            "a right click on the canvas has to open the layer menu"
        );
    }

    /// Reaching for a tool brings its own layer back — a crosshair that draws
    /// no cross, or a line tool that places invisible objects, reads as a
    /// broken tool rather than as a hidden layer.
    #[test]
    fn arming_a_tool_unhides_the_layer_it_draws_on() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let (mut app, _events, _commands, _book) = test_app();

        let navigate = |app: &mut QuantickApp| {
            with_flow_pane(app, |pane, chrome| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let area = ui.available_rect_before_wrap();
                            pane.handle_navigation(ui, area, chrome);
                        });
                    },
                );
            });
        };

        for (tool, layer) in [
            (
                Tool::Drawing(drawings::DRAWING_TOOLS[0]),
                ChartLayer::Drawings,
            ),
            (Tool::Crosshair, ChartLayer::Crosshair),
        ] {
            app.toolrail.arm(Tool::Pointer);
            switch_layer(&mut app, layer, false);
            navigate(&mut app);
            assert!(
                !layer_on(&app, layer),
                "{} must stay hidden while nothing needs it",
                layer.id()
            );
            app.toolrail.arm(tool);
            navigate(&mut app);
            assert!(
                layer_on(&app, layer),
                "arming its tool has to bring {} back",
                layer.id()
            );
        }
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

    /// The window every frame-driving test gets unless it asks for another:
    /// roomy, so a test about something else is never accidentally a test
    /// about a cramped layout.
    const TEST_WINDOW: egui::Vec2 = egui::vec2(1400.0, 900.0);
    /// The smallest window the app itself allows (`main.rs`
    /// `with_min_inner_size`). The layout has to hold here, and this is where
    /// the pane band is under real pressure.
    const MIN_WINDOW: egui::Vec2 = egui::vec2(900.0, 560.0);

    fn run_frame_with_modifiers(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        run_frame_sized(app, ctx, TEST_WINDOW, events, modifiers)
    }

    /// A frame at a chosen window size — how a test reaches the layout a
    /// smaller window produces without resizing anything global.
    fn run_frame_sized(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
            events,
            modifiers,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.draw_frame(ctx, Instant::now()))
    }

    /// A press-drag-release in a window of `size`.
    fn drag_sized(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        size: egui::Vec2,
        start: egui::Pos2,
        end: egui::Pos2,
    ) {
        run_frame_sized(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
            egui::Modifiers::NONE,
        );
        run_frame_sized(
            app,
            ctx,
            size,
            vec![egui::Event::PointerMoved(end)],
            egui::Modifiers::NONE,
        );
        run_frame_sized(
            app,
            ctx,
            size,
            vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
            egui::Modifiers::NONE,
        );
    }

    fn run_frame_at(app: &mut QuantickApp, ctx: &egui::Context, size: egui::Vec2) {
        run_frame_sized(app, ctx, size, Vec::new(), egui::Modifiers::NONE);
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
        if let Some(button) = app.toolrail.button_rect(tool) {
            click_chart(app, ctx, button.center());
        } else {
            // A folded family member has no direct slot: open the family
            // flyout through the slot's caret zone and arm it by its row —
            // the same pointer path a user takes.
            let family = drawing
                .family()
                .expect("only family members lack a direct slot");
            let slot = drawings::DRAWING_TOOLS
                .into_iter()
                .filter(|candidate| {
                    candidate
                        .family()
                        .is_some_and(|candidate_family| candidate_family.id == family.id)
                })
                .find_map(|candidate| app.toolrail.button_rect(Tool::Drawing(candidate)))
                .expect("the family slot was rendered");
            click_chart(app, ctx, slot.max - egui::vec2(4.0, 4.0));
            run_frame(app, ctx);
            let row = app
                .toolrail
                .flyout_row_rect(drawing)
                .expect("the flyout lists the folded member");
            click_chart(app, ctx, row.center());
        }
        assert_eq!(
            app.toolrail.tool(),
            tool,
            "arming {id} through the rail must land"
        );
        drawing
    }

    /// Count the line segments painted in the drawing colour — how many
    /// strokes of *this* object are on screen, across every pane.
    fn drawing_strokes(output: &egui::FullOutput) -> usize {
        let color = egui::epaint::ColorMode::Solid(crate::drawings::DEFAULT_DRAWING_COLOR);
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { stroke, .. } => stroke.color == color,
                _ => false,
            })
            .count()
    }

    /// The repo's capability rule: an unsupported property is *absent* from
    /// the inspector, never present and inert. A text note has glyphs and no
    /// stroke, so a "line width" slider on its Style tab would be a control
    /// that moves nothing — which reads as a broken app, not as a no-op.
    /// Found by the visual pass on a real screen.
    #[test]
    fn the_style_tab_offers_line_width_only_to_tools_that_have_a_stroke() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        let style_tab_labels = |app: &mut QuantickApp, ctx: &egui::Context| -> Vec<String> {
            app.inspector_tab = InspectorTab::Style;
            painted_text(&run_frame(app, ctx))
        };

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        let with_stroke = style_tab_labels(&mut app, &ctx);
        assert!(
            with_stroke.iter().any(|text| text.contains("line width")),
            "a stroked tool keeps its width slider; painted: {with_stroke:?}"
        );

        arm_drawing_from_toolbox(&mut app, &ctx, "text");
        click_chart(&mut app, &ctx, egui::pos2(760.0, 340.0));
        assert_eq!(
            app.active_tab()
                .flow_pane
                .drawings
                .selected()
                .and_then(|index| app
                    .active_tab()
                    .flow_pane
                    .drawings
                    .items()
                    .get(index)
                    .map(|drawing| drawing.tool.id())),
            Some("text"),
            "the note is the selection the inspector is describing"
        );
        let words_only = style_tab_labels(&mut app, &ctx);
        assert!(
            !words_only.iter().any(|text| text.contains("line width")),
            "a note has no stroke to widen; painted: {words_only:?}"
        );
        assert!(
            words_only.iter().any(|text| text.contains("Style")),
            "the tab itself is still there, with the colour control"
        );
    }

    /// Reported from the running app: "clico no desenho, abre a propriedade
    /// e fecha rapidamente — como se eu tivesse clicado fora dele". Selecting
    /// an object must survive the release that made it and every frame after.
    #[test]
    fn clicking_a_drawing_keeps_it_selected_after_the_release() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        let on_the_line = egui::pos2(700.0, 300.0);
        click_chart(&mut app, &ctx, on_the_line);
        run_frame(&mut app, &ctx);
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

        // Deselect the way the user would, then click the line again — this
        // is the gesture that flickers.
        app.active_tab_mut().flow_pane.drawings.select(None);
        run_frame(&mut app, &ctx);

        click_chart(&mut app, &ctx, egui::pos2(900.0, 300.0));
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "the click that selects must not also deselect"
        );
        for frame in 0..4 {
            run_frame(&mut app, &ctx);
            assert_eq!(
                app.active_tab().flow_pane.drawings.selected(),
                Some(0),
                "the selection vanished {frame} frames after the click"
            );
        }
    }

    /// A Fib level is something price is *going* to meet. Drawn only between
    /// the anchors, an extension's targets sit to the left of the leg that
    /// projects them — backwards on its face — and they disappear behind the
    /// tape the moment the market moves on. Reported from the running build.
    #[test]
    fn fib_levels_project_forward_from_the_swing_by_default() {
        use crate::drawings::fib::{Extend, FibPayload};
        assert_eq!(
            Extend::default(),
            Extend::Forward,
            "a level nobody can see at current price is not a level"
        );
        for tool in ["fib-retracement", "fib-extension"] {
            let payload = drawing_tool(tool).default_payload();
            let fib = payload
                .as_any()
                .downcast_ref::<FibPayload>()
                .expect("the fib tools carry a fib payload");
            assert_eq!(
                fib.extend,
                Extend::Forward,
                "{tool} must open projecting forward from its last point"
            );
        }
    }

    /// A three-anchor tool that stops following the pointer reads as frozen —
    /// reported from the running build after a drag left a channel sitting
    /// there. It is waiting for a click, and it now says so beside the
    /// cursor, not only in a badge on the far side of the screen.
    #[test]
    fn a_draft_says_what_the_next_click_will_do() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        // Drag the trend line, exactly the gesture that was reported.
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 400.0),
            egui::pos2(800.0, 340.0),
        );
        let hover = egui::pos2(820.0, 300.0);
        let texts = painted_text(&run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(hover)],
        ));
        assert_eq!(
            app.active_tab().flow_pane.drawings.draft_len(),
            2,
            "the drag placed the trend line and the object waits for its width"
        );
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Click the channel width")),
            "the draft must say what it is waiting for; painted: {texts:?}"
        );
    }

    /// A tool with nothing specific to say still reports progress, because a
    /// count beats an object that looks like it stopped responding.
    #[test]
    fn a_draft_without_a_hint_still_shows_its_progress() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        let fib = drawing_tool("fib-retracement");
        assert_eq!(fib.placement_hint(1), None, "this tool has no words for it");
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        click_chart(&mut app, &ctx, egui::pos2(600.0, 400.0));
        let hover = egui::pos2(700.0, 320.0);
        let texts = painted_text(&run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(hover)],
        ));
        assert!(
            texts.iter().any(|text| text == "1/2"),
            "an unnamed next step still shows the count; painted: {texts:?}"
        );
    }

    /// The control that shares a drawing across the tab's charts has to be
    /// *findable*. It lives with the anchors, on the Coordinates tab, because
    /// sharing is a statement about the anchors — and every tool has that tab.
    #[test]
    fn the_coordinates_tab_offers_sharing_across_charts() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.inspector_tab = InspectorTab::Coordinates;
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Show on all charts")),
            "the sharing control must be on screen, not folded away; painted: {texts:?}"
        );
    }

    /// The chore this removes: re-picking the same colour on every object.
    /// Saving a default must reach the *next* drawing and leave the ones
    /// already on the chart exactly as the trader drew them.
    #[test]
    fn a_saved_default_style_reaches_the_next_drawing_and_only_that() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        app.drawing_presets = drawings::presets::PresetStore::load_from(std::env::temp_dir().join(
            format!("quantick-default-style-{}.toml", std::process::id()),
        ));
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        run_frame(&mut app, &ctx);

        let mine = egui::Color32::from_rgb(0xFF, 0xA0, 0x10);
        {
            let drawing = app
                .active_tab_mut()
                .flow_pane
                .drawings
                .selected_mut()
                .expect("the placed line is selected");
            drawing.style.color = mine;
            drawing.style.width_px = 2.5;
        }
        let edited = app.active_tab().flow_pane.drawings.items()[0].style;
        app.drawing_presets
            .set_default_style(drawings::DRAWING_TOOLS[0].id(), Some(edited));
        // Saving for one tool is saving for one tool.
        app.drawing_presets
            .set_default_style("horizontal-line", Some(edited));

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        click_chart(&mut app, &ctx, egui::pos2(700.0, 380.0));
        run_frame(&mut app, &ctx);

        let items = app.active_tab().flow_pane.drawings.items();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[1].style, edited,
            "the next object opens with the saved look"
        );
        assert_eq!(
            items[0].style, edited,
            "the first object is the one that was edited, untouched by the save"
        );

        // A tool with no saved default still opens as it always did.
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        click_chart(&mut app, &ctx, egui::pos2(760.0, 420.0));
        click_chart(&mut app, &ctx, egui::pos2(860.0, 480.0));
        run_frame(&mut app, &ctx);
        let items = app.active_tab().flow_pane.drawings.items();
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[2].style,
            drawings::DrawingStyle::default(),
            "a default is per tool, not a global repaint"
        );

        let _ = std::fs::remove_file(app.drawing_presets.path());
    }

    /// Selecting is not moving. Without a drag threshold on the move gesture,
    /// a couple of pixels of hand tremor during a click re-angled a channel
    /// or shifted a level — and recorded it as an undo step, so the trader's
    /// line was quietly no longer where they put it. Placement already
    /// refused to read a twitch as a drag; moving refuses too now.
    #[test]
    fn a_twitch_while_clicking_does_not_move_the_drawing() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
        click_chart(&mut app, &ctx, egui::pos2(600.0, 400.0));
        click_chart(&mut app, &ctx, egui::pos2(900.0, 300.0));
        run_frame(&mut app, &ctx);
        let placed = app.active_tab().flow_pane.drawings.items()[0]
            .points
            .clone();

        // Press on the stroke, wobble inside the threshold, release.
        let grab = egui::pos2(750.0, 350.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
        );
        // Ending *away* from the press, still inside the threshold: a wobble
        // that returned to its origin would net to zero movement and prove
        // nothing about the threshold.
        let wobbled = grab + egui::vec2(3.0, 0.0);
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(wobbled)]);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(wobbled),
                pointer_button(wobbled, false),
            ],
        );

        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[0].points,
            placed,
            "a click that wobbled under the threshold must leave the geometry alone"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "and it is still a click, so it still selects"
        );

        // The same gesture past the threshold does move it.
        let far = grab + egui::vec2(40.0, 0.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
        );
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(far)]);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(far), pointer_button(far, false)],
        );
        assert_ne!(
            app.active_tab().flow_pane.drawings.items()[0].points,
            placed,
            "a real drag still moves it"
        );
    }

    /// The reported flicker, root cause.
    ///
    /// The pinned inspector is a `SidePanel::right` laid out *before* the
    /// central panel, so the frame a selection appears is the frame the
    /// canvas narrows by the panel's width — and every drawing slides left
    /// with it. Press on frame N (wide canvas) selects; release on frame N+1
    /// (narrow canvas) hit-tests the same screen pixel, finds the drawing has
    /// moved out from under it, and deselects. The panel opens and shuts in
    /// two frames, forever, with the mouse standing still.
    #[test]
    fn a_pinned_inspector_cannot_wipe_the_selection_that_opened_it() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
        click_chart(&mut app, &ctx, egui::pos2(600.0, 400.0));
        click_chart(&mut app, &ctx, egui::pos2(900.0, 300.0));
        run_frame(&mut app, &ctx);
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

        // Pinned, and the user has expressed the preference — the exact
        // state the report was in.
        app.inspector_pinned = true;
        app.inspector_pin_touched = true;
        app.active_tab_mut().flow_pane.drawings.select(None);
        run_frame(&mut app, &ctx);
        let wide = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("the canvas drew")
            .width();

        // Click the middle of the line: nowhere near a handle, squarely on
        // the stroke. Nothing about this gesture is marginal.
        click_chart(&mut app, &ctx, egui::pos2(750.0, 350.0));
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "the release must not undo the selection the press made"
        );

        let narrow = app
            .active_tab()
            .flow_pane
            .last_chart_area
            .expect("the canvas drew")
            .width();
        assert!(
            narrow < wide,
            "this proof needs the pinned panel to actually steal canvas              width: {wide} -> {narrow}"
        );

        for frame in 0..4 {
            run_frame(&mut app, &ctx);
            assert_eq!(
                app.active_tab().flow_pane.drawings.selected(),
                Some(0),
                "the selection vanished {frame} frames after the click"
            );
        }
    }

    /// The reported flicker, reduced to its cause.
    ///
    /// The press selects on an anchor grab (12 px); the release used to
    /// body-test only (10 px). Just past the end of a trend line those two
    /// disagree: the handle is in reach and the stroke is not. So the press
    /// opened the panel and the release closed it, over and over, with the
    /// mouse standing still.
    #[test]
    fn grabbing_a_handle_off_the_stroke_selects_and_stays_selected() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
        let start = egui::pos2(600.0, 400.0);
        let end = egui::pos2(800.0, 400.0);
        click_chart(&mut app, &ctx, start);
        click_chart(&mut app, &ctx, end);
        run_frame(&mut app, &ctx);
        assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);

        app.active_tab_mut().flow_pane.drawings.select(None);
        run_frame(&mut app, &ctx);

        // 11 px past the far anchor, straight along the line: inside the
        // anchor radius, outside the stroke radius.
        let past_the_end = egui::pos2(end.x + 11.0, end.y);
        click_chart(&mut app, &ctx, past_the_end);
        assert_eq!(
            app.active_tab().flow_pane.drawings.selected(),
            Some(0),
            "grabbing the handle is clicking the object"
        );
        for frame in 0..4 {
            run_frame(&mut app, &ctx);
            assert_eq!(
                app.active_tab().flow_pane.drawings.selected(),
                Some(0),
                "the selection was wiped {frame} frames after the handle grab"
            );
        }
    }

    /// The same gesture on a crowded chart — the demo hook's seventeen
    /// overlapping objects, which is what the report was looking at.
    #[test]
    fn clicking_a_drawing_on_a_crowded_chart_keeps_it_selected() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
            arm_drawing_from_toolbox(&mut app, &ctx, tool.id());
            for point in 0..tool.required_points() {
                let offset = index as f32;
                let step = point as f32;
                click_chart(
                    &mut app,
                    &ctx,
                    egui::pos2(
                        560.0 + (offset % 4.0) * 50.0 + step * 70.0,
                        250.0 + (offset % 3.0) * 70.0 + step * 50.0,
                    ),
                );
            }
        }
        run_frame(&mut app, &ctx);
        app.active_tab_mut().flow_pane.drawings.select(None);
        run_frame(&mut app, &ctx);

        // Press-release on a spot the objects cover.
        let spot = egui::pos2(630.0, 320.0);
        click_chart(&mut app, &ctx, spot);
        let picked = app.active_tab().flow_pane.drawings.selected();
        assert!(picked.is_some(), "the click found something to select");
        for frame in 0..5 {
            run_frame(&mut app, &ctx);
            assert_eq!(
                app.active_tab().flow_pane.drawings.selected(),
                picked,
                "the selection changed {frame} frames after the click"
            );
        }
    }

    /// Marina's ask (`docs/ux/drawing-tools-2026-08.md` §D7): a level drawn on
    /// one chart of the tab shows on the other, at the same moment in market
    /// time — one version of the truth instead of two hand-drawn ones.
    #[test]
    fn a_shared_drawing_is_painted_on_the_other_pane_of_the_tab() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        // One frame builds the time pane, the next lets both panes draw and
        // cache the projection a foreign mark is re-expressed through.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert!(
            app.active_tab().time_pane.is_some(),
            "the split is what this proof is about"
        );

        // Anchored on a real bar, so the anchor carries a real market time.
        let slot = 100;
        let anchored = {
            let pane = &app.active_tab().flow_pane;
            let time = pane.slot_open_time(slot).expect("a closed bar has a time");
            let price = pane
                .closed_bar(slot)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .expect("the bar has a close");
            drawings::ChartPoint::at_time(slot as f32 + 0.5, price, Some(time))
        };
        let pane = &mut app.active_tab_mut().flow_pane;
        assert!(pane.drawings.place_with(
            drawing_tool("horizontal-line"),
            &drawings::DrawingBand::Price,
            anchored,
            |tool| {
                drawings::NewDrawing {
                    style: drawings::DrawingStyle::default(),
                    payload: tool.default_payload(),
                }
            },
        ));

        let own_pane_only = drawing_strokes(&run_frame(&mut app, &ctx));
        assert!(
            own_pane_only > 0,
            "the object paints on the chart it was drawn on"
        );

        let drawing = app
            .active_tab_mut()
            .flow_pane
            .drawings
            .selected_mut()
            .expect("placement selects what it completed");
        assert!(drawing.shareable(), "an anchor on a real bar has a time");
        drawing.scope = drawings::DrawingScope::AllCharts;

        let both_panes = drawing_strokes(&run_frame(&mut app, &ctx));
        assert!(
            both_panes > own_pane_only,
            "sharing must add strokes on the other pane: {own_pane_only} -> {both_panes}"
        );
    }

    /// The reverse, so the test above cannot pass on a stroke that was always
    /// there: switching sharing off takes the foreign copy away again.
    #[test]
    fn unsharing_removes_the_copy_from_the_other_pane() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let slot = 100;
        let anchored = {
            let pane = &app.active_tab().flow_pane;
            let time = pane.slot_open_time(slot).expect("a closed bar has a time");
            let price = pane
                .closed_bar(slot)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .expect("the bar has a close");
            drawings::ChartPoint::at_time(slot as f32 + 0.5, price, Some(time))
        };
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings.place_with(
            drawing_tool("horizontal-line"),
            &drawings::DrawingBand::Price,
            anchored,
            |tool| drawings::NewDrawing {
                style: drawings::DrawingStyle::default(),
                payload: tool.default_payload(),
            },
        );
        app.active_tab_mut()
            .flow_pane
            .drawings
            .selected_mut()
            .expect("selected")
            .scope = drawings::DrawingScope::AllCharts;
        let shared = drawing_strokes(&run_frame(&mut app, &ctx));

        app.active_tab_mut()
            .flow_pane
            .drawings
            .selected_mut()
            .expect("selected")
            .scope = drawings::DrawingScope::ThisChart;
        let alone = drawing_strokes(&run_frame(&mut app, &ctx));
        assert!(
            alone < shared,
            "unsharing must take the foreign copy away: {shared} -> {alone}"
        );
    }

    /// Full UI interaction proof: every registered drawing is placed through
    /// egui pointer events against the real chart frame. This catches the
    /// original regression where multi-point tools silently ignored drags.
    #[test]
    fn every_toolbox_drawing_can_be_plotted_on_the_chart() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // Registry-driven, not a hand-written list: a tool added to
        // `DRAWING_TOOLS` without a rail path (family flyout, shortcut,
        // placement) fails here on the day it is registered, instead of
        // shipping unreachable.
        for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
            arm_drawing_from_toolbox(&mut app, &ctx, tool.id());
            for anchor in 0..tool.required_points() {
                let offset = index as f32;
                let step = anchor as f32;
                click_chart(
                    &mut app,
                    &ctx,
                    egui::pos2(
                        560.0 + (offset % 4.0) * 50.0 + step * 70.0,
                        250.0 + (offset % 3.0) * 70.0 + step * 50.0,
                    ),
                );
            }
        }

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

    /// A canvas point at price-line `y` that is provably clear of the open
    /// inspector.
    ///
    /// These proofs are about the canvas gesture, not about where the
    /// placement rule currently puts the panel — so they ask the panel where
    /// it is instead of encoding an answer that changes whenever the Style
    /// tab grows a row.
    fn canvas_point_clear_of_inspector(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        y: f32,
    ) -> egui::Pos2 {
        run_frame(app, ctx);
        let clear = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .filter(|rect| rect.y_range().contains(y))
            .map_or(400.0, |rect| rect.right() + 40.0);
        egui::pos2(clear, y)
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
        // Clear of the inspector: the panel is opaque to presses by
        // contract, so a proof about dragging must not start under it.
        let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
        drag_chart(&mut app, &ctx, start, start + egui::vec2(40.0, 40.0));
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
    fn a_press_on_the_inspector_never_grabs_the_stroke_beneath_it() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let anchor = egui::pos2(700.0, 300.0);
        click_chart(&mut app, &ctx, anchor);
        run_frame(&mut app, &ctx);

        // Automatic placement now sends the panel to a chart corner (§D3),
        // deliberately clear of the object — so park it over the stroke by
        // hand. What this proves is pointer routing over an opaque panel, not
        // where the panel opens.
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("placing the selected line opens its inspector");
        app.inspector_moved = true;
        app.inspector_pos = Some(egui::pos2(
            inspector.left(),
            anchor.y - inspector.height() / 2.0,
        ));
        run_frame(&mut app, &ctx);
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is still open");
        let line_y = anchor.y;
        let start = egui::pos2(inspector.center().x, line_y);
        assert!(
            inspector.contains(start),
            "this proof needs the inspector to cover a stroke pixel"
        );
        let before = app.active_tab().flow_pane.drawings.items()[0].points[0];

        drag_chart(&mut app, &ctx, start, egui::pos2(start.x, line_y + 100.0));

        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[0].points[0],
            before,
            "a press on the inspector must never fall through to the chart"
        );
        assert_eq!(
            app.active_tab().flow_pane.drawing_drag,
            DrawingDrag::None,
            "no drawing drag may start from a press on the inspector"
        );
    }

    #[test]
    fn a_canvas_drag_keeps_running_while_crossing_the_inspector() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        run_frame(&mut app, &ctx);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
        assert!(
            !inspector.contains(start),
            "the gesture must begin on the open canvas"
        );
        let before = app.active_tab().flow_pane.drawings.items()[0].points[0];

        // The gate applies at press time only: a drag that began on the
        // canvas keeps moving the object while the pointer crosses the panel.
        drag_chart(&mut app, &ctx, start, inspector.center());

        assert_ne!(
            app.active_tab().flow_pane.drawings.items()[0].points[0].price,
            before.price,
            "continuity: the drag survives crossing the inspector"
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

    /// The eight §4.2 candidates, clamped — restated here so a drifted
    /// implementation cannot silently shrink its own search space.
    fn placement_candidates(
        chart: egui::Rect,
        bbox: egui::Rect,
        size: egui::Vec2,
    ) -> [egui::Pos2; 8] {
        let gap = INSPECTOR_OBJECT_GAP_PX;
        [
            egui::pos2(bbox.right() + gap, bbox.top()),
            egui::pos2(bbox.left() - gap - size.x, bbox.top()),
            egui::pos2(bbox.left(), bbox.bottom() + gap),
            egui::pos2(bbox.left(), bbox.top() - gap - size.y),
            egui::pos2(chart.left() + gap, chart.top() + gap),
            egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
            egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
            egui::pos2(chart.right() - gap - size.x, chart.bottom() - gap - size.y),
        ]
        .map(|candidate| clamp_into_chart(candidate, size, chart))
    }

    /// The session's complaint (`docs/ux/drawing-tools-2026-08.md` §F3): the
    /// old rule put the panel 12 px beside a small object, right on top of
    /// the price action the trader drew it to read. A corner must win.
    #[test]
    fn placement_sends_the_panel_to_a_corner_not_beside_a_small_object() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 800.0));
        let bbox = egui::Rect::from_center_size(chart.center(), egui::vec2(40.0, 40.0));
        let size = egui::vec2(320.0, 280.0);
        let position = inspector_placement(chart, bbox, size);
        let rect = egui::Rect::from_min_size(position, size);
        assert!(!rect.intersects(bbox), "a clear candidate exists and wins");
        assert!(chart.contains_rect(rect));
        assert_ne!(
            position,
            egui::pos2(bbox.right() + INSPECTOR_OBJECT_GAP_PX, bbox.top()),
            "beside-the-object is exactly the placement this rule replaced"
        );
        assert!(
            placement_candidates(chart, bbox, size)[4..].contains(&position),
            "the winner is one of the four chart corners"
        );
        assert_eq!(
            position,
            inspector_placement(chart, bbox, size),
            "identical inputs give identical placements"
        );
    }

    /// With the object centred, all four corners are equidistant, so the
    /// order tie-break decides — and it must always decide the same way, or
    /// the panel appears somewhere new every time (Duda, §D3).
    #[test]
    fn placement_prefers_the_top_left_corner_on_a_tie() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 800.0));
        let bbox = egui::Rect::from_center_size(chart.center(), egui::vec2(40.0, 40.0));
        let size = egui::vec2(320.0, 280.0);
        let gap = INSPECTOR_OBJECT_GAP_PX;
        assert_eq!(
            inspector_placement(chart, bbox, size),
            egui::pos2(chart.left() + gap, chart.top() + gap)
        );
    }

    /// A panel taller than the placement assumed must not be sent to a bottom
    /// corner, where it would grow off the window and lose its last rows. The
    /// top-first order is what guarantees that, so it is worth a test of its
    /// own: whatever height the panel turns out to have, the chosen spot
    /// leaves room for it below.
    #[test]
    fn placement_leaves_a_tall_panel_room_to_grow_downwards() {
        let chart = egui::Rect::from_min_size(egui::pos2(60.0, 88.0), egui::vec2(1_224.0, 744.0));
        let bbox = egui::Rect::from_center_size(egui::pos2(700.0, 300.0), egui::vec2(40.0, 40.0));
        // The height the placement believes in, and the height the panel
        // turns out to want once its level editor is open.
        let assumed = egui::vec2(360.0, 280.0);
        let actual = 620.0;
        let position = inspector_placement(chart, bbox, assumed);
        assert!(
            position.y + actual <= chart.bottom(),
            "a panel that grows to {actual} px must still fit below {position:?}"
        );
    }

    /// The farthest clear corner wins, so the panel walks away from the
    /// object instead of hugging the nearest empty spot.
    #[test]
    fn placement_picks_the_corner_farthest_from_the_object() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 800.0));
        // Object parked in the bottom-left quadrant.
        let bbox = egui::Rect::from_center_size(egui::pos2(260.0, 640.0), egui::vec2(40.0, 40.0));
        let size = egui::vec2(320.0, 280.0);
        let gap = INSPECTOR_OBJECT_GAP_PX;
        assert_eq!(
            inspector_placement(chart, bbox, size),
            egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
            "the opposite corner is the farthest clear one"
        );
    }

    #[test]
    fn placement_picks_the_least_overlap_when_nothing_clears() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 600.0));
        // The object spans ~90% of the pane: every candidate overlaps.
        let bbox = chart.shrink2(egui::vec2(50.0, 30.0));
        let size = egui::vec2(320.0, 280.0);
        let position = inspector_placement(chart, bbox, size);
        let chosen = egui::Rect::from_min_size(position, size)
            .intersect(bbox)
            .area();
        for candidate in placement_candidates(chart, bbox, size) {
            let overlap = egui::Rect::from_min_size(candidate, size)
                .intersect(bbox)
                .area();
            assert!(
                chosen <= overlap + 0.01,
                "the chosen spot must cover the object least: {chosen} vs {overlap}"
            );
        }
        assert!(chart.contains_rect(egui::Rect::from_min_size(position, size)));
    }

    #[test]
    fn placement_never_returns_the_blind_first_candidate_at_the_right_edge() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 600.0));
        let bbox = egui::Rect::from_min_max(egui::pos2(900.0, 200.0), egui::pos2(1_000.0, 300.0));
        let size = egui::vec2(320.0, 280.0);
        let position = inspector_placement(chart, bbox, size);
        let rect = egui::Rect::from_min_size(position, size);
        assert!(
            !rect.intersects(bbox),
            "left of the object clears; the old code clamped right-of back onto it"
        );
        assert!(
            position.x < bbox.left(),
            "the panel must sit clear on the left, not clamp over the object"
        );
    }

    #[test]
    fn a_moved_inspector_keeps_its_position_across_selection_changes() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        for price_y in [250.0, 400.0] {
            app.toolrail
                .arm(Tool::Drawing(drawing_tool("horizontal-line")));
            click_chart(&mut app, &ctx, egui::pos2(700.0, price_y));
        }
        click_chart(&mut app, &ctx, egui::pos2(400.0, 250.0));
        run_frame(&mut app, &ctx);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        // Drag by the title bar (left of the trailing icons).
        let bar = egui::pos2(inspector.left() + 60.0, inspector.top() + 14.0);
        drag_chart(&mut app, &ctx, bar, bar + egui::vec2(150.0, 120.0));
        assert!(
            app.inspector_moved,
            "a title-bar drag records the manual move"
        );
        let held = app.inspector_pos.expect("the manual position is recorded");

        // Selecting the other line must not snap the window back.
        click_chart(&mut app, &ctx, egui::pos2(400.0, 400.0));
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.inspector_pos,
            Some(held),
            "the manual position survives a selection change"
        );
        assert!(app.inspector_moved, "the manual flag is never auto-cleared");
    }

    #[test]
    fn double_clicking_the_title_bar_returns_to_automatic_placement() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        run_frame(&mut app, &ctx);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        let bar = egui::pos2(inspector.left() + 60.0, inspector.top() + 14.0);
        drag_chart(&mut app, &ctx, bar, bar + egui::vec2(120.0, 90.0));
        assert!(app.inspector_moved);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("still open");
        let bar = egui::pos2(inspector.left() + 60.0, inspector.top() + 14.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(bar),
                pointer_button(bar, true),
                pointer_button(bar, false),
                pointer_button(bar, true),
                pointer_button(bar, false),
            ],
        );
        assert!(
            !app.inspector_moved,
            "double-click on the title bar re-arms automatic placement"
        );
    }

    fn run_sized_frame(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        run_frame_sized(app, ctx, size, events, egui::Modifiers::NONE)
    }

    fn click_sized(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        size: egui::Vec2,
        position: egui::Pos2,
    ) {
        run_sized_frame(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
            ],
        );
        run_sized_frame(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, false),
            ],
        );
    }

    #[test]
    fn a_narrow_chart_opens_the_inspector_pinned_until_the_pin_is_touched() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        let narrow = egui::vec2(1_150.0, 900.0);
        run_sized_frame(&mut app, &ctx, narrow, Vec::new());
        assert!(
            app.focused_pane()
                .last_chart_area
                .is_some_and(|chart| chart.width() < INSPECTOR_AUTO_PIN_CHART_WIDTH_PX),
            "this proof needs a chart narrower than the auto-pin threshold"
        );
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_sized(&mut app, &ctx, narrow, egui::pos2(600.0, 300.0));
        run_sized_frame(&mut app, &ctx, narrow, Vec::new());
        assert!(
            app.inspector_pinned,
            "a fresh selection on a narrow chart opens pinned"
        );

        // The user unpins: their preference holds from here on.
        run_sized_frame(&mut app, &ctx, narrow, Vec::new());
        let pin = app.inspector_pin_rect.expect("the panel renders its pin");
        click_sized(&mut app, &ctx, narrow, pin.center());
        assert!(!app.inspector_pinned, "the pin toggles the panel off");
        assert!(app.inspector_pin_touched, "the preference is recorded");

        // Unpinning with the same selection must recompute placement — not
        // fall back to the fixed default corner (the pinned host claimed the
        // selection each frame, so the floating host has to re-place it).
        run_sized_frame(&mut app, &ctx, narrow, Vec::new());
        run_sized_frame(&mut app, &ctx, narrow, Vec::new());
        let floating = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("unpinning reopens the floating inspector");
        assert_ne!(
            floating.min, DRAWING_INSPECTOR_DEFAULT_POSITION,
            "the reopened window must be placed, not parked at the default"
        );
        // The selected line's anchor bbox (anchor ± select radius).
        let bbox = egui::Rect::from_center_size(egui::pos2(600.0, 300.0), egui::vec2(24.0, 24.0));
        assert!(
            !floating.intersects(bbox),
            "the reopened window sits beside the selected object: {floating:?}"
        );

        // Deselect, reselect: same width, but the touched pin wins now.
        run_sized_frame(
            &mut app,
            &ctx,
            narrow,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        click_sized(&mut app, &ctx, narrow, egui::pos2(500.0, 300.0));
        run_sized_frame(&mut app, &ctx, narrow, Vec::new());
        assert!(
            !app.inspector_pinned,
            "once touched, the auto-pin width rule stops firing"
        );
    }

    #[test]
    fn hovering_the_inspector_sets_no_chart_cursor() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        run_frame(&mut app, &ctx);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        // Parked over the stroke by hand: automatic placement now clears the
        // object on purpose (§D3), and this proof needs the overlap.
        app.inspector_moved = true;
        app.inspector_pos = Some(egui::pos2(
            inspector.left(),
            300.0 - inspector.height() / 2.0,
        ));
        run_frame(&mut app, &ctx);
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is still open");
        // Over the inspector AND over the selected line's stroke: without
        // the chrome gate this hover would show a Move cursor.
        let hover = egui::pos2(inspector.center().x, 300.0);
        assert!(inspector.contains(hover));
        let output = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(hover)]);
        assert!(
            !matches!(
                output.platform_output.cursor_icon,
                egui::CursorIcon::Move
                    | egui::CursorIcon::ResizeNwSe
                    | egui::CursorIcon::NotAllowed
            ),
            "the chart must not read a hover through the inspector; got {:?}",
            output.platform_output.cursor_icon
        );
    }

    #[test]
    fn the_object_manager_opens_beside_the_rail() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        let objects = app
            .toolrail
            .objects_button_rect()
            .expect("the rail shows the Objects entry");
        click_chart(&mut app, &ctx, objects.center());
        assert!(app.drawing_manager_open);
        run_frame(&mut app, &ctx);

        let manager = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_manager")))
            .expect("the manager is open");
        let chart = app.focused_pane().last_chart_area.expect("chart laid out");
        // Default dock is Left: the manager opens one gap inboard of the
        // rail's inner edge, aligned with its leading (top) end.
        assert!(
            (manager.left() - (chart.left() + DRAWING_MANAGER_GAP_PX)).abs() < 1.0,
            "manager left edge: {} vs chart {}",
            manager.left(),
            chart.left()
        );
        assert!(
            (manager.top() - (chart.top() + DRAWING_MANAGER_GAP_PX)).abs() < 1.0,
            "manager top edge: {} vs chart {}",
            manager.top(),
            chart.top()
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
            app.toast.is_some(),
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
        // Clear of the inspector the selection opened.
        let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        for step in [
            start + egui::vec2(10.0, 12.0),
            start + egui::vec2(25.0, 26.0),
        ] {
            run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(step)]);
        }
        let end = start + egui::vec2(40.0, 40.0);
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
        app.active_tab_mut()
            .flow_pane
            .drawings
            .place(drawing_tool("horizontal-line"), ChartPoint::at(1.0, 100.0));

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        // Through the window's own drain: the tab drops the marks, and the
        // window is what turns that into the toast.
        app.drain_tabs();
        assert!(app.active_tab().flow_pane.drawings.items().is_empty());
        assert!(app.toast.is_some(), "the clear must raise the notice toast");

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
        run_frame(&mut app, &ctx);
        // The level editor is taller than the window. Everything in it must
        // still be *reachable* — which is what the panel's scroll is for, and
        // what a silent cut at the window edge used to deny.
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        let over = inspector.center();
        let mut texts = painted_text(&run_frame(&mut app, &ctx));
        for _ in 0..12 {
            if texts.iter().any(|text| text.contains("log scale")) {
                break;
            }
            texts = painted_text(&run_frame_with_events(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(over),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -120.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            ));
        }
        for label in ["band opacity", "log scale"] {
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
                    default_layout: None,
                    default_bars: None,
                },
                FeedConfig {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["BBB".to_string()],
                    bubble_preset: None,
                    default_layout: None,
                    default_bars: None,
                },
            ],
            metatrader: Default::default(),
            paper: Default::default(),
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
            default_layout: None,
            default_bars: None,
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

    /// Where `price` sits on `side`'s axis, computed the way that pane's own
    /// frame computes it.
    fn price_y(app: &QuantickApp, side: PaneSide, price: f64) -> f32 {
        let pane = app.active_tab().pane(side);
        let chart = pane.last_chart_area.expect("the pane reported its rect");
        let auto = pane.last_auto_range.expect("the pane fitted a range");
        let (lo, hi) = pane.price_view.resolve(auto);
        PriceScale::from_range(lo, hi, chart.top(), chart.bottom()).y(price)
    }

    /// Order entry follows the focused pane — both charts are trading
    /// surfaces, and the press that focuses a pane is the press that acts.
    ///
    /// Proven through the consequence, not the flag: with a fully bracketed
    /// position the entry line consumes a press without moving (it is
    /// history, not an order), so a vertical drag that starts on it pans
    /// nothing — on *either* pane, because the press itself moves the focus
    /// (and with it the simulator's pointer) to the pane it landed in.
    #[test]
    fn the_focused_pane_hands_the_pointer_to_the_simulator() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        app.apply_toolbar_action(ToolbarAction::PaperBuy);
        let fill = trade(4);
        app.active_tab_mut().ingest_live_trade_at(&fill, 0);
        run_frame(&mut app, &ctx);
        assert!(
            app.active_tab().paper.status_cell().is_some(),
            "this proof needs an open simulated position to grab"
        );
        // Both legs set, so the entry-line press blocks instead of starting
        // a bracket-creating drag — the no-pan consequence stays provable.
        app.active_tab_mut()
            .paper
            .apply_sim_command_for_tests(quantick_sim::Command::SetBracket {
                stop_loss: Some(fill.price - rust_decimal::Decimal::from(10)),
                take_profit: Some(fill.price + rust_decimal::Decimal::from(10)),
            });
        let entry = rust_decimal::prelude::ToPrimitive::to_f64(&fill.price)
            .expect("the fill price is finite");

        for side in [PaneSide::Time, PaneSide::Flow] {
            app.active_tab_mut().pane_mut(side).price_view.reset();
            let chart = app
                .active_tab()
                .pane(side)
                .last_chart_area
                .expect("the pane reported its rect");
            let start = egui::pos2(chart.center().x, price_y(&app, side, entry));
            assert!(
                chart.contains(start),
                "{side:?}: the entry line must cross this pane to be grabbed"
            );
            drag_chart(&mut app, &ctx, start, start + egui::vec2(0.0, 40.0));

            assert!(
                app.active_tab().pane(side).price_view.is_auto(),
                "{side:?}: the entry line owns the gesture on the pane the \
                 press focused — the chart must not pan under it"
            );
            assert_eq!(
                app.active_tab().focused_side(),
                side,
                "the press that traded is the press that focused"
            );
        }
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

    /// The Time layout is the timeframe chart alone: built and seeded like
    /// the split's pane, full window (no divider), header included, and the
    /// chrome speaks for it.
    #[test]
    fn the_time_layout_shows_the_timeframe_chart_alone() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        run_frame(&mut app, &ctx);
        app.active_tab_mut().set_layout(CanvasLayout::Time);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let tab = app.active_tab();
        let time = tab.time_pane.as_ref().expect("the pane was built");
        assert_eq!(
            time.state.trades().len(),
            200,
            "and seeded, so it opens showing the market"
        );
        assert_eq!(
            tab.focused_side(),
            PaneSide::Time,
            "the chrome speaks for the one visible chart"
        );
        assert!(
            tab.canvas_divider_rect().is_none(),
            "one pane, no divider to drag"
        );
        let chip = tab.time_header_chip(0).expect("chips recorded");
        assert!(
            chip.is_positive(),
            "the header still offers its timeframes at full width"
        );
    }

    /// A scratch workspace path, so a test never writes the real cockpit.
    fn scratch_ui_state(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "quantick-app-ui-state-{name}-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    /// What the window is showing is what the file records — the arrangement
    /// is read off the live state at save time, so nothing can be arranged
    /// through a path that forgot to mark it.
    #[test]
    fn the_saved_workspace_describes_the_window_that_saved_it() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("capture");
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        app.active_tab_mut().split_fraction = 0.35;
        app.active_tab_mut().focus = PaneSide::Flow;
        app.tz = TzOffset::new(-180);
        app.dock.open_tab(DockTab::Trading);
        app.toolrail.set_dock(ToolboxDock::Right);
        app.show_perf = false;

        let workspace = app.capture_workspace();

        assert_eq!(workspace.tabs.len(), 1);
        let tab = &workspace.tabs[0];
        assert_eq!(tab.layout, crate::config::DeclaredLayout::TimeAndFlow);
        assert_eq!(tab.split_fraction, Some(0.35));
        assert_eq!(tab.focus, Some(ui_state::SavedFocus::Flow));
        assert_eq!(
            tab.flow_bars,
            app.active_tab().flow_pane.state.spec().to_config_string(),
            "the recorded rule is the one the pane is actually on"
        );
        assert!(
            tab.time_bars.is_some(),
            "a tab showing the split records the interval its time pane is on"
        );
        let chrome = workspace.chrome.expect("the chrome is part of a workspace");
        assert_eq!(chrome.timezone_minutes, -180);
        assert_eq!(chrome.dock_tab, Some(ui_state::SavedDockTab::Trading));
        assert_eq!(chrome.rail_dock, ui_state::SavedRailDock::Right);
        assert!(!chrome.perf_readings);
    }

    /// And restoring it puts the window back. The pair is the whole feature:
    /// a capture nothing can reopen is a file, not a workspace.
    #[test]
    fn a_restored_workspace_puts_the_window_back() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.restore_workspace(ui_state::Workspace::new(
            true,
            None,
            0,
            vec![ui_state::SavedTab {
                feed: "binance".to_owned(),
                symbol: "TESTUSDT".to_owned(),
                layout: crate::config::DeclaredLayout::TimeAndFlow,
                split_fraction: Some(0.4),
                focus: Some(ui_state::SavedFocus::Flow),
                flow_bars: "dollar:250000".to_owned(),
                time_bars: Some("time:5m".to_owned()),
            }],
            Some(ui_state::SavedChrome {
                timezone_minutes: 330,
                dock_visible: false,
                dock_tab: Some(ui_state::SavedDockTab::Trades),
                rail_visible: false,
                rail_dock: ui_state::SavedRailDock::Bottom,
                perf_readings: false,
            }),
        ));
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let tab = app.active_tab();
        assert_eq!(tab.layout, CanvasLayout::TimeAndFlow);
        assert!((tab.split_fraction - 0.4).abs() < f32::EPSILON);
        assert_eq!(
            tab.focused_side(),
            PaneSide::Flow,
            "the saved focus wins over the pane the layout switch revealed"
        );
        assert_eq!(
            tab.flow_pane.state.spec(),
            &BarSpec::Dollar(rust_decimal::Decimal::from(250_000)),
            "the flow pane opens on the rule the workspace recorded"
        );
        assert_eq!(
            tab.time_pane.as_ref().map(|pane| pane.state.spec().clone()),
            Some(BarSpec::Time(300_000)),
            "and the time pane on its saved interval, not the header default"
        );
        assert_eq!(app.tz.minutes(), 330);
        assert!(
            !app.dock.visible(),
            "a dock the trader hid stays hidden, tab remembered underneath"
        );
        assert_eq!(app.dock.tab(), Some(DockTab::Trades));
        assert!(!app.toolrail.visible());
        assert_eq!(app.toolrail.dock(), ToolboxDock::Bottom);
        assert!(!app.show_perf);
    }

    /// The BARS selectors read the pane's own fields, so restoring the state
    /// without them would give the trader a chart whose controls disagree with
    /// it — and snap it back to a rule they never chose on first touch.
    #[test]
    fn a_restored_bar_rule_moves_the_selector_that_edits_it() {
        let (mut app, _commands) = app_with_history(50);
        app.restore_workspace(ui_state::Workspace::new(
            true,
            None,
            0,
            vec![ui_state::SavedTab {
                feed: "binance".to_owned(),
                symbol: "TESTUSDT".to_owned(),
                layout: crate::config::DeclaredLayout::Flow,
                split_fraction: None,
                focus: None,
                flow_bars: "tick:377".to_owned(),
                time_bars: None,
            }],
            None,
        ));
        let pane = &app.active_tab().flow_pane;
        assert_eq!(pane.state.spec(), &BarSpec::Tick(377));
        assert_eq!(pane.tick_n, 377, "the selector moved with the rule");
        assert_eq!(pane.kind, crate::state::BarKind::Tick);
    }

    /// Saving says so. A trader who arranges a cockpit and clicks Save has no
    /// other way to tell it worked than restarting — and it says so through
    /// the acknowledgement channel the window already has, rather than by
    /// pushing a cell onto the status line and sliding the readings sideways
    /// for eight seconds.
    #[test]
    fn saving_the_workspace_acknowledges_itself() {
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("notice");
        assert!(app.toast.is_none());

        app.save_workspace("test");

        let toast = app.toast.as_ref().expect("the save reports itself");
        assert!(
            toast.message.contains("saved"),
            "the answer has to say what happened, got '{}'",
            toast.message
        );
        assert!(
            !toast.offers_undo,
            "the file it replaced is gone; an Undo button here would lie"
        );
        assert!(
            app.ui_state_path.exists(),
            "and the file it claims to have written is on disk"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// Resetting forgets the file without rearranging the charts the trader is
    /// reading: the entry governs the *startup* layout, not this session.
    #[test]
    fn resetting_the_startup_layout_leaves_this_session_alone() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("reset");
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        app.save_workspace("test");
        assert!(app.ui_state_path.exists());

        app.forget_workspace();

        assert!(
            !app.ui_state_path.exists(),
            "the next launch opens on config"
        );
        assert_eq!(
            app.active_tab().layout,
            CanvasLayout::TimeAndFlow,
            "the charts on screen are not the trader's startup preference"
        );
    }

    /// A window that opens on the split focuses the flow chart, not the
    /// context beside it.
    ///
    /// Caught by looking at the shipped default on screen: the BARS group and
    /// the status line were speaking for the timeframe pane, so the first
    /// thing a trader touched on a fresh launch would have re-cut the context
    /// chart instead of quantick's own. `set_layout` focusing what it reveals
    /// is right for a menu click and wrong for an opening.
    #[test]
    fn a_window_that_opens_on_the_split_focuses_the_flow_chart() {
        let ctx = egui::Context::default();
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        let mut config = test_config();
        config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::TimeAndFlow);
        let mut app = QuantickApp::new(
            config,
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
        let _ends = (evt_tx, book_tx);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);
        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Flow,
            "a fresh window's controls speak for the chart quantick is built around"
        );
        assert_eq!(
            app.status_model().spec_summary,
            "tick(50)",
            "and so does the status line"
        );
    }

    /// The one layout that has no flow pane to focus still focuses something.
    #[test]
    fn a_window_that_opens_on_the_timeframe_alone_focuses_it() {
        let ctx = egui::Context::default();
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        let mut config = test_config();
        config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::Time);
        let mut app = QuantickApp::new(
            config,
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
        let _ends = (evt_tx, book_tx);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        assert_eq!(app.active_tab().focused_side(), PaneSide::Time);
    }

    /// Naming an arrangement keeps it without touching what the app opens on.
    /// The two are separate settings, and a trader saving a way back must not
    /// discover they also redefined their opening screen.
    #[test]
    fn naming_an_arrangement_does_not_change_what_opens() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("named-startup");
        app.active_tab_mut().set_layout(CanvasLayout::Single);
        run_frame(&mut app, &ctx);
        app.save_workspace("test");
        let startup_before = ui_state::load(&app.ui_state_path).tabs;

        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        app.save_named_workspace("scalp");

        let file = ui_state::load(&app.ui_state_path);
        assert_eq!(
            file.tabs, startup_before,
            "the startup arrangement is untouched by a bookmark"
        );
        let saved = file.named("scalp").expect("the bookmark is in the file");
        assert_eq!(
            saved.tabs.first().map(|tab| tab.layout),
            Some(crate::config::DeclaredLayout::TimeAndFlow),
            "and the bookmark holds the arrangement that was on screen"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// Saving the startup screen must not throw the bookmarks away: every
    /// write rewrites the whole file.
    #[test]
    fn saving_the_startup_screen_keeps_the_bookmarks() {
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("bookmarks-survive");
        app.save_named_workspace("scalp");

        app.save_workspace("test");

        assert!(
            ui_state::load(&app.ui_state_path).named("scalp").is_some(),
            "a bookmark cannot be collateral damage of saving the startup screen"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// The same name twice replaces, so the menu never grows five entries
    /// called "scalp".
    #[test]
    fn saving_over_a_name_replaces_that_bookmark() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("replace");
        app.active_tab_mut().set_layout(CanvasLayout::Single);
        run_frame(&mut app, &ctx);
        app.save_named_workspace("scalp");

        app.active_tab_mut().set_layout(CanvasLayout::Time);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        app.save_named_workspace("  scalp  ");

        let file = ui_state::load(&app.ui_state_path);
        assert_eq!(file.saved.len(), 1, "one name, one bookmark");
        assert_eq!(
            file.named("scalp")
                .and_then(|e| e.tabs.first())
                .map(|t| t.layout),
            Some(crate::config::DeclaredLayout::Time),
            "and it holds the newer arrangement"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// Opening a bookmark replaces the whole tab strip — which is only
    /// possible by growing before shrinking, since the last tab cannot close.
    #[test]
    fn opening_a_bookmark_replaces_what_is_on_screen() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("open");
        app.active_tab_mut().set_layout(CanvasLayout::Time);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        app.tz = TzOffset::new(0);
        app.save_named_workspace("context");

        // Drift away from it, then come back.
        app.active_tab_mut().set_layout(CanvasLayout::Single);
        app.tz = TzOffset::new(-180);
        run_frame(&mut app, &ctx);

        app.open_named_workspace("context");
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        assert_eq!(app.tabs.len(), 1, "the strip is replaced, not appended to");
        assert_eq!(app.active_tab().layout, CanvasLayout::Time);
        assert_eq!(app.tz.minutes(), 0, "the chrome comes back with it");
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// Deleting a bookmark throws away a way back, not the place you are.
    #[test]
    fn deleting_a_bookmark_leaves_the_window_alone() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("delete");
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        app.save_named_workspace("scalp");

        app.delete_named_workspace("scalp");

        assert!(ui_state::load(&app.ui_state_path).named("scalp").is_none());
        assert_eq!(
            app.active_tab().layout,
            CanvasLayout::TimeAndFlow,
            "the charts on screen are not what was deleted"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// The reason the user asked for named workspaces: a way back after a
    /// reset. Reset deleting the bookmarks would break the feature at exactly
    /// the moment it exists for.
    #[test]
    fn resetting_the_startup_layout_keeps_the_bookmarks() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("reset-keeps");
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        app.save_named_workspace("before the mess");
        app.save_workspace("test");

        app.forget_workspace();

        let file = ui_state::load(&app.ui_state_path);
        assert!(
            file.tabs.is_empty(),
            "the startup arrangement is what Reset clears"
        );
        assert!(
            file.named("before the mess").is_some(),
            "the way back survives the reset it exists for"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// With nothing named, Reset still removes the file outright.
    #[test]
    fn resetting_with_no_bookmarks_removes_the_file() {
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("reset-removes");
        app.save_workspace("test");
        assert!(app.ui_state_path.exists());

        app.forget_workspace();

        assert!(!app.ui_state_path.exists());
    }

    /// A name that is only whitespace is not a name.
    #[test]
    fn a_blank_name_saves_nothing_and_says_so() {
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("blank");

        app.save_named_workspace("   ");

        assert!(app.bookmarks.is_empty());
        assert!(
            !app.ui_state_path.exists(),
            "a refused save must not write the file either"
        );
        assert!(
            app.toast
                .as_ref()
                .is_some_and(|toast| toast.message.contains("needs a name")),
            "and the trader is told why nothing happened"
        );
    }

    /// A frame carrying the window's close request, which is the only signal
    /// the exit save has to work from.
    fn close_requested_frame(app: &mut QuantickApp, ctx: &egui::Context) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        let _ = ctx.run(input, |ctx| app.draw_frame(ctx, Instant::now()));
    }

    /// The automatic tier: a trader who never opens the Workspace menu still
    /// reopens where they left off. Without this the feature is only the
    /// explicit half, and the half most people would never find.
    #[test]
    fn closing_the_window_keeps_the_arrangement_when_autosave_is_on() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("exit-save");
        app.save_on_exit = true;
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);

        close_requested_frame(&mut app, &ctx);

        let saved = ui_state::load(&app.ui_state_path);
        assert_eq!(
            saved.tabs.first().map(|tab| tab.layout),
            Some(crate::config::DeclaredLayout::TimeAndFlow),
            "the window that closed is the window that reopens"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// And switching it off means exactly that: the trader who curates their
    /// startup layout by hand must not have it overwritten by whatever their
    /// last session drifted into.
    #[test]
    fn closing_the_window_writes_nothing_when_autosave_is_off() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("exit-no-save");
        app.save_on_exit = false;
        run_frame(&mut app, &ctx);

        close_requested_frame(&mut app, &ctx);

        assert!(
            !app.ui_state_path.exists(),
            "autosave off must leave the saved workspace untouched"
        );
    }

    /// Autosave is a property of the file it governs, so switching it has to
    /// reach the disk on the spot — waiting for the exit would mean waiting
    /// for the exit it may have just switched off.
    #[test]
    fn switching_autosave_off_is_itself_saved() {
        let (mut app, _commands) = app_with_history(50);
        app.ui_state_path = scratch_ui_state("autosave");
        app.save_on_exit = false;
        app.save_workspace("save_on_exit_toggled");

        assert!(
            !ui_state::load(&app.ui_state_path).save_on_exit,
            "a trader who switched autosave off must not find it back on"
        );
        let _ = std::fs::remove_file(&app.ui_state_path);
    }

    /// Switching layouts focuses the pane the switch reveals, so the first
    /// command after a switch already lands on the chart that just appeared
    /// (audit: opening the split did not focus the pane it created).
    #[test]
    fn switching_layouts_focuses_the_pane_the_switch_reveals() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(50);
        run_frame(&mut app, &ctx);

        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Time,
            "coming from Single, the split reveals the time pane"
        );

        app.active_tab_mut().set_layout(CanvasLayout::Single);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);

        app.active_tab_mut().set_layout(CanvasLayout::Time);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Time);

        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        assert_eq!(
            app.active_tab().focused_side(),
            PaneSide::Flow,
            "coming from Time, the split reveals the flow pane"
        );
    }

    /// The BARS group edits the focused pane — the same pane the status bar
    /// reads and indicator commands land on. In the Time layout that is the
    /// timeframe chart on screen; the hidden flow pane is untouched.
    #[test]
    fn the_bars_selectors_govern_the_focused_pane() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        run_frame(&mut app, &ctx);
        app.active_tab_mut().set_layout(CanvasLayout::Time);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let flow_spec = app.active_tab().flow_pane.state.spec().clone();

        // The exact selector fields the toolbar's BARS group borrows for the
        // focused pane, written through the same deferred-spec path.
        let pane = app.active_tab_mut().focused_pane_mut();
        pane.kind = crate::state::BarKind::Time;
        pane.time_interval_ms = 300_000;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();

        assert_eq!(
            app.active_tab()
                .time_pane
                .as_ref()
                .expect("time pane")
                .state
                .spec(),
            &BarSpec::Time(300_000),
            "the change lands on the chart on screen"
        );
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &flow_spec,
            "and not on the hidden one"
        );
    }

    /// A feed declaring `default_layout` and `default_bars` opens wearing
    /// them: the declared canvas, the declared spec on the flow pane, the
    /// declared interval on the timeframe pane — and the venue asked for the
    /// candle history the pane needs.
    #[test]
    fn a_feed_declaring_layout_and_bars_opens_wearing_them() {
        let ctx = egui::Context::default();
        let (_evt_tx, evt_rx) = mpsc::channel(64);
        let (_book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let mut config = test_config();
        config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::Time);
        config.feeds[0].default_bars = Some("time:5m".to_string());
        // The declared spec reaches Tab::new the same way main.rs resolves it.
        let spec = config.startup_spec_for("binance").expect("declared spec");
        let mut app = QuantickApp::new(
            config,
            "binance",
            "TESTUSDT",
            spec,
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(FeedCapabilities {
                    book_capture: false,
                    history_paging: true,
                    traded_volume: true,
                    ohlcv_history: true,
                    ohlcv_generation: 0,
                }),
                commands: cmd_tx,
                replay: None,
            },
        );

        assert_eq!(app.active_tab().layout, CanvasLayout::Time);
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &BarSpec::Time(300_000),
            "the flow pane opened on the declared spec"
        );
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab()
                .time_pane
                .as_ref()
                .expect("built the frame after startup")
                .state
                .spec(),
            &BarSpec::Time(300_000),
            "the timeframe pane opens on the declared interval too"
        );
        assert_eq!(app.active_tab().focused_side(), PaneSide::Time);
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            1,
            "the timeframe chart asked the venue for its history"
        );
    }

    /// A new tab on a feed that declares its own defaults takes them; one on
    /// a feed that declares nothing keeps inheriting from the tab you were on.
    #[test]
    fn a_new_tab_takes_the_feeds_declared_defaults_over_inheritance() {
        let mut config = test_config();
        config.feeds.push(FeedConfig {
            id: "mt".to_string(),
            name: "MetaTrader 5".to_string(),
            provider: ProviderKind::MetaTrader,
            symbols: vec!["WINQ26".to_string()],
            bubble_preset: None,
            default_layout: Some(crate::config::DeclaredLayout::TimeAndFlow),
            default_bars: Some("tick:7".to_string()),
        });
        let mut app = app_on(config, "binance", "TESTUSDT");
        assert_eq!(app.active_tab().layout, CanvasLayout::Single);

        app.adopt_tab("mt".to_string(), "WINQ26".to_string(), stub_feed().0, None);
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &BarSpec::Tick(7),
            "the declaration wins over the inherited spec"
        );
        assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);

        app.adopt_tab(
            "binance".to_string(),
            "ETHUSDT".to_string(),
            stub_feed().0,
            None,
        );
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &BarSpec::Tick(7),
            "a feed declaring nothing still inherits from the tab you were on"
        );
        assert_eq!(
            app.active_tab().layout,
            CanvasLayout::Single,
            "and opens on the factory canvas"
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

    /// Apply keeps the dialog open (audit M2): tuning is a nudge-and-look
    /// loop, and each Apply must land without re-opening anything. The nudge
    /// really lands — the EMA's length changes and the view retitles.
    #[test]
    fn apply_keeps_the_settings_dialog_open_and_lands_the_draft() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let point = pane_point(&app, PaneSide::Flow);
        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
        settle_indicators(&mut app);
        let slot = app.active_tab().flow_pane.indicators.all()[0].slot;
        app.apply_toolbar_action(ToolbarAction::OpenIndicatorSettings(slot.0));
        assert!(app.indicator_settings.is_some(), "the dialog opened");

        if let Some(dialog) = app.indicator_settings.as_mut()
            && let Some(quantick_indicators::InputValue::Int(len)) = dialog.draft.first_mut()
        {
            *len = 21;
        }
        app.apply_indicator_settings_draft();
        assert!(
            app.indicator_settings.is_some(),
            "Apply keeps the dialog open for the next nudge"
        );
        settle_indicators(&mut app);
        let label = app.active_tab().flow_pane.indicators.all()[0]
            .label()
            .to_owned();
        assert!(
            label.contains("21"),
            "the applied draft rebuilt the indicator: {label}"
        );
    }

    /// A legend row acts on the pane it is drawn on, never the focused one —
    /// the routing that keeps the audit's "commands target one pane, chrome
    /// speaks for another" contradiction from reappearing here.
    #[test]
    fn legend_actions_land_on_their_own_pane_not_the_focused_one() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        // An EMA on the time pane...
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
        settle_indicators(&mut app);
        let slot = app
            .active_tab()
            .time_pane
            .as_ref()
            .expect("time pane")
            .indicators
            .all()[0]
            .slot;
        // ...while focus returns to the flow pane.
        let point = pane_point(&app, PaneSide::Flow);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);

        let target = TabSlot {
            tab: app.active_tab().id,
            side: PaneSide::Time,
            slot,
        };
        app.toggle_indicator_hidden_at(target);
        assert!(
            app.active_tab()
                .time_pane
                .as_ref()
                .expect("time pane")
                .indicators
                .all()[0]
                .hidden,
            "the time pane's own slot was toggled, focus notwithstanding"
        );
        app.open_indicator_settings_at(target);
        assert!(app.indicator_settings.is_some());
        assert_eq!(
            app.indicator_settings_target.side,
            PaneSide::Time,
            "the dialog's Apply will land on the legend's pane"
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

    /// A divider drag belongs to the tab it started on. egui keeps drag state
    /// per interaction id, so one id shared across tabs would hand the
    /// in-flight gesture to the next tab's divider the moment `Ctrl+Tab`
    /// fires under a held button — the tab-level case of the rule
    /// [`crate::pane`] states for panes.
    #[test]
    fn a_divider_drag_does_not_follow_a_tab_switch() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        // A second market, split as well, so both tabs register a divider.
        app.adopt_tab(
            "binance".to_owned(),
            "ETHUSDT".to_owned(),
            stub_feed().0,
            None,
        );
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let untouched = app.tabs[1].split_fraction;

        // Back to the first tab, and press its divider.
        app.active_tab = 0;
        run_frame(&mut app, &ctx);
        let grab = app
            .active_tab()
            .canvas_divider_rect()
            .expect("the first tab's divider was registered")
            .center();
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
        );

        // Ctrl+Tab mid-gesture, with the button still down.
        app.cycle_tab(1);
        let moved = egui::pos2(grab.x + 120.0, grab.y);
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(moved)]);
        run_frame_with_events(&mut app, &ctx, vec![pointer_button(moved, false)]);

        assert_eq!(
            app.tabs[1].split_fraction, untouched,
            "the second tab's divider must not inherit the first tab's drag"
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

    /// A feed handle wired to channels nothing sends on, for a tab a test
    /// opens but does not drive.
    fn stub_feed() -> (FeedHandle, TabEnds) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        (
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
            TabEnds {
                events: evt_tx,
                book: book_tx,
                commands: cmd_rx,
            },
        )
    }

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
            None,
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

    // ---- venue candle history in the time pane ----

    /// A minute candle at `minute`, priced so a fold is visible in the result.
    fn venue_candle(minute: i64, seed: i64) -> quantick_engine::Bar {
        let open_time = minute * crate::feed::OHLCV_BASE_INTERVAL_MS;
        quantick_engine::Bar {
            open_time,
            close_time: open_time + crate::feed::OHLCV_BASE_INTERVAL_MS - 1,
            open: Decimal::from(100 + seed),
            high: Decimal::from(110 + seed),
            low: Decimal::from(90 + seed),
            close: Decimal::from(105 + seed),
            buy_volume: Decimal::from(2),
            sell_volume: Decimal::from(3),
            trade_count: 7,
        }
    }

    /// A trade one minute after the one before it, so a fixture fills an M1
    /// pane with real bars rather than one long forming candle.
    fn minute_trade(minute: u64) -> quantick_engine::Trade {
        minute_trade_at(minute as i64)
    }

    /// The same, for a minute that may sit before the fixture's first — what
    /// "load older" delivers.
    fn minute_trade_at(minute: i64) -> quantick_engine::Trade {
        let mut trade = trade(minute.unsigned_abs() + 1);
        trade.timestamp_ms = minute * crate::feed::OHLCV_BASE_INTERVAL_MS + 1_000;
        trade
    }

    /// Venue candles for the minutes *before* the fixture trades, which start
    /// in minute 0 — so the prefix never overlaps the engine's own bars.
    fn venue_history(count: i64) -> Vec<quantick_engine::Bar> {
        (-count..0)
            .map(|m| venue_candle(m, m.rem_euclid(5)))
            .collect()
    }

    /// Every FetchOhlcv sitting in the command channel.
    fn drain_ohlcv_requests(commands: &mut mpsc::Receiver<FeedCommand>) -> usize {
        let mut count = 0;
        while let Ok(command) = commands.try_recv() {
            if matches!(command, FeedCommand::FetchOhlcv { .. }) {
                count += 1;
            }
        }
        count
    }

    /// A split app whose feed reports candle history, with the channel ends
    /// the test drives.
    fn history_app(
        ctx: &egui::Context,
    ) -> (
        QuantickApp,
        mpsc::Sender<FeedEvent>,
        mpsc::Receiver<FeedCommand>,
    ) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let mut app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(1),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(FeedCapabilities {
                    book_capture: false,
                    history_paging: true,
                    traded_volume: true,
                    ohlcv_history: true,
                    ohlcv_generation: 0,
                }),
                commands: cmd_tx,
                replay: None,
            },
        );
        let _ = book_tx;
        let trades: Vec<_> = (0..200).map(minute_trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.drain_tabs();
        run_frame(&mut app, ctx);
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, ctx);
        run_frame(&mut app, ctx);
        (app, evt_tx, cmd_rx)
    }

    /// (a) A time pane on a feed that serves candles asks once, and the reply
    /// becomes bars in front of the ones cut from prints.
    #[test]
    fn a_time_pane_asks_for_venue_history_once_and_renders_the_reply() {
        let ctx = egui::Context::default();
        let (mut app, events, mut commands) = history_app(&ctx);

        assert_eq!(
            drain_ohlcv_requests(&mut commands),
            1,
            "the pane asks exactly once when it is built"
        );
        let slots_before = app.active_tab().pane(PaneSide::Time).slots();
        assert!(
            app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "and says it is waiting"
        );

        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();

        let pane = app.active_tab().pane(PaneSide::Time);
        assert_eq!(
            pane.seam_slot(),
            120,
            "the whole prefix stands in front of the engine's bars"
        );
        assert!(pane.slots() > slots_before, "and the chart grew by it");
        assert!(
            !app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "the wait ended with the reply"
        );
        // And it really paints: the axis is there and the candles are drawn.
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "the pane draws its chart: {texts:?}"
        );
    }

    /// S1 end to end on the single-pane route: `bars → time` on the flow
    /// pane asks the venue for candle history and wears the reply as its
    /// prefix — the fix for the audit's BLOCKER-1, where the toolbar route
    /// produced a 1-second chart and then an empty one. The venue prefix
    /// belongs to what a pane shows, never to which pane object it is.
    #[test]
    fn the_flow_pane_cutting_time_bars_earns_the_venue_prefix() {
        let ctx = egui::Context::default();
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let mut app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(1),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(FeedCapabilities {
                    book_capture: false,
                    history_paging: true,
                    traded_volume: true,
                    ohlcv_history: true,
                    ohlcv_generation: 0,
                }),
                commands: cmd_tx,
                replay: None,
            },
        );
        let _ = book_tx;
        let trades: Vec<_> = (0..200).map(minute_trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.drain_tabs();
        run_frame(&mut app, &ctx);
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            0,
            "a tick chart asks for no candles"
        );

        // The toolbar route: `bars → time`. The kind's default interval is a
        // real timeframe (QW2), so the spec that lands is one minute.
        app.active_tab_mut().flow_pane.kind = crate::state::BarKind::Time;
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert_eq!(
            *app.active_tab().flow_pane.state.spec(),
            BarSpec::Time(crate::time_header::DEFAULT_INTERVAL_MS),
            "bars → time opens on 1m, not one second"
        );
        assert!(
            drain_ohlcv_requests(&mut cmd_rx) >= 1,
            "the time-cutting flow pane asks the venue for history"
        );

        evt_tx
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        let tab = app.active_tab();
        assert_eq!(
            tab.flow_pane.seam_slot(),
            120,
            "the venue candles stand in front of the bars cut from prints"
        );

        // And leaving the time kind hands the prefix back: a tick chart is
        // the tape's alone.
        app.active_tab_mut().flow_pane.kind = crate::state::BarKind::Tick;
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().flow_pane.seam_slot(),
            0,
            "a pane that stopped cutting by time carries no venue candles"
        );
    }

    /// "Load older" moves the first engine bar backwards in time, and the
    /// prefix was trimmed against where that bar used to be.
    ///
    /// Three clicks reach this on Binance: split the canvas, let the venue
    /// history land, pull older trades. The venue candles covering the newly
    /// re-cut minutes then sat in front of engine bars covering the *same*
    /// minutes — the window drawn twice, `open_time` going backwards across
    /// the seam, and the precondition `slot_at_time` documents quietly false.
    #[test]
    fn pulling_older_trades_re_trims_the_venue_prefix() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 120);

        // Something anchored to a bar index, and a view off the live edge, so
        // the shift has something to preserve.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        let slots = app.active_tab().pane(PaneSide::Time).slots();
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .viewport
            .pan_pixels(40.0, slots);
        let edge_before = app.active_tab().pane(PaneSide::Time).right_edge_time();
        let mark_before = app.active_tab().pane(PaneSide::Time).drawings.items()[0].points[0];
        let mark_time_before = app
            .active_tab()
            .pane(PaneSide::Time)
            .slot_open_time(mark_before.bar as usize);
        assert!(edge_before.is_some(), "the view is off the live edge");

        // Five minutes of older trades, inside the window the prefix covers.
        let older: Vec<_> = (-5_i64..0).map(minute_trade_at).collect();
        events.try_send(FeedEvent::HistoryPrepended(older)).unwrap();
        app.drain_tabs();

        let pane = app.active_tab().pane(PaneSide::Time);
        let first_engine = pane
            .state
            .bars()
            .first()
            .or_else(|| pane.state.partial())
            .expect("the pane holds bars")
            .open_time;
        assert!(
            pane.history_prefix.iter().all(|bar| bar.open_time
                < crate::resample::bucket_start(first_engine, crate::feed::OHLCV_BASE_INTERVAL_MS)),
            "no venue candle may cover a minute the engine has now re-cut"
        );
        assert_eq!(
            pane.seam_slot(),
            115,
            "the five overlapping buckets left the prefix"
        );
        let opens: Vec<i64> = (0..pane.closed_slots())
            .filter_map(|slot| pane.slot_open_time(slot))
            .collect();
        assert!(
            opens.windows(2).all(|pair| pair[0] <= pair[1]),
            "and open_time still never decreases across the seam"
        );

        // The user was reading a market moment; they still are, and their mark
        // is still on the bar they put it on.
        assert_eq!(
            pane.right_edge_time(),
            edge_before,
            "the view kept the market time it was showing"
        );
        assert_eq!(
            pane.slot_open_time(pane.drawings.items()[0].points[0].bar as usize),
            mark_time_before,
            "and the mark kept the bar it was drawn against"
        );
    }

    /// (b) Changing the timeframe refolds what is already held. A chip click
    /// must never reach the venue.
    #[test]
    fn a_timeframe_change_refolds_locally_without_asking_again() {
        let ctx = egui::Context::default();
        let (mut app, events, mut commands) = history_app(&ctx);
        drain_ohlcv_requests(&mut commands);
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 120);

        // 1m → 5m: the same history, folded five ways.
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .time_interval_ms = 5 * crate::feed::OHLCV_BASE_INTERVAL_MS;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            24,
            "120 minutes are 24 five-minute bars"
        );
        assert_eq!(
            drain_ohlcv_requests(&mut commands),
            0,
            "and the venue was not asked again"
        );
    }

    /// §11's own clause, and one drag away in the UI: an interval that is not
    /// a whole number of minutes gets no prefix rather than an approximated
    /// one, and the pane keeps drawing.
    #[test]
    fn an_unfoldable_interval_drops_the_prefix_and_still_draws() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 120);

        // 90 seconds: a minute and a half, which no whole number of venue
        // candles adds up to.
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .time_interval_ms = 90_000;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            0,
            "no prefix rather than buckets built from fractions of a candle"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "and the pane keeps drawing what it does have: {texts:?}"
        );

        // Back to a foldable one, and the history returns from the same base.
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .time_interval_ms = 5 * crate::feed::OHLCV_BASE_INTERVAL_MS;
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 24);
    }

    /// A feed switch is a different market: the candles that described the old
    /// one go with it, and nothing is left waiting on a reply that can never
    /// arrive down a dropped channel.
    #[test]
    fn switching_the_feed_drops_the_prefix_and_clears_the_wait() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 120);

        // A fresh feed arrives, as a symbol switch installs one.
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        app.active_tab_mut().attach_for_test(FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            commands: cmd_tx,
            replay: None,
        });

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            0,
            "the old market's candles do not describe the new one"
        );
        assert!(
            !app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "and nothing waits on a reply that went with the old channel"
        );
        run_frame(&mut app, &ctx);
    }

    /// The interval a reply carries is tagged rather than assumed. A base this
    /// fold was not written for is refused, not folded wrongly.
    #[test]
    fn a_reply_at_an_unexpected_base_interval_is_refused() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);

        events
            .try_send(FeedEvent::OhlcvHistory {
                // Five-minute candles, from a venue that one day changes its
                // mind about what it serves.
                interval_ms: 5 * crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();

        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            0,
            "bars at an unknown base are not folded as if they were minutes"
        );
        assert!(
            !app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "the reply still answered the request"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(has_price_axis(&texts), "and the pane draws: {texts:?}");
    }

    /// (e) The rebuild an indicator sees spans the prefix, so an average over
    /// three months of context is a real average and not a warm-up.
    #[test]
    fn the_indicator_rebuild_covers_the_venue_prefix() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
        settle_indicators(&mut app);

        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        settle_indicators(&mut app);

        let pane = app.active_tab().pane(PaneSide::Time);
        let view = pane
            .indicators
            .all()
            .first()
            .expect("the EMA is on the time pane");
        assert!(
            view.columns[0].len() >= pane.seam_slot(),
            "the EMA has a row for every venue bar, not just the ones from prints"
        );
        // A value inside the prefix region is a real number, not a warm-up gap.
        let inside = view.columns[0]
            .iter()
            .take(pane.seam_slot())
            .rev()
            .find(|value| value.is_finite());
        assert!(
            inside.is_some(),
            "and the average is finite over the venue history"
        );
    }

    /// (f) The prefix arrives under a chart the user is already reading: the
    /// right edge must not move.
    #[test]
    fn installing_the_prefix_keeps_the_view_where_it_was() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        // Pan off the live edge, so there is a position to preserve.
        let slots = app.active_tab().pane(PaneSide::Time).slots();
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .viewport
            .pan_pixels(40.0, slots);
        let edge_time = app.active_tab().pane(PaneSide::Time).right_edge_time();
        let edge_bar = app
            .active_tab()
            .pane(PaneSide::Time)
            .viewport
            .right_edge_bar(slots);
        assert!(edge_time.is_some(), "the view is off the live edge");

        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();

        let pane = app.active_tab().pane(PaneSide::Time);
        assert_eq!(
            pane.viewport.right_edge_bar(pane.slots()),
            edge_bar + 120.0,
            "the right edge moved with the bars inserted in front of it"
        );
        assert_eq!(
            pane.right_edge_time(),
            edge_time,
            "so the user is still looking at the same market time"
        );
    }

    /// (g) MetaTrader narrows into serving candles after the bridge says
    /// hello, so a pane that asked early was told there was nothing held. The
    /// rising edge asks again, and the empty answer strands no spinner.
    #[test]
    fn a_capability_that_rises_later_is_asked_again() {
        let ctx = egui::Context::default();
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (_book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let (caps_tx, caps_rx) = tokio::sync::watch::channel(FeedCapabilities {
            book_capture: false,
            history_paging: true,
            traded_volume: true,
            ohlcv_history: false,
            ohlcv_generation: 0,
        });
        let mut app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(1),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: caps_rx,
                commands: cmd_tx,
                replay: None,
            },
        );
        let trades: Vec<_> = (0..50).map(minute_trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.drain_tabs();
        run_frame(&mut app, &ctx);
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            0,
            "a feed that says it serves no candles is not asked"
        );

        // The bridge says hello and the session turns out to hold rates.
        caps_tx.send_modify(|caps| caps.ohlcv_history = true);
        app.drain_tabs();
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            1,
            "the rising edge asks once"
        );

        // ...and the venue holds nothing after all.
        evt_tx
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: Vec::new(),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert!(
            !app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "an empty reply is a complete answer, and ends the wait"
        );
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            0,
            "with no prefix rather than a fabricated one"
        );
        app.drain_tabs();
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            0,
            "and the tab stops asking"
        );
    }

    /// (h) A recording is a fixed span of prints with no venue behind it.
    #[test]
    fn a_replaying_tab_never_asks_for_venue_history() {
        let ctx = egui::Context::default();
        let (mut app, _events, mut commands) = history_app(&ctx);
        drain_ohlcv_requests(&mut commands);
        // Put the tab on a recording, then give it a fresh time pane.
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
        let commands_after = std::mem::replace(&mut commands, mpsc::channel(1).1);
        drop(commands_after);
        run_frame(&mut app, &ctx);
        app.drain_tabs();

        assert!(app.active_tab().replay.is_some());
        assert!(
            !app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "a recording has no venue to wait on"
        );
    }

    /// (i) The status bar names all three sources, in the order the chart puts
    /// them in.
    #[test]
    fn the_status_bar_counts_venue_bars_separately() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        assert_eq!(
            app.status_model().venue_bars,
            0,
            "nothing to disclose before the reply"
        );

        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();

        let status = app.status_model();
        assert_eq!(status.venue_bars, 120);
        assert!(
            status.backfilled_bars > 0,
            "and the trade-derived counts are still their own"
        );
        // The flow pane beside it has no prefix and says nothing about one.
        let point = pane_point(&app, PaneSide::Flow);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.status_model().venue_bars, 0);
    }

    /// (j) A venue bucket covering the same window as the first engine bar
    /// would sit after it in time and before it in slot order. It is dropped,
    /// which is what keeps the composed series searchable.
    #[test]
    fn the_seam_drops_a_venue_bucket_that_overlaps_the_first_engine_bar() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        let first_engine_open = app
            .active_tab()
            .pane(PaneSide::Time)
            .state
            .bars()
            .first()
            .map(|bar| bar.open_time)
            .or_else(|| {
                app.active_tab()
                    .pane(PaneSide::Time)
                    .state
                    .partial()
                    .map(|bar| bar.open_time)
            })
            .expect("the pane holds the fixture trades");

        // Two candles before the engine's first bar and one covering it.
        let mut bars = venue_history(2);
        bars.push(venue_candle(
            first_engine_open / crate::feed::OHLCV_BASE_INTERVAL_MS,
            0,
        ));
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars,
                complete: true,
            })
            .unwrap();
        app.drain_tabs();

        let pane = app.active_tab().pane(PaneSide::Time);
        assert_eq!(
            pane.seam_slot(),
            2,
            "the overlapping bucket is dropped; what the app cut from prints \
             is the better record of that window"
        );
        // The composed series is non-decreasing in open_time, which is what
        // the slot search depends on.
        let opens: Vec<i64> = (0..pane.closed_slots())
            .filter_map(|slot| pane.slot_open_time(slot))
            .collect();
        assert!(
            opens.windows(2).all(|pair| pair[0] <= pair[1]),
            "open_time never decreases across the seam"
        );
    }

    /// (d) Two dividers, two boundaries: the venue seam and the backfill mark
    /// sit at their own slots and neither moves the other.
    #[test]
    fn the_seam_and_the_backfill_divider_mark_different_slots() {
        let ctx = egui::Context::default();
        let (mut app, events, _commands) = history_app(&ctx);
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(120),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        // Live prints after the backfill, so the backfill boundary is real.
        for minute in 200..205 {
            let trade = minute_trade(minute);
            app.active_tab_mut()
                .ingest_live_trade_at(&trade, trade.timestamp_ms);
        }
        run_frame(&mut app, &ctx);

        let pane = app.active_tab().pane(PaneSide::Time);
        let seam = pane.seam_slot();
        let backfill = pane
            .state
            .backfill_boundary()
            .expect("the pane took a backfill batch");
        assert_eq!(seam, 120);
        assert!(
            backfill + seam > seam,
            "the backfill mark sits inside the trade-derived half, past the seam"
        );
        // Both marks paint. The backfill divider is opt-in (see
        // `ChartPane::new`) and this test is about where it lands, so switch
        // it on first — on both panes, since either one's mark answers the
        // assertion below.
        for side in [PaneSide::Time, PaneSide::Flow] {
            app.active_tab_mut().pane_mut(side).set_layer_visible(
                ChartLayer::BackfillDivider,
                true,
                &mut chart_layers::LayerActions::default(),
            );
        }
        // The view follows the live edge, and three months of venue history is
        // far behind it, so bring the seam on screen the way a user scrolling
        // back would.
        let slots = app.active_tab().pane(PaneSide::Time).slots();
        let width = app
            .active_tab()
            .pane(PaneSide::Time)
            .last_chart_area
            .expect("the time pane was laid out")
            .width();
        app.active_tab_mut()
            .pane_mut(PaneSide::Time)
            .viewport
            .center_on_bar(seam as f32, width, slots);
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text == "venue"),
            "the seam names itself: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "backfill"),
            "and the backfill divider still draws: {texts:?}"
        );
    }

    // ---- adding a symbol from the picker (§11) ----

    /// A scratch sidecar path, so a test never touches the real one.
    fn symbols_scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "quantick-symbols-app-{}-{}.toml",
            name,
            std::process::id()
        ))
    }

    /// (a) Typing a contract the catalog does not have opens it and records
    /// it. The real driver: B3 rotates the mini index every two months, and
    /// the broker serves WINQ26 rather than the WIN$N alias.
    #[test]
    fn adding_a_symbol_opens_it_and_writes_it_to_the_sidecar() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let path = symbols_scratch("added");
        let _ = std::fs::remove_file(&path);
        app.symbols_path = path.clone();
        let tabs_before = app.tabs.len();

        // The real dialog: open it, type the contract, press Add.
        app.apply_tab_action(TabAction::New);
        run_frame(&mut app, &ctx);
        app.source_picker
            .as_mut()
            .expect("the + opened the picker")
            .set_draft_symbol("  WINQ26 ");
        run_frame(&mut app, &ctx);
        let add = app
            .source_picker
            .as_ref()
            .expect("still open")
            .add_button_rect()
            .expect("the Add button was laid out");
        click_chart(&mut app, &ctx, add.center());
        run_frame(&mut app, &ctx);

        assert!(
            app.source_picker.is_none(),
            "adding closes the dialog, because it opened the market"
        );
        assert_eq!(app.tabs.len(), tabs_before + 1);
        assert_eq!(app.active_tab().symbol, "WINQ26", "and it is what opened");
        // Both surfaces that list symbols see it, because both read the
        // catalog the app is running on.
        assert!(
            app.config
                .feed("binance")
                .expect("the feed")
                .symbols
                .iter()
                .any(|symbol| symbol == "WINQ26")
        );
        assert!(app.added_symbols.contains("binance", "WINQ26"));

        let written = std::fs::read_to_string(&path).expect("the sidecar was written");
        assert!(
            written.contains("WINQ26"),
            "it records the symbol: {written}"
        );
        assert!(
            written.contains("quantick.toml is never modified"),
            "and says the config file is left alone: {written}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A config with two MetaTrader feeds, one of them mapping a port — the
    /// shape where adding the wrong symbol used to write a file that killed
    /// the next launch.
    fn two_metatrader_feeds() -> AppConfig {
        let mut ports = std::collections::BTreeMap::new();
        ports.insert("US500".to_string(), 9102_u16);
        AppConfig {
            default_feed: "tickmill".to_string(),
            default_symbol: "US500".to_string(),
            feeds: vec![
                FeedConfig {
                    id: "tickmill".to_string(),
                    name: "MetaTrader 5 — Tickmill".to_string(),
                    provider: ProviderKind::MetaTrader,
                    symbols: vec!["US500".to_string()],
                    bubble_preset: None,
                    default_layout: None,
                    default_bars: None,
                },
                FeedConfig {
                    id: "b3".to_string(),
                    name: "MetaTrader 5 — B3".to_string(),
                    provider: ProviderKind::MetaTrader,
                    symbols: vec!["WIN$N".to_string()],
                    bubble_preset: None,
                    default_layout: None,
                    default_bars: None,
                },
            ],
            metatrader: crate::config::MetaTraderSettings {
                ports,
                ..Default::default()
            },
            paper: Default::default(),
        }
    }

    /// An addition that the *whole* config would reject is refused where it
    /// was typed, and nothing is written.
    ///
    /// Typing `US500` into the B3 feed made two MetaTrader feeds offer one
    /// mapped symbol — a configuration the app refuses to load. It used to be
    /// accepted, persisted, and then kill the next launch with an error naming
    /// the config file, which was not the file that broke.
    #[test]
    fn an_addition_the_config_would_reject_is_refused_and_not_written() {
        let ctx = egui::Context::default();
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let mut app = QuantickApp::new(
            two_metatrader_feeds(),
            "b3",
            "WIN$N",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::MetaTrader.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );
        let path = symbols_scratch("refused");
        let _ = std::fs::remove_file(&path);
        app.symbols_path = path.clone();
        let tabs_before = app.tabs.len();

        // The real dialog, on the B3 feed, typing the Tickmill instrument.
        app.apply_tab_action(TabAction::New);
        run_frame(&mut app, &ctx);
        {
            let picker = app.source_picker.as_mut().expect("the picker is open");
            picker.feed_id = "b3".to_string();
            picker.set_draft_symbol("US500");
        }
        run_frame(&mut app, &ctx);
        let add = app
            .source_picker
            .as_ref()
            .expect("still open")
            .add_button_rect()
            .expect("the Add button was laid out");
        click_chart(&mut app, &ctx, add.center());
        run_frame(&mut app, &ctx);

        let picker = app
            .source_picker
            .as_ref()
            .expect("the dialog stays open on a refusal");
        assert!(
            picker
                .refusal()
                .is_some_and(|reason| reason.contains("US500")),
            "the reason is shown where the symbol was typed: {:?}",
            picker.refusal()
        );
        assert_eq!(app.tabs.len(), tabs_before, "and no market was opened");
        assert!(
            !app.config
                .feed("b3")
                .expect("the feed")
                .symbols
                .iter()
                .any(|symbol| symbol == "US500"),
            "the catalog is untouched"
        );
        assert!(
            !path.exists(),
            "and nothing was persisted — the next launch is unharmed"
        );
        // The same symbol on the feed that *does* own it is still fine.
        assert!(app.add_symbol("tickmill", "WINQ26").is_ok());
        let _ = std::fs::remove_file(&path);
    }

    /// A push feed re-answers: an empty block, then a real one on the next
    /// reconnect. The capability flag rose once and stays, so only the
    /// generation can say the answer changed.
    #[test]
    fn a_new_candle_generation_is_asked_for_again_and_installed() {
        let ctx = egui::Context::default();
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (_book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let (caps_tx, caps_rx) = tokio::sync::watch::channel(FeedCapabilities {
            book_capture: false,
            history_paging: true,
            traded_volume: true,
            ohlcv_history: true,
            ohlcv_generation: 1,
        });
        let mut app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(1),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: caps_rx,
                commands: cmd_tx,
                replay: None,
            },
        );
        let trades: Vec<_> = (0..50).map(minute_trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.drain_tabs();
        run_frame(&mut app, &ctx);
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            1,
            "generation 1 is asked"
        );

        // A cold terminal: the block it had was empty.
        evt_tx
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: Vec::new(),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 0);
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            0,
            "an answered request is not repeated on its own"
        );

        // The reconnect stores a real block, and says so by moving the count.
        caps_tx.send_modify(|caps| caps.ohlcv_generation = 2);
        app.drain_tabs();
        assert_eq!(
            drain_ohlcv_requests(&mut cmd_rx),
            1,
            "a new generation is a new answer, and is asked for"
        );

        evt_tx
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(30),
                complete: false,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            30,
            "and the block it carried is installed through the usual path"
        );
        assert!(
            !app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "a short answer is still an answer, and ends the wait"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(has_price_axis(&texts), "the pane draws it: {texts:?}");
    }

    /// A block already held is replaced too: a reconnect can carry a longer or
    /// corrected one, and holding the first would pin the chart to it.
    #[test]
    fn a_new_generation_replaces_a_block_already_held() {
        let ctx = egui::Context::default();
        let (mut app, events, mut commands) = history_app(&ctx);
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(30),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        drain_ohlcv_requests(&mut commands);
        assert_eq!(app.active_tab().pane(PaneSide::Time).seam_slot(), 30);

        // The feed's capabilities are fixed in this fixture, so move the tab's
        // own record of what it has acted on — the same thing a bumped
        // generation does when it arrives.
        app.active_tab_mut().forget_ohlcv_generation_for_test();
        app.drain_tabs();
        assert_eq!(
            drain_ohlcv_requests(&mut commands),
            1,
            "the tab asks again rather than keeping the block it has"
        );

        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history(90),
                complete: true,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(
            app.active_tab().pane(PaneSide::Time).seam_slot(),
            90,
            "and the longer block replaces the shorter one"
        );
    }

    /// (b) The point of writing it: the next launch has it. Proven without
    /// restarting, by loading a config through the same path `load` uses.
    #[test]
    fn a_recorded_symbol_is_in_the_catalog_on_the_next_load() {
        let path = symbols_scratch("reload");
        let _ = std::fs::remove_file(&path);
        let mut added = crate::symbols_file::AddedSymbols::default();
        added.add("binance", "WINQ26");
        crate::symbols_file::save(&path, &added).expect("the scratch file is writable");

        let reloaded = crate::symbols_file::load(&path);
        let mut config = test_config();
        config.merge_added_symbols(&reloaded);

        assert!(
            config
                .feed("binance")
                .expect("the feed")
                .symbols
                .iter()
                .any(|symbol| symbol == "WINQ26"),
            "a restart finds the contract the user added"
        );
        assert!(config.validate().is_ok(), "and the merged catalog is valid");
        let _ = std::fs::remove_file(&path);
    }

    /// (c) Removing is a catalog edit and only that: the file and the picker
    /// lose the symbol, a tab showing that market does not.
    #[test]
    fn removing_a_symbol_updates_the_file_and_leaves_open_tabs_alone() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let path = symbols_scratch("removed");
        let _ = std::fs::remove_file(&path);
        app.symbols_path = path.clone();
        app.add_symbol("binance", "WINQ26")
            .expect("the catalog takes a symbol that fits");
        app.adopt_tab(
            "binance".to_owned(),
            "WINQ26".to_owned(),
            stub_feed().0,
            None,
        );
        run_frame(&mut app, &ctx);
        let open_tabs = app.tabs.len();

        app.remove_symbol("binance", "WINQ26");

        assert!(
            !app.config
                .feed("binance")
                .expect("the feed")
                .symbols
                .iter()
                .any(|symbol| symbol == "WINQ26"),
            "the catalog lost it"
        );
        assert!(!app.added_symbols.contains("binance", "WINQ26"));
        assert!(
            !crate::symbols_file::load(&path).contains("binance", "WINQ26"),
            "and so did the file"
        );
        assert_eq!(app.tabs.len(), open_tabs, "no tab was closed");
        assert_eq!(
            app.active_tab().symbol,
            "WINQ26",
            "and the one showing that market still is"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// (e) A sidecar naming a feed the config no longer has says so and is
    /// ignored — a renamed feed costs its additions, not the launch.
    #[test]
    fn a_sidecar_entry_for_a_dead_feed_is_ignored() {
        let mut added = crate::symbols_file::AddedSymbols::default();
        added.add("a-feed-that-was-renamed", "WINQ26");
        let mut config = test_config();
        let before = config.feeds.clone();

        config.merge_added_symbols(&added);

        assert_eq!(config.feeds, before, "nothing was invented for a dead id");
        assert!(config.validate().is_ok());
    }

    /// The remove affordance is refused for a market a tab is on: the picker
    /// greys it out, and the reason is that the next SOURCE correction would
    /// otherwise retarget that tab to another instrument.
    #[test]
    fn a_symbol_a_tab_is_showing_is_not_offered_for_removal() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        app.symbols_path = symbols_scratch("guard");
        let _ = std::fs::remove_file(&app.symbols_path);
        app.add_symbol("binance", "WINQ26")
            .expect("the catalog takes a symbol that fits");
        app.adopt_tab(
            "binance".to_owned(),
            "WINQ26".to_owned(),
            stub_feed().0,
            None,
        );
        run_frame(&mut app, &ctx);

        // What the picker is handed, and what it does with it.
        let open: Vec<(String, String)> = app
            .tabs
            .iter()
            .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
            .collect();
        assert!(
            open.iter()
                .any(|(feed, symbol)| feed == "binance" && symbol == "WINQ26"),
            "the tab is on the market the picker must protect"
        );
        // The app-side rule holds even if the affordance were clicked: the
        // catalog edit is refused for the last symbol and allowed otherwise,
        // and the tab is never touched either way.
        app.remove_symbol("binance", "WINQ26");
        assert_eq!(
            app.active_tab().symbol,
            "WINQ26",
            "removing a symbol never moves a tab off its market"
        );
        let _ = std::fs::remove_file(&app.symbols_path);
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

    /// Closing a tab ends that market's paper-trading session, and the
    /// simulator's honesty contract says a session ends in a labeled,
    /// journaled flatten — never by vanishing with its window. Everything
    /// else a tab owns can simply be dropped; an open position is state the
    /// user created, so it is the one thing `Tab::close` has to settle.
    #[test]
    fn closing_a_tab_flattens_and_journals_its_simulated_position() {
        let ctx = egui::Context::default();
        let (mut app, _cmd_rx) = app_with_history(50);
        let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
        let dir = std::env::temp_dir().join(format!(
            "quantick-paper-tab-close-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        app.active_tab_mut().paper.redirect_history_dir(dir.clone());

        // A filled position on the second tab: backfill seeds the mark, the
        // toolbar queues the order, the next live print fills it.
        ends.events
            .try_send(FeedEvent::Backfilled(vec![trade(2)]))
            .unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        app.apply_toolbar_action(ToolbarAction::PaperBuy);
        ends.events.try_send(FeedEvent::Live(trade(4))).unwrap();
        app.active_tab_mut().drain_feed_with_clock(|| 0);
        assert!(
            app.active_tab().paper.status_cell().is_some(),
            "this proof needs an open simulated position to lose"
        );

        app.apply_tab_action(TabAction::Close(1));

        assert_eq!(app.tabs.len(), 1, "the tab is gone");
        let files: Vec<_> = std::fs::read_dir(dir.join("ETHUSDT"))
            .expect("the flatten was journaled under the closed tab's symbol")
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one session, one history file");
        let _ = std::fs::remove_dir_all(&dir);
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
