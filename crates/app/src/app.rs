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

use crate::canvas_layout::{MAX_CANVAS_PANES, PaneIdAllocator};

mod layout_wiring;
use crate::chart_layers::{self, ChartLayer};
use crate::config::AppConfig;
use crate::dock::{Dock, DockEnv, DockTab};
use crate::drawings::{self, DeleteOutcome, DrawingAuthor};
use crate::feed::{self, FeedCommand, FeedHandle, ReplayControl};
use crate::feed_notice;
use crate::indicator_legend;
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::IndicatorView;
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::indicators::state_file::{self, SavedInput, SavedKind};
use crate::loading::{self, LoadingScope, LoadingTask};
use crate::metrics::{self, FrameStats};
use crate::orderflow::LaneWindow;
use crate::pane::{self, ChartPane, DRAWING_ANCHOR_RADIUS_PX, PaneSide};
use crate::replay_view::{ReplayAction, ReplayView};
use crate::state::BarSpec;
use crate::statusbar;
use crate::style::{CandlePreset, ChartStyle};
use crate::symbols_file::{self, AddedSymbols};
use crate::tab::{CanvasChrome, CanvasLayout, LegendFold, Tab};
use crate::tabstrip::{self, TabAction};
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolbar::{self, ToolbarAction};
use crate::toolrail::{Tool, ToolRail, ToolboxDock};
use crate::ui_state;
use crate::window_scale;
use smallvec::SmallVec;

/// Width of the right-hand price-axis gutter, in pixels (§5 zone 9).
const AXIS_GUTTER: f32 = 64.0;
/// Height of the bottom time-axis strip, in pixels (§5 zone 6).
const TIME_STRIP: f32 = 24.0;
/// Id of the tab the window opens with.
const FIRST_TAB_ID: u64 = 0;

/// Frames the `QUANTICK_LOAD_OLDER` hook waits for a chart worth paging from.
///
/// It cannot fire at startup: paging asks for trades older than the ones on
/// screen, and at launch there are none — the feed would refuse the request as
/// `nothing_charted_yet`. So the hook waits for the first block to land, and
/// gives up rather than hanging a capture run on a bridge that never connects.
/// About ten seconds at 60 fps, which is longer than any bridge takes to say
/// hello and shorter than a person's patience with a frozen window.
const LOAD_OLDER_HOOK_FRAMES: u32 = 600;

/// Frames the `QUANTICK_HISTORY_NOTE` hook holds its sentence on screen for.
///
/// A *hold*, not a wait — which is why it is not
/// [`LOAD_OLDER_HOOK_FRAMES`]: that one is sized against how long a bridge
/// takes to say hello, and retuning it for bridge latency must not silently
/// retune how long a capture has to photograph a note.
///
/// Comfortably past `tab::HISTORY_NOTE_LINGER`, so a run that waits out a
/// slow source still finds the sentence up; the note keeps its ordinary
/// linger from the last raise once the hold ends, so even a hooked run
/// photographs a note that expires.
const HISTORY_NOTE_HOOK_FRAMES: u32 = 900;

/// Frames the `QUANTICK_LOAD_OLDER_CANDLES` hook has for the *whole* run.
///
/// Much larger than the trade twin's, and for a reason the trade twin does
/// not have: a page of prints is one venue round trip, while a span of
/// candles is several slices of several pages each, and the hook is
/// documented as reaching the old ninety-day default in thirteen of them.
/// Every frame spent waiting for a span costs one tick here, so the budget
/// has to cover the legitimate fetching as well as the hang it exists to
/// bound. About a minute at 60 fps: longer than thirteen spans take against
/// a venue that is answering, and far shorter than a capture run's patience
/// with one that is not.
const LOAD_OLDER_CANDLES_HOOK_FRAMES: u32 = 3_600;

/// How long `QUANTICK_CONTROL_EVIDENCE=screenshot` waits for the window to
/// hand over a rasterised frame.
///
/// A window that presents answers on the frame after the request; a headless
/// or occluded one never does. About two seconds at 60 fps: long enough for a
/// surface that is coming up, short enough that a capture run gets a bundle
/// with an honest gap instead of waiting for one that will never arrive.
const CONTROL_EVIDENCE_HOOK_FRAMES: u32 = 120;

/// How much of the newest chart the `QUANTICK_DRAWINGS_DEMO` hook spreads its
/// objects across. Close to what a default viewport shows, so every object
/// lands on screen — a demo the camera cannot see proves nothing.
const DEMO_VISIBLE_SLOTS: usize = 90;
/// How many demo objects wide the visible window is — the reciprocal of how
/// far a multi-anchor object reaches. Four keeps a rectangle big enough to
/// read while still leaving the tools distinguishable from each other.
const DEMO_SPANS_PER_WINDOW: usize = 4;
/// How far apart, as a fraction of the visible price band, the demo places
/// the successive anchors of one object and the successive rows of objects.
/// Both are small enough that the widest object still lands inside the band
/// the chart is showing.
const DEMO_ANCHOR_BAND_STEP: f64 = 0.12;
const DEMO_ROW_BAND_STEP: f64 = 0.22;
/// Points the demo hook gives a freehand tool, which declares no anchor
/// count of its own. Enough to read as a path rather than as a line.
const DEMO_FREEHAND_POINTS: usize = 4;
/// How much of the visible window and of the visible price band the
/// `QUANTICK_DRAWING_DRAFT` hook's anchors span. Wide enough that the
/// half-made object reads at a glance, and centred, so the segment its
/// anchors describe passes through the parked pointer at the chart's middle.
///
/// Much wider than it is tall, because a trend line a trader would actually
/// draw runs *along* the tape. A near-vertical draft photographs the
/// mechanism but nothing about whether the shape reads, which is what a QA
/// reference image is for.
const DEMO_DRAFT_SPAN: f32 = 0.6;
const DEMO_DRAFT_BAND_SPAN: f64 = 0.12;
/// Where the parked pointer stands when a single anchor is down, as a
/// fraction of the chart from its centre.
///
/// A lone anchor sits at that centre, so parking the pointer there too gives
/// a rubber band of no length — an invisible draft, which is the one thing
/// this hook exists not to photograph. Offset, there is a line to see, and it
/// is a *sloped* one, which is what makes the levelled state worth its own
/// capture.
const DEMO_DRAFT_POINTER_OFFSET: egui::Vec2 = egui::vec2(0.2, -0.15);
/// The band to spread across before the pane has an auto-range to read (no
/// bars yet): a fraction of price, since there is nothing better to ask.
const DEMO_FALLBACK_BAND_FRACTION: f64 = 0.004;
/// How far before the loaded history the re-cut demo anchors its off-series
/// mark. Any distance the tab cannot possibly hold would do; an hour is
/// unambiguous at every timeframe the chart offers.
const DEMO_OFF_SERIES_LEAD_MS: i64 = 3_600_000;
/// Length of the EMA the toolbar's hardcoded M1 entry adds (the settings UI
/// generated from `InputSpec` replaces this in M4).
const DEFAULT_EMA_LEN: usize = 9;
/// How often the hot-reload poll checks script files for changes.
const SCRIPT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1_000);
/// How long after the last indicator change the state file is written.
const INDICATOR_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(1_000);
/// Horizontal offset of a duplicated drawing, so the copy is visibly a copy.
const DUPLICATE_OFFSET_BARS: f32 = 2.0;

/// What the open file dialog is for, so the one poll can land either answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspacePick {
    /// Choosing where to write a workspace file.
    Export,
    /// Choosing a workspace file to open.
    Import,
}

/// The slice the drawing chrome reads, assembled from the pieces of the
/// application it is allowed to see.
///
/// A free function rather than a method for the reason
/// [`indicator_preview_area`] is one: every caller has already split
/// `QuantickApp` into disjoint borrows to draw a surface through `&mut`, and a
/// method would want the whole of `self` back. That the compiler insists on
/// the split is the port working.
///
/// `manager_rows` is handed in rather than gathered here. Only one of the two
/// call sites draws the list, and building a row per object for the site that
/// does not would be a per-frame allocation for a window nobody is looking at.
fn drawing_env<'a>(
    tab: &'a Tab,
    toolrail: &ToolRail,
    presets: &'a drawings::presets::PresetStore,
    read: DrawingRead<'a>,
) -> crate::surfaces::DrawingEnv<'a> {
    let side = tab.drawing_side();
    let pane = tab.pane(side);
    let selected = pane.drawings.selected().and_then(|index| {
        pane.drawings
            .items()
            .get(index)
            .map(|drawing| crate::surfaces::drawing_chrome::SelectedDrawing { index, drawing })
    });
    crate::surfaces::DrawingEnv {
        selected,
        chart_area: pane.last_chart_area,
        focused_chart_area: tab.focused_pane().last_chart_area,
        lane_divider_x: pane.last_lane_divider_x,
        auto_range: pane.last_auto_range,
        selected_bbox: read.selected_bbox,
        selected_band: read.selected_band,
        tab: tab.id,
        side,
        drawing_tool_armed: matches!(toolrail.tool(), Tool::Drawing(_)),
        toolbox_dock: toolrail.dock(),
        authored_objects: read.authored_objects,
        manager_rows: read.manager_rows,
        presets,
    }
}

/// The parts of [`drawing_env`] that cost something to work out, gathered by
/// the caller so each pass pays only for what it draws.
///
/// Three fields and three prices. Projecting the selection's painted bounds
/// walks its anchors through the price scale; naming its band formats a
/// string; counting an assistant's objects walks every pane of every tab. All
/// three are per-frame while a selection is on screen, which is why the pass
/// that only runs the capture hooks gathers none of them and says so.
#[derive(Default)]
struct DrawingRead<'a> {
    selected_bbox: Option<egui::Rect>,
    selected_band: Option<String>,
    authored_objects: usize,
    manager_rows: &'a [crate::surfaces::drawing_chrome::ManagerRow],
}

/// The chart rectangle a settings dialog is previewing an unapplied draft on,
/// if one is.
///
/// The pane the *dialog* was opened over, not the focused one: a trader can
/// preview a curve on the left pane and then click the right, and the banner
/// belongs over the numbers that are actually provisional.
///
/// A free function rather than a method because its only caller has already
/// split `QuantickApp` into disjoint borrows to build the surface
/// environment, and a method would want the whole of `self` back. Per-frame,
/// and shaped to leave immediately: no dialog open — the ordinary case — is
/// one `Option` test before the tab scan is reached.
fn indicator_preview_area(
    tabs: &[Tab],
    dialog: Option<&SettingsDialog>,
    target: TabSlot,
) -> Option<egui::Rect> {
    if !dialog.is_some_and(|dialog| dialog.previewed) {
        return None;
    }
    tabs.iter()
        .find(|tab| tab.id == target.tab)
        .map(|tab| tab.pane(target.side))
        .and_then(|pane| pane.last_chart_area)
}

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);

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

/// Whether a pointer at `x` is on the lane divider's own resize handle.
///
/// The one strip of the tape a chart gesture may not have: the resize drag and
/// the pan must never both fire on the same pixel. Everything else in the band
/// is the candles' — the tape is pinned to the live edge and does not pan, so a
/// drag across it has no second meaning to protect, and reserving a third of
/// the canvas to guard a ten-pixel handle is a dead zone rather than a guard.
/// Without a lane there is no handle, and every pixel is the candles'.
#[must_use]
pub fn gesture_hits_lane_divider(divider_x: Option<f32>, x: f32, half_width: f32) -> bool {
    divider_x.is_some_and(|divider| (x - divider).abs() <= half_width)
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

/// What `QUANTICK_STRATEGY_DEMO` stages: the armed instance itself, or the
/// arming dialog a screenshot of the form needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrategyDemoMode {
    Armed,
    Popup,
    /// The arming dialog with the **alarm switched on** and its share gate
    /// picked, so every alarm control is on screen at once. The section
    /// folds itself away while the checkbox is clear — which is right for a
    /// trader and useless for a capture — so `popup` alone can never
    /// photograph it.
    AlarmPopup,
    /// The same dialog with the **sound picker dropped open** — the three
    /// headings and the thirty-two names under them. A combo box's list
    /// exists only while it is open, and opening it is a click no scripted
    /// run has a hand for, so the scene asks egui to open it on the frame
    /// the dialog appears.
    AlarmSounds,
    /// An armed instance whose region's drawn span no longer reaches the
    /// next bar — the badge clause telling the trader to stretch it right.
    /// A live tape reaches it only by walking past a hand-drawn right edge,
    /// which is minutes of market no capture can wait for. The instance
    /// stays armed and its alarm stays live: the region holds, it is not
    /// disarmed, so dragging the band forward resumes it with no button.
    EndedBadge,
    /// An instance whose region lost its footing on the series — the badge
    /// clause that says the bot is paused and why. Reached on a real chart
    /// only by a re-cut that strands an anchor, which a scripted run has no
    /// way to provoke on demand.
    PausedBadge,
    /// An **alarm-only** instance armed on the region, carrying a standing
    /// preview mark: the badge that says "this places nothing" and the
    /// provisional label, in one frame. Both are states a real tape reaches
    /// only when a force bar happens to be half-formed, which no capture
    /// can wait for.
    AlarmBadge,
}

/// Which pane a scripted right-click should land on.
///
/// The two panes now open different menus, so "open the context menu" is no
/// longer one instruction — a capture has to say which canvas it is asking
/// about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextMenuPane {
    /// The candles, left of the divider.
    Chart,
    /// The rolling tape, right of it.
    Tape,
    /// The price gutter — the axis's own menu (Inverted chart, and the
    /// compass's price half).
    Axis,
    /// The bottom time strip — the time axis's own menu (the compass's time
    /// half). The candles' segment of it: past the lane divider the strip is
    /// the tape's window and carries no menu.
    Time,
}

impl ContextMenuPane {
    /// Read one off `QUANTICK_CONTEXT_MENU`; `None` for anything else, so a
    /// typo opens no menu rather than the wrong one.
    fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chart" | "candles" => Some(Self::Chart),
            "tape" | "lane" => Some(Self::Tape),
            "axis" | "scale" => Some(Self::Axis),
            "time" | "clock" => Some(Self::Time),
            _ => None,
        }
    }
}

/// Read a scripted pointer position off `QUANTICK_POINTER`.
///
/// `<fx>,<fy>`, both fractions of the flow pane's *candle* area — `0,0` its
/// top-left corner, `1,1` its bottom-right, `0.99,0.5` out in the projection
/// margin past the newest bar. The flow pane and not the focused one, so this
/// hook and `QUANTICK_CONTEXT_MENU` aim at the same canvas: a capture that
/// opened an axis menu on one pane and parked the mouse on another would be
/// photographing two different charts at once. Fractions rather than pixels because the thing
/// a capture wants to point at is a candle, and the candles' pane moves with
/// the window size, the lane divider and the indicator band: an absolute pair
/// that framed the right bar at one window size frames a different one at the
/// next, and a capture that photographs the wrong bar and calls it a pass is
/// worse than one that photographs nothing.
///
/// `None` for anything else — a typo must not silently place the pointer
/// somewhere the author did not ask for.
fn parse_pointer_fraction(value: &str) -> Option<egui::Vec2> {
    let (x, y) = value.trim().split_once(',')?;
    let x: f32 = x.trim().parse().ok()?;
    let y: f32 = y.trim().parse().ok()?;
    ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then(|| egui::vec2(x, y))
}

/// Read a tape window off `QUANTICK_TAPE_WINDOW`.
///
/// `auto` follows the bars; a duration pins it, in the units a human would
/// type (`90s`, `2min`, `120000ms`, or bare milliseconds). `None` for anything
/// else, so a typo leaves the tape at its default rather than photographing an
/// invented window. The value is clamped by the setter, not here — one owner
/// for the drawable range.
fn parse_tape_window(value: &str) -> Option<LaneWindow> {
    let value = value.trim().to_ascii_lowercase();
    if value == "auto" {
        return Some(LaneWindow::default());
    }
    let (number, scale) = if let Some(rest) = value.strip_suffix("ms") {
        (rest, 1)
    } else if let Some(rest) = value.strip_suffix("min") {
        (rest, 60_000)
    } else if let Some(rest) = value.strip_suffix('m') {
        (rest, 60_000)
    } else if let Some(rest) = value.strip_suffix('s') {
        (rest, 1_000)
    } else {
        (value.as_str(), 1)
    };
    let parsed = number.trim().parse::<f64>().ok()?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return None;
    }
    Some(LaneWindow::Fixed {
        ms: (parsed * f64::from(scale)).round() as i64,
    })
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

/// Which venue-history frame `QUANTICK_VENUE_HISTORY_DEMO` opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VenueHistoryDemo {
    /// The finished prefix: the seam divider with no wait behind it.
    Complete,
    /// A run still arriving: prefix installed, loading indicator still up.
    Partial,
}

/// Read-only frame observations exposed to on-demand semantic projections.
pub(crate) struct ControlFrameMetrics {
    pub wall_average_ms: Option<f32>,
    pub wall_worst_ms: Option<f32>,
    pub frames_per_second: Option<f32>,
    pub cpu_average_ms: Option<f32>,
    pub cpu_worst_ms: Option<f32>,
}

/// The quantick chart window.
///
/// Which menu `QUANTICK_MENU` presses open.
///
/// A menu bar button is not something a scripted run can click, and every
/// entry behind one is therefore invisible to a capture without a hook. The
/// hook delivers a real click on the button's own published rectangle rather
/// than reaching into egui's popup state, so what opens is exactly what a
/// trader's click opens.
///
/// A token this build does not know opens nothing rather than the wrong menu:
/// a capture of the wrong surface that passes is worse than one that fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptedMenu {
    /// The Workspace menu — the only door to save, export, open and locate.
    Workspace,
    /// The toolbar's history caret — the reach chips, the span the `by time`
    /// reach pulls, the page size and the candle reach.
    History,
}

impl ScriptedMenu {
    /// Every menu this hook can open, by the token that names it.
    const ALL: [(&'static str, Self); 2] =
        [("workspace", Self::Workspace), ("history", Self::History)];

    fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(token))
            .map(|(_, menu)| menu)
    }
}

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
    /// Where the offline chip was drawn, or `None` when it was not.
    ///
    /// Written as part of drawing it, exactly as a pane records its own chart
    /// area, and for the same two reasons. It says what is *painted* rather
    /// than what a fresh reading of the clock would have painted a
    /// millisecond later, so the scene and the screen cannot disagree across
    /// the edge of a stall budget. And it is the one control on screen with
    /// no capability behind it — opening a popup is a gesture, not a call —
    /// so its rectangle is the only way an operator reaches it at all.
    ///
    /// A `Rect` per frame, and only while the chart is not being fed. Nothing
    /// is recorded on a healthy chart, which is every frame of a normal
    /// session.
    feed_chip_rect: Option<egui::Rect>,
    /// The tab whose chip opened the feed's recovery popup, if any.
    ///
    /// Opened by clicking the offline chip and by nothing else — the rule the
    /// trader asked for, after a card that opened itself over the chart every
    /// morning. It is the *tab's* id rather than a window-wide flag because
    /// one dead terminal stalls every MT5 tab at once: a bare flag opened on
    /// one chart and then found the next chart already offline, and drew
    /// itself there with nobody having clicked anything. The chip is window
    /// chrome speaking for the active market, and this says which market it
    /// was speaking for.
    ///
    /// Leaving that chart closes it, the way clicking elsewhere does: the
    /// frame answers for the tab it is drawing, so a switch clears the flag.
    /// A glance, not a mode — nothing waits on a chart nobody is looking at.
    feed_popup_tab: Option<u64>,
    /// Whether the toolbar's layout popover is open.
    layout_picker_open: bool,
    /// A pending request to open that popover, from the
    /// `QUANTICK_LAYOUT_PICKER` hook. Drained by the frame that honours it —
    /// the popover is a popup egui owns, so the hook asks for it through the
    /// same call the click makes rather than faking the surface.
    layout_picker_autostart: bool,
    /// The window's one source of pane ids. Pane ids namespace egui
    /// interaction state across the whole window rather than within a tab, so
    /// this may not be per-tab state: two panes sharing an id share a drag.
    pane_ids: PaneIdAllocator,
    /// The instruments the user added from the picker, already folded into
    /// `config`'s catalog. Kept apart from it so the picker can tell an
    /// addition — which it may take back out — from a shipped entry, which is
    /// the config file's and not the app's to touch.
    added_symbols: AddedSymbols,
    /// Where those additions persist. See [`crate::symbols_file`].
    symbols_path: std::path::PathBuf,

    config: AppConfig,

    /// Explicitly enabled local observer gateway. `None` only while this field
    /// is temporarily moved out to dispatch a frame without borrowing the app
    /// through itself.
    control_access: Option<crate::control::ControlAccess>,

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
    /// Slots placed hidden by a layout, applied when their Rebuilt lands —
    /// the view a hide acts on is born from the worker's first answer.
    pending_hidden: Vec<TabSlot>,
    /// Per-plot style layers placed by a layout, applied when their Rebuilt
    /// lands — the same deferral [`Self::pending_hidden`] performs, for the
    /// same reason.
    pending_styles: Vec<(TabSlot, crate::indicator_style::StyleOverride)>,
    /// `QUANTICK_INDICATOR_SETTINGS`: which indicator to open the dialog on,
    /// and on which tab, once its view exists. Cleared by the first open, so a
    /// dialog the run then closes stays closed.
    settings_autostart: Option<(usize, indicator_panel::SettingsTab)>,
    /// The workspace's layouts: the strip's tabs, their indicator sets and
    /// their per-market drawings. See [`crate::layouts`].
    layouts: crate::layouts::LayoutBook,
    /// Where the layouts persist.
    layouts_path: std::path::PathBuf,
    /// Set by any layout edit — a switch, a rename, a drawing, a settled
    /// indicator change; drained by the debounced save.
    layouts_dirty: bool,
    /// When the last layout change happened (the debounce clock).
    last_layout_change: Option<Instant>,
    /// Whether the layouts file may be written this session. `true` only
    /// when the file was there at launch, could not be read, and could not
    /// be set aside — the trader's only copy, which this session's empty
    /// book must never replace.
    layouts_save_blocked: bool,
    /// The layout being renamed in the strip, with the draft name.
    layout_rename: Option<(crate::layouts::LayoutId, String)>,
    /// The layout a delete is waiting on: deleting takes its drawings with
    /// it, on disk too, so it is the one strip action behind a confirmation.
    layout_delete_confirm: Option<crate::layouts::LayoutId>,
    /// Last hot-reload poll instant (the poll runs about once a second;
    /// file metadata every frame would be waste).
    last_script_poll: Instant,

    /// The browser window and, while the active tab replays, the transport.
    replay_view: ReplayView,

    // External chart chrome: the tabbed right dock and the edge-docked
    // drawing rail. Neither is painted over the chart canvas.
    dock: Dock,
    toolrail: ToolRail,

    /// Floating chrome that owns the state it draws: the assistant's popup,
    /// the acknowledgement toast. One field for the whole set, one module
    /// per surface — see [`crate::surfaces::Surfaces`].
    surfaces: crate::surfaces::Surfaces,
    /// The `QUANTICK_CONTROL_ACCESS` hook: enable observer access on the
    /// first frame, through the panel button's own `enable`.
    pending_control_access_enable: bool,
    /// The indicator slots an operator other than the trader attached — the
    /// only ones the annotate tier may take back off the chart. Keyed by the
    /// whole [`TabSlot`]: a slot number is allocated per pane and is reused
    /// by every other pane, so the number alone would mark one tab's slot 0
    /// as an operator's because another tab's slot 0 was.
    operator_slots: std::collections::BTreeSet<TabSlot>,
    /// The `QUANTICK_CONTROL_ANNOTATE` hook: an agent-authored label on the
    /// first frame, so every attribution surface can be photographed.
    pending_control_annotation: Option<String>,
    /// The `QUANTICK_CONTROL_NOTIFY` hook: `<channel>:<message>`.
    pending_control_notification: Option<String>,
    /// The `QUANTICK_CONTROL_EVIDENCE` hook: which scopes to capture, and
    /// whether to rasterise the window with them.
    pending_control_evidence: Option<String>,
    /// Frames the evidence hook has spent waiting for a rasterised window.
    control_evidence_hook_frames: u32,
    /// The `QUANTICK_CONTROL_MARK` hook: take a mark on the first frame,
    /// through the hotkey's own action, with the note the hook carried.
    pending_control_mark: Option<String>,
    // The floating inspector's position: automatic placement until the user
    // drags the title bar, manual from then on (only ever re-clamped). The
    // chart rectangle it is placed against belongs to the focused
    // [`ChartPane`], so a split window places against the pane the selection
    // lives on, not the window.
    // Scripted-validation hook: place one of every registered drawing on the
    // flow pane as soon as it has bars to anchor them to. Consumed once.
    pending_drawing_demo: bool,
    /// Pages of older history the `QUANTICK_LOAD_OLDER` hook still owes, and
    /// the frame budget it has left to wait for a chart to ask from.
    pending_load_older: Option<(usize, u32)>,
    /// The ending whose sentence the `QUANTICK_HISTORY_NOTE` hook asks the
    /// loading lane to carry, and the frame budget it has left to wait for a
    /// chart to carry it over. Consumed once.
    ///
    /// A settled reach speaks only when a real venue really refuses, which is
    /// a market condition and not a setting: on the feeds a validation run can
    /// arrange, the reach either lands its session or the source declares it
    /// cannot page at all and the button never takes a press. Without this the
    /// whole surface is invisible to anything but a bad afternoon.
    pending_history_note: Option<(crate::history_reach::CampaignEnd, u32)>,
    /// Spans of older *candles* the `QUANTICK_LOAD_OLDER_CANDLES` hook still
    /// owes, and the frame budget it has left to wait for a first reply to
    /// reach back from. The trade twin of this is `pending_load_older`; they
    /// are two records with two capabilities, so they are two hooks.
    pending_load_older_candles: Option<(usize, u32)>,
    // Scripted-validation hook: one fixed-range volume profile straddling the
    // venue-prefix seam (when there is one). Consumed once.
    pending_frvp_demo: bool,
    /// `QUANTICK_AVWAP_DEMO`: one anchored VWAP placed on the flow pane once
    /// it has bars — the band stack and anchor marker, photographable from a
    /// fresh launch. Consumed once, like the other demos.
    pending_avwap_demo: bool,
    /// `QUANTICK_REPLAY_RESTART_AFTER=<n>`: take the replay transport's own
    /// Restart once the session has closed `n` round trips.
    ///
    /// The one state where a closed-trade mark is asked to paint against a
    /// tape that has not reached its fill: the seek keeps the trades (they
    /// happened) and rebuilds the bars under them. It is reached in the app
    /// by pressing Restart mid-session, which a scripted capture cannot do,
    /// and it is the state the marks used to pile up in. Consumed once.
    pending_replay_restart: Option<usize>,
    /// `QUANTICK_VENUE_HISTORY_DEMO`: a venue candle prefix delivered to the
    /// focused tab through the feed's own path, so the seam divider — and,
    /// with `=partial`, a run still arriving — can be photographed from a
    /// fresh launch.
    ///
    /// The seam only exists where venue candles meet bars cut from prints,
    /// which on a live feed means waiting on a real venue for a real quarter
    /// of history. That is not a state a scripted capture can reach, and
    /// "arrived halfway" is not a state it can reach *at all*: it lasts a few
    /// seconds, once, at a moment nothing controls. Consumed once.
    pending_venue_history_demo: Option<VenueHistoryDemo>,
    // Scripted-validation hook: how many anchors of the armed tool are already
    // placed when the run opens, with the pointer parked where the next one
    // would go. Consumed once.
    pending_drawing_draft: Option<usize>,
    /// The popup's position changed by hand this frame and the workspace has
    /// not been told yet.
    ///
    /// A flag rather than a write on the spot, for two reasons. A drag reports
    /// a new position on *every* frame the hand is moving, and writing the file
    /// sixty times a second for a window that has not landed yet is a lot of
    /// disk for one decision. And the write itself belongs beside the other
    /// workspace writes ([`Self::maintain_workspace`]), not inside the closure
    /// that is painting the window — one place that knows how a workspace
    /// reaches the disk, not two.
    ///
    /// That host runs at the top of a frame, so the file is written on the
    /// frame *after* the one the hand came off in — sixteen milliseconds, and
    /// the frame that closes the window flushes this before taking the exit
    /// save, so nothing can be dropped between the two.
    inspector_position_dirty: bool,
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,

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
    saved_layer_mask: u32,
    /// Which tab [`Self::saved_layer_mask`] was taken from, so a tab *switch*
    /// is never mistaken for a switch the trader flipped.
    saved_layer_tab: u64,
    /// The window this app is drawing into, kept so the health summary can
    /// report the client area the platform believes it has — see
    /// [`crate::window_scale`] for why that number is worth logging, and for
    /// the defect it was measured chasing.
    surface: Option<window_scale::SurfaceProbe>,
    /// Whether `QUANTICK_WINDOW_MAXIMIZED=1` still owes the window a maximise.
    /// Drained on the first frame — see [`Self::apply_maximize_hook`].
    pending_maximize: bool,
    /// Where a pane's layer menu leaves the grid switch and the "an indicator
    /// was hidden" flag; drained right after the canvas is drawn.
    layer_actions: chart_layers::LayerActions,
    /// The footprint layer's signal tunables — resolved at boot (env >
    /// `config/footprint.toml` preset > saved edits > defaults), edited live
    /// by the layer menu's controls.
    footprint_config: crate::footprint_config::FootprintConfig,
    /// Where those edits persist (see `footprint_config::settings_path`).
    footprint_settings_path: std::path::PathBuf,
    // Named input setups per indicator kind, offered by the settings
    // dialog's preset picker.
    indicator_presets: preset_file::PresetStore,
    indicator_presets_path: std::path::PathBuf,
    /// The boot hooks' requests, kept so tabs opened later (replay
    /// autostart) get them too: `QUANTICK_FOOTPRINT_AUTOSTART`,
    /// `QUANTICK_CANDLE_WIDTH` and `QUANTICK_PAN_PX`.
    scripted_footprint: bool,
    /// Which pane a scripted run asked to right-click, until the click lands.
    ///
    /// Cleared by the first frame that can honour it — the divider only exists
    /// once the canvas has drawn once — so the menu opens exactly once and
    /// then behaves like any menu a hand opened.
    /// Whether a scripted run asked for the Workspace menu open
    /// (`QUANTICK_MENU=workspace`). A menu is a popup egui owns, so a capture
    /// reaches it by clicking the button, not by setting state — see
    /// [`Self::raw_input_hook`].
    scripted_menu: Option<ScriptedMenu>,
    /// The press's matching release, on the frame after it.
    scripted_menu_release: Option<egui::Pos2>,
    /// Where the Workspace button was drawn, published by the menu bar so the
    /// hook can click it rather than guess at a coordinate.
    workspace_menu_rect: Option<egui::Rect>,
    /// Where the toolbar's history caret is, published by the draw. `None`
    /// while the menu is unreachable — a feed that pages nothing has no menu
    /// to open, and a hook must photograph that rather than force it.
    history_menu_rect: Option<egui::Rect>,
    scripted_context_menu: Option<ContextMenuPane>,
    /// Where `QUANTICK_POINTER` parks the mouse, as a fraction of the focused
    /// pane's candle area. Re-delivered every frame, because a pointer is a
    /// position the app is *told* about continuously: one event and the next
    /// frame with no pointer over the canvas would take the compass away
    /// again.
    scripted_pointer: Option<egui::Vec2>,
    /// Where signal alarms are played. The shipped sink is the platform's
    /// own sounds; a test swaps in a recorder, which is how "the alarm
    /// sounded, once, and it was the sound the preset named" is asserted
    /// without a build machine making noise.
    alerts: Box<dyn crate::audio::AlertSink>,
    /// The last reason a sound could not be played, shown once in the
    /// dialog. A build with no audio backend, or a platform that refused,
    /// is reported: an alarm the trader never heard is never assumed heard.
    alert_failure: Option<String>,
    /// `QUANTICK_STRATEGY_DEMO`: rectangle + armed instance (`1`) or the
    /// arming dialog over it (`popup`), for validation runs. Consumed once
    /// the chart has bars enough, like the drawings demo.
    ///
    /// The dialog it stages lives in `surfaces::strategy_popup`; this hook
    /// stays here because it has to place the drawing the dialog arms over
    /// before it can open anything.
    pending_strategy_demo: Option<StrategyDemoMode>,
    /// Where the scripted press landed, until its release goes out.
    ///
    /// A real right-click spans frames: the button goes down, the app draws,
    /// and only then does it come up. Delivering both halves in one frame
    /// makes the menu open and close in the same pass, which is why a scripted
    /// click has to hold the button for a frame like a hand does.
    scripted_context_menu_release: Option<egui::Pos2>,
    scripted_candle_width: Option<f32>,
    /// `QUANTICK_PAN_PX`: a drag on the candles, in pixels, applied every
    /// frame until it lands. Negative pushes the chart left into the
    /// projection margin; positive walks back into history.
    ///
    /// Every frame, and not once at boot, because the gesture it stands for
    /// needs bars to move over: at boot the series is empty, `pan_pixels` is
    /// a no-op on it, and a hook that fired then would validate nothing. It
    /// re-applies while the view can still move, and stops as soon as the
    /// per-frame clamp holds it — which is exactly the state a projection
    /// screenshot wants.
    scripted_pan_px: Option<f32>,
    /// `QUANTICK_INDICATOR_SETTINGS`: open the settings dialog for the
    /// first indicator once its inputs have arrived from the worker. Armed
    /// at boot, fired (and disarmed) by the first frame that can honour it —
    /// the dialog needs a view whose `input_values` the Rebuilt event has
    /// filled, which no boot-time code can guarantee.
    scripted_indicator_settings: bool,

    /// The chart appearance every renderer reads. The window that edits it
    /// is `surfaces::style_panel`, which hands back a copy rather than
    /// holding a reference to this one.
    style: ChartStyle,
    style_revision: u64,
    // Whether the status bar shows the perf readings (View → perf readings).
    show_perf: bool,
    /// Whether venue candle history is asked for in slices, newest first
    /// (View → progressive venue history).
    ///
    /// On by default. A span of one-minute candles is a run of sequential
    /// venue round trips — seconds for the opening week, and another such run
    /// for every span the trader reaches back through — and fetched whole the
    /// chart shows nothing at all for the whole of it. Off restores exactly
    /// that: one
    /// request, one reply, one very late frame — kept because a trader on a
    /// metered or rate-limited connection may prefer the smaller number of
    /// requests, and because a setting whose "off" is not the old behaviour is
    /// not a setting the user can fall back to.
    progressive_history: bool,
    /// How far one press of the chart's *load older* button reaches — one
    /// page of trades, or back past the market's last close with a lead into
    /// the session before it.
    ///
    /// A standing choice of the window rather than of a market: a trader who
    /// wants to see yesterday wants it in the tab they open next too. Mirrored
    /// onto every tab each frame, which is where the press is actually served.
    history_reach: crate::history_reach::HistoryReach,
    /// Minutes of *traded* time one press of the `by time` reach pulls.
    ///
    /// On the window beside the reach it belongs to, and mirrored onto every
    /// tab by `drain_tabs`, exactly as the reach itself is: the two are one
    /// choice, and a tab opened after the trader set it must press the way
    /// they said. Seeded from `[history] reach_span_minutes` and editable
    /// afterwards, because it is the trader's own answer to "how much more
    /// tape per press" and that differs between a contract printing a million
    /// times a day and one printing a thousand.
    history_reach_span_minutes: u32,
    /// Whether a chart *not* cut by time may carry the venue's own candles in
    /// front of its bars.
    ///
    /// Off by default: a tick chart has always opened on the prints this
    /// session saw, and nothing is put in front of them unasked. On, a chart
    /// cut by trades gets the venue's 1-minute candles as a labelled prefix —
    /// the only way such a chart can show yesterday at all, since a candle
    /// cannot be folded into a tick bar and must never pretend to be one.
    venue_lead_in: bool,

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
    /// Whether the rail's pinned tools were staged by `QUANTICK_TOOL_FAVORITES`
    /// rather than chosen by the trader.
    ///
    /// A validation run dresses the rail through that hook to reach a state a
    /// screenshot needs; the stars in it are a costume. Since a star is written
    /// to the workspace the moment it is clicked, a run that toggles one would
    /// otherwise write the harness's list into the trader's real file — the
    /// same failure `replay_view.stored_pick()` guards for `QUANTICK_REPLAY_DIR`
    /// (see [`Self::capture_workspace`]). Set once at startup, never cleared:
    /// a session that began wearing a costume never takes it off.
    favorites_are_staged: bool,
    /// The arrangements the trader named and kept, in the order the file lists
    /// them.
    ///
    /// Held here because every write of the workspace file rewrites the whole
    /// file: capturing the live window and saving it would drop the bookmarks
    /// on the floor if the app did not carry them between load and save.
    bookmarks: Vec<ui_state::NamedArrangement>,
    /// Whether a workspace is on disk, so the menu can disable Reset without
    /// asking the filesystem. The menu body runs every frame it is open, and a
    /// `Path::exists` there is a syscall at 60 Hz for an answer that changes
    /// only when this app saves or forgets — the two places that update it.
    workspace_saved: bool,
    /// Workspace files exported or imported recently, newest first, as the
    /// file remembers them. Carried between load and save for the same reason
    /// `bookmarks` is: every write rewrites the whole file.
    recent_workspaces: Vec<String>,
    /// Which of them are actually on disk, resolved when the list changes
    /// rather than when the menu is drawn.
    ///
    /// The same rule `workspace_saved` above is here for: the menu body runs
    /// every frame it is open, so filtering ten paths there would be ten
    /// syscalls at 60 Hz for an answer that changes only when this app
    /// exports, imports or restores — the three places that refresh it.
    recent_on_disk: Vec<std::path::PathBuf>,
    /// The native file dialog, while one is open, and what it is for. One at
    /// a time, and off the UI thread — the OS dialog never blocks a frame.
    workspace_picker: Option<(
        WorkspacePick,
        std::sync::mpsc::Receiver<Option<std::path::PathBuf>>,
    )>,
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

/// An indicator slot together with the tab and pane that own it.
///
/// Slot ids are allocated per pane, so the id alone identifies nothing once
/// there are two panes, let alone two tabs: without the rest, removing one
/// tab's slot 0 would drop another's bookkeeping for its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
        let state_path = crate::paper_state::default_path();
        // Read before `config` is moved into the struct below: this seeds the
        // window's own copy of the span, which the trader then edits.
        let reach_span_minutes = config.history.reach_span_minutes;
        let paper_state = crate::paper_state::load(&state_path);
        let cmd_trading = crate::paper_trading::CmdTradingSettings::from_state(&paper_state);
        let (trades_dir, consolidated) = crate::paper_home::startup_home(
            config.paper.trades_dir.as_deref(),
            paper_state.trades_dir.as_deref(),
            &state_path,
        );
        let mut pane_ids = PaneIdAllocator::new();
        let loaded_layouts =
            Self::load_layouts(&crate::layouts::default_path(), &state_file::default_path());
        let mut tab = Tab::new(
            FIRST_TAB_ID,
            pane_ids.alloc(),
            feed_id.into(),
            symbol.into(),
            spec,
            feed,
            trades_dir.clone(),
        );
        tab.paper.set_cmd_trading(cmd_trading);
        // The named ladders the trader built last session, and the one the
        // ticket was set to. A selection naming a strategy the file no
        // longer carries selects nothing, which `set_order_strategies`
        // enforces rather than each caller remembering to.
        tab.paper.set_order_strategies(
            paper_state.order_strategies.clone().unwrap_or_default(),
            paper_state.selected_order_strategy.as_deref(),
        );
        // A step this build cannot parse is dropped rather than defaulted:
        // the instrument then follows its derived default, which is a real
        // answer, where a zero would be a silently broken wheel.
        tab.paper.set_ruler_steps(
            paper_state
                .ruler_steps
                .iter()
                .filter_map(|(symbol, step)| step.parse().ok().map(|value| (symbol.clone(), value)))
                .collect(),
        );
        // The risk per trade, and the money it is measured in. A trader who
        // never set any of this gets the mode off and an empty book, which
        // leaves every screen exactly as it was.
        //
        // Skipped when a launch hook set the risk for this run: an
        // environment variable is an explicit request for one run and
        // outranks the stored settings. Restoring them here left the hook's
        // whole point - the derived size, the sentence, the lock -
        // unreachable from a capture.
        if !tab.paper.risk_from_hook() {
            tab.paper
                .set_risk_settings(crate::risk_sizing::settings_from_sidecar(
                    paper_state.risk_per_trade_basis.as_deref(),
                    paper_state.risk_per_trade_amount.as_deref(),
                    paper_state.risk_per_trade_currency.as_deref(),
                    paper_state.risk_per_trade_percent.as_deref(),
                    paper_state.risk_per_trade_lock,
                ));
            tab.paper
                .set_capital(crate::risk_sizing::capital_from_records(
                    &paper_state.paper_capital,
                ));
        }
        tab.paper
            .set_instrument_money(crate::risk_sizing::book_from_records(
                &paper_state.instrument_money,
            ));
        // Resolved once: under test the settings path is a fresh scratch
        // file per call, and the load must read the same file the saves
        // will write.
        let footprint_settings_path = crate::footprint_config::settings_path();
        let indicator_presets_path = preset_file::default_path();
        let mut app = Self {
            tabs: vec![tab],
            active_tab: 0,
            next_tab_id: FIRST_TAB_ID + 1,
            layout_picker_open: false,
            layout_picker_autostart: std::env::var("QUANTICK_LAYOUT_PICKER")
                .is_ok_and(|value| value == "1"),
            pane_ids,
            added_symbols: symbols_file::load(&symbols_file::default_path()),
            symbols_path: symbols_file::default_path(),
            config,
            control_access: Some(crate::control::ControlAccess::new()),
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
            pending_styles: Vec::new(),
            settings_autostart: None,
            layouts: loaded_layouts.0,
            layouts_path: crate::layouts::default_path(),
            layouts_dirty: false,
            last_layout_change: None,
            layouts_save_blocked: loaded_layouts.1,
            layout_rename: None,
            layout_delete_confirm: None,
            last_script_poll: Instant::now(),
            replay_view: ReplayView::new(
                workspace.replay_folder.as_deref(),
                workspace.replay_day_before,
            ),
            dock: Dock::new(),
            toolrail: ToolRail::new(),
            surfaces: crate::surfaces::Surfaces::default(),
            pending_control_access_enable: false,
            operator_slots: std::collections::BTreeSet::new(),
            pending_control_annotation: None,
            pending_control_notification: None,
            pending_control_evidence: None,
            control_evidence_hook_frames: 0,
            pending_control_mark: None,
            pending_drawing_demo: false,
            pending_load_older: None,
            pending_load_older_candles: None,
            pending_history_note: None,
            pending_frvp_demo: false,
            pending_avwap_demo: false,
            pending_replay_restart: None,
            pending_venue_history_demo: None,
            pending_drawing_draft: None,
            inspector_position_dirty: false,
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            chart_layers_path: chart_layers::default_path(),
            saved_layer_mask: 0,
            saved_layer_tab: 0,
            surface: None,
            pending_maximize: std::env::var("QUANTICK_WINDOW_MAXIMIZED")
                .is_ok_and(|value| value == "1"),
            layer_actions: chart_layers::LayerActions::default(),
            footprint_config: crate::footprint_config::load(&footprint_settings_path),
            footprint_settings_path,
            indicator_presets: preset_file::PresetStore::load(&indicator_presets_path),
            indicator_presets_path,
            scripted_footprint: false,
            scripted_menu: None,
            scripted_menu_release: None,
            workspace_menu_rect: None,
            history_menu_rect: None,
            scripted_context_menu: None,
            scripted_context_menu_release: None,
            scripted_pointer: None,
            alerts: Box::new(crate::audio::Speaker::default()),
            alert_failure: None,
            pending_strategy_demo: None,
            scripted_indicator_settings: false,
            scripted_candle_width: None,
            scripted_pan_px: None,
            style: ChartStyle::default(),
            style_revision: 0,
            show_perf: true,
            progressive_history: true,
            history_reach: crate::history_reach::HistoryReach::default(),
            history_reach_span_minutes: reach_span_minutes,
            venue_lead_in: false,
            feed_chip_rect: None,
            // The hook stands in for a click on the opening tab's chip, which
            // is the first tab there is.
            feed_popup_tab: feed_notice::popup_open_from_env().then_some(FIRST_TAB_ID),
            tz: TzOffset::default(),
            trades_dir,
            trades_dir_picker: None,
            ui_state_path: ui_state::default_path(),
            save_on_exit: true,
            favorites_are_staged: false,
            bookmarks: Vec::new(),
            workspace_saved: false,
            recent_workspaces: Vec::new(),
            recent_on_disk: Vec::new(),
            workspace_picker: None,
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
        // The code's own baseline, and nothing more: what a launch actually
        // opens with is `config/chart-layers.toml`, applied by
        // `restore_chart_layers` immediately below and shipping the map on.
        // This line is what remains if that config is ever unreadable — a
        // layer nobody requested costing no projection. Capture is already
        // running either way, so it is a display choice and nothing else.
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
        // Local agent access, reachable without a click: the panel through the
        // Tools menu entry's own function, and the enable action through the
        // panel button's own function on the first frame — one path for the
        // human, the hook and any later operator. Enabling publishes a real
        // descriptor in the private runtime directory, removed on a clean exit.
        if std::env::var("QUANTICK_CONTROL_PANEL").is_ok_and(|value| value == "1")
            && let Some(access) = app.control_access.as_mut()
        {
            access.open_panel();
        }
        // Which scopes the next connection is granted, by ID — the panel's
        // own checkboxes without a hand on the mouse. `annotate` grants the
        // whole annotate tier (the profile follows the scopes), and any
        // comma-separated list of registered permission IDs is honoured, so a
        // scripted run can reproduce exactly the grant a trader would tick.
        if let Ok(scopes) = std::env::var("QUANTICK_CONTROL_SCOPES")
            && let Some(access) = app.control_access.as_mut()
            && let Err(error) = access.configure_scopes(&scopes)
        {
            {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_SCOPE_HOOK_REFUSED",
                    error = %error,
                    "QUANTICK_CONTROL_SCOPES named something this build does not register"
                );
            }
        }
        app.pending_control_access_enable =
            std::env::var("QUANTICK_CONTROL_ACCESS").is_ok_and(|value| value == "1");
        // A mark from a launch: `1` marks with no note, anything else is the
        // note. It goes through the same action the hotkey calls.
        app.pending_control_mark = std::env::var("QUANTICK_CONTROL_MARK")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| if value == "1" { String::new() } else { value });
        // An assistant's own object and an assistant's own interruption, from
        // a launch: the surfaces that say *who* acted cannot be photographed
        // without something an operator other than the trader put there.
        // One evidence bundle from a launch, through the same read a client
        // calls: the capture a validation run asserts against, and the
        // screenshot notice a capture run photographs.
        app.pending_control_evidence = std::env::var("QUANTICK_CONTROL_EVIDENCE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        app.pending_control_annotation = std::env::var("QUANTICK_CONTROL_ANNOTATE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        app.pending_control_notification = std::env::var("QUANTICK_CONTROL_NOTIFY")
            .ok()
            .filter(|value| !value.trim().is_empty());
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
        // Pinned favorites by tool id, comma-separated — the same restore
        // path the workspace file takes, so the hook cannot drift from it.
        if let Ok(ids) = std::env::var("QUANTICK_TOOL_FAVORITES") {
            let ids: Vec<String> = ids
                .split(',')
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect();
            app.toolrail.set_favorites(&ids);
            // A staged rail, so a star toggled during the run stays in the run.
            app.favorites_are_staged = true;
        }
        // Dock the rail against a named edge, so a validation run can shoot
        // the horizontal band without editing the workspace file.
        if let Ok(edge) = std::env::var("QUANTICK_TOOLBOX_DOCK") {
            let dock = match edge.trim() {
                "left" => Some(ToolboxDock::Left),
                "top" => Some(ToolboxDock::Top),
                "bottom" => Some(ToolboxDock::Bottom),
                _ => None,
            };
            if let Some(dock) = dock {
                app.toolrail.set_dock(dock);
            }
        }
        // Park the scrolling tool band mid-travel. Only the middle of the
        // run shows both chevrons live at once, and a screenshot cannot
        // click an arrow to get there.
        // Nonsense is refused rather than guessed, like the dock above: a
        // typo that silently parked the band at zero would photograph the
        // wrong state and call it the right one.
        if let Ok(offset) = std::env::var("QUANTICK_TOOLBAR_SCROLL") {
            let parked = match offset.trim() {
                "end" => Some(f32::INFINITY),
                other => other.parse::<f32>().ok().filter(|at| at.is_finite()),
            };
            if let Some(parked) = parked {
                app.toolrail.set_band_offset(parked);
            }
        }
        // Open a family flyout on the first frame — the star column lives
        // there, and a screenshot cannot click a caret.
        if let Ok(family_id) = std::env::var("QUANTICK_TOOLBOX_FLYOUT") {
            app.toolrail.request_flyout(family_id.trim().to_owned());
        }
        app.pending_drawing_demo = std::env::var("QUANTICK_DRAWINGS_DEMO")
            .is_ok_and(|value| matches!(value.as_str(), "1" | "bands"));
        // The replay seek, scripted: restart the recording once the session
        // has closed this many round trips. Zero is refused with everything
        // else that does not parse — a restart before any trade closed
        // photographs nothing this hook exists to show.
        app.pending_replay_restart = std::env::var("QUANTICK_REPLAY_RESTART_AFTER")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|trades| *trades > 0);
        // Pages of older trades fetched at launch — the "+ older" button
        // pressed, without a hand. The button's whole point is what it does
        // *after* the click, and the bars it prepends are the surface: a
        // capture that can only photograph the enabled button proves the
        // affordance exists and nothing about whether it works.
        app.pending_load_older = std::env::var("QUANTICK_LOAD_OLDER")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|pages| *pages > 0)
            .map(|pages| (pages, LOAD_OLDER_HOOK_FRAMES));
        // The same door onto the candle reach. A chart opens on one week
        // (`feed::TIME_HISTORY_SPAN_MS`) and the quarter is asked for a week at
        // a time, so "what does a deep chart look like" is a state no capture
        // could otherwise reach without a hand on the menu.
        app.pending_load_older_candles = std::env::var("QUANTICK_LOAD_OLDER_CANDLES")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|spans| *spans > 0)
            .map(|spans| (spans, LOAD_OLDER_CANDLES_HOOK_FRAMES));
        // And the *outcome* of a press, which is the half of this button a
        // trader actually acts on. Named by the ending's own log token
        // (`nothing_coming_back`, `venue_exhausted`, `page_budget_spent`, …)
        // and resolved through `CampaignEnd::from_action`, so an ending that
        // exists is photographable by name and one that does not yields no
        // note rather than the wrong one. It raises the real sentence through
        // `Tab::raise_history_note` — the same call a settled run makes — so
        // the picture is the picture a refusing venue gives.
        if let Ok(token) = std::env::var("QUANTICK_HISTORY_NOTE") {
            // Refused out loud, exactly as `QUANTICK_HISTORY_REACH` above
            // refuses an unknown reach: a capture run that silently got no
            // note reads the surface as broken, which is the conclusion this
            // whole branch exists to make impossible.
            match crate::history_reach::CampaignEnd::from_action(&token) {
                Some(end) if end.notice().is_some() => {
                    app.pending_history_note = Some((end, HISTORY_NOTE_HOOK_FRAMES));
                }
                Some(end) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_NOTE_HOOK_SILENT_ENDING",
                    ending = end.action(),
                    action = "no_note_raised",
                    "QUANTICK_HISTORY_NOTE named the one ending that says nothing"
                ),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_NOTE_HOOK_UNKNOWN",
                    token = %token,
                    accepted = %crate::history_reach::CampaignEnd::ALL
                        .map(crate::history_reach::CampaignEnd::action)
                        .join(", "),
                    action = "no_note_raised",
                    "QUANTICK_HISTORY_NOTE names no ending this build has"
                ),
            }
        }
        // How many anchors of the armed tool are already down when the run
        // opens — the half-placed state a screenshot cannot otherwise reach,
        // because it lives between two clicks. See `apply_drawing_draft`.
        app.pending_drawing_draft = std::env::var("QUANTICK_DRAWING_DRAFT")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|anchors| *anchors > 0);
        // One fixed-range volume profile, placed to straddle the venue-prefix
        // seam when there is one — the partial-coverage honesty label is a
        // surface, and this is how a validation run photographs it.
        app.pending_frvp_demo = std::env::var("QUANTICK_FRVP_DEMO")
            .is_ok_and(|value| matches!(value.trim(), "1" | "compare" | "stress"));
        // One anchored VWAP, anchored a stretch back so the average and its
        // band stack have room to develop on screen.
        app.pending_avwap_demo =
            std::env::var("QUANTICK_AVWAP_DEMO").is_ok_and(|value| value.trim() == "1");
        // The switch itself, so both sides of it are reachable without a
        // click. Set explicitly, it also overrides what the workspace saved:
        // a validation run must be able to pin the state it is photographing.
        // The same registry the menu lists from, so a hook can reach every
        // reach the trader can — and an unknown token is refused out loud
        // rather than silently leaving the default in place, which would look
        // like a press that ignored the run it was told to make.
        if let Ok(token) = std::env::var("QUANTICK_HISTORY_REACH") {
            match crate::history_reach::HistoryReach::from_token(&token) {
                Some(reach) => app.set_history_reach(reach),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_REACH_HOOK_UNKNOWN",
                    token = %token,
                    action = "keep_current_reach",
                    "QUANTICK_HISTORY_REACH names no reach this build has"
                ),
            }
        }
        if let Ok(raw) = std::env::var("QUANTICK_HISTORY_REACH_SPAN_MINUTES") {
            // Beside `QUANTICK_HISTORY_REACH`, because the reach and how far it
            // goes are one choice: a hook that could pick `by time` but not say
            // how much time would leave the operator setting half of it.
            match raw.trim().parse::<u32>() {
                Ok(minutes) => app.set_history_reach_span_minutes(minutes),
                Err(_) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_REACH_SPAN_HOOK_UNREADABLE",
                    value = %raw,
                    action = "keep_current_span",
                    "QUANTICK_HISTORY_REACH_SPAN_MINUTES is not a whole number of minutes"
                ),
            }
        }
        if let Ok(value) = std::env::var("QUANTICK_VENUE_LEAD_IN") {
            // `1` and `0`, and nothing else understood. A typo must not decide
            // a switch the trader set: read as a bare truthiness test, `true`
            // or `on` would silently turn the lead-in *off* and overwrite what
            // the workspace saved, and a capture run would photograph the off
            // state while reporting it as on.
            match value.trim() {
                "1" => app.venue_lead_in = true,
                "0" => app.venue_lead_in = false,
                other => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "VENUE_LEAD_IN_HOOK_UNKNOWN",
                    value = %other,
                    action = "keep_current_setting",
                    "QUANTICK_VENUE_LEAD_IN takes 1 or 0"
                ),
            }
        }
        if let Ok(value) = std::env::var("QUANTICK_PROGRESSIVE_HISTORY") {
            match value.trim() {
                "1" => app.progressive_history = true,
                "0" => app.progressive_history = false,
                // Nonsense is refused rather than guessed: a typo leaves the
                // trader's own setting alone instead of silently flipping it.
                _ => {}
            }
        }
        app.pending_venue_history_demo = std::env::var("QUANTICK_VENUE_HISTORY_DEMO")
            .ok()
            .and_then(|value| match value.trim() {
                "1" => Some(VenueHistoryDemo::Complete),
                "partial" => Some(VenueHistoryDemo::Partial),
                _ => None,
            });
        // The drawing chrome's five hooks, read here rather than on the first
        // drawn frame: the demo appliers run earlier in that frame and ask
        // whether the inspector is open, so a hook another hook depends on has
        // to be in place before any of them. They live with the fields they
        // set — see `surfaces::drawing_chrome::apply_launch_hooks`.
        crate::surfaces::drawing_chrome::apply_launch_hooks(&mut app.surfaces.drawing_chrome);

        // Same convenience for the aggression layer (bubbles + the live
        // column's footprint). Same code path as the toolbar toggle.
        if std::env::var("QUANTICK_BUBBLES_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().tape_mut().set_bubbles_enabled(true);
        }
        // The chart upside down, through the very setter the axis menu's
        // checkbox calls. The inverted frame is otherwise only reachable by
        // a long axis drag no scripted run can perform. Both panes of a
        // split layout: the hook exists so one capture audits every
        // price-mapped surface at once, and a half-inverted frame would
        // silently audit the time pane the right way up.
        if std::env::var("QUANTICK_INVERTED").is_ok_and(|value| value.trim() == "1") {
            let tab = app.active_tab_mut();
            for pane in tab.panes_mut() {
                pane.price_view.set_inverted(true);
            }
        }
        // The right-click itself, on the pane it names. The two panes open
        // different menus now, so a capture has to say which canvas it is
        // asking about; the click is delivered through the app's own input
        // path (see `raw_input_hook`), never by reaching into egui's menu
        // state, so what opens is what a trader's click opens.
        app.scripted_context_menu = std::env::var("QUANTICK_CONTEXT_MENU")
            .ok()
            .and_then(|value| ContextMenuPane::from_env_value(&value));

        // The mouse, parked over the candles. The pointer compass, the
        // crosshair and every hover readout exist only while a pointer is over
        // the chart, so without this the whole class of them is invisible to
        // anything but a hand on the mouse. Delivered as a real
        // `PointerMoved` through `raw_input_hook`, so what the chart does with
        // it is what it does with a trader's own mouse.
        app.scripted_pointer = std::env::var("QUANTICK_POINTER").ok().and_then(|value| {
            let parsed = parse_pointer_fraction(&value);
            if parsed.is_none() {
                tracing::warn!(
                    target: "quantick",
                    value = %value,
                    "POINTER_HOOK_REJECTED: expected <fx>,<fy> with both in 0..=1"
                );
            }
            parsed
        });

        // A rectangle with an armed force-bar strategy riding it (`1`), or
        // the arming dialog open over it (`popup`) — the strategy-anchor
        // surfaces, reachable by a validation run without a click. The
        // rectangle spans the recent tape and a stretch past its end, so
        // the chart-centre click of `QUANTICK_CONTEXT_MENU=chart` lands on
        // it and opens the per-drawing menu.
        app.pending_strategy_demo =
            std::env::var("QUANTICK_STRATEGY_DEMO")
                .ok()
                .and_then(|value| match value.trim() {
                    "1" | "armed" => Some(StrategyDemoMode::Armed),
                    "popup" => Some(StrategyDemoMode::Popup),
                    "alarm" => Some(StrategyDemoMode::AlarmPopup),
                    "alarm-sounds" => Some(StrategyDemoMode::AlarmSounds),
                    "alarm-badge" => Some(StrategyDemoMode::AlarmBadge),
                    "ended-badge" => Some(StrategyDemoMode::EndedBadge),
                    "paused-badge" => Some(StrategyDemoMode::PausedBadge),
                    _ => None,
                });

        // The Workspace menu, open. Its entries are the only door to
        // exporting, opening and locating a workspace, and a menu bar button
        // is not something a scripted run can press — so the hook presses it.
        // Anything but `workspace` opens nothing rather than the wrong menu.
        app.scripted_menu = std::env::var("QUANTICK_MENU")
            .ok()
            .and_then(|value| ScriptedMenu::from_token(value.trim()));

        // The tape switch in the canvas's top-right corner — the one control
        // that decides whether there is a band at all. Same setter the chip
        // calls, so a capture shows what a click shows. Anything but `on`/`off`
        // leaves the tape alone rather than guessing.
        if let Ok(value) = std::env::var("QUANTICK_TAPE") {
            match value.trim() {
                "on" => app.active_tab_mut().tape_mut().set_lane_enabled(true),
                "off" => app.active_tab_mut().tape_mut().set_lane_enabled(false),
                _ => {}
            }
        }
        // The tape's own layer switches. The two panes are configured apart and
        // the tape's menu is a right-click a scripted run cannot perform, so the
        // state behind it needs a door of its own — the state, not a second
        // way of drawing it: each entry calls the very setter the menu's
        // checkbox calls. Unlisted layers stay as they were, which is what
        // keeps this hook from being a second opinion about the whole tape.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_LAYERS") {
            let wanted: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .collect();
            let tape = app.active_tab_mut().tape_mut();
            if wanted.contains(&"none") {
                tape.set_lane_depth_visible(false);
                tape.set_lane_bubbles_enabled(false);
            } else {
                for entry in wanted {
                    match entry {
                        "heatmap" => tape.set_lane_depth_visible(true),
                        "bubbles" => tape.set_lane_bubbles_enabled(true),
                        "no-heatmap" => tape.set_lane_depth_visible(false),
                        "no-bubbles" => tape.set_lane_bubbles_enabled(false),
                        // A typo leaves the tape alone rather than guessing at
                        // a layer: a capture of the wrong state is worse than
                        // a capture of the default one.
                        _ => {}
                    }
                }
            }
        }
        // How much market time the tape shows: `auto` follows the bars, a
        // duration pins it (`90s`, `2min`, `120000ms`, or bare milliseconds).
        // Nonsense is refused rather than guessed at, so a typo photographs
        // the default instead of an invented window.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_WINDOW")
            && let Some(window) = parse_tape_window(value.trim())
        {
            app.active_tab_mut().tape_mut().set_live_lane_window(window);
        }
        // Same convenience for the candle footprint — the same field the
        // pane's layer menu writes, so a validation run sees exactly what a
        // click would show.
        if std::env::var("QUANTICK_FOOTPRINT_AUTOSTART").is_ok_and(|value| value == "1") {
            app.scripted_footprint = true;
            app.active_tab_mut().flow_pane.footprint_visible = true;
        }
        // Every style by its own id, resolved through the same registry the
        // panel's selector and the TOML read. A style reachable by click but
        // not by name is a style the second operator cannot pick, and one
        // more list to keep in step by hand.
        if let Ok(value) = std::env::var("QUANTICK_FOOTPRINT_STYLE") {
            match crate::footprint_config::FootprintStyle::from_id(value.trim()) {
                Some(style) => app.footprint_config.style = style,
                // Named and unknown is a typo in a validation script, and a
                // silent fallback to the default would have it photograph the
                // wrong style and call it a pass.
                None => tracing::warn!(
                    requested = %value,
                    known = ?crate::footprint_config::FootprintStyle::ALL
                        .map(crate::footprint_config::FootprintStyle::id),
                    "QUANTICK_FOOTPRINT_STYLE names no known style; keeping the current one",
                ),
            }
        }
        // The indicator settings dialog (sliders + live preview): pair with
        // an autostart hook that loads an indicator; the dialog opens for
        // the first slot as soon as its inputs arrive from the worker.
        app.scripted_indicator_settings =
            std::env::var("QUANTICK_INDICATOR_SETTINGS").is_ok_and(|value| value == "1");
        // The zoom, scriptable: the footprint's detail levels are functions
        // of candle width, and a validation run cannot drag a scroll wheel.
        // Same clamp as the gesture (see Viewport::set_px_per_bar).
        if let Ok(value) = std::env::var("QUANTICK_CANDLE_WIDTH")
            && let Ok(px) = value.trim().parse::<f32>()
        {
            app.scripted_candle_width = Some(px);
            app.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
        }
        // The bubble budget, scriptable. The fold is the one bubble state a
        // capture cannot otherwise reach: it needs a tape dense enough to
        // exhaust a budget of seven hundred, which is a market condition and
        // not a setting. `QUANTICK_BUBBLE_BUDGET=8` squeezes the same budget
        // the frame always spends, through the same field the projection
        // reads, so what a screenshot shows is what a busy session shows —
        // folded marks wearing their ring and their count.
        if let Ok(value) = std::env::var("QUANTICK_BUBBLE_BUDGET")
            && let Ok(budget) = value.trim().parse::<usize>()
            && budget > 0
        {
            for tab in &mut app.tabs {
                tab.tape_mut().set_primitive_budget(budget);
            }
        }
        // A starved tape, scriptable — the state this whole fix is about. The
        // bubbles trailing the lane's right edge, and past its window leaving
        // it empty, happen when the book keeps arriving and nothing prints. No
        // setting produces that and no capture can wait for the market to do
        // it, so `QUANTICK_TAPE_STARVE_AFTER_MS=8000` stops feeding the tape
        // eight seconds in and lets the book run. Nothing is forged: the
        // prints are withheld through the feed's own call, and the axis then
        // reports the age it actually observes.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_STARVE_AFTER_MS")
            && let Ok(after_ms) = value.trim().parse::<i64>()
            && after_ms >= 0
        {
            for tab in &mut app.tabs {
                tab.tape_mut().set_starve_tape_after_ms(after_ms);
            }
        }
        // The pan, scriptable, for the same reason: the projection margin and
        // the way back from history are states a screenshot cannot otherwise
        // reach. `QUANTICK_PAN_PX=-9000` is "shove the chart as far left as it
        // goes" — the margin then holds it exactly where the gesture would.
        if let Ok(value) = std::env::var("QUANTICK_PAN_PX")
            && let Ok(px) = value.trim().parse::<f32>()
            && px.is_finite()
        {
            app.scripted_pan_px = Some(px);
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
        // The folded legend, reachable from a clean launch: without it the
        // collapsed state is un-photographable by an agent, and a surface no
        // harness can reach is a surface no visual QA covers. Goes through
        // `set_focused_legend_collapsed`, the same call the chevron and the
        // menu entry make — never a field poked from the side.
        if std::env::var("QUANTICK_LEGEND_COLLAPSED").is_ok_and(|value| value == "1") {
            app.set_focused_legend_collapsed(true);
        }
        // Put the active layout on the first tab's panes before any autostart
        // hook: the file is what the user actually had open.
        app.seed_new_panes();
        // The layout strip's hooks (`ui-harness`): open on a named layout,
        // creating it when the file has none by that name, and open the
        // rename box on the active one.
        if let Ok(name) = std::env::var("QUANTICK_LAYOUT_TAB")
            && let Some(name) = crate::layouts::clean_name(&name)
        {
            let wanted = app.layouts().by_name(&name).map(|layout| layout.id);
            let outcome = match wanted {
                Some(id) => app.switch_layout(id).map(|_| id),
                None => app.create_layout(Some(&name)),
            };
            if let Err(error) = outcome {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUT_TAB_HOOK_REFUSED",
                    layout = %name,
                    %error,
                    action = "hook_ignored",
                    "QUANTICK_LAYOUT_TAB could not open the layout"
                );
            }
        }
        // One layout per pane, by name, in pane-address order (`flow,top,bottom`):
        // a capture of two charts on two layouts side by side. Names the book
        // lacks are created empty; an empty entry leaves that pane alone.
        if let Ok(names) = std::env::var("QUANTICK_PANE_LAYOUTS") {
            app.apply_pane_layouts_hook(&names);
        }
        if std::env::var("QUANTICK_LAYOUT_RENAME").is_ok_and(|value| value == "1") {
            let active = app.focused_pane_layout();
            app.begin_layout_rename(active);
        }
        if std::env::var("QUANTICK_LAYOUT_DELETE").is_ok_and(|value| value == "1") {
            let active = app.focused_pane_layout();
            app.apply_strip_action(crate::layout_strip::StripAction::Delete(active));
        }
        // The settings dialog, reachable without a pointer: the index of the
        // indicator to open it on, among the focused pane's views in add
        // order, and optionally the tab (`0` or `inputs`, `1` or `style`).
        // Every UI surface owes the harness a hook, and a dialog that can only
        // be reached by double-clicking is a dialog no visual capture can see.
        app.settings_autostart = std::env::var("QUANTICK_INDICATOR_SETTINGS")
            .ok()
            .and_then(|value| Self::parse_settings_hook(&value));
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
                    Some(_) => {
                        // Straight onto the focused pane, with no mirror: an
                        // env var is not a user edit. Without this, a scripted
                        // validation run appended its own scripts to the
                        // layout and they opened by themselves on the next
                        // plain launch — config presence activating
                        // something, which the rules forbid. The natives hook
                        // above never registers a kind, so it is already inert.
                        let (tab, side) = {
                            let tab = app.active_tab();
                            (tab.id, tab.focused_side())
                        };
                        app.add_indicator_at(
                            tab,
                            side,
                            &SavedKind::Script {
                                name: name.to_owned(),
                            },
                        );
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
        // Whether a recording opens with the day before it joined in front.
        // Read before anything loads a session, because that is the frame the
        // setting is consulted on. Staged rather than chosen: a validation run
        // states the screen it wants to photograph, and must not write a QA
        // preference into the trader's workspace — the same rule the replay
        // folder follows.
        if let Ok(value) = std::env::var("QUANTICK_REPLAY_DAY_BEFORE") {
            // Refused rather than guessed, like the autostart hook below it: a
            // typo that quietly meant "off" would photograph a single-day
            // chart under a run that believed it had staged a join, which is
            // the one state this hook exists to reach.
            let staged = match value.trim() {
                "1" | "true" | "on" => Some(true),
                "0" | "false" | "off" => Some(false),
                _ => None,
            };
            match staged {
                Some(enabled) => {
                    app.replay_view.stage_day_before(enabled);
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "REPLAY_DAY_BEFORE_STAGED",
                        enabled,
                        requested = value.trim(),
                        "the day before was staged for this run"
                    );
                }
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DAY_BEFORE_UNREADABLE",
                    requested = value.trim(),
                    action = "left_as_the_workspace_has_it",
                    "the day-before hook takes 0 or 1; this run keeps the trader's own setting"
                ),
            }
        }
        // Same convenience for Market Replay: scan the folder in force — the
        // hook, else the stored pick, else the documents home — and play its
        // first session. The same code path a click takes, so a scripted run
        // and a person get the same behaviour.
        // `1` loads and plays, as it always has. `paused` loads and waits,
        // which is what a person now gets when they open a recording, and a
        // state no other hook can reach.
        let autostart_play = match std::env::var("QUANTICK_REPLAY_AUTOSTART")
            .unwrap_or_default()
            .trim()
        {
            "1" => Some(true),
            "paused" => Some(false),
            _ => None,
        };
        if let Some(play) = autostart_play {
            let speed = std::env::var("QUANTICK_REPLAY_SPEED")
                .ok()
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|speed| *speed > 0.0)
                .unwrap_or(1.0);
            // Which recording, when the folder holds more than one. The
            // scan lists them oldest first, so without this a folder of days
            // always opens the one that can have nothing joined in front of
            // it — the single state this hook family exists to avoid.
            let day = std::env::var("QUANTICK_REPLAY_SESSION")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let started = app.replay_view.autostart(speed, day.as_deref(), play);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_AUTOSTART",
                // The folder actually scanned, not the environment variable:
                // once a stored pick can supply it, reading the hook back
                // would report an empty folder for a run that scanned a full
                // one — a log that lies about the input it acted on.
                folder = app.replay_view.folder_in_use(),
                speed,
                day = day.as_deref().unwrap_or("(first)"),
                day_before = app.replay_view.day_before(),
                play,
                started,
                action = if started { "load_first_session" } else { "open_browser" },
                "market replay autostart"
            );
        }
        // The session list, opened outright. The browser is one menu entry
        // deep and a validation run has no mouse, so without this the half
        // that shows what a trader already has is the one half no capture can
        // reach — and "I could not find my recordings" is a report about that
        // window, not about the list inside it.
        if std::env::var("QUANTICK_REPLAY_BROWSER").is_ok_and(|value| value == "1") {
            app.replay_view.open_browser();
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_BROWSER_AUTOSTART",
                folder = app.replay_view.folder_in_use(),
                "opened the session browser"
            );
        }
        // The download half of the same browser. Reached on its own because a
        // scripted run has to photograph the Get data tab without a click, and
        // it is a different screen from the session list beside it. Takes the
        // same path the tab click takes — including, for a bare `1`, the
        // chart's own instrument, because that is what clicking the tab now
        // fills the field with and a hook that opened it emptier than a click
        // would photograph a screen no person ever sees.
        if let Ok(value) = std::env::var(crate::replay_view::GET_DATA_ENV) {
            let symbol = match value.trim() {
                "1" | "" => Some(app.active_tab().symbol.clone()),
                symbol => Some(symbol.to_string()),
            };
            app.replay_view.open_get_data(symbol.as_deref());
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_GET_DATA_AUTOSTART",
                symbol = symbol.as_deref().unwrap_or(""),
                "opened the replay download tab"
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
        // The calendar the report grew: reachable open, on a chosen day or
        // a chosen span, with no clicks at all.
        if let Ok(spec) = std::env::var("QUANTICK_PAPER_CALENDAR") {
            match crate::paper_calendar::parse_selection(&spec) {
                Some(selection) => app.active_tab_mut().paper.autostart_calendar(selection),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_CALENDAR_AUTOSTART_UNKNOWN",
                    spec = %spec,
                    action = "calendar_left_closed",
                    "QUANTICK_PAPER_CALENDAR is not 1, YYYY-MM-DD or YYYY-MM-DD..YYYY-MM-DD"
                ),
            }
        }
        // Which instrument's saved history the ledger lists.
        if let Ok(spec) = std::env::var("QUANTICK_LEDGER_SCOPE") {
            let scope = match spec.trim() {
                "chart" => Some(crate::paper_trading::LedgerScope::Chart),
                "all" => Some(crate::paper_trading::LedgerScope::All),
                "" => None,
                symbol => Some(crate::paper_trading::LedgerScope::Symbol(symbol.to_owned())),
            };
            match scope {
                Some(scope) => app.active_tab_mut().paper.set_ledger_scope(scope),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LEDGER_SCOPE_AUTOSTART_UNKNOWN",
                    scope = %spec,
                    action = "ledger_left_on_the_chart",
                    "QUANTICK_LEDGER_SCOPE wants `chart`, `all`, or a symbol folder name"
                ),
            }
        }
        // And the ledger past its first page of saved history — a state
        // only a click on "show older" otherwise reaches.
        if let Ok(text) = std::env::var("QUANTICK_LEDGER_PAGES") {
            match text.trim().parse::<usize>() {
                Ok(pages) if pages >= 1 => {
                    app.active_tab_mut().paper.autostart_ledger_pages(pages);
                }
                _ => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LEDGER_PAGES_AUTOSTART_UNKNOWN",
                    pages = %text,
                    action = "ledger_left_on_its_first_page",
                    "QUANTICK_LEDGER_PAGES wants a whole number of pages, one or more"
                ),
            }
        }
        // Every day folded shut — the one-line-per-day read, which is
        // otherwise a click on each header.
        if std::env::var("QUANTICK_LEDGER_FOLD").is_ok_and(|value| value == "1") {
            let tz = app.tz;
            app.active_tab_mut().paper.autostart_folded_days(tz);
        }
        // The report's trade list is open by default, so the hook is how a
        // capture reaches it collapsed.
        if let Ok(value) = std::env::var("QUANTICK_PAPER_REPORT_LIST") {
            app.active_tab_mut()
                .paper
                .set_report_list_open(value.trim() != "0");
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
                    accepted = %crate::canvas_layout::LAYOUT_PRESETS
                        .iter()
                        .map(|preset| preset.id)
                        .collect::<Vec<_>>()
                        .join(", "),
                    // Built from the registry rather than spelled out: a
                    // hand-written list goes stale the day a preset is added,
                    // and a run that mistypes an id deserves the real one.
                    "QUANTICK_LAYOUT names no canvas layout"
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
        // The three file entries, reachable with no click for the same reason
        // (`.claude/skills/ui-harness`). Each runs the menu entry's own code
        // past the OS dialog — the dialog is the one thing a scripted run
        // cannot drive, so the path is given instead of picked. They really
        // write and really replace the cockpit, so point `QUANTICK_UI_STATE`
        // and its sibling stores at scratchpad files first.
        if let Ok(path) = std::env::var("QUANTICK_WORKSPACE_EXPORT") {
            app.export_workspace_to(std::path::Path::new(&path));
        }
        if let Ok(path) = std::env::var("QUANTICK_WORKSPACE_IMPORT") {
            app.import_workspace_from(std::path::Path::new(&path));
        }
        // An env var is not a user edit: what the autostart hooks switched on
        // must not be written back as though the user had asked for it every
        // launch from now on. Same rule the indicator state follows.
        app.saved_layer_mask = app.layer_mask();
        // The cockpit rescue ran in `main`, before any store was read. A
        // silent one would look like the app relocated the trader's settings
        // behind their back — and leave them not knowing which folder to back
        // up. Same toast channel the journal's rescue uses.
        // `QUANTICK_TOAST=paper`: a simulator acknowledgement, posted through
        // the panel's own `show_toast`.
        //
        // The surface's own hook can raise a message *in* the lane; only this
        // one proves the route to it, which is the half this change built —
        // the panel's outbox, the drain in `settle_paper_panels`, and the
        // eight-second clock the surface owns. Without it the paper path is
        // reachable from a launch only by waiting for a fill and hoping the
        // shutter lands inside the window: the demo trades within the first
        // second and the message is gone eight seconds later, so a capture
        // run photographs an empty lane and cannot tell that from a defect.
        if std::env::var("QUANTICK_TOAST").is_ok_and(|value| value == "paper") {
            app.tabs[0]
                .paper
                .show_toast("SIM: stop filled at 169 790 — flat.".to_owned());
        }
        if let Some(notice) = crate::store_home::rescue_notice() {
            app.tabs[0].paper.show_toast(notice);
        }
        if let Some(summary) = consolidated
            && summary.imported() > 0
        {
            // A silent rescue would look like the app moved files on its
            // own; the toast says what happened and that copies were made.
            app.tabs[0]
                .paper
                .show_toast(crate::paper_home::import_toast(&summary));
        }
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
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.trades_dir = Some(dir.display().to_string());
        crate::paper_state::save(&path, &state);
        self.trades_dir = dir;
        for tab in &mut self.tabs {
            tab.paper.set_trades_dir(self.trades_dir.clone());
        }
    }

    /// Persist the active tab's cmd-trading settings and fan them out —
    /// one gesture, one meaning, every tab (the trades-dir rule).
    fn persist_cmd_trading(&mut self) {
        let settings = self.active_tab().paper.cmd_trading();
        for tab in &mut self.tabs {
            tab.paper.set_cmd_trading(settings);
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.cmd_trading_enabled = Some(settings.enabled);
        state.cmd_buy_modifier = Some(settings.buy.as_str().to_owned());
        state.cmd_entry_kind = Some(settings.kind.as_str().to_owned());
        state.cmd_sell_modifier = Some(settings.sell.as_str().to_owned());
        crate::paper_state::save(&path, &state);
    }

    /// Save and fan out the strategies after a capability changed them, so a
    /// named call leaves the same durable trace a click does.
    pub(crate) fn control_persist_order_strategies(&mut self) {
        self.persist_order_strategies();
    }

    /// Save and fan out the risk per trade after a capability changed it.
    pub(crate) fn control_persist_risk_settings(&mut self) {
        self.persist_risk_settings();
    }

    /// Persist the risk per trade, the declared capital and the instrument
    /// money, and fan all three out.
    ///
    /// App-wide, like the ticket's other settings: a ceiling a trader sets
    /// in one tab is one they mean everywhere, and what a point of WIN is
    /// worth does not change because a second tab is looking at it.
    pub(crate) fn persist_risk_settings(&mut self) {
        let risk = self.active_tab().paper.risk_settings().clone();
        let capital = self.active_tab().paper.capital().clone();
        let book = self.active_tab().paper.instrument_money().clone();
        for tab in &mut self.tabs {
            tab.paper.set_risk_settings(risk.clone());
            tab.paper.set_capital(capital.clone());
            tab.paper.set_instrument_money(book.clone());
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.risk_per_trade_basis = Some(risk.basis.token().to_owned());
        state.risk_per_trade_amount = Some(risk.amount.normalize().to_string());
        state.risk_per_trade_percent = Some(risk.percent.normalize().to_string());
        state.risk_per_trade_lock = Some(risk.lock);
        state.paper_capital = crate::risk_sizing::records_from_capital(&capital);
        state.instrument_money = crate::risk_sizing::records_from_book(&book);
        crate::paper_state::save(&path, &state);
    }

    /// Persist the named exit strategies and the ticket's selection, and fan
    /// them out - app-wide like cmd trading, because a ladder a trader built
    /// in one tab is a ladder they mean everywhere.
    fn persist_order_strategies(&mut self) {
        // The wheel's per-instrument step rides with the strategies: both
        // are ticket settings the trader configures once, and both are
        // app-wide rather than per tab.
        let steps: std::collections::BTreeMap<String, String> = self
            .active_tab()
            .paper
            .ruler_steps()
            .iter()
            .map(|(symbol, step)| (symbol.clone(), step.normalize().to_string()))
            .collect();
        let strategies = self.active_tab().paper.order_strategies().to_vec();
        let selected = self
            .active_tab()
            .paper
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        for tab in &mut self.tabs {
            tab.paper
                .set_order_strategies(strategies.clone(), selected.as_deref());
            tab.paper.set_ruler_steps(
                steps
                    .iter()
                    .filter_map(|(symbol, step)| {
                        step.parse().ok().map(|value| (symbol.clone(), value))
                    })
                    .collect(),
            );
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.order_strategies = Some(strategies);
        state.selected_order_strategy = selected;
        state.ruler_steps = steps;
        crate::paper_state::save(&path, &state);
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

    /// Read-only application roots available to the on-demand control
    /// projections. The gateway never receives `QuantickApp`; it receives the
    /// owned DTOs built from these narrow views.
    /// One tab by position, for a control capability that resolved an id.
    pub(crate) fn control_tab_at(&self, index: usize) -> Option<&Tab> {
        self.tabs.get(index)
    }

    /// The mutable twin, for the cockpit tier.
    ///
    /// Narrow on purpose: the layout capabilities need to *change* a tab, and
    /// handing them the whole application would let a later one reach past the
    /// canvas into the feed or the simulator.
    pub(crate) fn control_tab_at_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    /// One tab beside the configuration it reads, by position.
    ///
    /// [`Self::active_with_config`] for a tab that is not necessarily the
    /// active one — a capability names the tab it acts on, and respawning a
    /// feed needs the feed table the same way a click in the corner does.
    pub(crate) fn control_tab_with_config(
        &mut self,
        index: usize,
    ) -> Option<(&mut Tab, &AppConfig)> {
        let Self { tabs, config, .. } = self;
        tabs.get_mut(index).map(|tab| (tab, &*config))
    }

    /// The trading host of the tab on screen — where the `trade.*` actions
    /// land. The active tab and not an addressed one: an order belongs to
    /// the symbol the trader is looking at, and a call that could quietly
    /// trade a chart nobody has open is a call nobody should be able to
    /// make.
    pub(crate) fn control_active_paper_mut(
        &mut self,
    ) -> Option<&mut crate::paper_trading::PaperTrading> {
        // Fallible, because the rest of the control code does not trust the
        // invariant either: `annotate::resolve_target` guards an empty tab
        // list and clamps the index, and two more sites clamp it. A
        // `trade.*` call must answer "this window has no chart open" rather
        // than panic the whole trading application, and it must resolve the
        // *same* tab its own read-back resolves.
        self.tabs.get_mut(self.active_tab).map(|tab| &mut tab.paper)
    }

    /// The read side of [`Self::control_active_paper_mut`], resolved the same
    /// way so a call and its read-back can never name different tabs.
    pub(crate) fn control_active_paper(&self) -> Option<&crate::paper_trading::PaperTrading> {
        self.tabs.get(self.active_tab).map(|tab| &tab.paper)
    }

    pub(crate) fn control_tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub(crate) fn control_active_tab_index(&self) -> usize {
        self.active_tab
    }

    /// Put one Quantick Pine script on the focused pane behind a fresh slot.
    ///
    /// The one door: the script library's click arrives here, and so does an
    /// authorized agent's `indicator.script.attach`. Returns the tab, the
    /// pane and the slot, which is what a caller needs to detach it again.
    pub(crate) fn attach_script_indicator(
        &mut self,
        name: String,
        text: String,
        by_operator: bool,
    ) -> (u64, crate::control::PaneSideDto, SlotId) {
        let slot = self
            .focused_pane_mut()
            .add_indicator(IndicatorSource::Script {
                name: name.clone(),
                text,
            });
        let owner = self.target_slot(slot);
        let kind = SavedKind::Script { name };
        self.slot_kinds.push((owner, kind.clone()));
        // Whose slot this is decides who may take it away again: the annotate
        // tier removes what it attached, never what the trader put there. An
        // operator's overlay stays on the one pane it was attached to and
        // out of the layout; the trader's script is a layout edit and goes
        // onto every pane.
        if by_operator {
            self.operator_slots.insert(owner);
        } else {
            self.mirror_add(owner, &kind);
        }
        self.note_indicator_edit_at(owner.tab, owner.side);
        (owner.tab, owner.side.into(), slot)
    }

    /// Take one slot **an operator attached** off the chart, through the same
    /// removal the indicator legend's own button uses.
    ///
    /// `Err` when the slot is one the trader put there: this tier adds and
    /// takes back its own, and never removes work done by hand (plan §2.6).
    /// `Ok(false)` when there is no such slot at all.
    pub(crate) fn detach_script_indicator(&mut self, slot: u64) -> Result<bool, ()> {
        // A slot number is allocated per pane, so several panes can carry the
        // same one: the operator's own is the one to take, and matching on the
        // number alone would remove whichever pane happened to be registered
        // first — the trader's, as often as not.
        let mut known = false;
        let mut target = None;
        for (owner, _) in &self.slot_kinds {
            if owner.slot.0 != slot {
                continue;
            }
            known = true;
            if self.operator_slots.contains(owner) {
                target = Some(*owner);
                break;
            }
        }
        let Some(target) = target else {
            // A slot that exists but belongs to the trader is refused; one
            // that exists nowhere simply was not there.
            return if known { Err(()) } else { Ok(false) };
        };
        self.remove_indicator_at(target);
        self.operator_slots.remove(&target);
        Ok(true)
    }

    /// Open the assistant's popup. One at a time: a second message replaces
    /// the first rather than stacking windows over a chart someone is
    /// trading, and the trader dismisses it.
    pub(crate) fn show_agent_popup(&mut self, popup: crate::control::AgentPopup) {
        self.surfaces.agent_popup.show(popup);
    }

    /// Post one line to the window's own acknowledgement lane — the same
    /// channel a delete or a workspace save uses, with no Undo: there is
    /// nothing to take back from having been told something.
    pub(crate) fn show_agent_toast(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }

    /// Ask for the platform's attention sound, through the same sink the
    /// alarms use, and report honestly when it could not be made rather
    /// than letting a client believe it was heard.
    pub(crate) fn sound_agent_alert(&mut self) -> Option<String> {
        self.alerts
            .play(&[crate::audio::Cue::default()])
            .err()
            .map(ToOwned::to_owned)
    }

    /// One pane, by tab position and side — the mutable half of
    /// [`Self::control_tabs`], for the actions that place objects.
    pub(crate) fn control_pane_mut(
        &mut self,
        tab_index: usize,
        side: crate::pane::PaneSide,
    ) -> &mut ChartPane {
        self.tabs[tab_index].pane_mut(side)
    }

    /// What a freshly placed object of `tool` opens with, through the same
    /// door the click path uses — saved defaults, named preset and all.
    pub(crate) fn control_new_drawing(&self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        drawings::new_drawing_from_defaults(&self.drawing_presets, tool)
    }

    pub(crate) fn control_config(&self) -> &AppConfig {
        &self.config
    }

    /// The window's footprint setup — the one a pane falls back to when it
    /// carries no override of its own.
    pub(crate) fn control_footprint_config(&self) -> &crate::footprint_config::FootprintConfig {
        &self.footprint_config
    }

    /// The window's shared chart style, which owns the layers no pane does.
    pub(crate) fn control_style(&self) -> &ChartStyle {
        &self.style
    }

    /// The drawing tool rail: which tool is armed, and whether it is on
    /// screen at all.
    pub(crate) fn control_tool_rail(&self) -> &ToolRail {
        &self.toolrail
    }

    /// The colour the chart's corner is wearing, or `None` while the chart
    /// is being fed.
    ///
    /// The status line's provenance dot takes this rather than deciding for
    /// itself. It used to read the connection alone, which is a socket's
    /// opinion: a terminal that froze with the socket open had the
    /// bottom-left of the window saying `live` while the bottom-right said
    /// `offline`, about the same feed, at the same moment. Two surfaces
    /// disagreeing about the one question the trader is asking is worse than
    /// either answer alone, so there is one report and both read it.
    fn feed_offline_accent(
        &self,
        stall: Option<&crate::feed::stall::Stall>,
    ) -> Option<egui::Color32> {
        feed_notice::report(&self.active_tab().notice, stall)
            .filter(feed_notice::Report::is_offline)
            .map(|report| report.accent())
    }

    /// Where the feed's offline chip was painted, or `None` when it was not.
    ///
    /// The projection reads what was drawn rather than re-deciding it, so the
    /// scene and the screen cannot disagree across the edge of a stall budget.
    pub(crate) fn control_feed_chip_rect(&self) -> Option<egui::Rect> {
        self.feed_chip_rect
    }

    /// Whether the recovery popup that chip opens is showing, on the chart
    /// the trader is looking at.
    pub(crate) fn control_feed_popup_open(&self) -> bool {
        self.feed_popup_tab == Some(self.active_tab().id)
    }

    /// The right-hand dock: whether it is shown, and which tab is open.
    pub(crate) fn control_dock(&self) -> &Dock {
        &self.dock
    }

    pub(crate) fn control_timezone(&self) -> TzOffset {
        self.tz
    }

    pub(crate) fn control_workspace_flags(&self) -> (bool, bool, bool) {
        (self.save_on_exit, self.show_perf, self.progressive_history)
    }

    /// Choose how far one press of *load older* reaches.
    ///
    /// The named call behind the history menu's reach chips and the
    /// `QUANTICK_HISTORY_REACH` hook — one path, so an operator without a
    /// mouse sets what a click sets. Mirrored onto every tab by `drain_tabs`,
    /// where a run in flight also reads it: withdrawing the longer reach is
    /// how a trader calls that run off.
    pub(crate) fn set_history_reach(&mut self, reach: crate::history_reach::HistoryReach) {
        self.history_reach = reach;
    }

    /// How far back one press of the `by time` reach pulls, in minutes of
    /// traded time.
    ///
    /// Clamped rather than refused: a span of zero is a press that asks for
    /// nothing, and the operator that sent it meant *some* history. The
    /// ceiling is the campaign's own span cap, past which no run can reach
    /// anyway, so accepting a larger number would be promising a reach the
    /// budgets forbid.
    pub(crate) fn set_history_reach_span_minutes(&mut self, minutes: u32) {
        let ceiling = (crate::history_reach::MAX_CAMPAIGN_SPAN_MS / 60_000) as u32;
        self.history_reach_span_minutes = minutes.clamp(1, ceiling);
    }

    /// What that span is now, for an operator reading back what it set.
    pub(crate) fn control_history_reach_span_minutes(&self) -> u32 {
        self.history_reach_span_minutes
    }

    /// How far the window's *load older* press reaches, and whether a chart
    /// cut by trades carries the venue's candles.
    ///
    /// Both are choices an operator without a mouse has to be able to read
    /// back after setting them — the reach especially, since it decides
    /// whether one press is one request or a run of them.
    pub(crate) fn control_history_settings(&self) -> (crate::history_reach::HistoryReach, bool) {
        (self.history_reach, self.venue_lead_in)
    }

    /// Whether a recording opens with the session day before it joined in
    /// front, and a download fetches that day's tape too.
    ///
    /// A choice an operator without a mouse has to be able to read back after
    /// setting it: it decides what a replay they are about to open will hold.
    pub(crate) fn control_replay_day_before(&self) -> bool {
        self.replay_view.day_before()
    }

    /// Invoke one registered control action from inside the application,
    /// attributed to the human at this window (or to automation when a
    /// control trace replays it). The hotkey, the `QUANTICK_CONTROL_MARK`
    /// hook and the tests all arrive here; there is no second path.
    pub(crate) fn control_action(
        &mut self,
        capability_id: &str,
        capability_version: u32,
        origin: crate::control::ActionOrigin,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        let outcome =
            access.invoke_local_action(self, capability_id, capability_version, input, origin);
        self.control_access = Some(access);
        outcome
    }

    /// How many objects an operator other than the trader placed, across
    /// every pane one can reach — an assistant may annotate any open tab, so
    /// counting the active pane alone would offer to take back a subset and
    /// call it all of them.
    fn authored_object_count(tabs: &[Tab]) -> usize {
        tabs.iter()
            .map(|tab| {
                tab.panes()
                    .map(|(pane, _side)| pane.drawings.authored_count())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Take back every object an operator placed, wherever it is. One undo
    /// entry per pane, and the resting orders of any armed strategy go with
    /// the objects they were anchored to.
    fn remove_every_authored_object(&mut self) -> usize {
        let mut removed = 0;
        for tab in &mut self.tabs {
            // Every pane the tab holds, not the two it used to. "Remove
            // objects placed for you" promises to take them *all* back, and a
            // sweep that skipped the second stacked chart would leave an
            // assistant's marks behind while reporting the job done.
            for pane in tab.panes_mut() {
                let taken = pane.drawings.remove_authored();
                if taken > 0 {
                    pane.sweep_strategy_orphans();
                    removed += taken;
                }
            }
        }
        removed
    }

    /// The annotate tier's launch hooks: one agent-authored label, one
    /// notification. Both go through the registered action with an agent
    /// actor — the same path the gateway takes for a remote client — so what
    /// a screenshot shows is what a real assistant would have produced.
    fn apply_control_annotate_hooks(&mut self) {
        if let Some(text) = self.pending_control_annotation.take() {
            let anchor = {
                let pane = self.active_tab().drawing_pane();
                let slot = pane.slots().saturating_sub(1);
                match (pane.slot_open_time(slot), pane.closed_bar(slot)) {
                    (Some(time), Some(bar)) => Some(serde_json::json!({
                        "time_unix_ms": time,
                        "price": rust_decimal::prelude::ToPrimitive::to_f64(&bar.close)
                            .unwrap_or(1.0)
                            .to_string(),
                    })),
                    // No bars yet: put the hook back and take it next frame,
                    // rather than annotating a chart that has nothing on it.
                    _ => {
                        self.pending_control_annotation = Some(text.clone());
                        None
                    }
                }
            };
            if let Some(anchor) = anchor {
                self.pending_control_annotation = None;
                self.run_hook_action(
                    "annotate.label.create",
                    serde_json::json!({ "anchors": [anchor], "text": text }),
                );
            }
        }
        if let Some(request) = self.pending_control_notification.take() {
            let (channel, message) = request
                .split_once(':')
                .unwrap_or(("toast", request.as_str()));
            let capability = match channel.trim() {
                "popup" => Some("notify.popup"),
                "sound" => Some("notify.sound"),
                "toast" => Some("notify.toast"),
                other => {
                    tracing::warn!(
                        target: "quantick::control",
                        event_code = "CONTROL_NOTIFY_HOOK_REFUSED",
                        channel = other,
                        "QUANTICK_CONTROL_NOTIFY names no notification channel"
                    );
                    None
                }
            };
            if let Some(capability) = capability {
                self.run_hook_action(
                    capability,
                    serde_json::json!({ "message": message, "title": "From your assistant" }),
                );
            }
        }
    }

    /// Invoke one registered action as an *agent* would, from inside this
    /// window. The hooks use it so a screenshot shows a real assistant's
    /// object, attribution and all, without a client on the socket.
    fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        // No identity, no actor to sign with: the same structured refusal an
        // action gets, rather than a panic on the first frame.
        let Some(actor) = access.hook_agent_actor() else {
            self.control_access = Some(access);
            return Err(quantick_control::error::ControlError::invalid_request(
                "this window has no control identity to act with",
            ));
        };
        let outcome = access.invoke_local_action(
            self,
            capability_id,
            1,
            input,
            crate::control::ActionOrigin::Remote(Box::new(actor)),
        );
        self.control_access = Some(access);
        outcome
    }

    /// The `QUANTICK_CONTROL_EVIDENCE` hook: capture one evidence bundle
    /// through the very read a connected client calls.
    ///
    /// The value is a comma-separated list of tokens: `all` (or `1`) means
    /// every registered scope the configured grant already reaches,
    /// `screenshot` asks for the window to be rasterised as well, and
    /// anything else is a snapshot scope ID. The manifest is logged, so a
    /// scripted validation run reads what the bundle covered — and what it
    /// did not — without a client on the socket.
    ///
    /// A capture that asked for an image waits for it: the window is asked to
    /// rasterise and the hook takes the next frame, giving up after
    /// [`CONTROL_EVIDENCE_HOOK_FRAMES`] rather than hanging a capture run on a
    /// surface that never presents.
    fn apply_control_evidence_hook(&mut self, ctx: &egui::Context) {
        if self.pending_control_evidence.is_none() {
            return;
        }
        // Access is taken *before* the request is, so a frame that finds it
        // borrowed leaves the hook pending rather than dropping it silently.
        let Some(mut access) = self.control_access.take() else {
            return;
        };
        let request = self
            .pending_control_evidence
            .take()
            .expect("the hook was pending one line above");
        let mut wants_screenshot = false;
        // A set, not a list: `all,scene.controls` is a reasonable thing to
        // type, and the capability refuses a scope named twice, so the tokens
        // are folded rather than concatenated.
        let mut scopes = std::collections::BTreeSet::new();
        for token in request.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            match token {
                "screenshot" => wants_screenshot = true,
                "all" | "1" => scopes.extend(
                    access
                        .readable_scopes()
                        .into_iter()
                        .map(|scope| scope.to_string()),
                ),
                scope => {
                    scopes.insert(scope.to_owned());
                }
            }
        }
        if scopes.is_empty() {
            scopes.extend(
                access
                    .readable_scopes()
                    .into_iter()
                    .map(|scope| scope.to_string()),
            );
        }
        // The scope is checked before anything is armed, not after. Arming is
        // what eventually raises the screenshot notice, and telling the trader
        // their window was captured on the way to refusing the capture would
        // make the one indicator `visual-qa` asserts on say something untrue.
        if wants_screenshot && !access.grants_screenshot() {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_SCREENSHOT_NOT_GRANTED",
                "QUANTICK_CONTROL_EVIDENCE asked for an image without observe.screenshot; \
                 capturing without one"
            );
            wants_screenshot = false;
        }
        if wants_screenshot {
            // Harvests as well as arms: the frame service that normally takes
            // the pixels runs only while the gateway is enabled, and this hook
            // is meant to work without a client on the socket.
            access.service_screenshot(self, ctx);
        }
        if wants_screenshot && !access.has_screenshot() {
            self.control_evidence_hook_frames = self.control_evidence_hook_frames.saturating_add(1);
            if self.control_evidence_hook_frames <= CONTROL_EVIDENCE_HOOK_FRAMES {
                // Every waiting frame asks for the next one. Without this a
                // quiescent window — a paused replay, no feed, exactly the
                // headless validation run the hook exists for — would never
                // repaint, the counter would never advance, and the hook would
                // neither complete nor give up.
                ctx.request_repaint();
                self.pending_control_evidence = Some(request);
                self.control_access = Some(access);
                return;
            }
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_GAVE_UP_ON_IMAGE",
                frames = CONTROL_EVIDENCE_HOOK_FRAMES,
                "the window never delivered a frame to rasterise; capturing without one"
            );
            // The request stays true on purpose. Clearing it would send
            // `screenshot: false`, and the bundle would then record
            // `screenshot/not_requested` — a lie about a run that asked for a
            // picture and waited two seconds for one. Left true, the capture
            // finds no image and records `frame_not_delivered`, which is what
            // actually happened.
        }
        let outcome = access.invoke_local_read(
            self,
            "evidence.capture",
            serde_json::json!({ "scopes": scopes, "screenshot": wants_screenshot }),
        );
        self.control_access = Some(access);
        match outcome {
            Ok(manifest) => tracing::info!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_CAPTURED",
                evidence_id = %manifest["evidence_id"].as_str().unwrap_or_default(),
                content_digest = %manifest["content_digest"].as_str().unwrap_or_default(),
                encoded_bytes = %manifest["encoded_bytes"].as_str().unwrap_or_default(),
                chunk_count = manifest["chunk_count"].as_u64().unwrap_or_default(),
                captured_scopes = manifest["source_scopes"].as_array().map_or(0, Vec::len),
                omitted_scopes = manifest["coverage"]["omitted_scopes"].as_array().map_or(0, Vec::len),
                not_captured = manifest["coverage"]["not_captured"].as_array().map_or(0, Vec::len),
                unavailable_fields = manifest["coverage"]["unavailable_fields"].as_array().map_or(0, Vec::len),
                screenshot = !manifest["screenshot"].is_null(),
                "an evidence bundle was captured through the control plane"
            ),
            Err(error) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_REFUSED",
                error_code = %error.code,
                error = %error.message,
                "QUANTICK_CONTROL_EVIDENCE could not capture a bundle"
            ),
        }
    }

    /// A launch hook's action, with its failure reported where a scripted run
    /// will see it: the hook is fire-and-forget, so nothing else would.
    fn run_hook_action(&mut self, capability_id: &str, input: serde_json::Value) {
        if let Err(error) = self.run_agent_action(capability_id, input) {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_HOOK_ACTION_FAILED",
                capability = capability_id,
                error_code = %error.code,
                error = %error.message,
                "an annotate hook could not run its action"
            );
        }
    }

    /// The mark hotkey's body: `attention.mark.create` with the resolved
    /// cursor target, attributed to the human.
    pub(crate) fn take_mark(&mut self, note: Option<String>) {
        let mut input = serde_json::Map::new();
        if let Some(note) = note {
            input.insert("note".to_owned(), serde_json::Value::String(note));
        }
        // No target: the action port resolves the pointer at the moment of
        // the gesture and records the resolved input, so the trace line
        // determines the mark on its own and a rerun marks the same bar.
        match self.control_action(
            crate::control::MARK_CAPABILITY_ID,
            crate::control::MARK_CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::Value::Object(input),
        ) {
            Ok(result) => tracing::info!(
                target: "quantick::control",
                event_code = "CONTROL_MARK_TAKEN",
                sequence = %result["sequence"],
                "mark taken"
            ),
            Err(error) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_MARK_REFUSED",
                code = %error.code,
                "mark refused"
            ),
        }
    }

    pub(crate) fn control_frame_metrics(&self) -> ControlFrameMetrics {
        ControlFrameMetrics {
            wall_average_ms: self.frames.avg_ms(),
            wall_worst_ms: self.frames.worst_ms(),
            frames_per_second: self.frames.fps(),
            cpu_average_ms: self.cpu_frames.avg_ms(),
            cpu_worst_ms: self.cpu_frames.worst_ms(),
        }
    }

    /// The pane the chrome speaks for: the active tab's focused pane (§11).
    fn focused_pane(&self) -> &ChartPane {
        self.active_tab().focused_pane()
    }

    /// See [`Self::focused_pane`].
    fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().focused_pane_mut()
    }

    /// The pane every drawing surface speaks for: the one holding the
    /// selection, which is the focused pane unless a shared mark was taken
    /// from the chart it is mirrored on (see [`Tab::drawing_side`]).
    ///
    /// The inspector, the keyboard, the object manager and the toast all read
    /// through here, so an object selected on either of its two charts is
    /// edited and deleted from either of them.
    fn drawing_pane(&self) -> &ChartPane {
        self.active_tab().drawing_pane()
    }

    /// See [`Self::drawing_pane`].
    fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().drawing_pane_mut()
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
        // Cmd trading is app-wide (the trades-dir rule): a new tab starts
        // with the settings every other tab already carries.
        let cmd_trading = self.active_tab().paper.cmd_trading();
        let inherited_strategies = self.active_tab().paper.order_strategies().to_vec();
        let inherited_selection = self
            .active_tab()
            .paper
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        // Orientation travels with the working state the new tab inherits —
        // a market opened to compare against the active one is only
        // comparable the same way up. Per pane; a pane the source tab does
        // not have follows its flow chart.
        // The layers the active tab is *actually showing*, read before the new
        // tab is pushed. This used to be `self.layer_defaults` — the map read
        // off the file at startup — which was only harmless while that map was
        // whatever partial thing the trader's file happened to hold. Now that a
        // file's silence resolves to the shipped answer (`chart_layers::load`),
        // that map speaks for every layer, and applying it here would undo the
        // switches of the session mid-flight. Reading the live state is also
        // what the comment below has always promised.
        let inherited_risk = self.active_tab().paper.risk_settings().clone();
        let inherited_capital = self.active_tab().paper.capital().clone();
        let inherited_money = self.active_tab().paper.instrument_money().clone();
        let inherited_layers = self.active_tab().flow_pane.layer_states(&self.style);
        let flow_inverted = self.active_tab().flow_pane.price_view.is_inverted();
        let time_inverted = self
            .active_tab()
            .time_pane()
            .map_or(flow_inverted, |pane| pane.price_view.is_inverted());
        // The layout the trader is looking at is the one the new chart
        // opens on — read before the new tab takes the focus.
        let inherited_layout = (!self.tabs.is_empty()).then(|| self.focused_pane_layout());
        let flow_pane_id = self.pane_ids.alloc();
        let mut tab = Tab::new(id, flow_pane_id, feed_id, symbol, spec, feed, trades_dir);
        tab.paper.set_cmd_trading(cmd_trading);
        tab.paper
            .set_order_strategies(inherited_strategies, inherited_selection.as_deref());
        // The risk per trade travels with them. It is app-wide like the rest
        // of the ticket's settings, and a tab that opened without it would
        // hand the trader a bare quantity field on a market they meant to
        // size the same way as the one beside it.
        tab.paper.set_risk_settings(inherited_risk);
        tab.paper.set_capital(inherited_capital);
        tab.paper.set_instrument_money(inherited_money);
        tab.flow_pane.layout = inherited_layout;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        self.active_tab_mut().ensure_book_capture(&config);
        self.active_tab_mut().apply_feed_bubble_preset(&config);
        self.active_tab_mut().apply_feed_declared_layout(&config);
        // The new tab opens on the layers the user left showing, over the
        // preset it just put on: opening a second market is not a request to
        // bring back the chrome they switched off.
        self.active_tab_mut()
            .flow_pane
            .apply_layer_states(&inherited_layers);
        // The scripted footprint/zoom hooks reach tabs opened later too: the
        // replay tab a validation run autostarts is the tab the run means,
        // and it does not exist yet when the boot hooks fire.
        if self.scripted_footprint {
            self.active_tab_mut().flow_pane.footprint_visible = true;
        }
        if let Some(px) = self.scripted_candle_width {
            self.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
        }
        // After the declared layout ran: that is what decides whether the
        // new tab has a time pane to orient at all.
        let tab = self.active_tab_mut();
        tab.flow_pane.price_view.set_inverted(flow_inverted);
        for time_pane in tab.time_panes.iter_mut() {
            time_pane.price_view.set_inverted(time_inverted);
        }
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
        self.operator_slots.retain(|owner| owner.tab != closed.id);
        self.script_files
            .retain(|(owner, ..)| owner.tab != closed.id);
        self.pending_hidden.retain(|owner| owner.tab != closed.id);
        self.pending_styles
            .retain(|(owner, _)| owner.tab != closed.id);
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

    /// Whether the toolbar's heatmap lamp is lit.
    ///
    /// The *switch*, not what capture lets through it — the same reading the
    /// layer file was taught in 848cba0, and for a sibling reason. A lamp lit
    /// from `depth_visible()` (`enabled && show_depth`) reports the heatmap off
    /// for as long as book capture is starting, and forever on a source with no
    /// book: the trader sees an unlit button, presses it, and switches the
    /// layer they wanted *off*. The button already has an honest way to say a
    /// source cannot fill it — `.enabled(...)` carrying its
    /// `disabled_explanation` — so the lamp beside it answers the only other
    /// question there is.
    ///
    /// A named reading rather than an expression inside the toolbar's own
    /// frame, so the rule can be asserted without painting a toolbar. What
    /// reads it back without looking at the screen is the semantic scene,
    /// which takes the same `Tab::layer_toggle_state` this delegates to.
    #[must_use]
    #[cfg(test)]
    fn heatmap_lamp_on(&self) -> bool {
        // Through the group's one reading, so this named rule and the lamp the
        // toolbar actually paints cannot become two answers to one question.
        // `#[cfg(test)]` because the toolbar now takes the group's reading
        // directly: keeping a second production entry point to the same answer
        // is how the two drift.
        self.active_tab()
            .layer_toggle_state(
                ChartLayer::Heatmap,
                &self.style,
                self.active_tab().capabilities(&self.config),
            )
            .0
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
        let candles_held = self.active_tab().venue_candles_held();
        let older_candles = self.active_tab().older_candles(capabilities);
        let feed_display_name = self.active_tab().feed_display_name(&self.config).to_owned();
        // One reading per lamp, taken through the call the semantic scene
        // makes too, so the button and what an operator captures cannot
        // disagree about a layer. Every lamp reports the *switch* rather than
        // what the source lets through it — the rule `heatmap_lamp_on` names,
        // now the whole group's.
        let layers = toolbar::LayerToggle::ALL.map(|toggle| {
            let (on, blocked) =
                self.active_tab()
                    .layer_toggle_state(toggle.layer(), &self.style, capabilities);
            toolbar::LayerToggleState { on, blocked }
        });
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
        let show_style = self.surfaces.style_panel.is_open();
        // Read before the tab is borrowed mutably. The reach is the window's
        // standing choice, like the progressive-history switch — a trader who
        // picked "previous session" once means it in the next tab too — so it
        // is split off and written back the way the layout picker's flags are.
        let history_reach_running = self.active_tab().history_reach_running();
        let mut history_reach = self.history_reach;
        let mut history_reach_span_minutes = self.history_reach_span_minutes;
        let mut history_menu_rect = self.history_menu_rect;
        // The SOURCE group writes straight into the active tab: a feed or
        // symbol change is that tab's market switch. The BARS group writes
        // into the *focused pane* — the pane the status bar reads and every
        // indicator command lands on (§11) — so the three chrome surfaces
        // can never disagree about which chart a command describes, and in
        // the Time layout the group governs the chart actually on screen.
        // Split off the picker's flags before the tab borrow: the model wants
        // both, and they live on the same struct.
        let mut layout_picker_open = self.layout_picker_open;
        // One shot: the hook opens the popover on the first drawn frame and
        // then gets out of the way, so a trader's click can close it.
        let layout_picker_autostart = std::mem::take(&mut self.layout_picker_autostart);
        let tab = self.active_tab_mut();
        let focused = tab.focused_side();
        let pane = match focused {
            PaneSide::Time(slot) => tab.time_panes.get_mut(slot).unwrap_or(&mut tab.flow_pane),
            PaneSide::Flow => &mut tab.flow_pane,
        };
        let mut model = toolbar::ToolbarModel {
            layout_preset: Some(tab.layout.preset()),
            layout_picker_open: &mut layout_picker_open,
            layout_picker_request_open: layout_picker_autostart,
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
            imbalance_unit: &mut pane.imbalance_unit,
            history_step: &mut tab.history_step,
            history_menu_rect: &mut history_menu_rect,
            history_reach_span_minutes: &mut history_reach_span_minutes,
            history_reach: &mut history_reach,
            history_reach_running,
            history_trades: tab.history_trades,
            history_candles: candles_held,
            older_candles,
            capabilities,
            layers,
            dock_visible,
            appearance_open: show_style,
            paper: toolbar::PaperTradeModel {
                // The lock reaches the toolbar too. Gating only the dock's
                // pair left these lit while the ticket refused, so a fast
                // click here only toasted - and the doc promises the entry
                // pair disables.
                ready: tab.paper.ready() && !tab.paper.risk_report().1,
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
        // The popover's own state, back where it lives. Without this the flag
        // resets every frame and the button never reads as open.
        drop(model);
        self.layout_picker_open = layout_picker_open;
        self.set_history_reach(history_reach);
        // Through the setter, so a value dragged past the campaign's own span
        // cap is clamped in the one place that knows the cap.
        self.set_history_reach_span_minutes(history_reach_span_minutes);
        self.history_menu_rect = history_menu_rect;
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

    /// Switch the active tab to `preset`.
    ///
    /// **The one path.** The toolbar's picker, the `View → Layout` menu and
    /// the keyboard all arrive here, so none of them can grow its own idea of
    /// what applying a layout does. A control-plane capability joins them by
    /// calling this, never by repeating it.
    fn apply_layout_preset(&mut self, preset: &'static crate::canvas_layout::LayoutPreset) {
        let Some(layout) = CanvasLayout::from_preset(preset) else {
            // A preset the canvas cannot draw yet is refused rather than
            // approximated: switching to the nearest arrangement would be the
            // picker showing one layout and the canvas drawing another.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUT_PRESET_UNSUPPORTED",
                preset = %preset.id,
                action = "layout_left_as_is",
                "the layout registry names an arrangement the canvas cannot draw yet"
            );
            return;
        };
        self.active_tab_mut().set_layout(layout);
    }

    /// One toolbar side effect. Layer toggles reuse the same code paths the
    /// old checkboxes took, so provider gating and command acknowledgement
    /// rules are unchanged.
    fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadOlder => {
                let (tab, config) = self.active_with_config();
                tab.request_older_history(config);
            }
            ToolbarAction::LoadOlderCandles => {
                // Read before the tab is borrowed mutably — and the capability
                // block rather than the whole config, because that is all the
                // request needs to know.
                let capabilities = self.active_tab().capabilities(&self.config);
                self.active_tab_mut()
                    .request_older_ohlcv_history(capabilities);
            }
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
            // The focused pane's own field, through the same setter the pane's
            // layer menu calls — so the button, the menu and the lamp can
            // never disagree about which chart the command described.
            ToolbarAction::SetFootprint(shown) => {
                self.focused_pane_mut().footprint_visible = shown;
            }
            ToolbarAction::OpenFootprintSettings => self.surfaces.footprint_settings.open(),
            ToolbarAction::OpenDockTab(tab) => self.dock.open_tab(tab),
            ToolbarAction::SetLayout(preset) => self.apply_layout_preset(preset),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.surfaces.style_panel.toggle(),
            // Every indicator command lands on the focused pane (§11), which
            // is the flow pane whenever the canvas is not split.
            // Adding an indicator by hand is the plainest possible request to
            // see one, so it opens a folded legend rather than letting the new
            // row land inside the puck — the trader would get one more dot and
            // no way to tell the add from a no-op. Not the auto-collapse the
            // design ruled out: that rule protects against hiding what nobody
            // asked to hide, and unfolding hides nothing. It lives on this
            // path, the trader's own, and not in `ChartPane::add_indicator`,
            // which the workspace restore and the harness hooks also travel —
            // there it would erase the fold on every launch.
            ToolbarAction::AddEmaIndicator => {
                self.set_focused_legend_collapsed(false);
                self.add_native_indicator(SavedKind::NativeEma);
            }
            ToolbarAction::AddCvdIndicator => {
                self.set_focused_legend_collapsed(false);
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
                self.set_focused_legend_collapsed(false);
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
    /// Parse `QUANTICK_INDICATOR_SETTINGS`: `<index>` or `<index>:<tab>`,
    /// where the tab is `inputs`/`0` or `style`/`1`.
    ///
    /// Nonsense is no hook at all rather than a guessed one: a validation run
    /// that captured the wrong tab because a typo silently defaulted would be
    /// a screenshot claiming to show something it does not.
    fn parse_settings_hook(value: &str) -> Option<(usize, indicator_panel::SettingsTab)> {
        let (index, tab) = value.split_once(':').unwrap_or((value, "inputs"));
        let index = index.trim().parse::<usize>().ok()?;
        let tab = match tab.trim().to_ascii_lowercase().as_str() {
            "inputs" | "0" => indicator_panel::SettingsTab::Inputs,
            "style" | "1" => indicator_panel::SettingsTab::Style,
            _ => return None,
        };
        Some((index, tab))
    }

    /// Open the dialog for whichever indicator a gesture on a pane asked
    /// about: the pane header, a collapsed strip, or an overlay's own line.
    ///
    /// Drained here rather than acted on where it was read, because the dialog
    /// belongs to the window and the gestures are read deep inside a pane's
    /// input pass. Both sides of a split are asked, and each request resolves
    /// against the pane that raised it — the legend's rule (MAJOR-4), applied
    /// to the same problem one layer down.
    fn open_requested_indicator_settings(&mut self) {
        let tab_id = self.active_tab().id;
        // The harness hook waits for the view it names, exactly as the restored
        // hidden flags do: the indicator is born from the worker's first
        // Rebuilt, which is several frames after the window opens.
        if let Some((index, tab)) = self.settings_autostart {
            // The focused pane first, then the flow pane. A split tab can open
            // with the *time* pane focused while every indicator — the ones the
            // other autostart hook adds, and the ones the state file restores —
            // lives on the flow pane, and a hook that only asked the focused
            // side then waited for a view that was never coming. Silence is the
            // worst failure a validation hook can have: the run captures a
            // chart with no dialog on it and nothing says why.
            let focused = self.active_tab().focused_side();
            let found = [focused, PaneSide::Flow].into_iter().find_map(|side| {
                self.tabs
                    .iter()
                    .find(|candidate| candidate.id == tab_id)
                    .map(|candidate| candidate.pane(side))
                    .and_then(|pane| pane.indicators.all().get(index))
                    .map(|view| (side, view.slot))
            });
            if let Some((side, slot)) = found {
                self.settings_autostart = None;
                self.open_indicator_settings_at(TabSlot {
                    tab: tab_id,
                    side,
                    slot,
                });
                if let Some(dialog) = self.indicator_settings.as_mut() {
                    dialog.tab = tab;
                }
            }
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        let requests: SmallVec<[(PaneSide, SlotId); MAX_CANVAS_PANES]> = tab
            .panes_with_sides_mut()
            .filter_map(|(pane, side)| pane.take_settings_request().map(|slot| (side, slot)))
            .collect();
        for (side, slot) in requests {
            {
                self.open_indicator_settings_at(TabSlot {
                    tab: tab_id,
                    side,
                    slot,
                });
            }
        }
    }

    /// Fold or unfold one pane's indicator legend.
    ///
    /// The one place the state changes. The chevron, the menu entry, the
    /// hotkey and the harness hook all arrive here with the outcome they want,
    /// so none of them can leave a pane in a state the others would not have
    /// produced — and an operator that is not holding the mouse names this
    /// call rather than a click.
    fn set_legend_collapsed(&mut self, tab_id: u64, side: PaneSide, collapsed: bool) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        tab.pane_mut(side).legend_collapsed = collapsed;
    }

    /// Whether the focused pane's legend is folded — what the menu label and
    /// the hotkey read before deciding which way to flip it.
    fn focused_legend_collapsed(&self) -> bool {
        let tab = self.active_tab();
        tab.pane(tab.focused_side()).legend_collapsed
    }

    /// Fold or unfold the legend of the pane the chrome speaks for. The menu
    /// entry and the hotkey both act through this, so "which chart did that
    /// affect" has one answer: the focused one, the same pane every other
    /// chrome control acts on.
    fn set_focused_legend_collapsed(&mut self, collapsed: bool) {
        let tab_id = self.active_tab().id;
        let side = self.active_tab().focused_side();
        self.set_legend_collapsed(tab_id, side, collapsed);
    }

    fn toggle_indicator_hidden_at(&mut self, target: TabSlot) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) else {
            return;
        };
        tab.pane_mut(target.side)
            .indicators
            .toggle_hidden(target.slot);
        self.mirror_hidden(target);
        self.note_indicator_edit_at(target.tab, target.side);
    }

    /// Remove a slot, wherever it lives. UI first (the entry vanishes this
    /// frame), worker second; events already in flight for the slot are
    /// dropped on apply.
    fn remove_indicator_at(&mut self, target: TabSlot) {
        if !self.tabs.iter().any(|tab| tab.id == target.tab) {
            return;
        }
        // The mirrors first, while the slot's layout position can still be
        // read off the bookkeeping.
        self.mirror_remove(target);
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) else {
            return;
        };
        let pane = tab.pane_mut(target.side);
        pane.indicators.remove(target.slot);
        pane.indicator_worker
            .send(IndicatorCommand::Remove(target.slot));
        self.slot_kinds.retain(|(owner, _)| *owner != target);
        self.operator_slots.remove(&target);
        self.script_files.retain(|(owner, ..)| *owner != target);
        self.pending_hidden.retain(|owner| *owner != target);
        self.pending_styles.retain(|(owner, _)| *owner != target);
        self.note_indicator_edit_at(target.tab, target.side);
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
            tab: indicator_panel::SettingsTab::default(),
            draft: view.input_values.clone(),
            committed: view.input_values.clone(),
            previewed: false,
            settled: false,
            preset_label: None,
            preset_name_draft: String::new(),
        });
        self.indicator_settings_target = target;
    }

    /// Draw each visible pane's indicator legend and run what its rows asked
    /// for. Actions resolve against the pane the legend was drawn on — the
    /// legend must never act on the chart beside it (the audit's MAJOR-4
    /// trap, avoided by construction).
    fn draw_indicator_legends(&mut self, ctx: &egui::Context) {
        let tab_id = self.active_tab().id;
        // Whether a context chart is on screen — from what the layout holds
        // and whether the column is collapsed, never from one variant. Matched
        // against `TimeAndFlow` alone, this drew no legend at all on the
        // stacked charts of `time+time+flow`, and drew one over the flow chart
        // against a stale rect while the column was put away.
        let split = self.active_tab().shows_context_charts();
        let mut pending: Vec<(PaneSide, indicator_legend::LegendAction)> = Vec::new();
        let shown = self.active_tab().context_panes_shown();
        let sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> = self.active_tab().sides().collect();
        for side in sides {
            // Only a context chart on screen draws a legend: the stack may
            // hold a pane the layout no longer shows.
            if let PaneSide::Time(slot) = side
                && (!split || slot >= shown)
            {
                continue;
            }
            let pane = self.active_tab().pane(side);
            // The rect is last frame's, like every anchor the input path
            // reads; a pane not yet drawn has none and draws no legend.
            let Some(mut rect) = pane.last_chart_area else {
                continue;
            };
            // The position HUD owns the very corner of the pane it paints on;
            // this legend rides just below it there, and nowhere else. That
            // pane is the one holding the HUD anchor — the focused one, not
            // the flow one, so a split with the time pane focused drops these
            // chips on the *time* pane. The order-flow key stacks under this
            // legend and reads the same offset from the same place, so the two
            // cannot drift apart.
            rect.min.y += indicator_legend::hud_offset_px(
                pane.paper_hud_anchor().is_some()
                    && self.active_tab().paper.position_summary().is_some(),
            );
            // The slot being live-previewed by the settings dialog, if it
            // lives on this pane: its legend row wears a "preview" chip.
            let preview_slot = self
                .indicator_settings
                .as_ref()
                .filter(|dialog| dialog.previewed)
                .filter(|_| {
                    self.indicator_settings_target.tab == tab_id
                        && self.indicator_settings_target.side == side
                })
                .map(|dialog| dialog.slot);
            for action in indicator_legend::draw(
                ctx,
                pane.id,
                rect,
                pane.indicators.all(),
                preview_slot,
                pane.legend_collapsed,
            ) {
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
                indicator_legend::LegendAction::SetCollapsed(collapsed) => {
                    self.set_legend_collapsed(tab_id, side, collapsed);
                }
            }
        }
    }

    /// Draw the settings dialog and execute its outcome. Apply goes through
    /// the worker (construct anew, replace, replay) — the same path every
    /// input change takes, UI or not.
    fn draw_indicator_settings(&mut self, ctx: &egui::Context) {
        // The boot hook's deferred half: fire once a slot can actually show
        // a dialog (see `scripted_indicator_settings`).
        if self.scripted_indicator_settings && self.indicator_settings.is_none() {
            let tab_id = self.active_tab().id;
            if let Some(slot) = self
                .active_tab()
                .flow_pane
                .indicators
                .all()
                .iter()
                .find(|view| !view.input_values.is_empty())
                .map(|view| view.slot)
            {
                self.open_indicator_settings_at(TabSlot {
                    tab: tab_id,
                    side: PaneSide::Flow,
                    slot,
                });
                self.scripted_indicator_settings = false;
            }
        }
        let target = self.indicator_settings_target;
        // The preset shelf for this slot's kind. A slot with no registered
        // kind (the natives autostart hook deliberately registers none) has
        // nowhere to save to, and the dialog hides the picker.
        let preset_names: Option<Vec<String>> = self
            .slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| {
                self.indicator_presets
                    .names_for(kind)
                    .map(str::to_owned)
                    .collect()
            });
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
                .iter_mut()
                .find(|tab| tab.id == target.tab)
                .map(|tab| tab.pane_mut(target.side))
                .and_then(|pane| pane.indicators.view_mut(dialog.slot))
            else {
                // The indicator was removed under the dialog.
                *indicator_settings = None;
                return;
            };
            // Follow the live label: an applied edit can retitle the
            // indicator (`EMA(9)` → `EMA(21)`), and a dialog that stays open
            // must not keep announcing the old one.
            dialog.title = view.label().to_owned();
            // The descriptor is read while the style layer beside it is
            // written, so the two halves are split off the same view here
            // rather than borrowed twice.
            let IndicatorView {
                descriptor, style, ..
            } = view;
            indicator_panel::draw(
                ctx,
                dialog,
                &descriptor.inputs,
                &descriptor.plots,
                style,
                preset_names.as_deref(),
            )
        };
        match outcome {
            SettingsOutcome::Open => {}
            SettingsOutcome::Close => {
                self.revert_indicator_settings_preview();
                self.indicator_settings = None;
            }
            SettingsOutcome::Apply => self.apply_indicator_settings_draft(),
            SettingsOutcome::Preview => self.preview_indicator_settings_draft(),
            SettingsOutcome::LoadPreset(name) => self.load_indicator_preset(name),
            SettingsOutcome::SavePreset(name) => self.save_indicator_preset(&name),
            SettingsOutcome::DeletePreset(name) => self.delete_indicator_preset(&name),
            // Style is render-side: the chart already shows it and there is
            // nothing to preview or commit, so all that is owed is the file.
            // Deliberately unlike an input edit — it takes no rebuild and no
            // replay, and it follows the legend's eye, which is also instant
            // and also persisted without an Apply.
            SettingsOutcome::StyleChanged => {
                let target = self.indicator_settings_target;
                self.mirror_style(target);
                self.note_indicator_edit_at(target.tab, target.side);
            }
        }
    }

    /// Replace the dialog's draft with a preset (`None` = the declared
    /// defaults) and preview it — the same nudge-and-look contract as a
    /// slider: Apply commits, Discard reverts. A saved value whose type no
    /// longer matches its input (the script evolved under the preset) falls
    /// back to that input's default rather than being silently coerced.
    fn load_indicator_preset(&mut self, name: Option<String>) {
        let target = self.indicator_settings_target;
        let Some(specs) = self
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
            .map(|view| view.descriptor.inputs.clone())
        else {
            return;
        };
        let saved: Option<Vec<Option<quantick_indicators::InputValue>>> = match &name {
            None => Some(Vec::new()),
            Some(name) => {
                let Some(kind) = self
                    .slot_kinds
                    .iter()
                    .find(|(owner, _)| *owner == target)
                    .map(|(_, kind)| kind)
                else {
                    return;
                };
                self.indicator_presets
                    .get(kind, name)
                    // `map`, not `filter_map`: a stored cell that no longer
                    // reads (a source whose name the dialect dropped, say)
                    // still has to hold its index. Dropping it from the list
                    // would slide every value after it one input to the left,
                    // and same-typed neighbours would take each other's
                    // settings without a word.
                    .map(|inputs| inputs.iter().map(SavedInput::to_value).collect())
            }
        };
        let Some(saved) = saved else {
            return;
        };
        // The same binder the worker binds a saved state file with: one rule
        // for what a saved value means, in one place.
        let bound = quantick_indicators::bind_by_position(&specs, &saved);
        if bound.kept < saved.len() {
            // The preset on screen is not the preset that was saved. Silence
            // here would show a chart that does not match the name above it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_PRESET_REBOUND",
                preset = %name.as_deref().unwrap_or(indicator_panel::DEFAULT_PRESET),
                saved = saved.len(),
                declared = specs.len(),
                kept = bound.kept,
                action = "bound_by_position",
                "some of this preset's values no longer bind; those inputs took their defaults"
            );
        }
        let draft = bound.values;
        let label = name.unwrap_or_else(|| indicator_panel::DEFAULT_PRESET.to_owned());
        if let Some(dialog) = self.indicator_settings.as_mut() {
            dialog.draft = draft;
            dialog.preset_label = Some(label);
        }
        self.preview_indicator_settings_draft();
    }

    /// Save the dialog's current draft under `name` for this slot's kind.
    /// Saving is a file write, never a chart change — the draft on screen
    /// stays exactly as it was.
    fn save_indicator_preset(&mut self, name: &str) {
        let target = self.indicator_settings_target;
        let Some(kind) = self
            .slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| kind.clone())
        else {
            return;
        };
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        let inputs: Vec<SavedInput> = dialog.draft.iter().map(SavedInput::from_value).collect();
        if self.indicator_presets.insert(&kind, name, inputs) {
            self.indicator_presets.save(&self.indicator_presets_path);
            dialog.preset_label = Some(name.trim().to_owned());
            dialog.preset_name_draft.clear();
        }
    }

    /// Forget a preset of this slot's kind. The values on screen stay —
    /// deleting a name never touches the chart.
    fn delete_indicator_preset(&mut self, name: &str) {
        let target = self.indicator_settings_target;
        let Some(kind) = self
            .slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| kind.clone())
        else {
            return;
        };
        if self.indicator_presets.remove(&kind, name) {
            self.indicator_presets.save(&self.indicator_presets_path);
            if let Some(dialog) = self.indicator_settings.as_mut()
                && dialog.preset_label.as_deref() == Some(name)
            {
                dialog.preset_label = None;
            }
        }
    }

    /// Send the open dialog's draft to the worker and keep the dialog open
    /// (audit M2): tuning is a nudge-and-look loop, and a dialog that dies
    /// on every Apply makes each attempt four clicks. The slot is the one
    /// the dialog was opened on, not whatever has focus now — clicking
    /// Apply must not retarget the edit.
    fn apply_indicator_settings_draft(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        dialog.committed = dialog.draft.clone();
        dialog.previewed = false;
        let (slot, values) = (dialog.slot, dialog.draft.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs {
                    slot,
                    values: values.clone(),
                });
        }
        self.mirror_inputs(target, &values);
        self.note_indicator_edit_at(target.tab, target.side);
    }

    /// Show the draft on the chart without committing it: same worker path
    /// as Apply, but the state file is never marked dirty — what survives a
    /// restart is always the last Apply, never a slider mid-drag. The worker
    /// coalesces a burst of these to its own cadence.
    fn preview_indicator_settings_draft(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        dialog.previewed = true;
        let (slot, values) = (dialog.slot, dialog.draft.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs { slot, values });
        }
    }

    /// Put the last committed values back on the chart. Close's half of the
    /// preview contract: a dialog dismissed mid-tuning leaves no trace.
    fn revert_indicator_settings_preview(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_ref() else {
            return;
        };
        if !dialog.previewed {
            return;
        }
        let (slot, values) = (dialog.slot, dialog.committed.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs { slot, values });
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
                let (_, _, slot) = self.attach_script_indicator(name, text, false);
                let owner = self.target_slot(slot);
                // Watch the file so a save reloads it. Registered here, with
                // the add, so the two cannot drift apart.
                if let Some((_, mtime)) = self.script_library.file_info(index) {
                    self.script_files.push((owner, index, mtime));
                }
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
                    bar_paint: Vec::new(),
                    rows: 0,
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
        self.slot_kinds.push((owner, kind.clone()));
        // Every other pane of every tab gets the same indicator now; the
        // settled reconciliation binds the layout's copy of it.
        self.mirror_add(owner, &kind);
        self.note_indicator_edit_at(owner.tab, owner.side);
        slot
    }

    /// An indicator edit happened on the focused pane. Edits that know their
    /// pane call [`Self::note_indicator_edit_at`] directly.
    fn mark_indicator_state_dirty(&mut self) {
        let (tab, side) = {
            let tab = self.active_tab();
            (tab.id, tab.focused_side())
        };
        self.note_indicator_edit_at(tab, side);
    }

    /// Undo the dirty mark an add just set — for indicators an env var asked
    /// for rather than the user. The slot still exists and still works; it
    /// simply does not enter the persisted set.
    fn forget_last_indicator_state_change(&mut self) {
        self.slot_kinds.pop();
    }

    /// Apply layout-placed hide flags and styles once their views exist.
    ///
    /// The layout itself is written by the edit that changed it — see
    /// `layout_wiring` — so nothing here reads a view back.
    fn maintain_indicator_state(&mut self) {
        self.apply_pending_indicator_state();
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
        if actions.footprint_changed {
            crate::footprint_config::save(&self.footprint_settings_path, &self.footprint_config);
        }
        if actions.open_footprint_settings {
            self.surfaces.footprint_settings.open();
        }
    }

    /// Settle every tab's paper panel and hand its acknowledgement to the
    /// window's one toast.
    ///
    /// # One lane, and what that cost
    ///
    /// The panel used to draw its own toast: the same `CENTER_BOTTOM` anchor
    /// as `ToastSurface`, 96px up instead of 44, on a 4-second clock instead
    /// of 8. Two acknowledgements could therefore sit in one lane, at two
    /// heights, disagreeing about how long an acknowledgement lasts. There is
    /// one now, and this is where the panel's messages join it.
    ///
    /// # Every tab, not just the one on screen
    ///
    /// `settle` runs for all of them because the jobs it finishes — an
    /// export, an import — belong to the tab that started them, and a trader
    /// who starts an export and then looks at another chart should not have
    /// to come back for it to land. The acknowledgements follow: a stop
    /// filling on a chart the trader is not looking at is precisely the news
    /// they most need, and dropping it silently is what the old per-tab toast
    /// did.
    ///
    /// A message from a background tab is **named**, because an unlabelled
    /// "SIM: dropped at the fill" would read as being about the chart on
    /// screen.
    ///
    /// # Which message wins a slot that holds one
    ///
    /// The watched tab's, always: it carries no prefix and is posted last, so
    /// it takes the slot from any background message raised on the same
    /// frame. Among background tabs the **first** in tab order wins and the
    /// rest of that frame are dropped — the same first-wins rule
    /// `SurfaceResponse::merge` uses for a request that carries a value, so
    /// the window has one tie-break rule rather than two. Posting each of
    /// them in turn would look like it showed them all and would in fact
    /// show whichever `tabs.iter()` reached last, which is tab order deciding
    /// in silence.
    fn settle_paper_panels(&mut self, now: Instant) {
        let Self {
            tabs,
            active_tab,
            surfaces,
            ..
        } = self;
        let mut watched = None;
        let mut background = None;
        for (index, tab) in tabs.iter_mut().enumerate() {
            tab.paper.settle();
            let Some(message) = tab.paper.take_toast() else {
                continue;
            };
            if index == *active_tab {
                watched = Some(message);
            } else if background.is_none() {
                // The interpunct is the window's own separator — the status
                // bar, the tape's axis caption and the layout strip all use
                // it, and the messages themselves already carry a colon
                // (`SIM: …`). A second one would read as two labels.
                background = Some(format!("{} · {message}", tab.symbol));
            }
        }
        if let Some(message) = background {
            surfaces.toast.note(message, now);
        }
        if let Some(message) = watched {
            surfaces.toast.note(message, now);
        }
    }

    /// Apply what the footprint settings window settled on.
    ///
    /// Whatever is edited also becomes the window default, which is what a
    /// trader configuring their first chart means; a second chart diverges
    /// only when they configure it too.
    fn apply_footprint_change(&mut self, change: crate::surfaces::FootprintChange) {
        let side = self.active_tab().focused_side();
        match change {
            crate::surfaces::FootprintChange::Applied(edited) => {
                self.active_tab_mut().pane_mut(side).footprint_override = Some((*edited).clone());
                self.footprint_config = *edited;
                crate::footprint_config::save(
                    &self.footprint_settings_path,
                    &self.footprint_config,
                );
            }
            crate::surfaces::FootprintChange::ResetToDefault => {
                self.active_tab_mut().pane_mut(side).footprint_override = None;
            }
        }
    }

    /// The visibility this app persists, as one bit per layer.
    ///
    /// Read off the active tab's flow pane: the file records the canvas
    /// quantick is built around, the same scope the indicator state file has
    /// (see [`Self::maintain_indicator_state`]). A tab's second pane opens
    /// matching it and is in-session from there.
    fn layer_mask(&self) -> u32 {
        self.active_tab().flow_pane.layer_mask(&self.style)
    }

    /// Save the layer visibility when it differs from what is on disk.
    ///
    /// Called once per frame instead of from each switch: the layers are owned
    /// by four different pieces of chrome, and a save hook on each is four
    /// chances to forget one.
    fn maintain_chart_layers(&mut self) {
        let mask = self.layer_mask();
        // Layer state is per-pane, and this reads the *active* tab's flow pane
        // — so activating a tab whose chart is set up differently changes the
        // mask with nobody having touched a switch. Left alone, Ctrl+Tab
        // records the other tab's opinion as the trader's choice, thrashes the
        // file between two of them, and fills the log below with switches no
        // hand moved. A different chart answering is a re-baseline, not an
        // edit; the file keeps whatever the last real switch put there.
        let tab = self.active_tab().id;
        if tab != self.saved_layer_tab {
            self.saved_layer_tab = tab;
            self.saved_layer_mask = mask;
            return;
        }
        if mask == self.saved_layer_mask {
            return;
        }
        // Name every switch that moved, before writing it down.
        //
        // This file is the trader's own answer, and it outranks the shipped
        // default from the next launch on — so a layer that goes off without a
        // click is not a display glitch, it is a choice attributed to someone
        // who never made it, and it lasts. The bug that motivated this line was
        // exactly that shape and cost a day to chase, because the only record
        // was the file itself: a trio of `false`s with no timestamp, no
        // sequence and nothing to say whether a hand had been near them.
        //
        // Off the frame path in every sense that matters: the mask compare
        // above already gates it, so this runs on the frames a switch actually
        // moves — a handful in a session — and not one of the other 60 a
        // second.
        let flipped = mask ^ self.saved_layer_mask;
        for (bit, layer) in ChartLayer::ALL.into_iter().enumerate() {
            if flipped & (1 << bit) == 0 {
                continue;
            }
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYER_SWITCHED",
                layer = layer.id(),
                on = mask & (1 << bit) != 0,
                action = "persist_switch",
                "a chart layer switch moved; recording it as the trader's choice"
            );
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
        let defaults = chart_layers::load(&self.chart_layers_path);
        // Whatever the file said (including nothing at all) is now on screen;
        // only a change from here is worth another write.
        if defaults.is_empty() {
            // Only reachable when the *shipped* config failed to parse, since
            // every other path in `load` falls back to it — a build-time
            // mistake, and the one launch where saying nothing would be worst:
            // the chart opens on whatever the code decided and no one is told
            // the product's own answer never arrived.
            tracing::error!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_UNAVAILABLE",
                path = %self.chart_layers_path.display(),
                action = "keep_code_defaults",
                "no layer visibility to apply; the shipped config did not parse"
            );
            self.saved_layer_mask = self.layer_mask();
            self.saved_layer_tab = self.active_tab().id;
            return;
        }
        if let Some(grid) = defaults.get(&ChartLayer::Grid) {
            self.style.canvas.grid_enabled = *grid;
        }
        self.apply_layer_defaults(&defaults);
        self.saved_layer_mask = self.layer_mask();
        self.saved_layer_tab = self.active_tab().id;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CHART_LAYERS_RESTORED",
            path = %self.chart_layers_path.display(),
            // `off`, not `hidden`: the map now always speaks for every layer,
            // the shipped-off `backfill_divider` included, so a count over it
            // is no longer "how many the trader switched off". Renamed rather
            // than quietly redefined — a field that keeps its name and changes
            // its meaning misleads every dashboard already reading it.
            off = defaults.values().filter(|visible| !**visible).count(),
            layers = defaults.len(),
            "chart layer visibility restored"
        );
    }

    /// Put every open pane on the saved visibility, once, at startup.
    ///
    /// A tab opened later does *not* come through here: it inherits the layers
    /// the active tab is showing at that moment (`inherited_layers` in
    /// [`Self::adopt_tab`]), which is the live state rather than the map read
    /// off disk during boot. The two policies differ on purpose — see the note
    /// there — so a reader arriving here for new-tab behaviour is in the wrong
    /// function.
    fn apply_layer_defaults(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        for tab in &mut self.tabs {
            // Every pane, by address: a default applied to "the flow pane and
            // the time pane" left the second stacked chart on whatever the
            // previous defaults were, so one canvas drew the same layer two
            // ways.
            for pane in tab.panes_mut() {
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
        // And the recent workspace files, for the same reason: a save that
        // dropped them would empty the Open-recent menu every time the
        // trader saved their layout.
        .with_recent(self.recent_workspaces.clone())
        // And so does the replay folder, for exactly the same reason: a save
        // that dropped it would send the browser back to nowhere on the next
        // launch, which is the failure this field was added to end. The
        // trader's *pick*, never the folder in use — a run under
        // `QUANTICK_REPLAY_DIR` must not write a QA scratch path into their
        // workspace, and accepting the default home is not a choice either.
        .with_replay_folder(self.replay_view.stored_pick().map(str::to_owned))
        .with_replay_day_before(self.replay_view.stored_day_before())
        // And the starred tools, for the third time and the same reason. They
        // are already on disk the moment the star is clicked; riding along
        // here keeps a full-file write from erasing what the star wrote.
        .with_favorites(self.starred_tool_ids())
    }

    /// The rail's pinned section as tool ids, in star order — the form the
    /// workspace file keeps it in.
    fn starred_tool_ids(&self) -> Vec<String> {
        self.toolrail
            .favorites()
            .iter()
            .map(|tool| tool.id().to_owned())
            .collect()
    }

    /// Put the chrome back — the mirror of the [`Self::capture_arrangement`]
    /// half that produced it.
    ///
    /// One function because there are two callers and no way for the compiler
    /// to notice when only one of them learns a new field: the startup
    /// workspace and a named bookmark describe the same thing, and
    /// [`ui_state::NamedArrangement`] says so in as many words. Restoring them
    /// through two copies of the same eight lines is how a field comes to
    /// persist but never come back from a bookmark — a bug with no compile
    /// error behind it.
    ///
    /// The starred tools are the one thing it does not speak for: they live at
    /// the file's own level rather than inside an arrangement
    /// ([`ui_state::Workspace::favorite_tools`]), precisely so that opening a
    /// bookmark cannot rearrange the rail the trader curated.
    fn restore_chrome(&mut self, chrome: &ui_state::SavedChrome) {
        self.tz = TzOffset::new(chrome.timezone_minutes);
        self.dock
            .restore(chrome.dock_visible, chrome.dock_tab.map(Into::into));
        self.toolrail.set_dock(chrome.rail_dock.into());
        self.toolrail.set_visible(chrome.rail_visible);
        self.show_perf = chrome.perf_readings;
        self.progressive_history = chrome.progressive_history;
        // A token this release does not know keeps the reach it had — the
        // default on startup, whatever the trader picked when a bookmark is
        // opened mid-session. Never a silent fallback to something else: the
        // reach decides how much a press fetches.
        if let Some(reach) = chrome
            .history_reach
            .as_deref()
            .and_then(crate::history_reach::HistoryReach::from_token)
        {
            self.history_reach = reach;
        }
        // Through the setter, so a hand-edited workspace cannot restore a span
        // the campaign could never reach.
        if let Some(minutes) = chrome.history_reach_span_minutes {
            self.set_history_reach_span_minutes(minutes);
        }
        self.venue_lead_in = chrome.venue_lead_in;
        self.surfaces
            .drawing_chrome
            .restore_inspector_position(chrome.inspector_position);
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
                context_collapsed: tab.context_collapsed,
                focus: Some(ui_state::SavedFocus::from_side(tab.focused_side()).0),
                focus_slot: ui_state::SavedFocus::from_side(tab.focused_side()).1,
                flow_bars: tab.flow_pane.state.spec().to_config_string(),
                // Only a pane that exists has an interval worth recording; a
                // tab that never showed the split restores on the default,
                // which is what it had.
                time_bars: tab
                    .time_pane()
                    .map(|pane| pane.state.spec().to_config_string()),
                context_bars: tab
                    .time_panes
                    .iter()
                    .map(|pane| pane.state.spec().to_config_string())
                    .collect(),
                flow_layout: tab.flow_pane.layout.map(|layout| layout.0),
                context_layouts: tab
                    .time_panes
                    .iter()
                    .map(|pane| {
                        pane.layout
                            .map_or(crate::ui_state::LAYOUT_UNRECORDED, |layout| layout.0)
                    })
                    .collect(),
                flow_legend_collapsed: tab.flow_pane.legend_collapsed,
                // A tab with no time pane has no second legend, and `false`
                // is what it will restore into when one is opened: a pane
                // that never existed cannot have been folded.
                time_legend_collapsed: tab.time_pane().is_some_and(|pane| pane.legend_collapsed),
            })
            .collect();
        let chrome = ui_state::SavedChrome {
            timezone_minutes: self.tz.minutes(),
            dock_visible: self.dock.visible(),
            dock_tab: self.dock.tab().map(Into::into),
            rail_visible: self.toolrail.visible(),
            rail_dock: self.toolrail.dock().into(),
            perf_readings: self.show_perf,
            // Never written any more: the stars are a standing choice and live
            // at the top of the file. An arrangement that carried a copy would
            // be an arrangement that could overwrite them on open.
            legacy_favorite_tools: Vec::new(),
            progressive_history: self.progressive_history,
            // The default writes no key: a workspace that says nothing about
            // the reach restores the press the button has always had, which is
            // exactly what the default is.
            // Written whenever it differs from what the config seeds, so a
            // workspace only carries an opinion its owner actually formed.
            history_reach_span_minutes: (self.history_reach_span_minutes
                != self.config.history.reach_span_minutes)
                .then_some(self.history_reach_span_minutes),
            history_reach: (self.history_reach != crate::history_reach::HistoryReach::default())
                .then(|| self.history_reach.token().to_owned()),
            venue_lead_in: self.venue_lead_in,
            inspector_position: self.surfaces.drawing_chrome.remembered_inspector_position(),
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
        self.recent_workspaces = workspace.recent_workspaces.clone();
        self.refresh_recent_workspaces();
        // One stat at boot, so the Reset entry can gate on a field instead of
        // the filesystem for the rest of the session. A file with no tabs
        // still counts: it carries the autosave setting, and Reset is how the
        // trader gets rid of it.
        self.workspace_saved = self.ui_state_path.exists();
        // Outside the chrome block deliberately: the stars belong to the file,
        // not to the arrangement, so a workspace with nothing else in it still
        // hands the rail back its pinned section.
        //
        // An empty list is silence, not an instruction. The format cannot tell
        // "the trader starred nothing" from "this file predates the field" or
        // "this bundle was written by an install that never saved a cockpit",
        // and this same function restores an *imported* workspace mid-session —
        // where emptying the rail on silence would throw away a curated rail on
        // the strength of a key that was never written. Unstarring the last
        // tool is not lost by this: that click writes the empty list itself,
        // and the rail it would be restored onto is already empty.
        if !workspace.favorite_tools.is_empty() {
            let unknown = self.toolrail.set_favorites(&workspace.favorite_tools);
            if !unknown.is_empty() {
                // Said out loud because the next star click writes the pruned
                // list back over the file: this is the only moment the id
                // still exists anywhere.
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "TOOL_FAVORITE_DROPPED",
                    tools = %unknown.join(","),
                    action = "no_such_drawing_tool",
                    "a starred tool this build does not offer was dropped from the rail"
                );
            }
        }
        if let Some(chrome) = &workspace.chrome {
            self.restore_chrome(chrome);
        }
        if workspace.is_empty() {
            return;
        }
        // Whether tab zero is already the workspace's first market.
        //
        // At startup it is: `main` read that market out of this very file and
        // spawned the window on it, so the tab exists and only its bar rule
        // can differ. Mid-session — a workspace file being opened — it is
        // whatever the trader was looking at, and adopting it would leave the
        // strip holding one market with another's name. Then every tab is
        // opened outright and the ones that were there are closed after, so
        // the strip is *replaced* rather than grown.
        let adopt_first = self.tabs.first().is_some_and(|tab| tab.id == FIRST_TAB_ID)
            && self.tabs.len() == 1
            && self.tabs[0].feed_id == workspace.tabs[0].feed
            && self.tabs[0].symbol == workspace.tabs[0].symbol;
        let stale: Vec<u64> = if adopt_first {
            Vec::new()
        } else {
            self.tabs.iter().map(|tab| tab.id).collect()
        };
        for (index, saved) in workspace.tabs.iter().enumerate() {
            // `restore` has already dropped anything unparseable, so a spec
            // reaching here is one a control could have produced.
            let flow = BarSpec::parse(&saved.flow_bars).ok();
            if index == 0 && adopt_first {
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
            let context_intervals =
                saved_context_intervals(&saved.context_bars, saved.time_bars.as_deref());
            let focus = saved.focus.map(|focus| focus.to_side(saved.focus_slot));
            // `open_tab` activates what it opened, so the tab just arranged is
            // always the last one — index zero on the first pass.
            let target = if index == 0 && adopt_first {
                0
            } else {
                self.tabs.len() - 1
            };
            self.tabs[target].restore_canvas(
                CanvasLayout::from(saved.layout),
                saved.split_fraction,
                saved.context_collapsed,
                focus,
                &context_intervals,
                LegendFold {
                    flow: saved.flow_legend_collapsed,
                    time: saved.time_legend_collapsed,
                },
            );
            self.tabs[target].set_opening_layouts(saved.flow_layout, &saved.context_layouts);
        }
        // The markets that were on screen before this workspace was opened.
        // Closed last, so the strip is never empty in between and `close_tab`
        // — which refuses to close the only tab — always has the restored
        // ones to keep. Each closes through its own path, so a simulated
        // position ends in a journaled flatten rather than vanishing.
        for id in stale {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                self.close_tab(index);
            }
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

    /// Ask the operating system where to put a workspace file, off the UI
    /// thread. One dialog at a time, the trades-folder picker's own pattern.
    ///
    /// The cockpit is written to its stores *first*, so what the file gets is
    /// the screen as it stands rather than whatever was last flushed —
    /// indicator state is written debounced, and an export that raced it
    /// would quietly save a cockpit the trader never had.
    fn open_workspace_export_picker(&mut self) {
        if self.workspace_picker.is_some() {
            return;
        }
        let start = crate::workspace_bundle::default_dir();
        let suggested = crate::workspace_bundle::file_name_for(&self.suggested_workspace_name());
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("quantick-workspace-export-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Export workspace")
                    .add_filter("quantick workspace", &["toml"])
                    .set_file_name(suggested);
                if let Some(start) = start {
                    let _ = std::fs::create_dir_all(&start);
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.save_file());
            })
            .expect("spawn workspace export picker thread");
        self.workspace_picker = Some((WorkspacePick::Export, receiver));
    }

    /// The same, for choosing a workspace file to open.
    fn open_workspace_import_picker(&mut self) {
        if self.workspace_picker.is_some() {
            return;
        }
        let start = crate::workspace_bundle::default_dir();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("quantick-workspace-import-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open workspace")
                    .add_filter("quantick workspace", &["toml"]);
                if let Some(start) = start
                    && start.is_dir()
                {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_file());
            })
            .expect("spawn workspace import picker thread");
        self.workspace_picker = Some((WorkspacePick::Import, receiver));
    }

    /// Land whatever the dialog answered.
    fn poll_workspace_picker(&mut self) {
        let Some((intent, receiver)) = &self.workspace_picker else {
            return;
        };
        let choice = match receiver.try_recv() {
            Ok(choice) => choice,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            // The dialog thread died without answering — no display server,
            // a COM failure. Treated as "no answer yet" this would leave the
            // field set forever and both menu entries silently dead, since
            // each refuses to open a second dialog.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.workspace_picker = None;
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_PICKER_LOST",
                    action = "picker_reset",
                    "the file dialog closed without an answer"
                );
                self.note_workspace(
                    "The file chooser could not open — see the log. Try again.".to_owned(),
                );
                return;
            }
        };
        let intent = *intent;
        self.workspace_picker = None;
        // A cancelled dialog is an answer, not a failure: say nothing.
        let Some(path) = choice else { return };
        match intent {
            WorkspacePick::Export => self.export_workspace_to(&path),
            WorkspacePick::Import => self.import_workspace_from(&path),
        }
    }

    /// A starting name for an export: what the cockpit is showing, so the
    /// trader does not have to invent one to get a usable file name.
    fn suggested_workspace_name(&self) -> String {
        let tab = self.active_tab();
        format!(
            "{} {}",
            tab.symbol,
            tab.flow_pane.state.spec().to_config_string()
        )
    }

    /// Write every store that is still only in memory, so a bundle captured
    /// next describes the screen rather than the last flush.
    fn flush_cockpit_stores(&mut self) {
        // The layouts file is written debounced, off the frame path. An
        // export is the one moment worth paying it immediately, or the
        // bundle would carry the layouts as they stood a second ago.
        self.flush_layouts();
        self.maintain_chart_layers();
    }

    /// Export the whole cockpit to one file, and say what happened.
    fn export_workspace_to(&mut self, path: &std::path::Path) {
        // Here rather than before the dialog: these two writes are how the
        // bundle comes to describe the screen instead of the last flush, and
        // doing them up front meant a trader who pressed Cancel had still
        // silently redefined what the app opens on. It also keeps the harness
        // hook on exactly the menu's path.
        self.save_workspace("export");
        self.flush_cockpit_stores();
        let name = crate::workspace_bundle::recent_label(path);
        let outcome = crate::workspace_bundle::capture(
            &name,
            crate::store_home::COCKPIT_STORES,
            &crate::workspace_bundle::live_paths,
        )
        .and_then(|bundle| crate::workspace_bundle::write(path, &bundle).map(|()| bundle.len()));
        match outcome {
            Ok(stores) => {
                crate::workspace_bundle::remember_recent(&mut self.recent_workspaces, path);
                self.refresh_recent_workspaces();
                self.save_workspace("export_recent");
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_EXPORTED",
                    path = %path.display(),
                    stores,
                    action = "workspace_written",
                    "workspace exported"
                );
                self.note_workspace(format!(
                    "Workspace exported to {} — {stores} settings groups",
                    path.display()
                ));
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_EXPORT_FAILED",
                    path = %path.display(),
                    %error,
                    action = "workspace_not_written",
                    "could not export the workspace"
                );
                self.note_workspace(format!("Workspace not exported — {error}"));
            }
        }
    }

    /// Open a workspace file over the live cockpit.
    ///
    /// Refused whole or applied whole: [`crate::workspace_bundle::apply`] checks
    /// every section before writing any, so a bad file leaves the screen
    /// exactly as it was and says why. What reaches the disk is then read
    /// back into the running app, because a cockpit that only changed on disk
    /// would be overwritten by this session's own save on exit.
    fn import_workspace_from(&mut self, path: &std::path::Path) {
        // What the debounce still holds is written first, or a bundle with no
        // layouts section would reload a file a second behind the screen.
        self.flush_layouts();
        let outcome = crate::workspace_bundle::read(path).and_then(|bundle| {
            crate::workspace_bundle::apply(
                &bundle,
                crate::store_home::COCKPIT_STORES,
                &crate::workspace_bundle::live_paths,
            )
        });
        match outcome {
            Ok(written) => {
                let stores = written.len();
                self.reload_cockpit_stores(&written);
                crate::workspace_bundle::remember_recent(&mut self.recent_workspaces, path);
                self.refresh_recent_workspaces();
                // The recent list lives in the workspace file the import just
                // replaced, so it has to be written back after the reload —
                // otherwise opening a file would forget that it was opened.
                self.save_workspace("import_recent");
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_IMPORTED",
                    path = %path.display(),
                    stores,
                    action = "cockpit_replaced",
                    "workspace imported"
                );
                self.note_workspace(format!(
                    "Workspace \"{}\" opened — {stores} settings groups restored",
                    crate::workspace_bundle::recent_label(path)
                ));
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_IMPORT_REFUSED",
                    path = %path.display(),
                    %error,
                    action = "cockpit_unchanged",
                    "workspace refused"
                );
                self.note_workspace(format!("Workspace not opened — {error}"));
            }
        }
    }

    /// Read every cockpit store back into the running app.
    ///
    /// Called after an import wrote them. Each store goes through the loader
    /// it already had, so an imported cockpit is restored by exactly the code
    /// that restores one at startup — a second, import-only restore path is
    /// how the two would drift.
    fn reload_cockpit_stores(&mut self, imported: &[&str]) {
        self.added_symbols = symbols_file::load(&self.symbols_path);
        self.drawing_presets = drawings::presets::PresetStore::load_from(
            drawings::presets::PresetStore::default_path(),
        );
        self.footprint_config = crate::footprint_config::load(&self.footprint_settings_path);
        self.surfaces.footprint_settings.reload_presets();
        self.indicator_presets = preset_file::PresetStore::load(&self.indicator_presets_path);

        // The tab strip first, and *before* the indicators: the restore adds
        // each indicator to whatever pane is focused right now, so the tabs
        // have to be the imported ones — and the focus on the pane the file
        // describes — before a single indicator is added. Getting this order
        // wrong puts a trader's imported indicators on the tab they happened
        // to be looking at, or on its time pane.
        let workspace = ui_state::load(&self.ui_state_path).restore(&self.config.clone());
        self.restore_workspace(workspace);
        self.restore_chart_layers();

        // The layouts come last, once the tabs are the imported ones: every
        // pane is stripped and re-seeded from the imported file.
        self.reload_layouts(imported);
    }

    /// Work out which remembered workspace files are still there.
    ///
    /// Called when the list changes, never from the menu body — see
    /// [`Self::recent_on_disk`]. The stored list keeps every entry: a file on
    /// a drive that is merely unplugged today comes back when it is plugged
    /// in, and only the menu is filtered.
    fn refresh_recent_workspaces(&mut self) {
        self.recent_on_disk = crate::workspace_bundle::existing_recent(&self.recent_workspaces);
    }

    /// Take every indicator off every pane.
    ///
    /// Straight off each pane's own collection rather than by walking
    /// `slot_kinds`: that list is bookkeeping for the state *file*, and an
    /// indicator can be on a pane without being in it — the autostart hooks
    /// add without registering, and `forget_last_indicator_state_change` pops
    /// an entry while leaving the indicator on screen. Clearing the list
    /// would have left those behind for the imported set to stack on top of.
    fn clear_indicators(&mut self) {
        /// Empty one pane, view and worker alike.
        fn strip(pane: &mut crate::pane::ChartPane) {
            let slots: Vec<SlotId> = pane.indicators.all().iter().map(|view| view.slot).collect();
            for slot in slots {
                pane.indicators.remove(slot);
                pane.indicator_worker.send(IndicatorCommand::Remove(slot));
            }
        }
        for tab in &mut self.tabs {
            // Every pane the tab holds, not the two it used to. `panes_mut`
            // rather than `pane_mut(Time)`: the latter falls back to the flow
            // pane when a tab was never split, which would strip it twice, and
            // it stops at the *first* context chart — so the second stacked
            // chart kept its indicators while `slot_kinds` was cleared out from
            // under them, and the imported set stacked on top.
            for pane in tab.panes_mut() {
                strip(pane);
            }
        }
        self.slot_kinds.clear();
        self.operator_slots.clear();
        self.script_files.clear();
        self.pending_hidden.clear();
        self.pending_styles.clear();
        self.mark_indicator_state_dirty();
    }

    /// Show the trader where the cockpit is kept, and open it.
    ///
    /// The answer to "where does this thing save my setup?" — which, before
    /// the durable home, had no single answer at all.
    fn reveal_cockpit_home(&mut self) {
        let Some(home) = crate::store_home::home() else {
            self.note_workspace(
                "This system reports no documents folder — quantick keeps the cockpit beside \
                 wherever it was launched from"
                    .to_owned(),
            );
            return;
        };
        self.note_workspace(format!("Cockpit saved in {}", home.display()));
        // The journal panel's opener, not a second copy of the platform
        // table: two of them means the next fix lands on one and misses the
        // other. Best effort — the path is on the status line either way, so
        // a system with no file manager still answers the question.
        crate::paper_trading::reveal_folder(&home);
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

    /// Update *only* where the properties popup goes, in the workspace file as
    /// it already stands. Says whether anything was written.
    ///
    /// Deliberately not [`Self::save_workspace`]. That one captures the whole
    /// window — every tab, its market, its bar rule, the layout, the window
    /// size — and adopts it as the startup screen. Parking a popup is not a
    /// statement about any of that: a trader who opens six tabs to research
    /// something, then nudges the popup out of the way, must not find those six
    /// tabs waiting for them tomorrow. The switch that governs whole-window
    /// saves says "when the window closes", and this write happens mid-session,
    /// so it has no business speaking for the tabs.
    ///
    /// A file with no chrome section is left alone rather than created: there
    /// is nothing to update, and inventing a startup workspace out of a drag
    /// would undo a `Reset startup layout` the trader just asked for. The exit
    /// save is what creates the file, and it carries the position with it.
    ///
    /// No toast either. The "Workspace saved" line answers a deliberate *Save*,
    /// and repeating it after every small gesture would turn the window's one
    /// acknowledgement channel into wallpaper — but the log still names the
    /// write, so a position that went missing is answerable.
    fn write_inspector_position(&mut self) -> bool {
        let mut file = ui_state::load(&self.ui_state_path);
        let Some(chrome) = file.chrome.as_mut() else {
            return false;
        };
        let position = self.surfaces.drawing_chrome.remembered_inspector_position();
        if chrome.inspector_position == position {
            return false;
        }
        chrome.inspector_position = position;
        let saved = ui_state::save(&self.ui_state_path, &file);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_POPUP_POSITION_SAVED",
            path = %self.ui_state_path.display(),
            parked = position.is_some(),
            saved,
            action = if saved { "position_written" } else { "position_not_written" },
            "properties popup position"
        );
        saved
    }

    /// Write the bookmarks without disturbing the startup arrangement.
    ///
    /// Reads the file back and swaps only the named entries, rather than
    /// capturing the live window: saving a bookmark must not redefine what the
    /// app opens on, and `capture_workspace` describes the screen *now*, which
    /// is exactly what the startup arrangement must not become.
    fn write_bookmarks(&mut self) -> bool {
        let bookmarks = self.bookmarks.clone();
        let written = self.edit_workspace_file("UI_STATE_BOOKMARKS_WRITTEN", |file| {
            file.saved = bookmarks;
        });
        self.workspace_saved |= written;
        written
    }

    /// Change one standing choice in the workspace file, leaving everything
    /// else in it exactly as it was. `true` when the change reached the disk.
    ///
    /// This file holds three choices that are not descriptions of the screen —
    /// the named bookmarks, the replay folder, the starred tools. Each is made
    /// by a single click and each is written on the spot rather than at exit,
    /// because "it forgot again" must not be one crash away. Each used to
    /// hand-roll the same read-swap-write, and three copies were three chances
    /// to differ: two carried `save_on_exit` through and one did not, so a
    /// trader with autosave off who picked a replay folder before the file
    /// existed had autosave quietly switched back on at the next launch.
    ///
    /// `save_on_exit` rides along here because it is the one live setting that
    /// belongs to the *file* rather than to any arrangement inside it.
    ///
    /// A file this build cannot read is never rewritten — see
    /// [`ui_state::load_for_edit`]. `workspace_saved` is deliberately not
    /// touched: whether a startup *arrangement* exists is a different question
    /// from whether this file does, and the caller answers it.
    fn edit_workspace_file(
        &mut self,
        event_code: &'static str,
        edit: impl FnOnce(&mut ui_state::Workspace),
    ) -> bool {
        let Some(mut file) = ui_state::load_for_edit(&self.ui_state_path) else {
            self.note_workspace(
                "The workspace file could not be read, so it was left alone — see the log"
                    .to_owned(),
            );
            return false;
        };
        file.save_on_exit = self.save_on_exit;
        edit(&mut file);
        let written = ui_state::save(&self.ui_state_path, &file);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code,
            path = %self.ui_state_path.display(),
            written,
            action = if written { "file_updated" } else { "file_not_written" },
            "a standing choice was written to the workspace file"
        );
        written
    }

    /// Write down the replay folder the trader just pointed the browser at,
    /// without disturbing anything else the workspace file holds.
    ///
    /// The same read-swap-write as [`Self::write_bookmarks`], and for the same
    /// reason: this is a standing choice, not a description of the screen, so
    /// it must not wait for a clean exit and must not drag the current
    /// arrangement into the file with it.
    fn write_replay_folder(&mut self, folder: Option<&str>) {
        let stored = folder.map(str::to_owned);
        let written = self.edit_workspace_file("REPLAY_FOLDER_REMEMBERED", |file| {
            file.replay_folder = stored;
        });
        self.workspace_saved |= written;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_FOLDER_REMEMBERED",
            folder = folder.unwrap_or_default(),
            written,
            action = if folder.is_some() {
                "store_replay_folder"
            } else {
                "forget_replay_folder"
            },
            "the replay folder is now the one this workspace opens on"
        );
    }

    /// Write down the *day before* choice the trader just made, without
    /// disturbing anything else the workspace file holds.
    ///
    /// The same read-swap-write as [`Self::write_replay_folder`], and for the
    /// same reason: whether yesterday is on the chart is a standing choice,
    /// not a description of the screen, so it must not wait for a clean exit.
    fn write_replay_day_before(&mut self, enabled: bool) {
        let written = self.edit_workspace_file("REPLAY_DAY_BEFORE_REMEMBERED", |file| {
            file.replay_day_before = Some(enabled);
        });
        self.workspace_saved |= written;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_DAY_BEFORE_REMEMBERED",
            enabled,
            written,
            action = if enabled {
                "join_day_before"
            } else {
                "chosen_day_only"
            },
            "the day before is now what this workspace opens recordings with"
        );
    }

    /// Write down the tools the trader just starred or unstarred, without
    /// disturbing anything else the workspace holds.
    ///
    /// The same read-swap-write as [`Self::write_replay_folder`], through the
    /// same [`Self::edit_workspace_file`], with two things of its own.
    ///
    /// First, `save_on_exit` does not gate it. That switch governs whether
    /// closing the window redefines the *arrangement* — which tabs open, how
    /// the panes are split. A starred tool is not an arrangement, and a trader
    /// who turned autosave off to stop their layout drifting has not asked to
    /// rebuild their rail every session.
    ///
    /// Second, `workspace_saved` is left alone. It answers "is there a startup
    /// arrangement to reset?", and starring a tool does not create one — a
    /// fresh install whose only saved thing is a star would otherwise light up
    /// a Reset entry that promises to forget a layout nobody ever saved.
    fn write_favorites(&mut self) {
        // A run under `QUANTICK_TOOL_FAVORITES` is wearing a rail the harness
        // dressed it in, not one the trader curated, and the same guard the
        // replay folder gets applies: a validation run must not write a QA
        // list into the trader's workspace. The hook stages a screen; it does
        // not make choices on their behalf.
        if self.favorites_are_staged {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "TOOL_FAVORITES_NOT_WRITTEN",
                action = "staged_by_hook",
                "a run under QUANTICK_TOOL_FAVORITES does not write the rail down"
            );
            return;
        }
        let tools = self.starred_tool_ids();
        let count = tools.len();
        let written = self.edit_workspace_file("TOOL_FAVORITES_REMEMBERED", |file| {
            file.favorite_tools = tools;
        });
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TOOL_FAVORITES_REMEMBERED",
            tools = count,
            written,
            action = if written { "favorites_written" } else { "favorites_not_written" },
            "the rail's pinned tools are now what this workspace opens on"
        );
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
            let context_intervals =
                saved_context_intervals(&saved.context_bars, saved.time_bars.as_deref());
            let opened = self.tabs.len() - 1;
            self.tabs[opened].restore_canvas(
                CanvasLayout::from(saved.layout),
                saved.split_fraction,
                saved.context_collapsed,
                saved.focus.map(|focus| focus.to_side(saved.focus_slot)),
                &context_intervals,
                LegendFold {
                    flow: saved.flow_legend_collapsed,
                    time: saved.time_legend_collapsed,
                },
            );
            self.tabs[opened].set_opening_layouts(saved.flow_layout, &saved.context_layouts);
        }
        for _ in 0..replaced {
            self.close_tab(0);
        }
        // The pinned section is deliberately untouched: the stars live beside
        // the chrome rather than inside it, and [`Self::restore_chrome`] does
        // not speak for them. A bookmark rearranges the cockpit; the tools the
        // trader keeps at hand are not part of the arrangement, and a bookmark
        // named before they starred anything used to wipe the rail on open.
        if let Some(chrome) = &entry.chrome {
            self.restore_chrome(chrome);
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
        let bookmarks_kept = !self.bookmarks.is_empty();
        // The starred tools survive it too, and for a plainer reason: they
        // were never part of the arrangement being reset. Resetting a layout
        // is not asking to rebuild the rail by hand — and the same goes for
        // every other standing choice this file holds. The replay folder and
        // the Open-recent list are facts about this installation; the entry
        // resets a *layout* and must not quietly take them with it.
        let stars = self.starred_tool_ids();
        let kept = bookmarks_kept
            || !stars.is_empty()
            || !self.recent_workspaces.is_empty()
            || self.replay_view.stored_pick().is_some();
        let bookmarks = self.bookmarks.clone();
        let stars_kept = !stars.is_empty();
        let forgotten = if kept {
            // Edited rather than rebuilt from the defaults: writing a fresh
            // `Workspace` would carry only what this function remembered to
            // thread through it, and the fields it forgot would be reset by
            // omission. Clearing the arrangement names what goes; everything
            // unnamed stays by construction.
            self.edit_workspace_file("UI_STATE_FORGOTTEN", |file| {
                file.tabs.clear();
                file.chrome = None;
                file.window = None;
                file.active_tab = 0;
                file.saved = bookmarks;
                file.favorite_tools = stars;
            })
        } else {
            ui_state::forget(&self.ui_state_path)
        };
        // The file still exists while it holds standing choices, so Reset
        // stays available — it is now a no-op for the startup screen and the
        // entry says as much. A reset that *failed* leaves the old
        // arrangement on disk and so leaves the entry live: the trader has to
        // be able to try again, and telling them "nothing saved yet" while the
        // next launch still reopens the layout they discarded would be a lie.
        self.workspace_saved = if forgotten { kept } else { true };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_FORGOTTEN",
            path = %self.ui_state_path.display(),
            forgotten,
            bookmarks_kept = self.bookmarks.len(),
            favorites_kept = self.toolrail.favorites().len(),
            action = if forgotten { "open_on_config_defaults" } else { "workspace_kept" },
            "workspace reset"
        );
        // What survived is named, never left to be discovered. A trader who
        // resets a layout and is told nothing else assumes nothing else was
        // kept — and would go looking for stars that are still there.
        let survivors = match (bookmarks_kept, stars_kept) {
            (true, true) => format!(
                " {} saved {} and the starred tools kept.",
                self.bookmarks.len(),
                if self.bookmarks.len() == 1 {
                    "workspace"
                } else {
                    "workspaces"
                }
            ),
            (true, false) => format!(
                " {} saved {} kept.",
                self.bookmarks.len(),
                if self.bookmarks.len() == 1 {
                    "workspace"
                } else {
                    "workspaces"
                }
            ),
            (false, true) => " The starred tools are kept.".to_owned(),
            (false, false) => String::new(),
        };
        self.note_workspace(if forgotten {
            format!(
                "Startup layout reset — the next launch opens on the configured default.{survivors}"
            )
        } else {
            "Workspace could not be reset — see the log".to_owned()
        });
    }

    /// Hand the app the window it is drawing into.
    ///
    /// Called once from `main`, which is where eframe offers the handle: the
    /// hook that needs it (`raw_input_hook`) is given only the input. One
    /// registration line rather than a constructor argument, because every
    /// other construction path — every test — wants the `None` this defaults
    /// to, which is also what every non-Windows target gets.
    pub fn attach_surface(&mut self, handle: &impl raw_window_handle::HasWindowHandle) {
        self.surface = window_scale::SurfaceProbe::new(handle);
    }

    /// Take the window manager's own maximise, once, on the first frame.
    ///
    /// Through [`egui::ViewportCommand::Maximized`] rather than the viewport
    /// builder's `with_maximized`: eframe 0.29 does not honour that flag beside
    /// an `inner_size`, and a hook that silently opens a 1100×650 window while
    /// reporting success is worse than no hook — a validation run would
    /// photograph the wrong state and call it a pass. The command is the one
    /// the platform runs when a hand hits the title bar, which is the state
    /// this hook exists to reach.
    fn apply_maximize_hook(&mut self, ctx: &egui::Context) {
        if !self.pending_maximize {
            return;
        }
        self.pending_maximize = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "WINDOW_MAXIMIZE_AUTOSTART",
            action = "maximize",
            "QUANTICK_WINDOW_MAXIMIZED asked for the maximised layout"
        );
    }

    /// Keep the window size the workspace would record, flush a popup the
    /// trader just re-parked, and take the exit save when the window is
    /// closing.
    ///
    /// **Per-frame cost**: two reads off the frame's own input state, a float
    /// compare and a `bool` test. No save is on this path — each of the three
    /// happens on one frame: the frame a drag ends, and the frame the close is
    /// requested, when the window is going away anyway.
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
        // Where the trader parks the properties popup is kept the moment the
        // hand comes off it, without a trip through the Workspace menu: the
        // window they dragged out of the way is the window they expect back,
        // and a memory that only survived a clean exit would lose it to the
        // one session that ended badly. Only that one field is written — see
        // [`Self::write_inspector_position`] for why a drag must not adopt the
        // tab strip as a startup screen.
        //
        // Gated on the same switch the exit save is, because the menu already
        // promises it: "Off, only Save workspace changes what quantick opens
        // on." A trader who curates their startup layout by hand still gets the
        // position within the session — it is live state — and an explicit Save
        // still records it.
        //
        // The closing frame takes the exit save instead: it writes the whole
        // window, this position included, so running both would serialise the
        // same file twice on the way out.
        if closing && self.save_on_exit {
            self.inspector_position_dirty = false;
            self.save_workspace("exit");
        } else if std::mem::take(&mut self.inspector_position_dirty) && self.save_on_exit {
            self.write_inspector_position();
        }
    }

    /// Post a Workspace-menu answer through the window's one acknowledgement
    /// channel ([`Toast`]).
    ///
    /// No Undo: the file it replaced is gone, and `Reset startup layout` is
    /// the honest way back rather than a button that pretends otherwise.
    fn note_workspace(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }

    /// Periodically log a perf summary and warn on threshold breaches.
    fn maybe_emit_summary(&mut self, now: Instant, ctx: &egui::Context) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        // The window's own geometry, because a chart that lays out wider than
        // the surface it is painted on loses its right edge — the toolbar's
        // layer group, the price axis, the live strip and the dock all live
        // there — and nothing else in this line would say so.
        //
        // `client_px` is the platform's *own* answer, not `screen * scale`:
        // that product is algebraically the same number as `screen_pt` beside
        // it and could never contradict anything, which would make the whole
        // block decoration. Two independent readings, so the line can be read
        // for whether they agree — and by the time it is written a correction
        // may already have restored that agreement, which `WINDOW_SCALE_CORRECTED`
        // is the record of.
        // Read outside the `input` closure: egui holds one lock for the whole
        // of it, and reaching back into the context from inside deadlocks.
        let zoom = ctx.zoom_factor();
        let client = self
            .surface
            .as_ref()
            .and_then(window_scale::SurfaceProbe::client_size_px);
        let (screen, scale, native_scale) = ctx.input(|input| {
            (
                input.screen_rect.size(),
                input.pixels_per_point(),
                input.viewport().native_pixels_per_point,
            )
        });
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
            // Both facts the tape states about its own prices — the grid they
            // land on and the magnitude they land at — and the row width the
            // ladder ended up drawing from them. A validation run reads all
            // three: the first two are the sizing rule's whole input, so
            // without them a run can see that rows are 1.00 and not why, and
            // cannot tell "the chart has not been told a tick yet" from "the
            // chart is drawing rows finer than the instrument can trade at".
            tape_price_step = self
                .active_tab()
                .flow_pane
                .state
                .tape_price_step()
                .map_or_else(|| "unknown".to_owned(), |step| step.to_string()),
            tape_reference_price = self
                .active_tab()
                .flow_pane
                .state
                .tape_reference_price()
                .map_or_else(|| "unknown".to_owned(), |price| price.to_string()),
            footprint_rows = %self.active_tab().flow_pane.state.footprint_group(),
            canvas_layout = ?self.active_tab().layout,
            screen_pt_w = screen.x,
            screen_pt_h = screen.y,
            client_px_w = client.map(|size| size.x),
            client_px_h = client.map(|size| size.y),
            scale = scale,
            native_scale = native_scale,
            zoom_factor = zoom,
            time_pane_spec = self.active_tab().time_pane().map(|pane| pane.state.spec().summary()),
            time_pane_count = self.active_tab().time_panes.len(),
            // Drawings are a per-frame, O(objects) paint cost, and the shared
            // ones are additionally reprojected on every other pane of the
            // tab. Counting them here is what lets a frame-cost reading be
            // attributed instead of guessed — and it is the only way a
            // headless run can prove the drawing overlay is populated at all.
            drawings = self
                .active_tab()
                .panes()
                .map(|(pane, _)| pane.drawings.items().len())
                .sum::<usize>(),
            shared_drawings = self
                .active_tab()
                .panes()
                .map(|(pane, _)| pane.drawings.shared_count())
                .sum::<usize>(),
            book_enabled = book.enabled,
            book_status = book.status,
            book_generation = book.generation,
            book_last_update_id = book.last_update_id,
            book_last_event_ms = book.last_event_ms,
            book_snapshot_observed_ms = book.last_snapshot_observed_ms,
            book_arrival_ms = book_lag,
            // How far the newest print sits behind the instant the lane calls
            // now. It is the pixel gap between the last bubble and the tape's
            // right edge, in milliseconds: a number, so "the bubbles are
            // trailing" can be measured rather than argued about.
            //
            // A distance between two *venue* clocks, not staleness against
            // this machine's. A dead session stops both, so this figure
            // freezes rather than growing — `feed_arrival_ms` above is the one
            // that answers "is anything still arriving".
            tape_age_ms = book.tape_age.map(|age| match age {
                crate::orderflow::TapeAge::Behind(ms) | crate::orderflow::TapeAge::NothingYet(ms) => ms,
            }),
            tape_age_kind = book.tape_age.map(|age| match age {
                crate::orderflow::TapeAge::Behind(_) => "behind",
                crate::orderflow::TapeAge::NothingYet(_) => "nothing_yet",
            }),
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
            heatmap_folded_aggressions = book.folded_aggressions,
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
        // Losses only. Folding is the expected steady state on a busy tape and
        // loses nothing — warning about it would tell an operator (and the
        // planned assistant reading these events) to go fix something that is
        // not broken. The fold count still rides in the info summary above.
        if book.dropped_cells > 0 || book.dropped_liquidity_events > 0 {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROJECTION_CAPPED",
                symbol = self.active_tab().symbol.as_str(),
                dropped_cells = book.dropped_cells,
                dropped_liquidity_events = book.dropped_liquidity_events,
                // Not "group harder". Grouping is exactly what the trader is
                // complaining about when marks read as one blob, and the
                // aggression budget no longer discards anything to begin with —
                // it folds, and says how much it folded. What is worth widening
                // is the budget or the pane, so that is what this names.
                action = "increase_grouping_or_reduce_retention",
                "heatmap depth primitive cap dropped items"
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
            feed_latency: self.active_tab().feed_latency(),
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
/// `Ctrl+1` … `Ctrl+9` apply the layout registry's presets, in table order.
///
/// The keys are listed; which preset each one reaches is not. A preset added
/// to `LAYOUT_PRESETS` gets its shortcut from its position without this array
/// or its dispatch being edited — the same rule the picker and the View menu
/// follow. Nine is what a number row has; `MAX_CANVAS_PANES` keeps the
/// registry far below that.
const LAYOUT_PRESET_KEYS: [egui::Key; 9] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
];

/// The shortcut that reaches the preset at `index` in the registry, if a
/// number key still reaches that far.
fn layout_preset_shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    LAYOUT_PRESET_KEYS
        .get(index)
        .map(|key| egui::KeyboardShortcut::new(egui::Modifiers::CTRL, *key))
}

/// The shortcut that reaches the layout tab at strip position `index`, if a
/// number key still reaches that far.
fn layout_tab_shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    LAYOUT_PRESET_KEYS
        .get(index)
        .map(|key| egui::KeyboardShortcut::new(egui::Modifiers::ALT, *key))
}

/// `Ctrl+0` puts the context charts away, or brings them back.
///
/// The number row's own zero, beside `Ctrl+1..9` for the presets: nine keys
/// choose an arrangement and the tenth dismisses the column that arrangement
/// put beside the heatmap. Without it the only way to collapse was a drag,
/// which a trader working by keyboard cannot make and which WCAG 2.2's
/// dragging rule wants an alternative to besides.
const COLLAPSE_CONTEXT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Num0);

const DOCK_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B);
/// Folds the focused pane's on-chart indicator legend to its count puck, or
/// opens it back up (see [`crate::indicator_legend`]).
///
/// Ctrl+letter like the dock's own switch above, not the bare `L` the drawing
/// tools answer to: bare letters are the toolbox's namespace, and a chrome
/// switch borrowing one would arm a tool on every trader who learned it there.
const LEGEND_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::L);
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
        // Gated on the same condition the View menu's entry is gated on: only
        // a layout that carves a context column *beside* the flow pane has a
        // column to put away. Ungated, `Ctrl+0` on the Flow or Timeframe
        // layout set a flag nothing drew — and swallowed the key besides, so
        // egui's own "reset zoom" never ran.
        if self.active_tab().layout.shows_time()
            && self.active_tab().layout.shows_flow()
            && ctx.input_mut(|i| i.consume_shortcut(&COLLAPSE_CONTEXT_SHORTCUT))
        {
            let collapsed = self.active_tab().context_collapsed;
            self.active_tab_mut().set_context_collapsed(!collapsed);
        }
        // Layout by number, straight off the registry. The same
        // `apply_layout_preset` the picker and the menu call — three doors,
        // one room.
        for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
            let Some(shortcut) = layout_preset_shortcut(index) else {
                break;
            };
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.apply_layout_preset(preset);
            }
        }
        // Layout tabs by number: `Alt+1..9`, beside `Ctrl+1..9` for the
        // presets — one row of keys, two things a trader switches by number.
        // Not while a text field has the keyboard — a rename box, a note,
        // the ticket — where Alt+1 is text, not a switch.
        let typing = ctx.memory(|memory| memory.focused().is_some());
        for index in 0..LAYOUT_PRESET_KEYS.len() {
            let Some(shortcut) = layout_tab_shortcut(index) else {
                break;
            };
            if !typing
                && ctx.input_mut(|i| i.consume_shortcut(&shortcut))
                && let Err(error) = self.switch_layout_index(index)
            {
                self.note_workspace(error.to_string());
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&LEGEND_SHORTCUT)) {
            let collapsed = self.focused_legend_collapsed();
            self.set_focused_legend_collapsed(!collapsed);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&crate::control::MARK_SHORTCUT)) {
            self.take_mark(None);
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
                            tab.close_replay(config);
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
                        ui.menu_button("Layouts", |ui| {
                            // The strip's tabs, from the book: switch the
                            // focused pane by name, and the three edits the
                            // strip's own menu holds.
                            let active = self.focused_pane_layout();
                            let names: Vec<(crate::layouts::LayoutId, String)> = self
                                .layouts()
                                .layouts()
                                .iter()
                                .map(|layout| (layout.id, layout.name.clone()))
                                .collect();
                            for (index, (id, name)) in names.iter().enumerate() {
                                let mut button = egui::Button::new(name.as_str());
                                if let Some(shortcut) = layout_tab_shortcut(index) {
                                    button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                                }
                                if ui.add(button.selected(*id == active)).clicked() {
                                    self.apply_strip_action(crate::layout_strip::StripAction::Switch(*id));
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            let can_add = names.len() < crate::layouts::MAX_LAYOUTS;
                            if ui
                                .add_enabled(can_add, egui::Button::new("New layout"))
                                .clicked()
                            {
                                self.apply_strip_action(crate::layout_strip::StripAction::Create);
                                ui.close_menu();
                            }
                            if ui.button("Rename layout…").clicked() {
                                self.apply_strip_action(crate::layout_strip::StripAction::BeginRename(active));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(names.len() > 1, egui::Button::new("Delete layout"))
                                .clicked()
                            {
                                self.apply_strip_action(crate::layout_strip::StripAction::Delete(active));
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("Layout", |ui| {
                            // Read from the registry, like the picker: a menu
                            // holding its own list of layouts is the second
                            // opinion that goes stale the day one is added.
                            let current = self.active_tab().layout.preset();
                            for (index, preset) in
                                crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate()
                            {
                                // The menu is where a shortcut is learned, so
                                // it carries the binding beside the name.
                                let label = match layout_preset_shortcut(index) {
                                    Some(shortcut) => format!(
                                        "{}	{}",
                                        preset.label,
                                        ui.ctx().format_shortcut(&shortcut)
                                    ),
                                    None => preset.label.to_owned(),
                                };
                                if ui
                                    .selectable_label(current.id == preset.id, label)
                                    .clicked()
                                {
                                    self.apply_layout_preset(preset);
                                    ui.close_menu();
                                }
                            }
                        });
                        // Collapsing was drag-only, which a trader working
                        // by keyboard could not do at all — while the
                        // assistant had `layout.pane.collapse`. Same call,
                        // three doors.
                        if self.active_tab().layout.shows_time()
                            && self.active_tab().layout.shows_flow()
                        {
                            let collapsed = self.active_tab().context_collapsed;
                            let label = if collapsed {
                                "Show context charts"
                            } else {
                                "Hide context charts"
                            };
                            if ui
                                .add(
                                    egui::Button::new(label).shortcut_text(
                                        ui.ctx().format_shortcut(&COLLAPSE_CONTEXT_SHORTCUT),
                                    ),
                                )
                                .clicked()
                            {
                                self.active_tab_mut().set_context_collapsed(!collapsed);
                                ui.close_menu();
                            }
                        }
                        // Reposition without a drag. WCAG 2.2's dragging rule
                        // wants a single-pointer alternative to every drag, and
                        // TradingView — the reference the trader named — moves
                        // charts by a menu command rather than by dragging at
                        // all. Both go through `Tab::move_context_pane`.
                        let context_panes = self.active_tab().pane_count().saturating_sub(1);
                        if context_panes > 1 {
                            ui.menu_button("Move chart", |ui| {
                                for slot in 1..=context_panes {
                                    let up = ui
                                        .add_enabled(slot > 1, egui::Button::new(format!(
                                            "Chart {slot} up"
                                        )))
                                        .on_disabled_hover_text("already the top chart");
                                    if up.clicked() {
                                        let tab_id = self.active_tab().id;
                                        self.move_context_pane_at(tab_id, slot, slot - 1);
                                        ui.close_menu();
                                    }
                                    let down = ui
                                        .add_enabled(
                                            slot < context_panes,
                                            egui::Button::new(format!("Chart {slot} down")),
                                        )
                                        .on_disabled_hover_text("already the bottom chart");
                                    if down.clicked() {
                                        let tab_id = self.active_tab().id;
                                        self.move_context_pane_at(tab_id, slot, slot + 1);
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
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
                        // The legend belongs to a pane, so this entry names
                        // the focused one's state — the same pane the chevron
                        // on screen would fold.
                        let collapsed = self.focused_legend_collapsed();
                        // Split open: say *which* chart, the way the layout
                        // entries above name the charts they show. The action
                        // follows the focus like every other chrome control,
                        // and a trader reading "Collapse indicator legend"
                        // over two charts has no way to know which corner is
                        // about to change.
                        let split = self.active_tab().shows_context_charts();
                        let pane_name = self.active_tab().focused_side().title();
                        let legend_label = match (collapsed, split) {
                            (true, false) => "Show indicator legend".to_owned(),
                            (false, false) => "Collapse indicator legend".to_owned(),
                            (true, true) => format!("Show indicator legend ({pane_name})"),
                            (false, true) => format!("Collapse indicator legend ({pane_name})"),
                        };
                        // A pane with no indicators has no legend to fold, and
                        // an entry that is enabled and does nothing reads as a
                        // broken feature rather than as an empty chart.
                        let has_legend = {
                            let tab = self.active_tab();
                            !tab.pane(tab.focused_side()).indicators.all().is_empty()
                        };
                        if ui
                            .add_enabled(
                                has_legend,
                                egui::Button::new(legend_label)
                                    .shortcut_text(ui.ctx().format_shortcut(&LEGEND_SHORTCUT)),
                            )
                            .on_hover_text(
                                "Folds the healthy rows to a count on the focused chart. Errored and stale indicators stay on it.",
                            )
                            .on_disabled_hover_text(
                                "This chart has no indicators, so there is no legend to fold",
                            )
                            .clicked()
                        {
                            self.set_focused_legend_collapsed(!collapsed);
                            ui.close_menu();
                        }
                        ui.menu_button("Drawing toolbar", |ui| {
                            for (dock, label) in [
                                (ToolboxDock::Left, "Left"),
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
                        ui.checkbox(&mut self.progressive_history, "Progressive venue history")
                            .on_hover_text(
                                "Build the venue's candle history from now backwards, a week at \
                                 a time, so the chart fills in while the rest arrives. Off asks \
                                 for the whole span in one request: fewer calls, nothing on \
                                 screen until all of it lands.",
                            );
                        ui.checkbox(&mut self.venue_lead_in, "Venue candles on charts cut by trades")
                            .on_hover_text(
                                "A tick, volume, dollar or imbalance chart cannot fold venue \
                                 candles into its own bars, so it opens holding only the prints \
                                 this session saw. Switch this on to put the venue's 1-minute \
                                 candles in front of them anyway — counted apart from built bars \
                                 on the status bar — so yesterday is on screen to compare \
                                 against. They stay candles: a minute never becomes a tick bar, \
                                 and an indicator running across the seam is averaging both \
                                 kinds.",
                            );
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
                    // The button's own rect, published for the capture hook —
                    // read, never acted on, so the menu behaves identically
                    // whether or not a scripted run is watching.
                    let workspace_menu = ui.menu_button("Workspace", |ui| {
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
                            // Says what a bookmark keeps *and what it does
                            // not*: it is the tabs and the panels, not the
                            // indicators or the colours. Two entries in one
                            // menu that both "save a workspace" but restore
                            // different amounts is exactly how a trader comes
                            // to believe the app forgets things — use Export
                            // to file for the whole cockpit.
                            .on_hover_text(
                                "Keep these tabs and panels under a name you can reopen later. \
                                 It does not change what quantick opens on, and it does not \
                                 keep indicators or colours — use Export to file for those.",
                            )
                            .clicked()
                        {
                            self.surfaces.workspace_name.open();
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
                        // Files, named apart from the two groups above again:
                        // those live inside quantick, these are documents the
                        // trader owns, can copy, back up and carry to another
                        // machine. That is the difference the wording carries.
                        if ui
                            .button("Export to file…")
                            .on_hover_text(
                                "Save the whole cockpit — tabs, indicators, layers, drawing \
                                 colours, footprint and added symbols — as one file in your \
                                 documents",
                            )
                            .clicked()
                        {
                            self.open_workspace_export_picker();
                            ui.close_menu();
                        }
                        if ui
                            .button("Open from file…")
                            .on_hover_text(
                                "Open a workspace file. It replaces the cockpit on screen; a \
                                 file that cannot be read changes nothing.",
                            )
                            .clicked()
                        {
                            self.open_workspace_import_picker();
                            ui.close_menu();
                        }
                        // Read off the field, not the filesystem: this body
                        // runs every frame the menu is open.
                        let mut reopen: Option<std::path::PathBuf> = None;
                        ui.add_enabled_ui(!self.recent_on_disk.is_empty(), |ui| {
                            ui.menu_button("Open recent", |ui| {
                                for path in &self.recent_on_disk {
                                    if ui
                                        .button(crate::workspace_bundle::recent_label(path))
                                        // The same warning the bookmark list
                                        // carries: this replaces the cockpit,
                                        // and a trader mid-tape has to read
                                        // that before the click, not after.
                                        .on_hover_text(format!(
                                            "Replaces the cockpit on screen\n{}",
                                            path.display()
                                        ))
                                        .clicked()
                                    {
                                        reopen = Some(path.clone());
                                        ui.close_menu();
                                    }
                                }
                            })
                            .response
                            .on_disabled_hover_text("No workspace files opened yet");
                        });
                        if let Some(path) = reopen {
                            self.import_workspace_from(&path);
                        }
                        if ui
                            .button("Show where it's saved")
                            .on_hover_text(
                                "Open the folder quantick keeps your cockpit in, so you can see \
                                 it and back it up",
                            )
                            .clicked()
                        {
                            self.reveal_cockpit_home();
                            ui.close_menu();
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
                    self.workspace_menu_rect = Some(workspace_menu.response.rect);
                    ui.menu_button("Tools", |ui| {
                        if ui.button("Appearance…").clicked() {
                            self.surfaces.style_panel.open();
                            ui.close_menu();
                        }
                        let access_label = self
                            .control_access
                            .as_ref()
                            .map_or("Local agent access…", |access| access.menu_label());
                        if ui.button(access_label).clicked() {
                            if let Some(access) = self.control_access.as_mut() {
                                access.open_panel();
                            }
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("Replay file format…").clicked() {
                            self.replay_view.open_format_help();
                            ui.close_menu();
                        }
                    });
                    if self
                        .control_access
                        .as_ref()
                        .is_some_and(crate::control::ControlAccess::is_enabled)
                        && ui.button("Agent access: on").clicked()
                        && let Some(access) = self.control_access.as_mut()
                    {
                        access.open_panel();
                    }
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
        // Read the name before the object is gone. "Drawing deleted" makes
        // the undo useless on a crowded chart: the trader has to know *what*
        // they lost to know whether they want it back — and the context bar
        // deletes on a bare glyph, so the toast is what pays for that.
        let doomed = self.drawing_pane().drawings.selected().and_then(|index| {
            let drawing = self.drawing_pane().drawings.items().get(index)?;
            // The trader's own name when one was given; the tool name
            // otherwise — a positional index would be noise on an object
            // that no longer has a position.
            let label = drawing
                .name
                .clone()
                .unwrap_or_else(|| drawing.tool.name().to_owned());
            Some((drawing.id, label))
        });
        match self.drawing_pane_mut().drawings.delete_selected(false) {
            DeleteOutcome::Deleted => {
                self.surfaces.drawing_chrome.set_delete_confirm(false);
                // The instance dies with its drawing, immediately — not on
                // the next closed bar, which a quiet tape may never bring.
                if let Some((id, _)) = &doomed {
                    self.drawing_pane_mut().remove_strategy_for_drawing(*id);
                }
                let name = doomed.map(|(_, label)| label);
                let message = name.map_or_else(
                    || "Drawing deleted.".to_owned(),
                    |name| format!("{name} deleted."),
                );
                self.surfaces.toast.note_with_undo(message, now);
            }
            DeleteOutcome::NeedsConfirmation => {
                self.surfaces.drawing_chrome.set_delete_confirm(true);
            }
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
        //
        // The context bar's parked position is deliberately *not* a layer of
        // this stack. It is a preference, not a gesture left half-finished:
        // a trader who moved the bar out of the way wants it to stay out of
        // the way, and Escape is a key they press many times an hour to drop
        // a selection. Spending it on the parked point would undo, several
        // times a session and without being asked, the very thing parking it
        // was for. The way back is the grip's double-click, which is aimed at
        // the bar and at nothing else — and, for an operator with no hand on
        // the mouse, `ContextBar::clear_manual`, which is what that
        // double-click calls rather than reimplements.
        if keys.escape {
            if self.toolrail.drag_active() {
                // The rail consumes this Esc to abort its dock drag.
            } else if self.active_tab_mut().paper.cancel_interaction() {
                // An armed order placement or a grabbed order line was
                // dropped; nothing else loses state on this press. Only the
                // active tab can have one in flight — a background tab has
                // no pointer over it to arm or grab with.
            } else if self.surfaces.drawing_chrome.delete_confirm() {
                self.surfaces.drawing_chrome.set_delete_confirm(false);
            } else if self.inline_text_editing().is_some() {
                // A note being typed is its own layer, and it has to be one:
                // egui clears widget focus at the top of the frame Escape
                // arrives on, so by the time this stack runs the editor no
                // longer looks focused — and without a layer of its own the
                // press fell straight through to "drop the selection",
                // taking away the context bar for the note just written.
                self.end_inline_text_edit();
            } else if self.drawing_pane().drawings.draft().is_some() {
                self.drawing_pane_mut().drawings.cancel_draft();
                self.toolrail.arm(Tool::Pointer);
            } else if self.drawing_pane().drawings.selected().is_some() {
                self.drawing_pane_mut().drawings.select(None);
            } else {
                self.toolrail.arm(Tool::Pointer);
            }
        }
        if self.drawing_pane().drawings.draft().is_some() {
            // During placement the delete keys belong to the draft workflow:
            // Backspace steps back one anchor.
            if keys.backspace {
                self.drawing_pane_mut().drawings.remove_last_draft_anchor();
            }
        } else if keys.delete || keys.backspace {
            self.request_delete_selected(now);
        }
        if keys.undo {
            let pane = self.drawing_pane_mut();
            pane.drawings.undo();
            // Undoing a rectangle's placement takes its drawing away from
            // any armed instance without passing through the removal
            // funnel; the sweep keeps a resting bot order from outliving
            // its badge (redo below, and delete-all, share the risk).
            pane.sweep_strategy_orphans();
        }
        if keys.redo {
            let pane = self.drawing_pane_mut();
            pane.drawings.redo();
            pane.sweep_strategy_orphans();
        }
        if keys.lock
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let locked = self.drawing_pane().drawings.items()[index].locked;
            self.drawing_pane_mut()
                .drawings
                .set_selected_locked(!locked);
        }
        if keys.hide
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let hidden = self.drawing_pane().drawings.items()[index].hidden;
            self.drawing_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if keys.duplicate {
            self.duplicate_selected_drawing();
        }
        if (keys.nudge_bars != 0.0 || keys.nudge_px != 0.0)
            && self.drawing_pane().drawings.selected().is_some()
        {
            // Arrows write the same honest chart coordinates a drag does:
            // one bar per horizontal step, one pixel's worth of *that
            // object's own axis* per vertical step. Each press lands as one
            // undo entry. Reading the candles' scale for an object on an
            // indicator band would nudge a CVD level by a quantity of price —
            // a wrong number arriving through the one gesture that exists for
            // precision. Asked of the pane that owns the mark, which for a
            // shared one is not always the focused pane.
            let price_per_px = self.drawing_pane().selected_value_per_px().unwrap_or(0.0);
            self.drawing_pane_mut().drawings.begin_gesture();
            self.drawing_pane_mut()
                .drawings
                .translate_selected(keys.nudge_bars, f64::from(keys.nudge_px) * price_per_px);
            // Same rule as the drag: the instants behind the anchors move
            // with them, or the object's shared twin stays behind.
            self.drawing_pane_mut().retime_selected();
            self.drawing_pane_mut().drawings.commit_gesture();
        }
    }

    /// Record one edit gesture as the single undo entry it earned, on the pane
    /// it started on.
    ///
    /// That pane's tab may have been closed under the gesture, in which case
    /// the object it described is gone with it.
    fn record_drawing_edit(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        index: usize,
        before: drawings::Drawing,
    ) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.pane_mut(side).drawings.record_edit_of(index, before);
        }
    }

    /// Put the caret in a note, on the chart — the one call that opens the
    /// editor, whether a placement, a double click or a script asked for it.
    ///
    /// The surface decides whether the caret is allowed: an object that holds
    /// no words, or a locked one, refuses it, because an editor that opened and
    /// then dropped every keystroke would be worse than none. The store command
    /// and the per-pane stand-down are the host's, so they happen here.
    pub fn begin_inline_text_edit(&mut self, index: usize) -> bool {
        let Self {
            tabs,
            active_tab,
            surfaces,
            ..
        } = self;
        let tab = &tabs[*active_tab];
        let side = tab.drawing_side();
        let Some(drawing) = tab.pane(side).drawings.items().get(index) else {
            return false;
        };
        if !surfaces
            .drawing_chrome
            .begin_inline_text_edit(tab.id, side, index, drawing)
        {
            return false;
        }
        self.drawing_pane_mut().drawings.select(Some(index));
        self.sync_content_editing();
        true
    }

    /// Close the editor, keeping whatever was typed and recording it as the one
    /// edit it was — on the pane the note actually lives on, which is not
    /// necessarily the one in front when it closes.
    fn end_inline_text_edit(&mut self) {
        if let Some(edit) = self.surfaces.drawing_chrome.end_inline_text_edit() {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        self.sync_content_editing();
    }

    /// Tell every pane whether one of its objects is having its content typed
    /// somewhere else on screen, so exactly one object anywhere stands down.
    ///
    /// Every pane, not just the one in front: the flag is what suppresses the
    /// object's own painting, and a pane left holding a stale index would keep
    /// a note invisible for the rest of the session with no way back.
    fn sync_content_editing(&mut self) {
        let editing = self.surfaces.drawing_chrome.content_editing_target();
        for tab in &mut self.tabs {
            let target = editing
                .filter(|(id, _, _)| *id == tab.id)
                .map(|(_, side, index)| (side, index));
            tab.set_content_editing(target);
        }
    }

    /// Which note is being typed on the chart right now — what a second
    /// operator reads to know the keyboard belongs to an object.
    #[must_use]
    pub fn inline_text_editing(&self) -> Option<usize> {
        self.surfaces.drawing_chrome.inline_text_editing()
    }

    /// The rows the object manager lists.
    ///
    /// A row's facts come from the drawing, the pane's band registry and the
    /// tab's layout, and assembling them here is what keeps the manager from
    /// needing all three. Built only while the window is open, like the market
    /// dialog's list of open markets: a dozen short strings once a frame, on a
    /// window that is shut the rest of the session.
    fn drawing_manager_rows(&self) -> Vec<crate::surfaces::drawing_chrome::ManagerRow> {
        let pane = self.drawing_pane();
        let selected = pane.drawings.selected();
        let focused = self.focused_pane();
        pane.drawings
            .items()
            .iter()
            .enumerate()
            .map(
                |(index, drawing)| crate::surfaces::drawing_chrome::ManagerRow {
                    name: drawing.display_label(index),
                    selected: selected == Some(index),
                    locked: drawing.locked,
                    hidden: drawing.hidden,
                    shared: drawing.scope == drawings::DrawingScope::AllCharts,
                    off_series: drawing.off_series,
                    foreign_market: drawing.foreign_market,
                    author: drawing.author.as_ref().map(DrawingAuthor::label),
                    band: focused.band_label(drawing),
                },
            )
            .collect()
    }

    /// Where the selected object is painted, in screen points. The chrome
    /// cannot work this out for itself: it needs the viewport and the price
    /// scale the host owns.
    ///
    /// Two separate answers rather than one pair, because they cost different
    /// things and not every pass wants both — this one walks the object's
    /// anchors through the price scale. Nothing is projected while nothing is
    /// selected, which is every frame of an ordinary session.
    fn selected_drawing_bbox(&self) -> Option<egui::Rect> {
        let pane = self.drawing_pane();
        let index = pane.drawings.selected()?;
        let chart = pane.last_chart_area?;
        self.drawing_bbox_on_screen(chart, index)
    }

    /// What the band the selected object lives on is called, for the
    /// inspector's title. `None` on the price band, where a suffix on every
    /// object would be noise. Formats a string, so it is asked for only by a
    /// pass that shows the title.
    fn selected_drawing_band(&self) -> Option<String> {
        let pane = self.drawing_pane();
        let index = pane.drawings.selected()?;
        self.focused_pane()
            .band_label(pane.drawings.items().get(index)?)
            .chip()
    }

    /// Carry out what the drawing chrome asked for.
    ///
    /// One applier for all four pieces, in the order the trunk applied them
    /// when each drew itself: the edit lands before the gesture that coalesces
    /// it is committed, and both land before anything that can delete the
    /// object they describe.
    fn apply_drawing_chrome(
        &mut self,
        ask: crate::surfaces::drawing_chrome::DrawingChromeAsk,
        now: Instant,
    ) {
        if let Some(edited) = ask.edited {
            // Through the selection, never `items_mut`: that hatch is
            // documented for derived-state refresh only, and a style or a
            // note's words take part in payload equality — writing them through
            // it would let an unrelated in-flight gesture swallow this edit.
            if let Some(drawing) = self.drawing_pane_mut().drawings.selected_mut() {
                *drawing = *edited;
            }
        }
        if let Some(edit) = ask.commit_edit_gesture {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        if ask.toggle_selected_hidden
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let hidden = self.drawing_pane().drawings.items()[index].hidden;
            self.drawing_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if ask.toggle_selected_locked
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let locked = self.drawing_pane().drawings.items()[index].locked;
            self.drawing_pane_mut()
                .drawings
                .set_selected_locked(!locked);
        }
        if ask.request_delete {
            self.request_delete_selected(now);
        }
        if ask.cancel_delete {
            // After the request, never before: a frame carrying both must end
            // with the prompt gone, and a delete on a locked object raises it.
            self.surfaces.drawing_chrome.set_delete_confirm(false);
        }
        if ask.force_delete {
            let doomed = {
                let pane = self.drawing_pane();
                pane.drawings
                    .selected()
                    .and_then(|index| pane.drawings.items().get(index))
                    .map(|drawing| drawing.id)
            };
            if self.drawing_pane_mut().drawings.delete_selected(true) == DeleteOutcome::Deleted {
                if let Some(id) = doomed {
                    self.drawing_pane_mut().remove_strategy_for_drawing(id);
                }
                self.surfaces.toast.note_with_undo("Drawing deleted.", now);
            }
        }
        if ask.duplicate {
            self.duplicate_selected_drawing();
        }
        if let Some(saved) = ask.saved_default {
            // Nothing to undo: this changed a preference, not the chart.
            self.surfaces.toast.note(saved.message(), now);
        }
        for write in ask.presets {
            write.apply_to(&mut self.drawing_presets);
        }
        if ask.sweep_authored {
            let removed = self.remove_every_authored_object();
            if removed > 0 {
                self.surfaces
                    .toast
                    .note_with_undo(format!("{removed} object(s) placed for you removed."), now);
            }
        }
        if ask.delete_all {
            let pane = self.drawing_pane_mut();
            let deleted = pane.drawings.delete_all();
            // Every armed instance just lost its drawing at once; sweep them
            // now so no resting bot order outlives its badge.
            pane.sweep_strategy_orphans();
            if deleted > 0 {
                self.surfaces
                    .toast
                    .note_with_undo("All drawings deleted.", now);
            }
        }
        if let Some(index) = ask
            .manager_select
            .filter(|index| *index < self.drawing_pane().drawings.items().len())
        {
            self.drawing_pane_mut().drawings.select(Some(index));
            // Centre the viewport on the object's bar span.
            let slots = self.drawing_pane().slots();
            if let Some(chart) = self.drawing_pane().last_chart_area {
                let points = &self.drawing_pane().drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    self.drawing_pane_mut()
                        .viewport
                        .center_on_bar(mid, chart.width(), slots);
                }
            }
        }
        // Through `get`, never `[]`: the rows were snapshotted before any of
        // the four pieces drew, and a destructive ask applied above — delete
        // all, the assistant sweep, a confirmed delete — can have shortened
        // the list under an index that was valid when the row was clicked. The
        // store's own setters are already bounds-safe; these two reads were
        // the only raw ones left.
        if let Some(hidden) = ask.manager_toggle_hidden.and_then(|index| {
            Some((
                index,
                self.drawing_pane().drawings.items().get(index)?.hidden,
            ))
        }) {
            self.drawing_pane_mut()
                .drawings
                .set_hidden_at(hidden.0, !hidden.1);
        }
        if let Some(locked) = ask.manager_toggle_locked.and_then(|index| {
            Some((
                index,
                self.drawing_pane().drawings.items().get(index)?.locked,
            ))
        }) {
            self.drawing_pane_mut()
                .drawings
                .set_locked_at(locked.0, !locked.1);
        }
        if let Some(index) = ask.manager_bring_to_front {
            self.drawing_pane_mut().drawings.bring_to_front(index);
        }
        if let Some(index) = ask.manager_delete {
            // The exact same command path as the inspector button and the
            // keyboard: select, then request. Locked rows raise the same
            // confirmation in the inspector.
            self.drawing_pane_mut().drawings.select(Some(index));
            self.request_delete_selected(now);
        }
        if ask.show_all {
            self.drawing_pane_mut().drawings.set_all_hidden(false);
        }
        if ask.unlock_all {
            self.drawing_pane_mut().drawings.set_all_locked(false);
        }
        if ask.place_text_note && self.place_text_note() {
            self.surfaces.drawing_chrome.note_text_note_placed();
        }
        if let Some(edit) = ask.record_inline_edit {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        if ask.content_editing_changed {
            self.sync_content_editing();
        }
        if self.surfaces.drawing_chrome.take_inspector_position_dirty() {
            self.inspector_position_dirty = true;
        }
    }

    /// The docked inspector.
    ///
    /// Its own call site because a `SidePanel` has to be declared *before* the
    /// central canvas — the canvas pays its width, and a panel declared after
    /// it would overlay the chart instead of docking beside it.
    fn draw_pinned_inspector(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.surfaces.drawing_chrome.inspector_pinned() {
            return;
        }
        // No painted bounds: a docked panel has no placement rule to keep
        // clear of the object, so the projection the floating one needs is not
        // gathered here. The band still is — it is in the title.
        let read = DrawingRead {
            selected_band: self.selected_drawing_band(),
            ..DrawingRead::default()
        };
        let ask = self.draw_chrome_pass(ctx, read, false);
        self.apply_drawing_chrome(ask, now);
    }

    /// The four floating pieces, registered after the canvas so they stay in
    /// front of the chart they are anchored to.
    fn draw_drawing_chrome(&mut self, ctx: &egui::Context, now: Instant) {
        let manager_open = self.surfaces.drawing_chrome.manager_open();
        let rows = if manager_open {
            self.drawing_manager_rows()
        } else {
            Vec::new()
        };
        // The band name goes in the inspector's title and nowhere else, so
        // it is formatted only when one of the two inspector hosts is on
        // screen. A selection alone raises the context bar, which never shows
        // it — and `band_label` scans the pane's indicator views and `chip`
        // allocates, every frame, for a value nothing would read.
        let inspector_showing = self.surfaces.drawing_chrome.inspector_open()
            || self.surfaces.drawing_chrome.inspector_pinned();
        let read = DrawingRead {
            selected_bbox: self.selected_drawing_bbox(),
            selected_band: inspector_showing
                .then(|| self.selected_drawing_band())
                .flatten(),
            // Counted only for the window that offers to take them back, and
            // over every tab: an object an assistant placed on another chart
            // still belongs in that count.
            authored_objects: if manager_open {
                Self::authored_object_count(&self.tabs)
            } else {
                0
            },
            manager_rows: &rows,
        };
        let ask = self.draw_chrome_pass(ctx, read, true);
        self.apply_drawing_chrome(ask, now);
    }

    /// One split for both call sites. `floating` picks which of the surface's
    /// two entry points runs.
    fn draw_chrome_pass(
        &mut self,
        ctx: &egui::Context,
        read: DrawingRead<'_>,
        floating: bool,
    ) -> crate::surfaces::drawing_chrome::DrawingChromeAsk {
        // Split into disjoint borrows, like the surface registry above: the
        // chrome is drawn through `&mut` while what it reads is borrowed from
        // the rest of the application.
        let Self {
            surfaces,
            tabs,
            active_tab,
            toolrail,
            drawing_presets,
            ..
        } = self;
        let env = drawing_env(&tabs[*active_tab], toolrail, drawing_presets, read);
        if floating {
            surfaces.drawing_chrome.draw_floating(ctx, &env)
        } else {
            surfaces.drawing_chrome.draw_pinned_panel(ctx, &env)
        }
    }

    /// The `QUANTICK_TEXT_NOTE` hook's other half: place a note in the middle
    /// of the window and open its editor, through the same two calls a click
    /// makes.
    ///
    /// Here rather than in the surface because every line of it is the host's:
    /// where the visible window is, what the tape last closed at, and the saved
    /// defaults a fresh object opens with.
    fn place_text_note(&mut self) -> bool {
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
        else {
            return false;
        };
        let point = {
            let pane = self.drawing_pane();
            let slots = pane.slots();
            if pane.last_chart_area.is_none() || slots == 0 {
                // No laid-out pane yet, and nothing to place against. The ask
                // stands and the next frame tries again.
                return false;
            }
            let close = pane
                .closed_bar(slots.saturating_sub(1))
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .unwrap_or(1.0);
            let centre = pane
                .last_auto_range
                .filter(|(lo, hi)| hi > lo)
                .map_or(close, |(lo, hi)| (lo + hi) / 2.0);
            let visible = DEMO_VISIBLE_SLOTS.min(slots);
            let slot = (slots - visible / 2).min(slots.saturating_sub(1));
            drawings::ChartPoint::at_time(slot as f32 + 0.5, centre, pane.slot_open_time(slot))
        };
        // Through the same door the click path uses, saved defaults and all —
        // and on the same pane every drawing surface reads, so the index the
        // editor opens on is the object this just placed.
        let fresh = drawings::new_drawing_from_defaults(&self.drawing_presets, tool);
        let placed = self.drawing_pane_mut().drawings.place_with(
            tool,
            &drawings::DrawingBand::Price,
            point,
            |_| fresh,
        );
        if placed && let Some(index) = self.drawing_pane().drawings.selected() {
            self.begin_inline_text_edit(index);
        }
        placed
    }

    /// The selected object's screen bounding box, expanded by the anchor
    /// radius — the rectangle the inspector must not cover. Projected on the
    /// focused pane, which is where the selection lives.
    fn drawing_bbox_on_screen(&self, chart: egui::Rect, index: usize) -> Option<egui::Rect> {
        let total = self.drawing_pane().slots();
        let auto = self.drawing_pane().last_auto_range?;
        let scale = self.drawing_pane().price_view.scale(
            auto,
            self.drawing_pane().last_chart_top,
            self.drawing_pane().last_chart_top + self.drawing_pane().last_chart_height,
        );
        let history_right = self
            .drawing_pane()
            .last_lane_divider_x
            .unwrap_or(chart.right());
        let drawing = self.drawing_pane().drawings.items().get(index)?;
        let points =
            self.drawing_pane()
                .projected_drawing_points(drawing, history_right, total, &scale);
        let first = points.first()?;
        let mut bbox = egui::Rect::from_min_max(*first, *first);
        for point in &points {
            bbox.extend_with(*point);
        }
        // What the tool paints, which is not always where its anchors are: a
        // fixed-range profile anchors at one price and covers the axis. Every
        // popup that keeps clear of an object reads this rectangle, so asking
        // the anchors alone is what let a panel land in the middle of a
        // profile while believing it had walked around it.
        let bbox = drawing.tool.painted_bounds(bbox, chart);
        Some(bbox.expand(DRAWING_ANCHOR_RADIUS_PX))
    }

    /// Carry out what the replay interface asked for.
    /// Whether the action reached its destination. Only a transport control
    /// can fail to — see the drop below — and the one caller that gets a
    /// single shot at it (the scripted seek) reads this before spending it.
    fn apply_replay_action(&mut self, action: ReplayAction) -> bool {
        match action {
            ReplayAction::Open(request) => {
                let (tab, config) = self.active_with_config();
                tab.open_replay(config, *request);
                true
            }
            ReplayAction::Close => {
                let (tab, config) = self.active_with_config();
                tab.close_replay(config);
                true
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
                    return false;
                }
                true
            }
        }
    }
}

impl QuantickApp {
    /// Where a scripted right-click should land to reach `pane`'s menu.
    ///
    /// Mid-height, and mid-pane horizontally, off the geometry the draw
    /// published — so the click lands on the canvas rather than on the axis,
    /// the legend or the divider handle. `None` until the pane has drawn once
    /// (no divider yet), and `None` for the tape on a canvas that has none:
    /// there is no tape menu to open where there is no tape.
    fn scripted_context_menu_pos(&self, pane: ContextMenuPane) -> Option<egui::Pos2> {
        let flow = &self.active_tab().flow_pane;
        // The axis's menu lives on the gutter, off the canvas entirely — the
        // draw publishes that band the same way it publishes the divider.
        if pane == ContextMenuPane::Axis {
            return Some(flow.last_price_gutter?.center());
        }
        // The time axis, likewise off the canvas — and its own published band,
        // because the segment past the lane divider is the tape's.
        if pane == ContextMenuPane::Time {
            return Some(flow.last_time_strip?.center());
        }
        let rect = flow.last_chart_rect?;
        let divider = flow.last_lane_divider_x;
        let x = match (pane, divider) {
            (ContextMenuPane::Tape, Some(divider)) => (divider + rect.right()) / 2.0,
            (ContextMenuPane::Tape, None) => return None,
            // Axis and Time returned above; anything else is the candles'
            // canvas.
            (_, Some(divider)) => (rect.left() + divider) / 2.0,
            (_, None) => rect.center().x,
        };
        Some(egui::pos2(x, rect.center().y))
    }

    /// Where `QUANTICK_POINTER` puts the mouse this frame, in window points.
    ///
    /// Resolved against the *drawing* area rather than the whole chart, so a
    /// fraction means a place among the candles whatever share of the canvas
    /// the live lane has taken, and against the flow pane for the same reason
    /// [`Self::scripted_context_menu_pos`] does — one canvas per capture.
    /// `None` until the pane has drawn once: there is no candle area to be a
    /// fraction of before then, and guessing one would park the pointer
    /// somewhere the author did not ask for.
    fn scripted_pointer_pos(&self) -> Option<egui::Pos2> {
        let fraction = self.scripted_pointer?;
        let flow = &self.active_tab().flow_pane;
        let candles = flow.drawing_area(flow.last_chart_rect?);
        Some(egui::pos2(
            candles.left() + fraction.x * candles.width(),
            candles.top() + fraction.y * candles.height(),
        ))
    }

    /// Deliver the parked pointer, every frame it is parked.
    fn push_scripted_pointer(&self, raw_input: &mut egui::RawInput) {
        if let Some(position) = self.scripted_pointer_pos() {
            raw_input.events.push(egui::Event::PointerMoved(position));
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Whatever the debounce was still holding: a level drawn a moment
        // before closing is a level the trader expects back.
        self.flush_layouts();
        if let Some(access) = self.control_access.as_mut() {
            access.shutdown_for_exit();
        }
    }

    /// The one place a scripted run can put a pointer event into the app.
    ///
    /// A context menu opens on a real right-click and on nothing else — there
    /// is no "open the menu" call to reach for, and reaching into egui's menu
    /// state to fake one would be a second activation path that drifts from
    /// the first. So the hook supplies the click itself, on the pane it names,
    /// and every line after that is the code a trader's own click runs.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Second frame: the button comes up where it went down, and the menu
        // that opened on the press stays open.
        if let Some(position) = self.scripted_context_menu_release.take() {
            raw_input.events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });
            return;
        }
        // The menu bar's own button, clicked. A menu is a popup egui owns, so
        // there is no state to set that would not be a second way of opening
        // it; the hook supplies the press, and every line after it is what a
        // trader's click runs. The rect is published by the draw, so the
        // first frame has none — wait for it rather than guess.
        if let Some(position) = self.scripted_menu_release.take() {
            raw_input.events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });
            return;
        }
        if let Some(menu) = self.scripted_menu
            && let Some(position) = match menu {
                ScriptedMenu::Workspace => self.workspace_menu_rect,
                ScriptedMenu::History => self.history_menu_rect,
            }
            .map(|rect| rect.center())
        {
            self.scripted_menu = None;
            self.scripted_menu_release = Some(position);
            raw_input.events.push(egui::Event::PointerMoved(position));
            raw_input.events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            });
            return;
        }
        // The parked pointer, re-delivered — before the menu branch and not
        // inside its `else`, because a menu whose position never resolves
        // returns early for ever. `QUANTICK_CONTEXT_MENU=tape` on a canvas
        // with no lane is exactly that, and it used to take the pointer hook
        // down with it: the capture showed no compass, no crosshair and no
        // hover readout at all, and read as "the compass does not draw"
        // rather than "the menu never opened".
        self.push_scripted_pointer(raw_input);
        let Some(pane) = self.scripted_context_menu else {
            return;
        };
        // The divider is published by the draw, so the first frame has none:
        // wait for it rather than guess where the tape is.
        let Some(position) = self.scripted_context_menu_pos(pane) else {
            return;
        };
        self.scripted_context_menu = None;
        self.scripted_context_menu_release = Some(position);
        raw_input.events.push(egui::Event::PointerMoved(position));
        raw_input.events.push(egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
    }
}

impl QuantickApp {
    /// The scripted view hooks (`QUANTICK_CANDLE_WIDTH`, `QUANTICK_PAN_PX`),
    /// re-applied every frame.
    ///
    /// Every frame rather than once at boot, for two reasons. A pan needs bars
    /// to move over and at boot there are none — repeating it is what makes
    /// `QUANTICK_PAN_PX=-9000` mean "as far left as it goes" whatever the
    /// zoom: each frame pushes, the per-frame clamp holds, and the view settles
    /// on the projection margin.
    ///
    /// And the view is *rebuilt* under both hooks by anything that re-cuts the
    /// series: `ChartPane::reset_series` hands back a fresh `Viewport`, which a
    /// replay autostart does before its first frame. A zoom set at boot was
    /// therefore thrown away, and every scripted capture of a recorded session
    /// photographed the default zoom rather than the one it asked for.
    ///
    /// A run with neither variable set does nothing here.
    fn apply_scripted_view(&mut self) {
        let (width, pan) = (self.scripted_candle_width, self.scripted_pan_px);
        if width.is_none() && pan.is_none() {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        if let Some(px) = width {
            pane.viewport.set_px_per_bar(px);
        }
        let slots = pane.slots();
        if let Some(dx) = pan
            && slots > 0
        {
            pane.viewport.pan_pixels(dx, slots);
        }
    }

    /// The `QUANTICK_LOAD_OLDER` hook: press "+ older" this many times, once
    /// the chart has something to page back from.
    ///
    /// Goes through [`Tab::request_older_history`] — the very function the
    /// toolbar button calls — rather than reaching for the feed command itself,
    /// so a run under this hook exercises the trader's path including its
    /// loading indicator, and cannot drift from it.
    ///
    /// One page per frame at most: the pages are answered asynchronously and
    /// the feed serves one request at a time, so firing them together would
    /// have every page after the first refused and answered empty — a capture
    /// of the drop path rather than of the feature.
    fn apply_load_older(&mut self) {
        let Some((pages, budget)) = self.pending_load_older else {
            return;
        };
        if self.active_tab().flow_pane.slots() == 0 {
            // Nothing charted yet. Wait, but not forever.
            self.pending_load_older = budget.checked_sub(1).map(|left| (pages, left));
            if self.pending_load_older.is_none() {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_AUTOSTART_GAVE_UP",
                    pages,
                    frames_waited = LOAD_OLDER_HOOK_FRAMES,
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER found no bars to page back from"
                );
            }
            return;
        }
        if self.active_tab().loading.is_active(LoadingTask::History) {
            // The previous page is still coming. Asking now would be refused
            // and answered empty, which is not what the hook is for.
            return;
        }
        let (tab, config) = self.active_with_config();
        tab.request_older_history(config);
        self.pending_load_older = (pages > 1).then_some((pages - 1, budget));
    }

    /// The `QUANTICK_HISTORY_NOTE` hook: the sentence a settled reach leaves,
    /// held up over a chart for as long as the hook's budget lasts.
    ///
    /// Re-applied every frame rather than raised once, the way
    /// `QUANTICK_PAN_PX` re-applies its drag — and for a reason a one-shot
    /// could not survive. Switching source clears the note along with the run
    /// that raised it, exactly as it should: a new market has nothing to say
    /// about the last one's press. But a launch under `QUANTICK_REPLAY_AUTOSTART`
    /// *is* a source switch, arriving a second after the first bars, so a note
    /// raised once was swept away before any shutter could open on it.
    ///
    /// Holding it re-raises only while it is absent, so the surface itself is
    /// unchanged — the same sentence, from the same call, in the same lane.
    /// When the budget runs out the note keeps its ordinary
    /// [`crate::tab::HISTORY_NOTE_LINGER`] from the last raise and then leaves
    /// on its own, so even a hooked run photographs a note that expires.
    fn apply_history_note_hook(&mut self) {
        let Some((end, budget)) = self.pending_history_note else {
            return;
        };
        let Some(left) = budget.checked_sub(1) else {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_NOTE_HOOK_RELEASED",
                ending = end.action(),
                frames_held = HISTORY_NOTE_HOOK_FRAMES,
                action = "note_left_to_expire",
                "QUANTICK_HISTORY_NOTE let go of its sentence"
            );
            self.pending_history_note = None;
            return;
        };
        self.pending_history_note = Some((end, left));
        let tab = self.active_tab();
        // Nothing charted yet, or the note is already up: nothing to raise.
        //
        // And never while a request is out. Paired with `QUANTICK_LOAD_OLDER`
        // — which the harness table pairs it with — the press clears the note
        // and sends a real `load_older`, and re-raising here would paint a
        // settled verdict over a request still in flight, with the spinner
        // turning above it. That is the dishonesty this branch removes, and a
        // hook has no business manufacturing it for a capture.
        if tab.flow_pane.slots() == 0
            || tab.history_note().is_some()
            || tab.loading.is_active(LoadingTask::History)
        {
            return;
        }
        // Always `Some`: the hook only ever holds an ending the env read above
        // kept, and it keeps only endings that have words.
        let Some(notice) = end.notice() else {
            return;
        };
        self.active_tab_mut().raise_history_note(notice);
    }

    /// The `QUANTICK_LOAD_OLDER_CANDLES` hook: the history menu's "+ older
    /// candles" entry, pressed without a hand, once per frame at most.
    ///
    /// Same shape and same reasons as [`Self::apply_load_older`], against a
    /// different record: it goes through `Tab::request_older_ohlcv_history`
    /// rather than the feed command, so a run under this hook exercises the
    /// trader's own path; it waits, because there is nothing to reach back
    /// *from* until the opening request has landed; and it gives up rather
    /// than hanging a capture on a venue that never answers.
    fn apply_load_older_candles(&mut self) {
        let Some((spans, budget)) = self.pending_load_older_candles else {
            return;
        };
        let capabilities = self.active_tab().capabilities(&self.config);
        // Waiting costs budget, but a *slower* budget. A span really being
        // fetched is the feature working, and charging it at the same rate as
        // an empty chart would give up around the fourth of the documented
        // thirteen spans. Charging it nothing, though, is how a venue that
        // simply never answers hangs a capture run for the life of the
        // process — which is the exact failure this counter exists to bound,
        // and what the doc above promises it does. So a fetching frame spends
        // one tick of a budget scaled to how long fetching legitimately takes.
        if self
            .active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory)
        {
            self.pending_load_older_candles = budget.checked_sub(1).map(|left| (spans, left));
            if self.pending_load_older_candles.is_none() {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    reason = "venue_never_answered",
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES gave up waiting for a span to arrive"
                );
            }
            return;
        }
        if !self.active_tab().can_load_older_candles(capabilities) {
            // Nothing to reach back *from* yet, or the venue's record starts
            // here. Both are worth waiting a bounded while for, and both end
            // the same way; the log names what the tab held so an operator can
            // tell them apart.
            self.pending_load_older_candles = budget.checked_sub(1).map(|left| (spans, left));
            if self.pending_load_older_candles.is_none() {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = LOAD_OLDER_HOOK_FRAMES,
                    candles_held = self.active_tab().venue_candles_held(),
                    ohlcv_history = capabilities.ohlcv_history,
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES found nothing to reach back from"
                );
            }
            return;
        }
        // Only a request that actually went out costs a *span*. A full command
        // channel is a busy frame, not a span delivered, and counting it as one
        // would quietly shorten the reach the operator asked for — but it still
        // costs a frame of budget, or a permanently saturated channel leaves
        // the hook armed for the life of the process with nothing ever logged.
        if self
            .active_tab_mut()
            .request_older_ohlcv_history(capabilities)
        {
            self.pending_load_older_candles = (spans > 1).then_some((spans - 1, budget));
        } else {
            self.pending_load_older_candles = budget.checked_sub(1).map(|left| (spans, left));
            if self.pending_load_older_candles.is_none() {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    reason = "request_never_queued",
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES could not get a request out"
                );
            }
        }
    }

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
        // Which object ends up selected. Selection is what puts an object's
        // handles on screen, so "show me the channel's handles" is a question
        // no screenshot could answer while only the last-placed tool was ever
        // selected.
        let select_tool = std::env::var("QUANTICK_DRAWINGS_DEMO_SELECT").ok();
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
        let close = pane
            .closed_bar(slots.saturating_sub(1))
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        // Spread the objects across the band the chart is actually showing,
        // never across a fixed percentage of price. A tick chart's window is
        // a few tenths of a percent tall, so a fixed ±0.2 % dropped a third
        // of the demo below the visible range — objects placed off screen
        // photograph as an empty chart, which is the one failure this hook
        // exists to prevent.
        let (centre, band) = pane
            .last_auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or((close, close * DEMO_FALLBACK_BAND_FRACTION), |(lo, hi)| {
                ((lo + hi) / 2.0, hi - lo)
            });
        let mut requested_selection = None;
        for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
            // A freehand tool declares no anchor count, so the demo gives it
            // a short path of its own. A stroke no screenshot can reach is a
            // tool that ships unvalidated.
            let anchors = if tool.freehand() {
                DEMO_FREEHAND_POINTS
            } else {
                tool.required_points()
            };
            for anchor in 0..anchors {
                let slot = (first + index * stride + anchor * span).min(slots.saturating_sub(1));
                let point = drawings::ChartPoint::at_time(
                    slot as f32 + 0.5,
                    centre + (f64::from(anchor as i32) - 1.0) * band * DEMO_ANCHOR_BAND_STEP
                        - (f64::from(index as i32 % 3) - 1.0) * band * DEMO_ROW_BAND_STEP,
                    pane.slot_open_time(slot),
                );
                let mut completed =
                    pane.drawings
                        .place_with(tool, &drawings::DrawingBand::Price, point, |tool| {
                            drawings::NewDrawing {
                                style: tool.default_style(),
                                payload: tool.default_payload(),
                            }
                        });
                // The release, for the tool whose gesture has one.
                if tool.freehand() && anchor + 1 == anchors {
                    completed = pane.drawings.finish_draft();
                }
                // Placement selects what it completed, so this reaches the
                // object just made — no separate index bookkeeping.
                if completed
                    && share
                    && let Some(drawing) = pane.drawings.selected_mut()
                    && drawing.shareable()
                {
                    drawing.scope = drawings::DrawingScope::AllCharts;
                }
                if completed && select_tool.as_deref() == Some(tool.id()) {
                    requested_selection = pane.drawings.selected();
                }
            }
        }
        if let Some(index) = requested_selection {
            pane.drawings.select(Some(index));
            // Selecting an object the viewport is not looking at proves
            // nothing: the handles are on screen or they are not photographed
            // at all. Centre on the object's bar span, the object manager's
            // own "select and centre".
            if let Some(chart) = pane.last_chart_area {
                let points = &pane.drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    pane.viewport.center_on_bar(mid, chart.width(), slots);
                }
            }
        }
        if bands {
            Self::seed_band_demo(pane, first, visible, slots);
        }
        self.carry_inspector_across_selection();
        self.apply_drawing_demo_recut();
    }

    /// The `QUANTICK_DRAWING_DRAFT=<anchors>` hook: the tool armed by
    /// `QUANTICK_DRAWING_TOOL`, part-placed, with the pointer parked where
    /// the next anchor would go.
    ///
    /// The live preview of a half-made object is a surface like any other,
    /// and for a multi-anchor tool it is the *whole* feedback of the gesture:
    /// it is what tells the trader that a drag has fixed the trend line and a
    /// click is owed for the width. It is also the one surface no click-free
    /// launch could reach, because it exists only between two clicks and only
    /// while a pointer is over the chart — so this hook does both halves.
    ///
    /// The anchors go down through `place_with`, the same call the click path
    /// makes; nothing here is a parallel placement path. They straddle the
    /// middle of the visible window so the line they draw runs through the
    /// parked pointer — which is the exact spot the reported defect lived at,
    /// the pointer standing on the trend line it just drew. A screenshot of
    /// this state is a screenshot of the fix.
    ///
    /// Waits for bars, and is consumed once, for the same reasons
    /// [`Self::apply_drawing_demo`] is.
    fn apply_drawing_draft(&mut self) {
        let Some(anchors) = self.pending_drawing_draft else {
            return;
        };
        let Some(tool) = self.toolrail.tool().drawing_tool() else {
            return;
        };
        // Bars first, and a chart rectangle to park a pointer in: both are
        // answers only a drawn frame has.
        let pane = &self.active_tab().flow_pane;
        let slots = pane.slots();
        let (Some(chart), true) = (pane.last_chart_area, slots > 0) else {
            return;
        };
        self.pending_drawing_draft = None;
        let pane = &mut self.active_tab_mut().flow_pane;
        // One short of the count, whatever was asked for: a draft that
        // completed itself is not a draft, and photographs as a finished
        // object rather than as the gesture under test.
        let anchors = anchors.min(tool.required_points().saturating_sub(1));
        let visible = DEMO_VISIBLE_SLOTS.min(slots);
        let first = slots - visible;
        let close = pane
            .closed_bar(slots.saturating_sub(1))
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        let (centre, band) = pane
            .last_auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or((close, close * DEMO_FALLBACK_BAND_FRACTION), |(lo, hi)| {
                ((lo + hi) / 2.0, hi - lo)
            });
        for anchor in 0..anchors {
            // Symmetric about the middle of the window, so the segment the
            // anchors describe has the chart's own centre as its midpoint.
            let offset = (anchor as f32 + 0.5) / anchors as f32 - 0.5;
            let slot = ((first as f32 + visible as f32 * (0.5 + offset * DEMO_DRAFT_SPAN))
                as usize)
                .min(slots.saturating_sub(1));
            let point = drawings::ChartPoint::at_time(
                slot as f32 + 0.5,
                centre + f64::from(offset) * band * DEMO_DRAFT_BAND_SPAN,
                pane.slot_open_time(slot),
            );
            pane.drawings
                .place_with(tool, &drawings::DrawingBand::Price, point, |tool| {
                    drawings::NewDrawing {
                        style: tool.default_style(),
                        payload: tool.default_payload(),
                    }
                });
        }
        // The hand this run does not have. Parked on the line the anchors
        // drew, which is where a real hand is sitting the instant a drag lets
        // go — and where a channel used to be born with no width at all.
        //
        // `QUANTICK_DRAWING_CONSTRAIN=1` presses Shift for it. The levelled
        // draft is a state of its own — a corridor held flat looks different
        // from one following the pointer — and a modifier is the one input a
        // capture run cannot supply any other way.
        // With two anchors down the pointer belongs *on* the line they drew —
        // the exact spot the reported defect lived at. With one, that spot is
        // the anchor itself, so it steps aside far enough to leave a rubber
        // band worth looking at.
        let parked = if anchors >= 2 {
            chart.center()
        } else {
            chart.center()
                + egui::vec2(
                    chart.width() * DEMO_DRAFT_POINTER_OFFSET.x,
                    chart.height() * DEMO_DRAFT_POINTER_OFFSET.y,
                )
        };
        pane.parked_hand = Some(pane::ParkedHand {
            position: parked,
            constrain: if std::env::var("QUANTICK_DRAWING_CONSTRAIN")
                .is_ok_and(|value| value == "1")
            {
                drawings::Constrain::Level
            } else {
                drawings::Constrain::Free
            },
        });
    }

    /// The `QUANTICK_VENUE_HISTORY_DEMO` hook: a venue candle prefix in front
    /// of the bars cut from prints, delivered through the very path a feed's
    /// reply takes, so what is photographed is the real seam and the real
    /// loading state rather than a picture of them.
    ///
    /// `partial` stops one slice short: the prefix is installed and the run is
    /// left open, which is the mid-load frame progressive delivery exists to
    /// produce and the one no capture could otherwise catch.
    fn apply_venue_history_demo(&mut self) {
        /// Candles the venue-history scene installs: enough for the seam and
        /// the divider to read, few enough to stay one screenful of context.
        const DEMO_PREFIX_CANDLES: i64 = 90;
        let Some(demo) = self.pending_venue_history_demo else {
            return;
        };
        let tab = self.active_tab_mut();
        // Wait for bars to sit the prefix in front of: a seam needs both sides.
        if tab.flow_pane.state.bars().len() < 12 {
            return;
        }
        self.pending_venue_history_demo = None;
        let slice = match demo {
            VenueHistoryDemo::Complete => crate::feed::OhlcvSlice::Last { complete: true },
            VenueHistoryDemo::Partial => crate::feed::OhlcvSlice::More,
        };
        self.deliver_synthetic_prefix(DEMO_PREFIX_CANDLES, slice);
    }

    /// Deliver `candles` synthetic venue candles for the minutes immediately
    /// before the first engine bar, so the prefix meets the tape without
    /// overlapping it.
    ///
    /// The wiggle is a fixed function of the minute — the capture has to be
    /// the same picture every run, or a visual diff means nothing. One
    /// generator, shared by every hook that needs a prefix: a second one would
    /// be a second history to keep honest.
    /// Returns whether the candles were delivered: without a bar to sit them
    /// in front of there is no seam to anchor on, and a caller that promised a
    /// long history in its caption had better wait rather than photograph a
    /// short one.
    fn deliver_synthetic_prefix(&mut self, candles: i64, slice: crate::feed::OhlcvSlice) -> bool {
        let tab = self.active_tab_mut();
        let Some(first) = tab.flow_pane.state.bars().first() else {
            return false;
        };
        let (first_open, anchor) = (first.open_time, first.open);
        let interval = crate::feed::OHLCV_BASE_INTERVAL_MS;
        let bars: Vec<quantick_engine::Bar> = (-candles..0)
            .map(|minute| {
                let open_time = first_open + minute * interval;
                let drift = rust_decimal::Decimal::from(minute.rem_euclid(7) - 3);
                let open = anchor + drift;
                quantick_engine::Bar {
                    open_time,
                    close_time: open_time + interval - 1,
                    open,
                    high: open + rust_decimal::Decimal::from(2),
                    low: open - rust_decimal::Decimal::from(2),
                    close: open + rust_decimal::Decimal::from(minute.rem_euclid(3) - 1),
                    buy_volume: rust_decimal::Decimal::from(2),
                    sell_volume: rust_decimal::Decimal::from(3),
                    trade_count: 7,
                }
            })
            .collect();
        tab.deliver_ohlcv_slice(interval, bars, slice);
        true
    }

    /// Play the alarm cues every tab's armed instances asked for this
    /// frame, and empty their queues.
    ///
    /// Every tab, not only the active one: a tab the trader is not looking
    /// at keeps its feed running and its instances judging, and an alarm
    /// exists precisely to be heard when the eyes are elsewhere.
    ///
    /// One cue per *distinct* cue per frame, across every tab. The kernel's
    /// repeat rule has already thinned each instance's stream to one per
    /// bar (or one per cooldown); this is the second, blunter guard, for
    /// the frame that ingested a burst of prints and closed several bars at
    /// once — four identical beeps stacked into one instant are one noise,
    /// not four alarms.
    ///
    /// Deduplicating by *cue* rather than collapsing to one is the whole
    /// point of letting a preset choose a sound: a trader who gave two
    /// regions two sounds did it to tell them apart, and swallowing the
    /// second because it shared a frame with the first would hide a signal
    /// and leave no trace that it had. The frame's cues go to the sink as
    /// one batch, which plays them in order — so the second is heard after
    /// the first rather than instead of it. The set of sounds is small and
    /// fixed, so this is bounded by the catalogue however busy the tape.
    ///
    /// Per frame, but cheap: the walk is over a handful of tabs whose
    /// queues are empty on every frame but the one a signal happened on,
    /// and the sink is only asked when something is queued.
    fn play_pending_alarms(&mut self) {
        // Order of first request, duplicates dropped — `dedup` alone would
        // only collapse neighbours.
        let mut distinct: Vec<crate::audio::Cue> = Vec::new();
        for tab in &mut self.tabs {
            for cue in tab.pending_alarm_sounds.drain(..) {
                if !distinct.contains(&cue) {
                    distinct.push(cue);
                }
            }
        }
        if distinct.is_empty() {
            return;
        }
        let outcome = self.alerts.play(&distinct);
        self.report_alert_attempt(outcome);
    }

    /// Record whether a sound actually reached the trader.
    ///
    /// A notification that never arrived is reported, never assumed — so a
    /// first failure raises a toast rather than waiting for the trader to
    /// reopen the arming dialog, which they may never do. Only the *first*
    /// of a run: a build with no audio backend fails on every alarm, and a
    /// toast per bar would be its own noise. A success clears the reason, so
    /// one transient refusal does not leave a permanent red line behind it.
    fn report_alert_attempt(&mut self, outcome: Result<(), &'static str>) {
        match outcome {
            Ok(()) => self.alert_failure = None,
            Err(reason) => {
                let first = self.alert_failure.as_deref() != Some(reason);
                self.alert_failure = Some(reason.to_owned());
                if first {
                    self.show_agent_toast(format!("no alarm sound was played: {reason}"));
                }
            }
        }
    }

    /// Arm one instance on a drawing: compile the form, warm the trigger on
    /// the bars already closed (gates shut, so nothing fires from history),
    /// attach it, and start the paper host listening. `Err` carries the
    /// human-readable refusal for the dialog to show.
    /// Duplicate the selected drawing with everything riding it.
    ///
    /// The one door, because a duplication is not only a copied mark: an
    /// armed strategy rides the drawing today and whatever docks next will
    /// ride it too. Two call sites — the hotkey and the context bar — each
    /// spelling out "copy, then carry" is a third one that copies and
    /// forgets, and a band that silently loses its bot is exactly the class
    /// of silence this change exists to end.
    ///
    /// Rate: rare — one keystroke or one click.
    fn duplicate_selected_drawing(&mut self) {
        let side = self.active_tab().drawing_side();
        let Some(duplicated) = self
            .drawing_pane_mut()
            .drawings
            .duplicate_selected(DUPLICATE_OFFSET_BARS)
        else {
            return;
        };
        self.carry_strategy_to_duplicate(side, duplicated);
    }

    /// Carry an armed strategy across a duplication.
    ///
    /// A copied region is a region the trader wants watched the same way —
    /// duplicating the band and then re-typing the form is a step that only
    /// exists because the copy forgot. The copy is armed through
    /// [`Self::arm_strategy_instance`], the same door the dialog uses, from
    /// the stored form the source kept: one construction path, so a copy
    /// cannot quietly differ from what the dialog would have built.
    ///
    /// **Only a watching instance travels.** A source that is `Done`, or
    /// disarmed for any reason, was stopped — by the trader's own hand, by
    /// a rejected entry, by a spent one shot — and a copy that springs back
    /// to life places orders the trader last said no to. The copy lands
    /// offset to the *right*, the direction that makes a dead span live
    /// again, so Ctrl+D would otherwise be the one gesture that silently
    /// revives what was deliberately stopped.
    ///
    /// **State does not travel either.** The copy starts `Armed`, with a
    /// fresh ruler warmed on this pane's own bars, a fresh alarm (no
    /// inherited cooldown, no inherited preview mark) and no order id.
    /// Cloning a `Fired` instance would hang a second badge on one order.
    ///
    /// A refusal is reported rather than swallowed: `duplicate_selected`
    /// clones `hidden`, `off_series` and `foreign_market` verbatim — only
    /// `locked` is reset — and arming refuses all three. A trader who
    /// pressed hide-all and then Ctrl+D would otherwise unhide to two
    /// identical bands wearing one badge and believe both were watching.
    ///
    /// Rate: rare — one keystroke.
    fn carry_strategy_to_duplicate(
        &mut self,
        side: pane::PaneSide,
        duplicated: drawings::Duplicated,
    ) {
        use quantick_strategy::ArmedState;
        let Some((spec, label)) = self
            .active_tab()
            .pane(side)
            .strategies
            .for_drawing(duplicated.source)
            .filter(|instance| {
                matches!(
                    instance.armed.state(),
                    ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition
                )
            })
            .map(|instance| (instance.spec.clone(), instance.preset.clone()))
        else {
            return;
        };
        if let Err(reason) = self.arm_strategy_instance(side, duplicated.copy, &spec, label) {
            self.note_workspace(format!("the copy carries no strategy: {reason}"));
        }
    }

    fn arm_strategy_instance(
        &mut self,
        side: pane::PaneSide,
        drawing: drawings::DrawingId,
        form: &crate::strategy_presets::StoredPreset,
        preset_label: String,
    ) -> Result<(), String> {
        let Some(compiled) = form.to_kernel() else {
            return Err(
                "a field does not parse: quantity, factors and multipliers must be numbers, \
                 and an instance that neither trades nor alarms cannot be armed"
                    .to_owned(),
            );
        };
        let crate::strategy_presets::CompiledPreset {
            params,
            force,
            alarm,
        } = compiled;
        let tab = self.active_tab_mut();
        let replaced_cleanup = {
            let pane = tab.pane_mut(side);
            // Re-validate everything the menu's gate promised: this is also
            // the seam a future programmatic caller (the NL layer) comes
            // through, and it must not be able to arm what the menu would
            // refuse — the wrong shape, another band, a drawing with no
            // footing here, or one nobody can see.
            let Some(index) = pane.drawings.index_of(drawing) else {
                return Err("the drawing is gone".to_owned());
            };
            let target = &pane.drawings.items()[index];
            if target.tool.id() != drawings::RECTANGLE_TOOL_ID
                || target.band != drawings::DrawingBand::Price
                || target.points.len() != 2
            {
                return Err("only price-band rectangles carry strategies".to_owned());
            }
            if target.foreign_market || target.off_series {
                return Err(
                    "this drawing belongs to another market or lost its series — redraw the \
                     region here first"
                        .to_owned(),
                );
            }
            if target.hidden || pane.drawings.all_hidden() {
                return Err("unhide the drawing first — an armed region stays visible".to_owned());
            }
            // A region whose drawn span can no longer cover a future bar
            // can never fire: the badge would show "armed" over a bot that
            // is structurally done — the silent halt the named disarms
            // exist to prevent. One predicate, shared with re-arm and the
            // evaluation sweep (`Pane::strategy_region_can_fire`), refuses
            // it with the fix in hand.
            if !pane.strategy_region_can_fire(drawing) {
                return Err(
                    "the region ends before the next bar, so nothing can ever fire — \
                     stretch it past the right edge, or turn on \"extend right\" in its \
                     Region settings"
                        .to_owned(),
                );
            }
            let mut armed = quantick_strategy::ArmedStrategy::new(
                params,
                Box::new(quantick_strategy::ForceTrigger::new(force.clone())),
            );
            // Warm the ruler on the bars the chart is already showing —
            // armed means armed now, not after another twenty bars of
            // warmup the trader cannot see the reason for. The trigger
            // declares its own depth (`warmup_bars`), and the pane keeps
            // venue-prefix candles out: they measure another ruler
            // entirely (a 1-minute body dwarfs a tick-bar body).
            armed.warm(&pane.strategy_warmup_bars(armed.trigger().warmup_bars()));
            pane.strategies
                .arm(crate::strategy_anchors::AnchoredInstance {
                    drawing,
                    preset: preset_label,
                    spec: form.clone(),
                    armed,
                    alarm: alarm.map(|setup| quantick_strategy::SignalAlarm::new(setup.params)),
                    cue: alarm.map(|setup| setup.cue).unwrap_or_default(),
                    mark: crate::strategy_anchors::AlarmMark::Quiet,
                })
        };
        for command in replaced_cleanup {
            // Arming over an instance with a pending entry sweeps that
            // entry — a resting order must never outlive its bot.
            let _ = tab.paper.apply_strategy_command(command);
        }
        tab.paper.set_bot_listening(true);
        // Only now, past every gate: the sink opens its device at arm time
        // so the first signal does not pay for it on the tape's path, but a
        // *refused* arm must open nothing. Ctrl+D over a band the copy
        // cannot be armed on discards the `Err` by design — the absent
        // badge is the message — and warming above the gates turned that
        // silence into an audio stack enumerated once per keypress, against
        // the sink's own promise that a chart which never arms an alarm
        // never touches a device.
        if let Some(setup) = alarm {
            self.alerts.warm_up(setup.cue);
        }
        Ok(())
    }

    /// The `QUANTICK_REPLAY_RESTART_AFTER` hook: press the transport's own
    /// Restart once the session has closed that many round trips.
    ///
    /// The seek is the only way to put a closed trade ahead of the tape the
    /// chart holds — the recording starts over, the round trips stay in the
    /// ledger because they happened, and their fills are now at instants no
    /// bar on screen covers. That is the state the marks used to stack on
    /// the edge bar in, and it takes a click on a transport button a
    /// scripted capture cannot make. Nothing happens without a recording
    /// playing: there is no timeline to seek on a live feed.
    ///
    /// Consumed once, whether or not the trades ever arrived — an env var
    /// is a request for this run, not a standing rule.
    fn apply_replay_restart(&mut self) {
        let Some(after) = self.pending_replay_restart else {
            return;
        };
        let tab = self.active_tab();
        if tab.replay.is_none() || tab.paper.session_trades().len() < after {
            return;
        }
        // Spent only once the transport took it. A hook that cleared itself
        // on a dropped command would leave the capture photographing an
        // un-seeked timeline while the harness believed otherwise; the next
        // frame simply tries again.
        if self.apply_replay_action(ReplayAction::Control(ReplayControl::Restart)) {
            self.pending_replay_restart = None;
        }
    }

    /// The `QUANTICK_STRATEGY_DEMO` hook: a named rectangle over the recent
    /// tape with a force-bar instance armed on it (`1`), the arming dialog
    /// open over it (`popup`), that dialog with the **alarm section
    /// unfolded** (`alarm`), or an **alarm-only** instance wearing a
    /// standing preview mark (`alarm-badge`). The rectangle spans the
    /// visible middle of the chart so `QUANTICK_CONTEXT_MENU=chart`'s centre
    /// click lands on it and opens the per-drawing menu. Consumed once the
    /// chart has bars enough, like the drawings demo.
    fn apply_strategy_demo(&mut self) {
        let Some(mode) = self.pending_strategy_demo else {
            return;
        };
        /// Fewest bars before the demo stages: enough for the shipped
        /// 20-body window to be warm and the rectangle to have tape to span.
        const DEMO_STRATEGY_MIN_SLOTS: usize = 25;
        /// How far back of the newest bar the rectangle's left edge sits.
        const DEMO_STRATEGY_LOOKBACK_BARS: usize = 40;
        /// How far past the newest bar its right edge reaches, keeping the
        /// region alive into the future like a stretched hand-drawn one.
        const DEMO_STRATEGY_AHEAD_BARS: f32 = 6.0;
        /// Half-height of the region, as a fraction of the newest close.
        const DEMO_STRATEGY_BAND_FRACTION: f64 = 0.03;
        // The newest *closed* bar anchors the demo: `slots()` counts the
        // forming partial too, whose `closed_bar` is `None` on almost every
        // frame — bailing on it must keep the flag armed for the next
        // frame, or the hook silently stages nothing.
        let closed = self.active_tab_mut().flow_pane.closed_slots();
        if closed < DEMO_STRATEGY_MIN_SLOTS {
            return;
        }
        let Some(rectangle) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        else {
            // No rectangle in the registry: staging can never succeed, so
            // the flag is consumed rather than retried forever.
            self.pending_strategy_demo = None;
            return;
        };
        let drawing_id = {
            let pane = &mut self.active_tab_mut().flow_pane;
            let newest = closed - 1;
            let Some(close) = pane
                .closed_bar(newest)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            else {
                return;
            };
            let start = newest.saturating_sub(DEMO_STRATEGY_LOOKBACK_BARS);
            #[allow(clippy::cast_precision_loss)]
            let anchors = [
                drawings::ChartPoint::at_time(
                    start as f32,
                    close * (1.0 - DEMO_STRATEGY_BAND_FRACTION),
                    pane.slot_open_time(start),
                ),
                // Past the newest bar no market time exists to name; the
                // anchor carries none, like a hand-dropped one would.
                drawings::ChartPoint::at_time(
                    newest as f32 + DEMO_STRATEGY_AHEAD_BARS,
                    close * (1.0 + DEMO_STRATEGY_BAND_FRACTION),
                    None,
                ),
            ];
            for point in anchors {
                pane.drawings
                    .place_with(rectangle, &drawings::DrawingBand::Price, point, |tool| {
                        drawings::NewDrawing {
                            style: drawings::DrawingStyle::default(),
                            payload: tool.default_payload(),
                        }
                    });
            }
            let index = pane.drawings.items().len().saturating_sub(1);
            pane.drawings.rename_at(index, "demo região");
            pane.drawings.items()[index].id
        };
        // Staged: the rectangle exists, so the hook is consumed.
        self.pending_strategy_demo = None;
        let mut form =
            crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
        // The alarm scenes tick the checkbox the trader would tick, and the
        // share gate under it, so the section is unfolded with every control
        // it owns on screen.
        if matches!(
            mode,
            StrategyDemoMode::AlarmPopup
                | StrategyDemoMode::AlarmSounds
                | StrategyDemoMode::AlarmBadge
        ) {
            form.alarm = true;
            form.alarm_when = "share".to_owned();
            form.alarm_repeat = "cooldown".to_owned();
            // A library clip with a cut, so the row that exists only for a
            // clip is on screen too. The first standard clip, whatever it
            // is called: a name here would go stale the day a clip is
            // renamed, and the scene would silently photograph the
            // system-sound caveat instead.
            let clip = crate::audio::AlertSound::in_category(crate::audio::SoundCategory::Standard)
                .next()
                .expect("the shipped library has a standard clip");
            form.alarm_sound = clip.token().to_owned();
            form.alarm_play_secs = Some(crate::strategy_presets::DEFAULT_ALARM_PLAY_SECS);
        }
        match mode {
            StrategyDemoMode::Armed => {
                let _ = self.arm_strategy_instance(
                    pane::PaneSide::Flow,
                    drawing_id,
                    &form,
                    "demo BF".to_owned(),
                );
            }
            StrategyDemoMode::AlarmBadge => {
                form.alarm_only = true;
                let _ = self.arm_strategy_instance(
                    pane::PaneSide::Flow,
                    drawing_id,
                    &form,
                    "demo alarm".to_owned(),
                );
                // Stand a provisional judgement on the badge. The mark is
                // the surface under test, and the tape reaches it only when
                // a force bar happens to be half-formed — so the scene
                // stages the mark itself rather than waiting for a market
                // that may not oblige before the shutter.
                let pane = self.active_tab_mut().pane_mut(pane::PaneSide::Flow);
                if let Some(instance) = pane.strategies.for_drawing_mut(drawing_id) {
                    instance.mark = crate::strategy_anchors::AlarmMark::Preview;
                }
                // Placing a drawing selects it, and a selected drawing raises
                // the context bar across its own top edge — which is where
                // the badge this scene exists to photograph sits. Drop the
                // selection so the badge is the thing on screen.
                pane.drawings.select(None);
            }
            StrategyDemoMode::EndedBadge | StrategyDemoMode::PausedBadge => {
                let _ = self.arm_strategy_instance(
                    pane::PaneSide::Flow,
                    drawing_id,
                    &form,
                    "demo BF".to_owned(),
                );
                let pane = self.active_tab_mut().pane_mut(pane::PaneSide::Flow);
                if mode == StrategyDemoMode::EndedBadge {
                    // End the span the way the trader does — by moving the
                    // rectangle — rather than by stamping a state on. The
                    // shared demo band reaches `DEMO_STRATEGY_AHEAD_BARS`
                    // past the newest bar, which is why arming accepted it;
                    // pulling both anchors behind the tape is the drag that
                    // ends a region. Faking the state instead would
                    // photograph the words over a band that can still fire,
                    // and a reviewer would sign off on a scene the
                    // application cannot produce.
                    if let Some(index) = pane.drawings.index_of(drawing_id) {
                        #[allow(clippy::cast_precision_loss)]
                        let behind = closed.saturating_sub(2) as f32;
                        for point in &mut pane.drawings.items_mut()[index].points {
                            point.bar = point.bar.min(behind);
                        }
                    }
                } else if let Some(index) = pane.drawings.index_of(drawing_id) {
                    // A re-cut stranding an anchor is what sets this on a
                    // real chart; nothing scripted can provoke one on cue.
                    pane.drawings.items_mut()[index].off_series = true;
                }
                // As with the alarm badge: a selected drawing raises the
                // context bar across the very edge the badge sits on.
                pane.drawings.select(None);
            }
            StrategyDemoMode::Popup
            | StrategyDemoMode::AlarmPopup
            | StrategyDemoMode::AlarmSounds => {
                if mode == StrategyDemoMode::AlarmSounds {
                    self.surfaces.strategy_popup.stage_sound_picker();
                }
                let tab = self.active_tab().id;
                self.surfaces
                    .strategy_popup
                    .open(tab, pane::PaneSide::Flow, drawing_id, form);
            }
        }
        // This demo places its rectangle, which selects it, which closes a
        // panel the launch may have asked for. Same door as the others.
        self.carry_inspector_across_selection();
    }

    /// The `QUANTICK_FRVP_DEMO` hook: one fixed-range volume profile on the
    /// flow pane. When the pane carries a venue history prefix the range
    /// starts inside it, so the partial-coverage honesty label ("profile
    /// from N of M bars") is on screen — the surface this hook exists to
    /// photograph. Consumed once, like the drawings demo.
    fn apply_frvp_demo(&mut self) {
        /// One-minute venue candles the `=stress` scene installs behind the
        /// tape — a time chart's worth of history, and far more than one fold
        /// pass spends, so the range folds across frames rather than in the one
        /// that placed it. The time pane folds them to whatever interval it is
        /// showing, so the slot count is this number only at 1m — the scene is
        /// about the fold surviving a long history, not about an exact count.
        const FRVP_STRESS_CANDLES: i64 = 25_000;
        if !self.pending_frvp_demo {
            return;
        }
        // `=compare` places two adjacent profiles over the same stretch of
        // map, one in each over-heatmap mode — the before/after of the
        // silhouette decision in a single frame, which no toggle-and-wait
        // pair of screenshots can prove as cleanly.
        let mode = std::env::var("QUANTICK_FRVP_DEMO").unwrap_or_default();
        let compare = mode.trim() == "compare";
        let stress = mode.trim() == "stress";
        // The venue's candles land on the **time** pane — that is the chart
        // whose history runs to tens of thousands of bars, and the one a
        // trader drops a session profile on. Every other scene stays on the
        // flow pane, where the tape and the map are.
        let side = if stress {
            pane::PaneSide::Time(0)
        } else {
            pane::PaneSide::Flow
        };
        // The time pane is built the first time the split is shown, and
        // `pane_mut` answers with the flow pane until it is. Waiting is the
        // only honest option here: a stress scene photographed on the tape
        // pane would be a picture of twenty bars captioned as twenty-five
        // thousand.
        if stress && self.active_tab_mut().time_panes.is_empty() {
            return;
        }
        let slots = self.active_tab_mut().pane_mut(side).slots();
        if slots < 12 {
            return;
        }
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        else {
            self.pending_frvp_demo = false;
            return;
        };
        // `=stress` is the scene this object used to freeze the app on: a
        // venue history longer than any single fold pass, with one profile
        // over the whole of it. What it photographs is the *filling* state —
        // a partial histogram and its `loading N of M bars` line — which is
        // only a state at all because the fold is resumable. Pair it with
        // QUANTICK_FRVP_FOLD_BUDGET=1 to hold that frame indefinitely.
        //
        // The delivery has to land before the range is placed: a scene that
        // drew "the whole chart" over the handful of bars a pane starts with
        // would be captioned as twenty-five thousand and be a picture of
        // twenty. It stays armed and tries again next frame instead.
        if stress
            && !self.deliver_synthetic_prefix(
                FRVP_STRESS_CANDLES,
                crate::feed::OhlcvSlice::Last { complete: true },
            )
        {
            return;
        }
        self.pending_frvp_demo = false;
        // Re-read after the delivery: the prefix that just landed is part of
        // the chart the range is about to span.
        let slots = self.active_tab_mut().pane_mut(side).slots();
        let prefix = self.active_tab_mut().pane_mut(side).history_prefix.len();
        // The compare scene exists to photograph profiles *over the map*, and
        // the map only covers bars closed after book capture began — the
        // session's earliest bars never get cells. So it waits for enough
        // freshly-built bars and anchors on those, where the coverage is.
        if compare && slots.saturating_sub(prefix) < 60 {
            self.pending_frvp_demo = true;
            return;
        }
        let pane = self.active_tab_mut().pane_mut(side);
        // Straddle the seam when there is one; else the newest stretch.
        let start = if prefix > 0 && prefix < slots {
            prefix.saturating_sub(5)
        } else {
            slots.saturating_sub(30)
        };
        let end = (start + 29).min(slots - 1);
        let close = pane
            .closed_bar(end)
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        let newest = slots - 1;
        let ranges: &[(usize, usize, bool)] = if stress {
            // The whole chart, oldest slot to newest: the range a trader
            // drags when they want the session's own volume-by-price, and the
            // one that used to take the window with it.
            &[(0, newest, true)]
        } else if compare {
            // Two adjacent 25-bar ranges over the newest (map-covered) tape.
            // Left object keeps the honest default; right one is forced to
            // "always fill", the composed-into-the-map look under review.
            &[
                (newest.saturating_sub(49), newest.saturating_sub(25), true),
                (newest.saturating_sub(24), newest, false),
            ]
        } else {
            &[(start, end, true)]
        };
        // Selecting what the demo drew is what makes the *context bar* — the
        // strip a trader edits a profile from — reachable without a click.
        // Without it a validation run can photograph a profile but never the
        // controls over it, which is exactly the surface whose placement this
        // change is about (`ui-harness`: every surface owes a hook).
        let select = std::env::var("QUANTICK_FRVP_DEMO_SELECT").is_ok_and(|v| v.trim() == "1");
        for &(from, to, outline) in ranges {
            for slot in [from, to] {
                pane.drawings.place_with(
                    tool,
                    &drawings::DrawingBand::Price,
                    // On the slot's centre, where a trader's click lands —
                    // anchoring on its trailing edge drew a box one candle
                    // wider than the range it folded.
                    drawings::ChartPoint::at_time(slot as f32, close, pane.slot_open_time(slot)),
                    |tool| {
                        let mut payload = tool.default_payload();
                        if let Some(frvp) =
                            payload.as_any_mut().downcast_mut::<drawings::FrvpPayload>()
                        {
                            frvp.outline_over_heatmap = outline;
                        }
                        drawings::NewDrawing {
                            style: drawings::DrawingStyle::default(),
                            payload,
                        }
                    },
                );
            }
            if select {
                let last = pane.drawings.items().len().saturating_sub(1);
                pane.drawings.select(Some(last));
            }
        }
        if select {
            self.carry_inspector_across_selection();
        }
    }

    /// The `QUANTICK_AVWAP_DEMO` hook: one anchored VWAP on the flow pane,
    /// anchored a stretch back with the first two band pairs on — the band
    /// stack, the fills and the anchor marker in a single deterministic
    /// frame. Consumed once, like the drawings demo.
    fn apply_avwap_demo(&mut self) {
        /// Fewest bars the demo waits for before anchoring — enough for the
        /// average and one band pair to visibly develop.
        const DEMO_AVWAP_MIN_SLOTS: usize = 12;
        /// How far back of the newest bar the demo drops its anchor.
        const DEMO_AVWAP_LOOKBACK_BARS: usize = 40;
        if !self.pending_avwap_demo {
            return;
        }
        let slots = self.active_tab_mut().flow_pane.slots();
        if slots < DEMO_AVWAP_MIN_SLOTS {
            return;
        }
        self.pending_avwap_demo = false;
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == crate::avwap::TOOL_ID)
        else {
            return;
        };
        let pane = &mut self.active_tab_mut().flow_pane;
        // Far enough back for the average to develop, on a bar that exists.
        let anchor = slots
            .saturating_sub(DEMO_AVWAP_LOOKBACK_BARS)
            .min(slots - 1);
        let close = pane
            .closed_bar(anchor)
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        pane.drawings.place_with(
            tool,
            &drawings::DrawingBand::Price,
            drawings::ChartPoint::at_time(anchor as f32, close, pane.slot_open_time(anchor)),
            |tool| {
                let mut payload = tool.default_payload();
                if let Some(avwap) = payload
                    .as_any_mut()
                    .downcast_mut::<drawings::AvwapPayload>()
                {
                    // 1σ ships on; the demo also opens 2σ so the stack and
                    // its fill layering are on screen.
                    avwap.bands[1].on = true;
                }
                drawings::NewDrawing {
                    style: tool.default_style(),
                    payload,
                }
            },
        );
        self.carry_inspector_across_selection();
    }

    /// Keep a `QUANTICK_DRAWING_INSPECTOR=1` request alive across a selection
    /// the *app* made, rather than the trader.
    ///
    /// The context bar closes the panel on every selection change, which is
    /// right for a trader clicking from one object to the next and wrong for a
    /// demo hook that places objects and selects one on the frame it runs:
    /// the pairing the harness table prescribes then photographs a chart with
    /// no panel on it. Re-requested through `pending_open_settings`, the door
    /// a tool that asks for its own settings on placement already uses, which
    /// is applied *after* the clear rather than before it.
    ///
    /// One function rather than the line copied into each hook, because the
    /// omission is silent: the demo that forgets it produces a screenshot that
    /// looks merely uninteresting, and that is how three of these hooks came to
    /// disagree about it.
    fn carry_inspector_across_selection(&mut self) {
        self.surfaces.drawing_chrome.carry_across_selection();
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

    /// The `QUANTICK_DRAWINGS_DEMO_RECUT` hook: re-cut the bars under the demo
    /// objects, so a screenshot shows what a timeframe switch does to them.
    ///
    /// It is the only way to reach the two surfaces this behaviour added
    /// without a human touching the BARS selector: marks that survived a
    /// re-cut and are still on their own instants, and a mark the new series
    /// cannot reach, faded and labelled off-series. One extra object is placed
    /// an hour before the first bar to produce the second.
    fn apply_drawing_demo_recut(&mut self) {
        if !std::env::var("QUANTICK_DRAWINGS_DEMO_RECUT").is_ok_and(|value| value == "1") {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        // An anchor before anything the tab has loaded: honest input for the
        // off-series path, not a flag set by hand.
        if let Some(first) = pane.slot_open_time(0) {
            let base = pane
                .closed_bar(0)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .unwrap_or(1.0);
            // A one-anchor tool by name, not `DRAWING_TOOLS[0]` — that is the
            // trend line, which needs two, so a single `place_with` left a
            // half-finished draft and no object at all. The whole point here
            // is to produce one *completed* off-series mark.
            let single_anchor = drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == "horizontal-line");
            if let Some(tool) = single_anchor {
                let placed = pane.drawings.place_with(
                    tool,
                    &drawings::DrawingBand::Price,
                    drawings::ChartPoint::at_time(0.5, base, Some(first - DEMO_OFF_SERIES_LEAD_MS)),
                    |tool| drawings::NewDrawing {
                        style: drawings::DrawingStyle::default(),
                        payload: tool.default_payload(),
                    },
                );
                debug_assert!(placed, "a horizontal line completes on one anchor");
            }
        }
        // Half the bars, same trades — the plainest re-cut there is. Two
        // settle frames because a spec change waits for the selector to hold
        // still for one (`Tab::apply_spec_change`).
        pane.tick_n = pane.tick_n.saturating_mul(2).max(2);
        self.active_tab_mut().apply_spec_changes();
        self.active_tab_mut().apply_spec_changes();
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
        // A "load older" outcome is a passing remark: it leaves after
        // `tab::HISTORY_NOTE_LINGER` whether or not anyone read it. Every tab,
        // not only the one on screen — a background tab keeps draining, so it
        // can settle a run while hidden, and bringing it forward minutes later
        // must not surface a sentence about a press that is long over.
        for tab in &mut self.tabs {
            tab.expire_history_note(now);
        }
        // After the expiry, never before it: the hook re-raises a note it
        // finds absent, and running it first would let a note expire *after*
        // it looked, drawing one frame with an empty lane before the next
        // raise. A shutter timed on the linger catches exactly that frame.
        self.apply_history_note_hook();
        if self.pending_control_access_enable {
            self.pending_control_access_enable = false;
            if let Some(access) = self.control_access.as_mut() {
                access.enable(ctx);
            }
        }
        // Replay determinism: a session with a control trace beside it
        // re-injects its actions at their logical time, connected or not.
        // Before the hook's mark, so a loaded sidecar has seeded the trace
        // sequence the mark will take.
        if let Some(mut access) = self.control_access.take() {
            access.service_replay_trace(self);
            self.control_access = Some(access);
        }
        if let Some(note) = self.pending_control_mark.take() {
            let note = (!note.is_empty()).then_some(note);
            self.take_mark(note);
        }
        self.apply_control_annotate_hooks();
        // After the annotate hooks and before the gateway's own drain: a
        // bundle captured from a launch then describes the window an
        // assistant has already written on, which is the state a validation
        // run is actually asking about.
        self.apply_control_evidence_hook(ctx);
        if self
            .control_access
            .as_ref()
            .is_some_and(crate::control::ControlAccess::needs_frame_service)
            && let Some(mut access) = self.control_access.take()
        {
            access.begin_frame(self, ctx);
            self.control_access = Some(access);
        }
        self.apply_scripted_view();
        self.apply_drawing_demo();
        self.apply_load_older();
        self.apply_load_older_candles();
        self.apply_drawing_draft();
        self.apply_venue_history_demo();
        self.apply_frvp_demo();
        self.apply_avwap_demo();
        self.apply_strategy_demo();
        self.apply_replay_restart();
        self.apply_maximize_hook(ctx);
        self.maybe_emit_summary(now, ctx);
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
        if let Some(access) = self.control_access.as_mut() {
            access.draw_panel(ctx);
        }
        self.draw_toolbar(ctx);
        // Before the dialog is drawn, so a double click on a pane or a curve
        // opens it on the same frame the gesture happened rather than the next.
        self.open_requested_indicator_settings();
        self.draw_indicator_settings(ctx);
        self.draw_indicator_legends(ctx);
        // **After** the dialogs above, and that placement is load-bearing.
        // The preview watermark reads whether a settings dialog is previewing
        // an unapplied draft, and `draw_indicator_settings` is what sets that
        // — so an environment built before it would put the banner on screen
        // a frame after the legend chip that says the same thing, and take it
        // off a frame later too. Two surfaces the trader reads as one is this
        // repo's own bug class; sixteen milliseconds of it is still one
        // frame a capture can photograph.
        //
        // It is also where the windows this pass drew used to sit: the
        // appearance and footprint panels ran after the toolbar that toggles
        // them, so a click on LOOK opens the panel on the same frame rather
        // than the next.
        // A pane's right-click asked to arm one of its drawings. Drained
        // here, into the surface that owns the dialog: the click happens
        // while the canvas draws, which is later in this frame than the
        // surfaces are, so the dialog opens on the next one — a frame the
        // trader cannot see, and the price of the dialog no longer living in
        // the trunk.
        let sides: SmallVec<[pane::PaneSide; MAX_CANVAS_PANES]> =
            self.active_tab().sides().collect();
        for side in sides {
            let request = self
                .active_tab_mut()
                .pane_mut(side)
                .strategy_popup_request
                .take();
            if let Some(drawing) = request {
                let form = crate::strategy_presets::StoredPreset::starting_point(
                    quantick_engine::Side::Buy,
                );
                let tab = self.active_tab().id;
                self.surfaces.strategy_popup.open(tab, side, drawing, form);
            }
        }
        // The bar rules the arming dialog's alarm section reads: a share of
        // the bar only means something where the rule closes on a count.
        // Built only while that dialog is open, like the open markets below.
        // `hooks_pending` is the frame a capture hook opens a surface from
        // inside `draw_all`: it is not open yet when this runs, but it is
        // about to be, and it must not draw its first frame against an empty
        // environment.
        let staging = self.surfaces.hooks_pending();
        let counted_bar_sides: SmallVec<[pane::PaneSide; MAX_CANVAS_PANES]> =
            if staging || self.surfaces.strategy_popup.is_open() {
                let tab = self.active_tab();
                tab.sides()
                    .filter(|side| tab.pane(*side).state.progress().is_some())
                    .collect()
            } else {
                SmallVec::new()
            };
        // The markets tabs are showing: the dialog greys out removing one of
        // those, because a tab left on a symbol the catalog no longer offers
        // gets silently retargeted by the next SOURCE correction. Built only
        // while the dialog is open — it is a `String` pair per tab, and no
        // frame should pay for it to be thrown away.
        let open_markets: Vec<(String, String)> =
            if staging || self.surfaces.source_picker.is_open() {
                self.tabs
                    .iter()
                    .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
                    .collect()
            } else {
                Vec::new()
            };
        // Split into disjoint borrows: the surfaces are drawn through `&mut`
        // while the environment they read is borrowed from the rest of the
        // application. That the compiler insists on the split is the port
        // working — a surface cannot be handed the trunk it is being kept
        // out of.
        let Self {
            surfaces: registry,
            bookmarks,
            style,
            footprint_config,
            tabs,
            active_tab,
            indicator_settings,
            indicator_settings_target,
            config,
            added_symbols,
            alert_failure,
            ..
        } = self;
        let focused_tab = &tabs[*active_tab];
        // Read once. `focused_pane` resolves the same side internally, and
        // the answer is not a field lookup — it reads the layout, because
        // focus on a collapsed pane is focus on nothing.
        let focused_side = focused_tab.focused_side();
        let focused_pane = focused_tab.pane(focused_side);
        let surfaces = registry.draw_all(
            ctx,
            &crate::surfaces::SurfaceEnv {
                bookmarks,
                now,
                indicator_preview_area: indicator_preview_area(
                    tabs,
                    indicator_settings.as_ref(),
                    *indicator_settings_target,
                ),
                focused_chart_area: focused_pane.last_chart_area,
                style,
                footprint: focused_pane.footprint_config(footprint_config),
                footprint_customized: focused_pane.footprint_override.is_some(),
                focused_side,
                config,
                added_symbols,
                open_markets: &open_markets,
                active_tab: focused_tab.id,
                counted_bar_sides: &counted_bar_sides,
                alert_failure: alert_failure.as_deref(),
            },
        );
        if let Some(name) = surfaces.save_workspace_as {
            self.save_named_workspace(&name);
        }
        if let Some(style) = surfaces.style {
            self.style = style;
            self.style_revision = self.style_revision.saturating_add(1);
        }
        // After the assignment, never before: the log line reports the
        // appearance that is now in force, and the revision it landed on.
        if let Some(request) = surfaces.log_style_change {
            self.emit_style_changed(request.applied_preset);
        }
        // The audition goes through the one speaker every armed instance
        // shares, and reports a sound that could not be heard exactly as a
        // missed signal would.
        if let Some(cue) = surfaces.test_alert {
            let outcome = self.alerts.play(&[cue]);
            self.report_alert_attempt(outcome);
        }
        if let Some(request) = surfaces.arm_strategy {
            let outcome = self.arm_strategy_instance(
                request.side,
                request.drawing,
                &request.form,
                request.label,
            );
            self.surfaces.strategy_popup.settle_arm(outcome);
        }
        if let Some(request) = surfaces.market {
            self.apply_market_request(request);
        }
        if let Some(change) = surfaces.footprint {
            self.apply_footprint_change(change);
        }
        if surfaces.undo_drawing {
            let pane = self.drawing_pane_mut();
            pane.drawings.undo();
            // Same orphan risk as the keyboard undo: the drawing an armed
            // instance rides may just have been taken away.
            pane.sweep_strategy_orphans();
        }
        self.poll_script_files();
        self.maintain_indicator_state();
        self.maintain_chart_layers();
        // This tab's judgement about its own feed, taken once for the frame:
        // the status bar reads it here and the corner reads it below, and two
        // readings a millisecond apart could disagree about whether a budget
        // had run out.
        let stall = self
            .active_tab()
            .stall_at(&self.config, metrics::wall_clock_ms());
        let offline_accent = self.feed_offline_accent(stall.as_ref());
        let status = self.status_model();
        let status_response = statusbar::draw(ctx, &status, &mut self.tz, offline_accent);
        if status_response.open_trading_tab {
            self.dock.open_tab(DockTab::Trading);
        }
        // Above the status bar, below the canvas: the layout tabs.
        self.draw_layout_strip(ctx);
        self.draw_layout_delete_confirm(ctx);
        // The browser window and, while the *active* tab plays a session, its
        // transport bar. A background tab's recording keeps advancing on its
        // own feed thread; what it does not get is the strip, which speaks for
        // one tab at a time (§11).
        let replay_action = {
            let Self {
                replay_view,
                tabs,
                active_tab,
                config,
                ..
            } = self;
            let tab = &tabs[*active_tab];
            // The instruments the download tab offers with one click. A dated
            // contract rolls every couple of months, and typing `WINV26` from
            // memory is not a thing a trader should have to get right to see
            // what they can replay.
            //
            // Filtered by what the download source actually serves, which the
            // source itself answers: offering a Binance pair to a MetaTrader
            // exporter would be a click that can only end in a refusal, and
            // the chart behind this window is often on another venue entirely.
            let serves = replay_view.download_provider();
            let market = crate::replay_view::MarketMenu {
                current: (config.provider_of(&tab.feed_id) == Some(serves))
                    .then_some(tab.symbol.as_str()),
                catalogue: config
                    .feeds
                    .iter()
                    .filter(|feed| feed.provider == serves)
                    .flat_map(|feed| feed.symbols.iter().map(String::as_str))
                    .collect(),
            };
            replay_view.draw(ctx, tab.replay.as_ref(), &market)
        };
        if let Some(action) = replay_action {
            self.apply_replay_action(action);
        }
        // A folder the trader just pointed the browser at is written down on
        // the frame they pointed it, not at exit: "it forgot my folder again"
        // must not be one crash away.
        if let Some(pick) = self.replay_view.take_folder_change() {
            self.write_replay_folder(pick.as_deref());
        }
        // The same, for the tick that decides whether yesterday is on the
        // chart. Either row can have been the one clicked; the browser owns
        // the setting, so there is one place to pick the change up.
        if let Some(enabled) = self.replay_view.take_day_before_change() {
            self.write_replay_day_before(enabled);
        }
        {
            // The focused pane's objects: the toolbox lists and manages what a
            // click on the canvas would act on.
            let side = self.active_tab().focused_side();
            // The flag lives with the window it opens, so it travels through
            // a local rather than a `&mut` handed out of the surface.
            let mut manager_open = self.surfaces.drawing_chrome.manager_open();
            {
                let Self {
                    toolrail,
                    tabs,
                    active_tab,
                    ..
                } = self;
                let tab = &mut tabs[*active_tab];
                toolrail.draw(ctx, &mut tab.pane_mut(side).drawings, &mut manager_open);
            }
            self.surfaces.drawing_chrome.set_manager_open(manager_open);
        }
        // A star clicked this frame is on disk this frame, like the replay
        // folder above: the pinned rail is what the trader reaches for without
        // looking, and rebuilding it after a crash is not a thing anyone
        // should have to do twice.
        if self.toolrail.take_favorites_change() {
            self.write_favorites();
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
        // The strategy editor is a window of the active tab's ticket, drawn
        // whatever the dock is showing and whether it is showing at all: it
        // is opened from the Trading tab but it does not belong to it, and a
        // trader who opens it and then looks at the ledger has not asked for
        // it to close.
        if self.active_tab_mut().paper.draw_strategy_editor(ctx) {
            self.persist_order_strategies();
        }
        if dock_response.restart_book_capture {
            self.active_tab_mut().restart_book_capture();
        }
        if let Some(action) = dock_response.replay_action {
            // A click that lost its slot has the trader's next click behind
            // it; only the one-shot hook below cares about the answer.
            let _ = self.apply_replay_action(action);
        }
        // The ledger's jump-to-trade: center the flow pane on the round
        // trip's midpoint, the object manager's own "select and centre".
        //
        // The covering lookup, the same one the marks are painted through:
        // a trade the flow chart's bars do not reach has nowhere to be
        // centred on, and scrolling to the clamped edge instead would land
        // the trader on a bar holding no mark and no explanation. Saying so
        // is the whole of the handling — the row stays in the ledger.
        //
        // The message names the flow chart rather than "the chart": in a
        // split tab the time pane keeps its own, longer window, so the same
        // round trip can be off this one and painted on that one.
        if let Some((opened, closed)) = dock_response.navigate_to_trade {
            let tab = self.active_tab_mut();
            let covered = tab
                .flow_pane
                .covering_slot_at_time(opened)
                .zip(tab.flow_pane.covering_slot_at_time(closed));
            match covered {
                Some((entry, exit)) => {
                    let pane = &mut tab.flow_pane;
                    if let Some(area) = pane.last_chart_area {
                        let slots = pane.slots();
                        let mid = (entry + exit) as f32 / 2.0;
                        pane.viewport.center_on_bar(mid, area.width(), slots);
                    }
                }
                None => {
                    // Said as an event as well as on screen: an operator
                    // driving the ledger without eyes on the toast must be
                    // able to tell a refusal from a silent no-op.
                    tracing::info!(
                        target: "quantick::app",
                        event_code = "TRADE_NAVIGATE_OFF_TAPE",
                        opened_ms = opened,
                        closed_ms = closed,
                        "jump-to-trade refused: the flow chart has no bar for the fills"
                    );
                    tab.paper.show_toast(
                        "This trade is outside the bars on the flow chart - nothing to centre on."
                            .to_owned(),
                    );
                }
            }
        }
        if dock_response.pick_trades_dir {
            self.open_trades_dir_picker();
        }
        if dock_response.order_strategies_changed {
            self.persist_order_strategies();
        }
        if dock_response.cmd_trading_changed {
            self.persist_cmd_trading();
        }
        if dock_response.risk_settings_changed {
            self.persist_risk_settings();
        }
        self.poll_trades_dir_picker();
        self.poll_workspace_picker();
        // The pinned inspector is chrome: declared before the central canvas
        // so the chart pays its width, exactly like the dock.
        self.draw_pinned_inspector(ctx, now);
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        let (tab, config) = self.active_with_config();
        tab.maybe_switch_feed(config);
        // Both deferrals settle here, a frame after the click that armed
        // them, so the frame carrying the change paints its overlay first.
        let Self {
            tabs,
            config,
            style,
            pane_ids,
            ..
        } = self;
        for tab in tabs.iter_mut() {
            tab.apply_pending_layout(config, style, pane_ids);
        }
        // Right after panes appear and markets switch, so a pane built this
        // frame is seeded this frame and a tab that changed symbol swaps its
        // drawings before anything paints them.
        self.maintain_layouts();
        self.active_tab_mut().apply_spec_changes();
        // Waits owned by other components, mirrored level-style each frame so
        // the overlay needs no push notifications from either.
        let replay_loading = self.replay_view.is_loading();
        let book_syncing = self.active_tab().tape().is_syncing();
        let tab = self.active_tab_mut();
        tab.loading
            .set_active(LoadingTask::ReplaySession, replay_loading);
        tab.loading.set_active(LoadingTask::BookSync, book_syncing);

        let mut notice_action = feed_notice::NoticeAction::None;
        // Read before the canvas borrows `self`, and answered after it lets go.
        let popup_tab = self.active_tab().id;
        let popup_open = self.feed_popup_tab == Some(popup_tab);
        let mut chip_clicked = false;
        let mut dismissed = false;
        // Where the corner landed, and so whether there was one at all. A feed
        // that recovered while the popup was open closes it, rather than
        // leaving a stale explanation over a chart that is fine again.
        let mut chip_rect = None;
        // The layer menu offers what this source can produce; resolved once
        // here rather than per pane, per entry, inside the canvas.
        let capabilities = self.active_tab().capabilities(&self.config);
        // Same one-per-frame resolution for the side-honesty label the
        // footprint legend carries.
        let side_inferred = self.active_tab().side_note(&self.config).is_some();
        // Told before the canvas paints, not after: the object holding the
        // words the editor is showing must stand down on the *same* frame,
        // or the note flashes its placeholder under the field for one.
        self.sync_content_editing();
        // Raised by a placement that wants its note typed, and handed to the
        // chrome below: the flag belongs to the editor that owns the caret,
        // not to the canvas that asks for it.
        let mut begin_text_edit = false;
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
                        footprint_config,
                        ..
                    } = self;
                    let mut chrome = CanvasChrome {
                        toolrail,
                        presets: drawing_presets,
                        begin_text_edit: &mut begin_text_edit,
                        style,
                        tz: *tz,
                        capabilities,
                        side_inferred,
                        footprint: footprint_config,
                        layers: layer_actions,
                    };
                    tabs[*active_tab].draw_canvas(ui, area, &mut chrome);
                }
                // The grid and the indicator state belong to the window, not
                // to the pane whose menu switched them.
                self.apply_layer_actions();
                let tab = self.active_tab();
                // Each wait on the surface it is about. The panes published
                // their rects on the draw just above, so these are this
                // frame's geometry rather than the previous one's.
                let history_note = tab.history_note();
                loading::overlay_scoped(ui, area, &tab.loading, LoadingScope::Whole, history_note);
                // A scope whose surface is not on screen falls back to the
                // canvas rather than dropping its wait: the flow pane is not
                // painted in the Time layout, and a flow-only layout has no
                // time pane, and in both cases the wait is still running. A
                // spinner in the wrong place is a placement complaint; a
                // missing one reads as a frozen application.
                loading::overlay_scoped(
                    ui,
                    tab.flow_pane.last_area.unwrap_or(area),
                    &tab.loading,
                    LoadingScope::Flow,
                    history_note,
                );
                let time_panes: Vec<egui::Rect> = tab
                    .panes()
                    .filter(|(_, side)| matches!(side, PaneSide::Time(_)))
                    .filter_map(|(pane, _)| pane.last_area)
                    .collect();
                if time_panes.is_empty() {
                    loading::overlay_scoped(
                        ui,
                        area,
                        &tab.loading,
                        LoadingScope::TimePanes,
                        history_note,
                    );
                } else {
                    for rect in time_panes {
                        loading::overlay_scoped(
                            ui,
                            rect,
                            &tab.loading,
                            LoadingScope::TimePanes,
                            history_note,
                        );
                    }
                }
                // And the feed's own report, in the corner rather than over
                // the chart. Progress never gets here: a first connection and a
                // history block already have the loading overlay above, and a
                // second badge beside it would be the interface talking about
                // itself twice.
                if let Some(report) = feed_notice::report(&tab.notice, stall.as_ref())
                    && report.is_offline()
                {
                    // Measured once, then handed to everything that needs
                    // it: the chip's own hit test, the popup's anchor, the
                    // dismissal test, and the scene's bounds.
                    let chip = feed_notice::chip_rect(ui.painter(), area);
                    chip_rect = Some(chip);
                    chip_clicked = feed_notice::draw_chip(ui, chip, &report, popup_open);
                    // A pane with nothing on it has room to say why, and a
                    // corner chip alone on a blank canvas is a puzzle. One
                    // muted line, no border and no buttons — the way out is
                    // still the corner.
                    //
                    // Not while the popup is up. The line and the popup carry
                    // the same headline, and on the empty chart that is
                    // exactly where both of them draw: one sentence, twice, a
                    // hand apart.
                    if !popup_open && let Some((pane_rect, 0)) = tab.starved_pane() {
                        feed_notice::draw_empty_pane_note(ui.painter(), pane_rect, &report);
                    }
                    if popup_open {
                        // A click anywhere else puts it away, measured against
                        // the rectangles that were actually drawn — so a click
                        // on the edge of what the trader can see is never read
                        // as a click outside it, and the popup is laid out
                        // once rather than measured again to ask.
                        let popup;
                        (notice_action, popup) = feed_notice::draw_popup(ui, area, chip, &report);
                        dismissed = ui.input(|input| {
                            input.pointer.any_click()
                                && input
                                    .pointer
                                    .interact_pos()
                                    .is_some_and(|at| !popup.contains(at) && !chip.contains(at))
                        });
                    }
                }
            });
        if begin_text_edit {
            self.surfaces.drawing_chrome.request_text_edit();
        }
        // Floating drawing controls must be registered after the opaque
        // central canvas so they stay in front of the chart. That is why the
        // drawing chrome is the one surface `Surfaces::draw_all` does not
        // draw: it is anchored *to* the chart rather than floating over the
        // window, so it is commanded by name from here instead.
        self.draw_drawing_chrome(ctx, now);
        // The menus above may have disarmed a bot over a resting retest
        // limit; its cancel goes to the simulator on this same frame, not
        // on the next print. Every tab, not just the active one: a menu
        // click and a tab switch can land on the same frame, and the old
        // tab's feed keeps running — its cancel must not sit stranded
        // until the tab is looked at again.
        for tab in &mut self.tabs {
            tab.apply_strategy_cleanup();
        }
        self.play_pending_alarms();
        // Both are window chrome reading the active tab, like the offline
        // corner and the transport strip: they speak for one market at a time.
        let tz = self.tz;
        self.active_tab_mut().paper.draw_report_window(ctx, tz);
        self.settle_paper_panels(now);
        // Both controls go through the tab's own methods, which are also what
        // the registered control-plane actions call: a click and a named call
        // must be able to disagree about nothing.
        match notice_action {
            feed_notice::NoticeAction::None => {}
            feed_notice::NoticeAction::Reconnect => {
                let (tab, config) = self.active_with_config();
                let _ = tab.reconnect_feed(config);
            }
            feed_notice::NoticeAction::Reload => {
                let (tab, config) = self.active_with_config();
                let _ = tab.reload_feed(config);
            }
        }
        self.feed_chip_rect = chip_rect;
        self.feed_popup_tab = feed_notice::popup_still_open(
            popup_open,
            chip_clicked,
            chip_rect.is_some(),
            dismissed,
            notice_action,
        )
        .then_some(popup_tab);
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
        let config = &self.config;
        let progressive_history = self.progressive_history;
        let history_reach = self.history_reach;
        let history_reach_span_minutes = self.history_reach_span_minutes;
        let venue_lead_in = self.venue_lead_in;
        let mut trades = 0_u64;
        for tab in &mut self.tabs {
            let before = tab.live_trades;
            tab.drain_feed();
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
            // The switch lives on the window, the request is phrased by the
            // tab: mirrored here so every tab asks the way the trader last
            // said, including one opened after the choice was made.
            tab.progressive_history = progressive_history;
            tab.history_reach = history_reach;
            tab.history_reach_span_minutes = history_reach_span_minutes;
            // Through the setter, not the field: flipping the lead-in refolds
            // the prefix, and a tab that only had the field written would keep
            // drawing the answer to the previous choice until the next candle
            // landed. Idempotent, so the steady state costs one comparison.
            tab.set_venue_lead_in(venue_lead_in);
            tab.poll_ohlcv_capability(config);
            trades += tab.live_trades - before;
        }
        // What the window ingested, across every market it is holding.
        self.trades_since_summary += trades;
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
            self.surfaces.source_picker.open(&self.config);
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

    /// Do what the "Open market" dialog settled on.
    fn apply_market_request(&mut self, request: crate::surfaces::MarketRequest) {
        use crate::surfaces::MarketRequest;
        match request {
            MarketRequest::Open { feed_id, symbol } => self.open_tab(feed_id, symbol, None),
            MarketRequest::Add { feed_id, symbol } => match self.add_symbol(&feed_id, &symbol) {
                Ok(()) => {
                    self.surfaces.source_picker.close();
                    self.open_tab(feed_id, symbol, None);
                }
                // The dialog stays open carrying the reason: the user is one
                // keystroke from a symbol that does fit, and closing would
                // make the refusal look like a crash.
                Err(reason) => self.surfaces.source_picker.refuse(reason),
            },
            MarketRequest::Remove { feed_id, symbol } => self.remove_symbol(&feed_id, &symbol),
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
            TabAction::New => self.surfaces.source_picker.open(&self.config),
        }
    }
}

/// The interval a saved bar rule names, when it is a time rule at all — a
/// workspace that recorded `tick:50` for a context chart is a file written by
/// hand, and the chart opens on the default rather than on a guess.
fn saved_time_interval(text: Option<&str>) -> Option<i64> {
    text.and_then(|text| match BarSpec::parse(text) {
        Ok(BarSpec::Time(ms)) => Some(ms),
        _ => None,
    })
}

/// Every context chart's opening interval, top to bottom, from the rules a
/// workspace saved. A rule that is not a time rule keeps the default for its
/// slot so the slots after it still line up with their charts. A file written
/// before the stack existed carries only `time_bars`, which is the top chart's.
fn saved_context_intervals(bars: &[String], time_bars: Option<&str>) -> Vec<i64> {
    if bars.is_empty() {
        return saved_time_interval(time_bars).into_iter().collect();
    }
    bars.iter()
        .map(|text| {
            saved_time_interval(Some(text)).unwrap_or(crate::time_header::DEFAULT_INTERVAL_MS)
        })
        .collect()
}

#[cfg(test)]
mod tests;
