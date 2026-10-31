//! The context toolbar: *what am I looking at* on the left, *what is drawn
//! on it* on the right (`docs/ux/ui-design-model.md` §6).
//!
//! Left: SOURCE (feed + symbol, or the amber session label while replaying),
//! HISTORY (a `+ older ▾` split button whose menu holds the page size). Right: LAYERS (one icon toggle per visual
//! layer; right-click opens its dock tab), LOOK (appearance dialog), PANELS
//! (dock show/hide). Between HISTORY and the right groups sits TRADE — the
//! simulated BUY/SELL market buttons (`docs/ux/paper-trading.md`). The
//! toolbar never wraps: [`collapse_plan`] folds groups into the `⋯` overflow
//! menu in the §6 order — LOOK → PANELS → HISTORY → TRADE → the feed name
//! shrinks to its initial. The symbol and LAYERS never fold. Bars and
//! indicators belong to each pane, on its header line (`crate::pane_header`).
//!
//! The widgets edit the app's state through [`ToolbarModel`]'s borrows;
//! anything with a side effect beyond a field write comes back as a
//! [`ToolbarAction`] for the app to carry out.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::chart_layers::{ChartLayer, LayerBlock};
use crate::config::FeedCapabilities;
use crate::dock::DockTab;
use quantick_chart::toolbar_collapse::{CollapsePlan, collapse_plan};
use quantick_engine::bar_selection::BarInputAvailability;
pub(crate) mod bar_parameters;
use crate::theme;
use crate::widgets::{IconButton, TOOLBAR_ICON};
use quantick_feed::history_reach::HistoryReach;
use quantick_feed::history_run::RunStatus;
use quantick_paper::desk::ticket::ENTRY_FACE_MAX_CHARS;
use quantick_session::{HistoryPagingOff, fmt_history_span};

/// Height of the toolbar, in pixels (§5 zone 2).
pub const TOOLBAR_HEIGHT: f32 = 44.0;

// The LAYERS group's glyphs. Named here, and asserted as a set by
// `the_layer_toggles_speak_one_visual_language`, because a test that spells
// the literal itself proves nothing: revert the button and it still passes.
// The two layers that own a dock tab take the tab's glyph instead of a
// constant here, so a toggle and the panel behind it cannot drift apart.
/// The live strip: a sideways histogram, which is what it draws.
const LAYER_STRIP_ICON: &str = icons::CHART_BAR_HORIZONTAL;
/// The footprint: the grid of per-price cells the layer draws inside each
/// candle. Same alphabet rule as its neighbours — the glyph is the shape on
/// the chart, never a metaphor for it.
const LAYER_FOOTPRINT_ICON: &str = icons::GRID_NINE;

/// One TRADE button's own horizontal padding, in pixels.
const TRADE_BUTTON_PAD_PX: f32 = 16.0;
/// Estimated width of one strong glyph in a TRADE button, in pixels: upper
/// case and digits, wider than the average label glyph.
const TRADE_GLYPH_PX: f32 = 8.0;
/// The one width both entry buttons wear, in every position state: room
/// for the longest face the paper desk ever writes
/// ([`ENTRY_FACE_MAX_CHARS`]), so a press lands where it landed flat.
const TRADE_ENTRY_WIDTH_PX: f32 =
    TRADE_BUTTON_PAD_PX + ENTRY_FACE_MAX_CHARS as f32 * TRADE_GLYPH_PX;
/// Characters the close button shows before it cuts with `…`: room for
/// `× Close 999 SHORT`. The whole label is always in its hover.
const TRADE_CLOSE_FACE_CHARS: usize = 17;
/// The close button's fixed width, so the reverse beside it never moves on
/// a flip from LONG to SHORT or on a size change.
const TRADE_CLOSE_WIDTH_PX: f32 =
    TRADE_BUTTON_PAD_PX + TRADE_CLOSE_FACE_CHARS as f32 * TRADE_GLYPH_PX;
/// The reverse button's face.
const TRADE_REVERSE_LABEL: &str = "Reverse";
/// The reverse button's fixed width.
const TRADE_REVERSE_WIDTH_PX: f32 =
    TRADE_BUTTON_PAD_PX + TRADE_REVERSE_LABEL.len() as f32 * TRADE_GLYPH_PX;
/// TRADE's width in the collapse plan: the entry pair and the open-state
/// close and reverse, reserved flat too. Opening or closing a position then
/// never changes what folds, so nothing left of TRADE - and nothing in it -
/// moves when a fill lands.
const W_TRADE: f32 = 2.0 * TRADE_ENTRY_WIDTH_PX + TRADE_CLOSE_WIDTH_PX + TRADE_REVERSE_WIDTH_PX;

/// The amber label that replaces SOURCE while a recording plays.
pub struct ReplaySource {
    /// What the label says.
    pub label: String,
    /// The hover detail (file, side source).
    pub hover: String,
}

/// The toolbar's view of the app: fields it edits directly, and read-only
/// context for gating and display.
pub struct ToolbarModel<'a> {
    /// The arrangement the canvas is showing, if it still matches a registered
    /// preset. `None` marks a custom row and lights no cell.
    pub layout_preset: Option<&'static crate::canvas_layout::LayoutPreset>,
    /// Whether the layout popover is open.
    pub layout_picker_open: &'a mut bool,
    /// Open the layout popover this frame (the `QUANTICK_LAYOUT_PICKER` hook).
    pub layout_picker_request_open: bool,
    /// `(id, display label)` for every configured feed.
    pub feeds: Vec<(String, String)>,
    /// The selected feed id.
    pub feed_id: &'a mut String,
    /// Display name of the selected feed.
    pub feed_display_name: String,
    /// Symbols offered by the selected feed.
    pub symbols: Vec<String>,
    /// The selected symbol.
    pub symbol: &'a mut String,
    /// Present while a recording is the source; replaces the SOURCE combos.
    pub replay: Option<ReplaySource>,
    /// The deal recorder shown beside the symbol, when supported.
    pub deal_recording: Option<crate::deal_recording::RecordingView>,
    /// Open the REC popover through the launch hook.
    pub deal_recording_menu: bool,
    /// The bar kind of the focused pane, which the REC popover speaks about.
    pub focused_bars: &'static quantick_engine::bar_registry::BarDefinition,
    /// Where the history menu's own button ended up, written back by the draw.
    ///
    /// Published for the same reason the Workspace menu's rect is: a menu is a
    /// popup egui owns, so the only honest way to open one from a hook is to
    /// deliver a click on the button's real rectangle. Guessing a pixel would
    /// photograph whatever happens to be there.
    pub history_menu_rect: &'a mut Option<egui::Rect>,
    /// The History button: its target, this tab's run and the clock face.
    pub history: HistoryButton,
    /// Venue candles held so far, for the history menu readout. Zero on a
    /// feed that serves none.
    pub history_candles: usize,
    /// Whether asking for another span of older candles could get the trader
    /// anything, and when it could not, why — see `Tab::older_candles`.
    /// Decided by the tab, which is the only thing that knows what it has
    /// already asked for. Carried as the reason rather than a bool because the
    /// disabled tooltip is nothing but the reason.
    pub older_candles: crate::tab::OlderCandles,
    /// What the active source's backend can do.
    pub capabilities: FeedCapabilities,
    /// The LAYERS group's lamps, in [`LayerToggle::ALL`] order: whether
    /// each layer is drawn, and what blocks it where something does. Both
    /// come from `ChartPane::layer_blocked` / `layer_visible` through
    /// [`crate::tab::Tab::layer_toggle_state`], which the semantic scene reads
    /// too — so a button cannot tell the trader one thing and an assistant
    /// another.
    pub layers: [LayerToggleState; LayerToggle::COUNT],
    /// Whether the dock (strip included) is shown.
    pub dock_visible: bool,
    /// Whether the appearance dialog is open.
    pub appearance_open: bool,
    /// The TRADE group: readiness, state-aware entry labels and the close
    /// button, all computed by the paper host (`PaperTrading`).
    pub paper: PaperTradeModel,
}

/// What the TRADE group shows this frame: the paper desk's
/// [`TradeBar`](quantick_paper::desk::TradeBar), decided in one read by the
/// headless desk so the toolbar stays decoupled from the simulator's types.
/// The faces name side and size only (`SELL 1`) so the buttons never move;
/// the hovers disclose what the press would do to the open position.
pub type PaperTradeModel = quantick_paper::desk::TradeBar;

/// One active indicator as the picker shows it. Raw slot numbers
/// keep the toolbar decoupled from the worker's types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndicatorMenuEntry {
    /// The app-side slot id this entry answers to.
    pub slot: u64,
    /// Display label (the indicator's short title).
    pub label: String,
    /// Whether the eye toggle currently hides it.
    pub hidden: bool,
    /// Whether the indicator is disabled by a runtime error.
    pub errored: bool,
    /// Whether the running version is stale (the file on disk has errors).
    pub stale: bool,
}

/// A side effect the toolbar asks the app to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Switch the canvas to a named arrangement from the layout registry.
    ///
    /// Carries the registry entry rather than a copy of its shape, so the
    /// toolbar never holds a second opinion about what a layout contains.
    SetLayout(&'static crate::canvas_layout::LayoutPreset),
    /// Load back to a target: the main click's, or one the menu named.
    LoadHistory(HistoryReach),
    /// Stop the history run in flight and keep what arrived.
    CancelHistory,
    /// What the REC control beside the symbol asked for.
    DealRecording(crate::deal_recording::DealRecordingAction),
    /// Fetch and prepend one more span of older venue candles.
    ///
    /// The trade twin above and this one are two different records — a page of
    /// prints and a span of buckets — served by two different venue endpoints
    /// under two different capabilities. A feed can page one and not the
    /// other, so they are two actions rather than one with a mode.
    LoadOlderCandles,
    /// Set one layer through the same operation as its chart menu.
    SetLayer(LayerToggle, bool),
    /// Open the footprint's settings window. Like every other right-click in
    /// this group, looking is not enabling: it opens with the layer off too,
    /// because configuring before switching on is a legitimate order of
    /// operations.
    OpenFootprintSettings,
    /// Open a dock tab (a layer's settings; never toggles the layer).
    OpenDockTab(DockTab),
    /// Show or hide the dock.
    ToggleDock,
    /// Open or close the appearance dialog.
    ToggleAppearance,
    /// Add a native indicator, by its catalog id
    /// (`quantick_indicators::native`). One variant for every native there
    /// will ever be — the menu is drawn from the catalog, so the toolbar
    /// never learns which natives exist.
    AddNative(&'static str),
    /// Flip an indicator's render-side eye toggle (no recompute).
    ToggleIndicatorHidden(u64),
    /// Remove an indicator.
    RemoveIndicator(u64),
    /// Load a script from the library, by index into `ToolbarModel::scripts`.
    AddScriptIndicator(usize),
    /// Open the settings dialog of an indicator.
    OpenIndicatorSettings(u64),
    /// Simulated market buy at the Trading tab's quantity (paper trading).
    PaperBuy,
    /// Simulated market sell at the Trading tab's quantity (paper trading).
    PaperSell,
    /// Exit the open simulated position at the next print.
    PaperClose,
    /// Turn the open simulated position around at the next print.
    PaperReverse,
}

/// Draw the toolbar as the 44 px top panel and return what was asked of the
/// app this frame. One pointer produces at most one click per frame, so the
/// list carries zero or one action in practice; a `Vec` only spares every
/// widget the "was something already clicked" bookkeeping.
pub fn draw(ctx: &egui::Context, model: &mut ToolbarModel) -> Vec<ToolbarAction> {
    let mut actions = Vec::new();
    egui::TopBottomPanel::top("toolbar")
        .exact_height(TOOLBAR_HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(theme::CHROME)
                .inner_margin(egui::Margin::symmetric(8.0, 0.0)),
        )
        .show(ctx, |ui| {
            let rec_width = if model.deal_recording.is_some() {
                crate::deal_recording_ui::REC_BUTTON_MIN_WIDTH_PX + ui.spacing().item_spacing.x
            } else {
                0.0
            };
            let plan = collapse_plan(ui.available_width() - rec_width, W_TRADE);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                draw_source(ui, model, plan, &mut actions);
                if plan.history_inline {
                    ui.separator();
                    draw_history(ui, model, &mut actions);
                }
                if plan.trade_inline {
                    ui.separator();
                    draw_trade(ui, &model.paper, &mut actions);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if plan.overflow_needed() {
                        draw_overflow(ui, model, plan, &mut actions);
                    }
                    if plan.panels_inline {
                        let panels = IconButton::new(icons::SIDEBAR_SIMPLE, TOOLBAR_ICON)
                            .active(model.dock_visible)
                            .hover_text("show or hide the panels dock (Ctrl+Shift+B)")
                            .show(ui);
                        if panels.clicked() {
                            actions.push(ToolbarAction::ToggleDock);
                        }
                    }
                    if let Some(preset) = crate::layout_picker::draw(
                        ui,
                        crate::layout_picker::PickerModel {
                            current: model.layout_preset,
                            open: model.layout_picker_open,
                            request_open: model.layout_picker_request_open,
                        },
                    ) {
                        actions.push(ToolbarAction::SetLayout(preset));
                    }
                    if plan.look_inline {
                        let look = IconButton::new(icons::PAINT_BRUSH, TOOLBAR_ICON)
                            .active(model.appearance_open)
                            .hover_text("appearance: candles, canvas, grid")
                            .show(ui);
                        if look.clicked() {
                            actions.push(ToolbarAction::ToggleAppearance);
                        }
                    }
                    ui.separator();
                    draw_layers(ui, model, &mut actions);
                });
            });
        });
    actions
}

/// SOURCE: feed + symbol combos, or the amber session label during replay.
fn draw_source(
    ui: &mut egui::Ui,
    model: &mut ToolbarModel,
    plan: CollapsePlan,
    actions: &mut Vec<ToolbarAction>,
) {
    if let Some(replay) = &model.replay {
        ui.label(egui::RichText::new("source:").color(theme::TEXT_MUTED));
        ui.label(
            egui::RichText::new(&replay.label)
                .color(theme::AMBER)
                .strong(),
        )
        .on_hover_text(&replay.hover);
        return;
    }

    let feed_text = if plan.feed_full_name {
        model.feed_display_name.clone()
    } else {
        model
            .feed_display_name
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default()
    };
    ui.label(egui::RichText::new("feed").color(theme::TEXT_MUTED));
    egui::ComboBox::from_id_salt("feed_sel")
        .selected_text(feed_text)
        .show_ui(ui, |ui| {
            for (id, name) in &model.feeds {
                ui.selectable_value(model.feed_id, id.clone(), name);
            }
        });
    ui.label(egui::RichText::new("symbol").color(theme::TEXT_MUTED));
    egui::ComboBox::from_id_salt("symbol_sel")
        .selected_text(model.symbol.clone())
        .show_ui(ui, |ui| {
            for symbol in &model.symbols {
                ui.selectable_value(model.symbol, symbol.clone(), symbol);
            }
        });
    if let Some(view) = &model.deal_recording
        && let Some(action) = crate::deal_recording_ui::draw_button(
            ui,
            view,
            model.focused_bars,
            model.deal_recording_menu,
        )
    {
        actions.push(ToolbarAction::DealRecording(action));
    }
}

/// What the running source can build bars from: the one answer each pane
/// header's bars menu reads.
pub(crate) fn available_bar_inputs(
    capabilities: FeedCapabilities,
    deal_recording: Option<&crate::deal_recording::RecordingView>,
) -> BarInputAvailability {
    BarInputAvailability {
        traded_volume: capabilities.traded_volume,
        deal_counter: capabilities.deal_counter,
        deal_count: deal_recording
            .is_some_and(crate::deal_recording::RecordingView::deal_count_available),
    }
}

/// HISTORY: the `History` split button. The main click repeats this tab's
/// last target (yesterday until it has pressed); the caret holds the one-click
/// targets and the candle reach. While a run pages, the button names its
/// target and how far back it has got, and a click (or Esc) cancels it.
///
/// The button gates on `history_paging` — older *trades* — but the caret does
/// not: a feed can serve candle history without paging its tape (Hyperliquid
/// is exactly that), and gating the menu on the button's capability would
/// leave the candle reach behind a control the trader cannot open.
fn draw_history(ui: &mut egui::Ui, model: &mut ToolbarModel, actions: &mut Vec<ToolbarAction>) {
    let paging = model.capabilities.history_paging;
    let menu = history_menu_reachable(model);
    let load = ui
        .add_enabled(
            paging,
            egui::Button::new(history_button_label(&model.history)),
        )
        .on_hover_text(history_button_hover(&model.history))
        .on_disabled_hover_text(history_disabled_hover(model));
    if load.clicked() {
        actions.push(history_button_action(&model.history));
    }
    let caret = ui.add_enabled_ui(menu, |ui| {
        ui.menu_button(icons::CARET_DOWN, |ui| {
            draw_history_menu(ui, model, actions);
        })
        .response
        .rect
    });
    *model.history_menu_rect = menu.then_some(caret.inner);
}

/// What the History button shows and does this frame.
#[derive(Debug, Clone, Copy)]
pub struct HistoryButton {
    /// What the main click loads.
    pub main: HistoryReach,
    /// Where this tab's run stands.
    pub status: RunStatus,
    /// The clock face times are written in.
    pub tz: crate::timezone::TzOffset,
}

impl HistoryButton {
    fn running(&self) -> bool {
        !matches!(self.status, RunStatus::Idle)
    }
}

/// The button's words: the target it loads, or the run it is making.
fn history_button_label(button: &HistoryButton) -> String {
    let at = |ms| quantick_civil::fmt_weekday_minute(ms, button.tz);
    match button.status {
        RunStatus::Idle => format!(
            "{} History: {}",
            icons::CLOCK_COUNTER_CLOCKWISE,
            button.main.label()
        ),
        RunStatus::Queued(reach) => format!("Queued {}\u{2026} {}", reach.label(), icons::X),
        RunStatus::Loading(progress) | RunStatus::Paused(progress) => format!(
            "Loading {}\u{2026} back to {} {}",
            progress.reach.label(),
            at(progress.oldest_ms),
            icons::X
        ),
    }
}

/// The main click repeats the target; while running it cancels.
fn history_button_action(button: &HistoryButton) -> ToolbarAction {
    if button.running() {
        ToolbarAction::CancelHistory
    } else {
        ToolbarAction::LoadHistory(button.main)
    }
}

/// What one press of the button does right now. One owner, read by the bar's
/// button and the overflow entry alike.
fn history_button_hover(button: &HistoryButton) -> String {
    match button.status {
        RunStatus::Idle => format!(
            "History: load back {} \u{2014} {}. The menu beside it has the other targets.",
            button.main.label(),
            button.main.hover()
        ),
        RunStatus::Queued(_) => "waiting for the session to finish loading, then this \
             target starts; click or Esc to cancel"
            .to_owned(),
        RunStatus::Loading(_) | RunStatus::Paused(_) => "the chart stays as it is while \
             pages arrive and is rebuilt once at the end; click or Esc to cancel and keep \
             what arrived"
            .to_owned(),
    }
}

/// The button's refusal, read off the model. One owner, called by the bar's
/// button and by the overflow entry.
fn history_disabled_hover(model: &ToolbarModel) -> &'static str {
    HistoryPagingOff::of(model.capabilities.history_paging, model.older_candles)
        .map_or("", HistoryPagingOff::hover)
}

/// Whether the history menu has anything in it — trade paging, candle reach,
/// or both. One owner, called by the bar's caret and by the overflow entry.
fn history_menu_reachable(model: &ToolbarModel) -> bool {
    model.capabilities.history_paging || model.capabilities.ohlcv_history
}

/// The history caret/overflow menu body: the one-click targets, then the
/// candle reach.
fn draw_history_menu(
    ui: &mut egui::Ui,
    model: &mut ToolbarModel,
    actions: &mut Vec<ToolbarAction>,
) {
    // The trade half, behind the trade capability: a feed that serves candles
    // without paging its tape offers no target it could not serve.
    if model.capabilities.history_paging {
        ui.label("Load back");
        let running = model.history.running();
        for reach in HistoryReach::PRESETS {
            let entry = ui
                .add_enabled(!running, egui::Button::new(reach.label()))
                .on_hover_text(reach.hover())
                .on_disabled_hover_text("a run is loading; cancel it first");
            if entry.clicked() {
                actions.push(ToolbarAction::LoadHistory(reach));
                ui.close_menu();
            }
        }
        if running && ui.button(format!("{} Cancel loading", icons::X)).clicked() {
            actions.push(ToolbarAction::CancelHistory);
            ui.close_menu();
        }
    }
    // Candles are the other record, and the other reach: offered only where
    // the feed serves candles but cannot page its trades (a recording's
    // run-up, Hyperliquid), worded as candles so it is never mistaken for
    // the targets above. A chart opens on one week of them
    // (`feed::TIME_HISTORY_SPAN_MS`); this reaches back a week at a time.
    if model.capabilities.ohlcv_history && !model.capabilities.history_paging {
        ui.separator();
        // The reach is named from the constant that owns it, never spelled out
        // beside it: the span was ninety days one release ago, and a sentence
        // carrying its own copy of that number starts lying the day it moves.
        let reach = fmt_history_span(quantick_feed::TIME_HISTORY_SPAN_MS);
        let older = ui
            .add_enabled(
                model.older_candles.is_available(),
                egui::Button::new(format!("{} older candles", icons::PLUS)),
            )
            .on_hover_text(format!(
                "fetch another {reach} of venue candles and prepend it"
            ));
        // The reason, not a list of reasons: see `Tab::older_candles`.
        let older = match model.older_candles.why_not() {
            Some(reason) => older.on_disabled_hover_text(reason),
            None => older,
        };
        if older.clicked() {
            actions.push(ToolbarAction::LoadOlderCandles);
            ui.close_menu();
        }
        // "1-minute", said out loud: the base is always at
        // `OHLCV_BASE_INTERVAL_MS` while the pane folds it to whatever it
        // shows, so a 1-hour chart holding a week would otherwise read
        // "10 080 venue candles held" beside 168 drawn bars.
        ui.small(format!(
            "{} 1-minute venue candles held",
            model.history_candles
        ));
    }
    if let Some(view) = &model.deal_recording
        && !view.days.is_empty()
    {
        ui.separator();
        ui.label("recorded deals");
        let mut action = None;
        crate::deal_recording_ui::draw_days(ui, view, &mut action);
        if let Some(action) = action {
            actions.push(ToolbarAction::DealRecording(action));
        }
    }
}

/// TRADE: the simulated market entries and, while a position is open, its
/// exit (`docs/ux/paper-trading.md`). Quantity and brackets live in the
/// Trading dock tab; the labels disclose them. Gated on the simulator having
/// seen a price, never on the provider — paper trading works on any feed.
fn draw_trade(ui: &mut egui::Ui, paper: &PaperTradeModel, actions: &mut Vec<ToolbarAction>) {
    // The entries speak the chip language: solid side colour, dark ink —
    // the same grammar as the price chips in the gutter, because the button
    // and the entry line it will create are one statement. Both wear one
    // fixed size in every state, the face truncating rather than growing,
    // so opening a position never slides SELL out from under the pointer.
    let entry = |ui: &mut egui::Ui, label: &str, fill: egui::Color32, enabled: bool| {
        ui.add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                egui::vec2(TRADE_ENTRY_WIDTH_PX, 24.0),
                egui::Button::new(egui::RichText::new(label).color(theme::CHIP_INK).strong())
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .fill(fill)
                    .stroke(egui::Stroke::NONE)
                    .rounding(egui::Rounding::same(3.0)),
            )
        })
        .inner
    };
    let enabled = paper.blocked.is_none();
    let locked = paper.blocked.as_deref().unwrap_or_default();
    let buy = entry(ui, &paper.buy.face, theme::BUY, enabled)
        .on_hover_text(&paper.buy.hover)
        .on_disabled_hover_text(locked);
    if buy.clicked() {
        actions.push(ToolbarAction::PaperBuy);
    }
    let sell = entry(ui, &paper.sell.face, theme::SELL, enabled)
        .on_hover_text(&paper.sell.hover)
        .on_disabled_hover_text(locked);
    if sell.clicked() {
        actions.push(ToolbarAction::PaperSell);
    }
    // The exit controls, on the same surface as the entries: an open
    // position must never require a dock dive to leave or flip (audit pain
    // 1, B2). Outlined, not filled — leaving is deliberate, never the
    // loudest thing on the bar. Fixed widths too, so a flip from LONG to
    // SHORT never slides the reverse. Close is never locked: leaving adds
    // no risk. The reverse opens a position, so the entries' lock holds it.
    if let Some(close) = &paper.close {
        let outlined = |ui: &mut egui::Ui, text: String, width: f32, enabled: bool| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    egui::vec2(width, 24.0),
                    egui::Button::new(
                        egui::RichText::new(text)
                            .color(theme::TEXT_PRIMARY)
                            .strong(),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .fill(theme::CONTROL)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(egui::Rounding::same(3.0)),
                )
            })
            .inner
        };
        let button = outlined(ui, format!("× {close}"), TRADE_CLOSE_WIDTH_PX, true).on_hover_text(
            format!("{close}: exit the open simulated position at the next print (market)"),
        );
        if button.clicked() {
            actions.push(ToolbarAction::PaperClose);
        }
        let reverse = outlined(
            ui,
            TRADE_REVERSE_LABEL.to_owned(),
            TRADE_REVERSE_WIDTH_PX,
            enabled,
        )
        .on_hover_text(&paper.reverse_hover)
        .on_disabled_hover_text(locked);
        if reverse.clicked() {
            actions.push(ToolbarAction::PaperReverse);
        }
    }
}

/// One LAYERS lamp this frame: drawn or not, and what blocks it if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerToggleState {
    pub on: bool,
    pub blocked: Option<LayerBlock>,
}

/// The visual layers the LAYERS group toggles, declared once.
///
/// The toolbar draws from this list and the semantic scene projects from it,
/// so a layer cannot wear a button the trader sees and be missing from what an
/// operator can read, nor the reverse. A hand-kept list beside this one is the
/// drift this type exists to prevent.
///
/// The INDICATORS menu is deliberately not a member: it opens a menu rather
/// than toggling a layer, and folding it in would give the group two shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerToggle {
    Bubbles,
    Heatmap,
    Footprint,
    LiveStrip,
}

impl LayerToggle {
    /// Every toggle in *call* order, which the right-to-left layout turns
    /// into right-to-left screen order: the trader reads them the other way
    /// round. Anything that wants reading order reverses this — the scene
    /// does, and says so — because the draw order is what the layout needs
    /// and reordering here would move the buttons under the trader's hand.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Bubbles,
        Self::Heatmap,
        Self::Footprint,
        Self::LiveStrip,
    ];

    /// How many toggles the group has. Named so the model's array, the list
    /// and any fixture widen together when a layer earns a button.
    pub const COUNT: usize = 4;

    /// The canvas layer this button switches.
    ///
    /// A toolbar button is a shortcut into the pane's own layer menu, never a
    /// second switch beside it: the identifier, the label and the state all
    /// come from [`ChartLayer`], which already resolves each layer to the one
    /// field that owns it. Restating any of them here is how a button and a
    /// menu start disagreeing about a pixel.
    #[must_use]
    pub(crate) fn layer(self) -> ChartLayer {
        match self {
            Self::Bubbles => ChartLayer::Bubbles,
            Self::Heatmap => ChartLayer::Heatmap,
            Self::Footprint => ChartLayer::Footprint,
            Self::LiveStrip => ChartLayer::LiveStrip,
        }
    }

    /// The glyph. Where a dock tab configures the layer it lends its own, so
    /// the toggle and the panel behind it cannot wear two different marks.
    #[must_use]
    fn icon(self) -> &'static str {
        match self {
            Self::Bubbles => DockTab::Bubbles.icon(),
            Self::Heatmap => DockTab::L2.icon(),
            Self::Footprint => LAYER_FOOTPRINT_ICON,
            Self::LiveStrip => LAYER_STRIP_ICON,
        }
    }

    #[must_use]
    fn accent(self) -> egui::Color32 {
        match self {
            Self::Bubbles => theme::BUY,
            Self::Heatmap | Self::LiveStrip => theme::ACCENT,
            Self::Footprint => theme::POC,
        }
    }

    #[must_use]
    fn hover_text(self) -> &'static str {
        match self {
            Self::Bubbles => {
                "aggression bubbles (Ctrl+B): confirmed executions from the trade stream — \
                 right-click for settings"
            }
            Self::Heatmap => {
                "L2 heatmap: show the recorded depth map — recording never stops, so hiding it \
                 loses nothing. Right-click for settings"
            }
            Self::Footprint => {
                "candle footprint (Ctrl+F): the buy/sell split per price inside each bar — detail follows \
                 the zoom. Right-click for style and thresholds"
            }
            Self::LiveStrip => {
                "live strip: the book's resting depth and the forming bar's aggression, \
                 beside the price axis — right-click for settings"
            }
        }
    }

    #[must_use]
    fn toggle_action(self, on: bool) -> ToolbarAction {
        ToolbarAction::SetLayer(self, on)
    }

    /// Where a right-click goes: the panel that configures the layer. Looking
    /// is not enabling, so this never toggles on the way.
    #[must_use]
    fn settings_action(self) -> ToolbarAction {
        match self {
            Self::Bubbles => ToolbarAction::OpenDockTab(DockTab::Bubbles),
            // The strip reads the same book the depth map draws, so the same
            // tab configures both.
            Self::Heatmap | Self::LiveStrip => ToolbarAction::OpenDockTab(DockTab::L2),
            Self::Footprint => ToolbarAction::OpenFootprintSettings,
        }
    }
}

/// LAYERS: one icon toggle per visual layer. Left-click toggles the layer;
/// right-click opens its dock tab — looking is not enabling. Drawn inside the
/// right-to-left layout, so the call order is the reverse of what is seen.
///
/// One alphabet for the group: **every glyph is the shape its layer draws on
/// the chart**, never a metaphor for it. A line for the indicators, circles
/// for the prints, the book's stacked rows for the depth map, a sideways
/// histogram for the strip. The metaphors that used to sit here (a flame for
/// the heatmap, a brick wall for the strip) had nothing to do with each other
/// or with what the trader sees — and the depth map, which *is* the bid/ask
/// lists, was the one that read least like itself.
fn draw_layers(ui: &mut egui::Ui, model: &ToolbarModel, actions: &mut Vec<ToolbarAction>) {
    // Zipped rather than indexed: the array is filled in `ALL` order, so
    // walking the two together is what makes that the only ordering. A second
    // hand-written position map beside `ALL` is how a lamp ends up reporting
    // the layer next to it.
    for (layer, state) in LayerToggle::ALL.into_iter().zip(model.layers) {
        let on = state.on;
        let response = IconButton::new(layer.icon(), TOOLBAR_ICON)
            .active(on)
            .accent(layer.accent())
            .enabled(state.blocked.is_none())
            .hover_text(layer.hover_text())
            .disabled_explanation(state.blocked.map_or("", |block| block.explanation))
            .show(ui);
        if response.clicked() {
            actions.push(layer.toggle_action(!on));
        }
        if response.secondary_clicked() {
            actions.push(layer.settings_action());
        }
    }
}

/// The `⋯` menu holding every folded group, in a fixed order so muscle
/// memory survives resizes.
fn draw_overflow(
    ui: &mut egui::Ui,
    model: &mut ToolbarModel,
    plan: CollapsePlan,
    actions: &mut Vec<ToolbarAction>,
) {
    ui.menu_button(egui::RichText::new(icons::DOTS_THREE).size(16.0), |ui| {
        if !plan.look_inline
            && ui
                .button(format!("{} Appearance…", icons::PAINT_BRUSH))
                .clicked()
        {
            actions.push(ToolbarAction::ToggleAppearance);
            ui.close_menu();
        }
        if !plan.panels_inline
            && ui
                .button(format!("{} Show/hide panels", icons::SIDEBAR_SIMPLE))
                .clicked()
        {
            actions.push(ToolbarAction::ToggleDock);
            ui.close_menu();
        }
        if !plan.history_inline {
            ui.separator();
            let paging = model.capabilities.history_paging;
            let load = ui
                .add_enabled(
                    paging,
                    egui::Button::new(history_button_label(&model.history)),
                )
                .on_hover_text(history_button_hover(&model.history))
                .on_disabled_hover_text(history_disabled_hover(model));
            if load.clicked() {
                actions.push(history_button_action(&model.history));
                ui.close_menu();
            }
            if history_menu_reachable(model) {
                draw_history_menu(ui, model, actions);
            }
        }
        if !plan.trade_inline {
            ui.separator();
            // The menu has no width to keep, so it says the whole label.
            let paper = &model.paper;
            let enabled = paper.blocked.is_none();
            let locked = paper.blocked.as_deref().unwrap_or_default();
            let buy = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(format!("{} at market (SIM)", paper.buy.label)),
                )
                .on_hover_text(&paper.buy.hover)
                .on_disabled_hover_text(locked);
            if buy.clicked() {
                actions.push(ToolbarAction::PaperBuy);
                ui.close_menu();
            }
            let sell = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(format!("{} at market (SIM)", paper.sell.label)),
                )
                .on_hover_text(&paper.sell.hover)
                .on_disabled_hover_text(locked);
            if sell.clicked() {
                actions.push(ToolbarAction::PaperSell);
                ui.close_menu();
            }
            if let Some(close) = &paper.close {
                if ui.button(format!("× {close} (SIM)")).clicked() {
                    actions.push(ToolbarAction::PaperClose);
                    ui.close_menu();
                }
                let reverse = ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(format!("{TRADE_REVERSE_LABEL} (SIM)")),
                    )
                    .on_hover_text(&paper.reverse_hover)
                    .on_disabled_hover_text(locked);
                if reverse.clicked() {
                    actions.push(ToolbarAction::PaperReverse);
                    ui.close_menu();
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {

    fn layer_states(on: [bool; LayerToggle::COUNT]) -> [LayerToggleState; LayerToggle::COUNT] {
        on.map(|on| LayerToggleState { on, blocked: None })
    }

    use super::*;

    /// The LAYERS group speaks one visual language: each glyph is the shape
    /// its layer draws, and a layer that also has a dock tab wears the tab's
    /// glyph rather than a second one of its own. The depth map in particular
    /// has to read as the book — the stacked bid/ask lists — which is what
    /// the flame it used to wear never did.
    #[test]
    fn the_layer_toggles_speak_one_visual_language() {
        assert_eq!(DockTab::L2.icon(), icons::ROWS, "the book is its own rows");
        assert_eq!(DockTab::Bubbles.icon(), icons::CIRCLES_THREE);

        // Every glyph in the group is distinct — two layers wearing one mark
        // would read as one feature — and none of them is a metaphor the
        // chart never draws.
        // Every entry comes from what the buttons actually use: revert one of
        // them and this fails, which a test spelling its own literals cannot.
        let group = [
            DockTab::Bubbles.icon(), // prints: circles
            DockTab::L2.icon(),      // depth map: the book's rows
            LAYER_STRIP_ICON,        // live strip: a sideways histogram
        ];
        for (index, glyph) in group.iter().enumerate() {
            assert!(
                !group[index + 1..].contains(glyph),
                "two layer toggles wear the same glyph"
            );
            assert!(![icons::FIRE, icons::WALL, icons::STACK].contains(glyph));
        }
    }

    /// With bars and indicators gone from the row, every group fits a 1280 px
    /// window at TRADE's reserved width. REC needs more: beside it, 1280 px folds
    /// LOOK, PANELS and HISTORY into the overflow, and 1920 px folds nothing.
    #[test]
    fn nothing_folds_at_the_common_window_widths() {
        let rec = crate::deal_recording_ui::REC_BUTTON_MIN_WIDTH_PX;
        let margins = 16.0;
        for available in [1280.0 - margins, 1920.0 - margins, 1920.0 - margins - rec] {
            assert_eq!(collapse_plan(available, W_TRADE), CollapsePlan::FULL);
        }
        let with_rec = collapse_plan(1280.0 - margins - rec, W_TRADE);
        assert!(!with_rec.history_inline && with_rec.trade_inline);
    }

    /// Opening a position never changes what folds: at every window width,
    /// flat or open, the real toolbar paints BUY and SELL on the same
    /// pixels, so the history group left of TRADE cannot fold out from
    /// under a fill.
    #[test]
    fn opening_a_position_never_changes_the_collapse_plan() {
        let open = PaperTradeModel {
            close: Some("Close 88888 SHORT".to_owned()),
            ..flat_trade(true)
        };
        let full = CollapsePlan::FULL.width(W_TRADE);
        for width in [full + 200.0, full, full - 40.0, full - 200.0, full - 400.0] {
            let flat_rects = entry_rects(&toolbar_shapes(flat_trade(true), width));
            let open_rects = entry_rects(&toolbar_shapes(open.clone(), width));
            assert_eq!(flat_rects, open_rects, "BUY/SELL moved at width {width}");
            if width >= full {
                assert!(flat_rects.0.is_some(), "TRADE is inline at {width}");
            }
        }
    }

    /// Lay the toolbar out for real, off-screen, live and replaying — a
    /// duplicated widget id or a panicking widget fails here instead of on
    /// the chart, and an un-clicked frame must ask nothing of the app.
    #[test]
    fn the_toolbar_lays_out_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut feed_id = "binance".to_owned();
        let mut symbol = "BTCUSDT".to_owned();
        let mut history_menu_rect = None;
        for replaying in [false, true] {
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let mut layout_picker_open = false;
                    let mut model = ToolbarModel {
                        layout_preset: None,
                        layout_picker_open: &mut layout_picker_open,
                        layout_picker_request_open: false,
                        feeds: vec![("binance".to_owned(), "Binance".to_owned())],
                        feed_id: &mut feed_id,
                        feed_display_name: "Binance".to_owned(),
                        symbols: vec!["BTCUSDT".to_owned()],
                        symbol: &mut symbol,
                        replay: replaying.then(|| ReplaySource {
                            label: "BTCUSDT · 2026-03-16".to_owned(),
                            hover: "Replaying a recorded session".to_owned(),
                        }),
                        deal_recording: None,
                        deal_recording_menu: false,
                        focused_bars: quantick_engine::BarKind::Tick.into(),
                        history_menu_rect: &mut history_menu_rect,
                        history: HistoryButton {
                            main: HistoryReach::default(),
                            status: RunStatus::Idle,
                            tz: crate::timezone::TzOffset::default(),
                        },
                        history_candles: 0,
                        older_candles: crate::tab::OlderCandles::NotArrivedYet,
                        capabilities: FeedCapabilities {
                            book_capture: !replaying,
                            history_paging: !replaying,
                            history_boundary: None,
                            traded_volume: true,
                            deal_counter: false,
                            ohlcv_history: !replaying,
                            ohlcv_generation: 0,
                            ohlcv_daily_generation: 0,
                            ohlcv_aggressor_split: false,
                        },
                        layers: layer_states([true, false, false, false]),
                        dock_visible: true,
                        appearance_open: false,
                        paper: flat_trade(true),
                    };
                    let actions = draw(ctx, &mut model);
                    assert!(actions.is_empty(), "no clicks, no actions");
                });
            }
        }
    }

    /// The same layout for a venue that prints no volume — the shape a live
    /// Tickmill US500 session produces. Every size-measuring affordance is
    /// disabled rather than absent, so the toolbar must still lay out whole.
    #[test]
    fn a_quote_driven_feed_lays_the_toolbar_out_with_its_volume_widgets_disabled() {
        let ctx = egui::Context::default();
        let mut feed_id = "metatrader".to_owned();
        let mut symbol = "US500".to_owned();
        let mut history_menu_rect = None;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                let mut layout_picker_open = false;
                let mut model = ToolbarModel {
                    layout_preset: None,
                    layout_picker_open: &mut layout_picker_open,
                    layout_picker_request_open: false,
                    feeds: vec![("metatrader".to_owned(), "MetaTrader 5".to_owned())],
                    feed_id: &mut feed_id,
                    feed_display_name: "MetaTrader 5".to_owned(),
                    symbols: vec!["US500".to_owned()],
                    symbol: &mut symbol,
                    replay: None,
                    deal_recording: None,
                    deal_recording_menu: false,
                    focused_bars: quantick_engine::BarKind::Tick.into(),
                    history_menu_rect: &mut history_menu_rect,
                    history: HistoryButton {
                        main: HistoryReach::default(),
                        status: RunStatus::Idle,
                        tz: crate::timezone::TzOffset::default(),
                    },
                    history_candles: 0,
                    older_candles: crate::tab::OlderCandles::NotArrivedYet,
                    capabilities: FeedCapabilities {
                        book_capture: false,
                        history_paging: false,
                        history_boundary: None,
                        traded_volume: false,
                        deal_counter: false,
                        ohlcv_history: false,
                        ohlcv_generation: 0,
                        ohlcv_daily_generation: 0,
                        ohlcv_aggressor_split: false,
                    },
                    layers: layer_states([false, false, false, false]),
                    dock_visible: true,
                    appearance_open: false,
                    // Not yet ready: the TRADE pair must lay out in its
                    // disabled state without asking anything of the app.
                    paper: flat_trade(false),
                };
                let actions = draw(ctx, &mut model);
                assert!(actions.is_empty(), "no clicks, no actions");
            });
        }
    }

    /// With a position open, the exit control reaches actual toolbar pixels
    /// — the audit's pain 1 was precisely this button not existing anywhere
    /// the user looks.
    #[test]
    fn an_open_position_paints_the_close_button() {
        let paper = PaperTradeModel {
            close: Some("Close 2 LONG".to_owned()),
            ..flat_trade(true)
        };
        let mut painted = String::new();
        for shape in toolbar_shapes(paper, 1900.0) {
            if let egui::epaint::Shape::Text(text) = shape.shape {
                painted.push_str(text.galley.text());
                painted.push(' ');
            }
        }
        assert!(
            painted.contains("Close 2 LONG"),
            "the exit control never reached the toolbar; painted: {painted}"
        );
        assert!(
            painted.contains("SELL 1") && !painted.contains("(closes"),
            "the face names side and size only; painted: {painted}"
        );
    }

    /// A flat, ready TRADE group — the tests' baseline. Not ready reads as
    /// the simulator before its first print.
    fn flat_trade(ready: bool) -> PaperTradeModel {
        let texts = |face: &str, hover: &str| quantick_paper::desk::EntryTexts {
            face: face.to_owned(),
            label: face.to_owned(),
            hover: hover.to_owned(),
        };
        PaperTradeModel {
            blocked: (!ready).then(|| quantick_paper::desk::NO_MARKET_YET.to_owned()),
            buy: texts("BUY 1", "simulated market buy"),
            sell: texts("SELL 1", "simulated market sell"),
            close: None,
            reverse_hover: "reverse · Shift+R".to_owned(),
        }
    }

    /// The whole toolbar painted at `width`, settled, over a tick chart.
    fn toolbar_shapes(paper: PaperTradeModel, width: f32) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut feed_id = "binance".to_owned();
        let mut symbol = "BTCUSDT".to_owned();
        let mut history_menu_rect = None;
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 400.0),
            )),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(input(), |ctx| {
                let mut layout_picker_open = false;
                let mut model = ToolbarModel {
                    layout_preset: None,
                    layout_picker_open: &mut layout_picker_open,
                    layout_picker_request_open: false,
                    feeds: vec![("binance".to_owned(), "Binance".to_owned())],
                    feed_id: &mut feed_id,
                    feed_display_name: "Binance".to_owned(),
                    symbols: vec!["BTCUSDT".to_owned()],
                    symbol: &mut symbol,
                    replay: None,
                    deal_recording: None,
                    deal_recording_menu: false,
                    focused_bars: quantick_engine::BarKind::Tick.into(),
                    history_menu_rect: &mut history_menu_rect,
                    history: HistoryButton {
                        main: HistoryReach::default(),
                        status: RunStatus::Idle,
                        tz: crate::timezone::TzOffset::default(),
                    },
                    history_candles: 0,
                    older_candles: crate::tab::OlderCandles::NotArrivedYet,
                    capabilities: FeedCapabilities {
                        book_capture: true,
                        history_paging: true,
                        history_boundary: None,
                        traded_volume: true,
                        deal_counter: false,
                        ohlcv_history: true,
                        ohlcv_generation: 0,
                        ohlcv_daily_generation: 0,
                        ohlcv_aggressor_split: false,
                    },
                    layers: layer_states([false, false, false, false]),
                    dock_visible: true,
                    appearance_open: false,
                    paper: paper.clone(),
                };
                let actions = draw(ctx, &mut model);
                assert!(actions.is_empty(), "no clicks, no actions");
            });
            shapes = output.shapes;
        }
        shapes
    }

    /// The BUY and SELL buttons' rectangles in painted shapes, if inline.
    fn entry_rects(
        shapes: &[egui::epaint::ClippedShape],
    ) -> (Option<egui::Rect>, Option<egui::Rect>) {
        let fill = |colour| {
            shapes.iter().find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == colour => Some(rect.rect),
                _ => None,
            })
        };
        (fill(theme::BUY), fill(theme::SELL))
    }

    /// The button names its target idle, names the run and its progress while
    /// loading, and its click becomes the cancel.
    #[test]
    fn the_history_button_names_its_target_and_cancels_a_run() {
        use quantick_feed::history_reach::ReachProgress;
        let tz = crate::timezone::TzOffset::new(-180);
        let idle = HistoryButton {
            main: HistoryReach::Sessions(1),
            status: RunStatus::Idle,
            tz,
        };
        assert!(history_button_label(&idle).ends_with("Yesterday"));
        assert_eq!(
            history_button_action(&idle),
            ToolbarAction::LoadHistory(HistoryReach::Sessions(1))
        );
        // Thursday 2026-10-01 13:14 UTC is 10:14 at UTC-03:00.
        let loading = HistoryButton {
            status: RunStatus::Loading(ReachProgress {
                reach: HistoryReach::Sessions(5),
                oldest_ms: 1_790_860_440_000,
                sessions_reached: 2,
                traded_ms: 0,
                prints_pulled: 0,
                pages: 3,
            }),
            ..idle
        };
        let label = history_button_label(&loading);
        assert!(
            label.starts_with("Loading 5 days\u{2026} back to Thu 10:14"),
            "{label}"
        );
        assert_eq!(
            history_button_action(&loading),
            ToolbarAction::CancelHistory
        );
        let queued = HistoryButton {
            status: RunStatus::Queued(HistoryReach::Hours(2)),
            ..idle
        };
        assert!(history_button_label(&queued).starts_with("Queued +2 h"));
        assert_eq!(history_button_action(&queued), ToolbarAction::CancelHistory);
        assert!(
            HistoryPagingOff::NothingToPage
                .hover()
                .contains("only streams forward"),
            "and a feed that cannot page gets the other reason, not the run's"
        );
    }

    /// Paint the TRADE group alone, settled: the rectangles of BUY, SELL,
    /// Close and Reverse (the last two `None` while flat), with every text
    /// it painted and whether the paint had to cut that text to fit.
    fn paint_trade(paper: &PaperTradeModel) -> (TradeRects, Vec<(String, bool)>) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 200.0),
            )),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(input(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let mut actions = Vec::new();
                        draw_trade(ui, paper, &mut actions);
                        assert!(actions.is_empty(), "no clicks, no actions");
                    });
                });
            });
            shapes = output.shapes;
        }
        let (buy, sell) = entry_rects(&shapes);
        // The outlined pair, in paint order: Close then Reverse.
        let outlined: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill == theme::CONTROL => Some(rect.rect),
                _ => None,
            })
            .collect();
        let texts = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.galley.elided))
                }
                _ => None,
            })
            .collect();
        let rects = TradeRects {
            buy: buy.expect("buy painted"),
            sell: sell.expect("sell painted"),
            close: outlined.first().copied(),
            reverse: outlined.get(1).copied(),
        };
        (rects, texts)
    }

    /// Where the TRADE group's buttons landed.
    #[derive(Debug, PartialEq)]
    struct TradeRects {
        buy: egui::Rect,
        sell: egui::Rect,
        close: Option<egui::Rect>,
        reverse: Option<egui::Rect>,
    }

    /// Every TRADE button keeps one width and one place in every position
    /// state — flat, long, short, a reverse-sized order and the longest
    /// faces the desk writes — so a press lands where it landed before, and
    /// no entry face is cut by its button.
    #[test]
    fn the_trade_buttons_keep_one_place_in_every_state() {
        use quantick_engine::Side;
        use quantick_paper::desk::EntryTexts;
        use quantick_paper::desk::ticket::entry_face;
        use rust_decimal::Decimal;
        let face = |side, qty: Decimal| {
            let face = entry_face(side, Some(qty));
            EntryTexts {
                label: face.clone(),
                hover: face.clone(),
                face,
            }
        };
        let state = |buy: Decimal, sell: Decimal, close: Option<&str>| PaperTradeModel {
            buy: face(Side::Buy, buy),
            sell: face(Side::Sell, sell),
            close: close.map(str::to_owned),
            ..flat_trade(true)
        };
        let one = Decimal::ONE;
        let states = [
            state(one, one, None),
            state(one, one, Some("Close 2 LONG")),
            state(one, one, Some("Close 2 SHORT")),
            state(
                Decimal::from(999),
                Decimal::from(999),
                Some("Close 999 SHORT"),
            ),
            state(
                Decimal::new(888, 5),
                Decimal::from(999_999),
                Some("Close 999 LONG"),
            ),
            state(
                Decimal::from(88_888_888),
                Decimal::from(12_345_678),
                Some("Close 88888888 SHORT"),
            ),
        ];
        let (flat, _) = paint_trade(&states[0]);
        assert_eq!(flat.buy.width(), TRADE_ENTRY_WIDTH_PX);
        assert_eq!(flat.sell.width(), TRADE_ENTRY_WIDTH_PX);
        assert_eq!((flat.close, flat.reverse), (None, None));
        let (long, _) = paint_trade(&states[1]);
        let close = long.close.expect("close painted");
        let reverse = long.reverse.expect("reverse painted");
        assert_eq!(close.width(), TRADE_CLOSE_WIDTH_PX);
        assert_eq!(reverse.width(), TRADE_REVERSE_WIDTH_PX);
        for paper in &states {
            let (rects, texts) = paint_trade(paper);
            assert_eq!(rects.buy, flat.buy, "BUY moved or resized for {paper:?}");
            assert_eq!(rects.sell, flat.sell, "SELL moved or resized for {paper:?}");
            if paper.close.is_some() {
                assert_eq!(rects.close, Some(close), "Close moved for {paper:?}");
                assert_eq!(rects.reverse, Some(reverse), "Reverse moved for {paper:?}");
            }
            for face in [&paper.buy.face, &paper.sell.face] {
                let (_, elided) = texts
                    .iter()
                    .find(|(text, _)| text == face)
                    .unwrap_or_else(|| panic!("{face} never painted"));
                assert!(!elided, "{face} does not fit the fixed width");
            }
        }
        let (_, texts) = paint_trade(&states[3]);
        assert!(
            texts
                .iter()
                .any(|(text, elided)| text == "× Close 999 SHORT" && !elided),
            "a three-digit close fits its fixed width uncut"
        );
    }

    /// Reverse sits beside Close exactly while a position is open, and is
    /// held by the lock that holds the entries.
    #[test]
    fn reverse_shows_only_beside_an_open_position() {
        let painted = |paper: &PaperTradeModel| {
            let (_, texts) = paint_trade(paper);
            texts.iter().any(|(text, _)| text == TRADE_REVERSE_LABEL)
        };
        assert!(!painted(&flat_trade(true)), "flat has no reverse");
        let open = PaperTradeModel {
            close: Some("Close 1 LONG".to_owned()),
            ..flat_trade(true)
        };
        assert!(painted(&open), "an open position wears the reverse");
        let (rects, _) = paint_trade(&open);
        let reverse = rects.reverse.expect("reverse painted").center();
        assert_eq!(
            click_trade(&open, reverse),
            vec![ToolbarAction::PaperReverse]
        );
        let locked = PaperTradeModel {
            blocked: Some("the risk lock is on".to_owned()),
            ..open
        };
        assert!(
            click_trade(&locked, reverse).is_empty(),
            "the lock that holds the entries holds the reverse"
        );
        assert!(click_trade(&locked, rects.buy.center()).is_empty());
        let close = rects.close.expect("close painted").center();
        assert_eq!(
            click_trade(&locked, close),
            vec![ToolbarAction::PaperClose],
            "leaving is never locked"
        );
    }

    /// One primary click at `at` on the TRADE group laid out as
    /// [`paint_trade`] lays it, and what it asked for.
    fn click_trade(paper: &PaperTradeModel, at: egui::Pos2) -> Vec<ToolbarAction> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let mut actions = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
            Vec::new(),
        ] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| draw_trade(ui, paper, &mut actions));
                });
            });
        }
        actions
    }
}
