//! The context toolbar: *what am I looking at* on the left, *what is drawn
//! on it* on the right (`docs/ux/ui-design-model.md` §6).
//!
//! Left to right, each group behind a full-height separator: `≡`, a second
//! door to the window menu the title bar's `⋮` opens; the tab's market as a
//! dropdown button that opens the palette in switch mode (or the amber
//! session label while replaying), with REC beside it; HISTORY (a
//! `+ older ▾` split button whose menu holds the page size); TRADE, the
//! simulated BUY/SELL market buttons (`docs/ux/paper-trading.md`). Free
//! space, then the [`capsule`] — the live layout miniature and the
//! workspace, the floppy glued left while it holds edits — and, alone at the
//! edge, PANELS (dock show/hide, a tray). Appearance has no toolbar button:
//! the chart itself and the Settings window open it. The toolbar never wraps:
//! [`collapse_plan`] folds groups into the `⋯` overflow menu in the §6 order —
//! PANELS → HISTORY → TRADE → the feed name shrinks to its initial. The
//! symbol, the capsule and `≡` never fold. Bars, indicators and layers
//! belong to each pane, on its header line (`crate::pane_header`).
//!
//! The widgets edit the app's state through [`ToolbarModel`]'s borrows;
//! anything with a side effect beyond a field write comes back as a
//! [`ToolbarAction`] for the app to carry out.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::config::FeedCapabilities;
use quantick_chart::toolbar_collapse::{
    CollapsePlan, SEPARATOR_AIR_PX, SEPARATOR_WIDTH_PX, W_PICKER, collapse_plan,
};
use quantick_engine::bar_selection::BarInputAvailability;
pub(crate) mod bar_parameters;
pub mod capsule;
pub mod layout_popover;
use crate::theme;
use crate::widgets::{IconButton, RAIL_ICON, TOOLBAR_ICON};
use quantick_feed::history_reach::HistoryReach;
use quantick_feed::history_run::RunStatus;
use quantick_paper::desk::{
    TRADE_CLOSE_WIDTH_PX, TRADE_ENTRY_WIDTH_PX, TRADE_GAP_PX, TRADE_REVERSE_LABEL,
    TRADE_REVERSE_WIDTH_PX, W_TRADE,
};
use quantick_session::{HistoryPagingOff, fmt_history_span};

/// Height of the toolbar, in pixels (§5 zone 2).
pub const TOOLBAR_HEIGHT: f32 = 44.0;

/// How far the tray's ink sits inside its hit target. The tray wears the
/// dock strip's larger [`RAIL_ICON`], whose glyph inks 16 px (measured).
const DOCK_INSET_PX: f32 = (RAIL_ICON.hit - 16.0) / 2.0;
/// Gap between the market button and REC, in pixels.
const MARKET_REC_GAP_PX: f32 = 10.0;
/// The row's padding: `≡` centred on the tool rail's column 22 px in, 12 px
/// after the tray's glyph.
const ROW_MARGIN: egui::Margin = egui::Margin {
    left: 22.0 - TOOLBAR_ICON.hit / 2.0,
    right: 12.0 - DOCK_INSET_PX,
    top: 0.0,
    bottom: 0.0,
};

/// What `≡` and its separator take from the row, beside what the collapse
/// plan counts.
const WINDOW_MENU_RUN_PX: f32 = TOOLBAR_ICON.hit + SEPARATOR_AIR_PX * 2.0 + SEPARATOR_WIDTH_PX;

/// Gap between History's main button and its caret, in pixels.
const HISTORY_SPLIT_GAP_PX: f32 = 6.0;

/// The amber label that replaces SOURCE while a recording plays.
pub struct ReplaySource {
    /// What the label says.
    pub label: String,
    /// The hover detail (file, side source).
    pub hover: String,
}

/// The toolbar's `≡`: the window menu the title bar's `⋮` opens, one door
/// nearer the chart.
pub struct WindowMenuDoor<'a> {
    /// Whether the menu is up and hangs from this button.
    pub open: bool,
    /// The button's hover, shortcut included.
    pub hover: String,
    /// Where the button ended up, written back so the menu hangs from it.
    pub rect: &'a mut Option<egui::Rect>,
}

/// A toolbar control as drawn, for the observer scene.
#[derive(Clone, Debug)]
pub struct DrawnControl {
    pub id: String,
    pub label: String,
    pub rect: egui::Rect,
    pub selected: bool,
    /// The capability that does what pressing it does.
    pub capability: Option<&'static str>,
}

/// The toolbar's view of the app: fields it edits directly, and read-only
/// context for gating and display.
pub struct ToolbarModel<'a> {
    /// The `≡` at the row's left end.
    pub window_menu: WindowMenuDoor<'a>,
    /// The capsule: the live layout and the tab's workspace.
    pub capsule: capsule::CapsuleModel<'a>,
    /// Where the capsule's parts ended up, written back by the draw.
    pub capsule_rects: &'a mut capsule::CapsuleRects,
    /// The market button: the venue's letter and the tab chip's own
    /// `SYMBOL · venue`, so the two cannot disagree.
    pub market: (char, &'a str),
    /// Present while a recording is the source; replaces the market label.
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
    /// Whether the dock (strip included) is shown.
    pub dock_visible: bool,
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
    /// Open the palette in switch mode on this tab (the market button).
    SwitchMarket,
    /// What the REC control beside the symbol asked for.
    DealRecording(crate::deal_recording::DealRecordingAction),
    /// Fetch and prepend one more span of older venue candles.
    ///
    /// The trade twin above and this one are two different records — a page of
    /// prints and a span of buckets — served by two different venue endpoints
    /// under two different capabilities. A feed can page one and not the
    /// other, so they are two actions rather than one with a mode.
    LoadOlderCandles,
    /// Show or hide the dock.
    ToggleDock,
    /// Open or close the window menu, hung from the toolbar's `≡`.
    ToggleWindowMenu,
    /// Open or close the layout popover, hung from the capsule's miniature.
    ToggleLayoutPopover,
    /// Open or close the workspace menu, hung from the capsule's name.
    ToggleWorkspaceMenu,
    /// Save the tab's edits to its workspace (the floppy, Ctrl+S).
    SaveWorkspace,
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
    let palette = theme::get(ctx);
    let mut actions = Vec::new();
    egui::TopBottomPanel::top(egui::Id::new((ctx.viewport_id(), "toolbar")))
        .exact_height(TOOLBAR_HEIGHT)
        .frame(
            // One sheet with the active market tab above it.
            egui::Frame::none()
                .fill(*palette.get(quantick_appearance::Token::ToolbarSheet))
                .inner_margin(ROW_MARGIN),
        )
        .show(ctx, |ui| {
            let rec_width = if model.deal_recording.is_some() {
                crate::deal_recording_ui::REC_BUTTON_MIN_WIDTH_PX + MARKET_REC_GAP_PX
            } else {
                0.0
            };
            let fixed = ui.available_width() - rec_width - WINDOW_MENU_RUN_PX + W_PICKER;
            let mut plan = collapse_plan(fixed - capsule::width(ui, &model.capsule), W_TRADE);
            if !plan.trade_inline {
                model.capsule.compact = true;
                plan = collapse_plan(fixed - capsule::width(ui, &model.capsule), W_TRADE);
            }
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let door = &mut model.window_menu;
                let list = IconButton::new(icons::LIST, TOOLBAR_ICON)
                    .active(door.open)
                    .hover_text(&door.hover)
                    .show(ui);
                *door.rect = Some(list.rect);
                if list.clicked() {
                    actions.push(ToolbarAction::ToggleWindowMenu);
                }
                row_separator(ui, SEPARATOR_AIR_PX, SEPARATOR_AIR_PX);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = MARKET_REC_GAP_PX;
                    draw_source(ui, model, plan, &mut actions);
                });
                if plan.history_inline {
                    row_separator(ui, SEPARATOR_AIR_PX, SEPARATOR_AIR_PX);
                    draw_history(ui, model, &mut actions);
                }
                if plan.trade_inline {
                    row_separator(ui, SEPARATOR_AIR_PX, SEPARATOR_AIR_PX);
                    draw_trade(ui, &model.paper, &mut actions);
                    row_separator(ui, SEPARATOR_AIR_PX, 0.0);
                }
                // The right run grows inwards, opening free space after HISTORY.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if plan.overflow_needed() {
                        draw_overflow(ui, model, plan, &mut actions);
                    }
                    if plan.panels_inline {
                        let panels = IconButton::new(icons::TRAY, RAIL_ICON)
                            .active(model.dock_visible)
                            .hover_text("show or hide the panels dock (Ctrl+Shift+B)")
                            .show(ui);
                        if panels.clicked() {
                            actions.push(ToolbarAction::ToggleDock);
                        }
                    }
                    row_separator(ui, SEPARATOR_AIR_PX - DOCK_INSET_PX, SEPARATOR_AIR_PX);
                    *model.capsule_rects = capsule::draw(ui, &model.capsule, &mut actions);
                });
            });
        });
    actions
}

/// A group separator: the row's height but one pixel, in the palette's
/// border colour, with `before` pixels of air on the side drawn so far.
fn row_separator(ui: &mut egui::Ui, before: f32, after: f32) {
    ui.add_space(before);
    let size = egui::vec2(SEPARATOR_WIDTH_PX, TOOLBAR_HEIGHT - 1.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme::get(ui.ctx()).border());
    ui.add_space(after);
}

/// The market button (venue letter, symbol, muted venue, caret) whose click
/// opens the palette in switch mode, or the amber session label during replay.
fn draw_source(
    ui: &mut egui::Ui,
    model: &mut ToolbarModel,
    plan: CollapsePlan,
    actions: &mut Vec<ToolbarAction>,
) {
    let palette = theme::get(ui.ctx());
    if let Some(replay) = &model.replay {
        ui.label(egui::RichText::new("source:").color(palette.text_muted()));
        ui.label(
            egui::RichText::new(&replay.label)
                .color(palette.amber())
                .strong(),
        )
        .on_hover_text(&replay.hover);
        return;
    }
    if draw_market(ui, model.market, plan.feed_full_name).clicked() {
        actions.push(ToolbarAction::SwitchMarket);
    }
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

/// The market button's hover.
const MARKET_HOVER: &str = "Switch this tab's market \u{b7} Ctrl+T opens a new tab instead";
/// Room the venue letter's box takes before the symbol.
const MARKET_CHIP_PX: f32 = 16.0;

/// The market as one dropdown button, bordered at rest like History (bare
/// until hovered under [`theme::MARKET_BUTTON_QUIET`]); the venue shrinks to
/// its initial when the row is short.
fn draw_market(ui: &mut egui::Ui, (chip, label): (char, &str), full: bool) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let (symbol, venue) = quantick_workspace::arrangement::strip::chip_parts(label);
    let initial = venue.chars().next().map_or(0, char::len_utf8);
    let venue = if full { venue } else { &venue[..initial] };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let format = |color| egui::TextFormat::simple(font.clone(), color);
    let mut job = egui::text::LayoutJob::default();
    job.append(symbol, MARKET_CHIP_PX + 6.0, format(palette.text_primary()));
    job.append(
        &format!(" \u{b7} {venue}"),
        0.0,
        format(palette.text_muted()),
    );
    job.append(icons::CARET_DOWN, 6.0, format(palette.text_faint()));
    let response = ui
        .scope(|ui| {
            if theme::MARKET_BUTTON_QUIET {
                let idle = &mut ui.visuals_mut().widgets.inactive;
                (idle.weak_bg_fill, idle.bg_stroke) =
                    (egui::Color32::TRANSPARENT, egui::Stroke::NONE);
            }
            ui.add(egui::Button::new(job))
        })
        .inner;
    let left = response.rect.left() + ui.spacing().button_padding.x;
    let size = egui::Vec2::splat(MARKET_CHIP_PX);
    let mark = egui::Rect::from_min_size(egui::pos2(left, response.rect.center().y - 8.0), size);
    let painter = ui.painter();
    painter.rect(
        mark,
        4.0,
        palette.inset(),
        egui::Stroke::new(1.0_f32, palette.border()),
    );
    let letter = egui::FontId::proportional(9.5);
    painter.text(
        mark.center(),
        egui::Align2::CENTER_CENTER,
        chip,
        letter,
        palette.text_muted(),
    );
    response.on_hover_text(MARKET_HOVER)
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
        .on_hover_text(model.history.status.button_hover(model.history.main))
        .on_disabled_hover_text(history_disabled_hover(model));
    if load.clicked() {
        actions.push(history_button_action(&model.history));
    }
    ui.add_space(HISTORY_SPLIT_GAP_PX);
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
    if button.status.running() {
        ToolbarAction::CancelHistory
    } else {
        ToolbarAction::LoadHistory(button.main)
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
        let running = model.history.status.running();
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
    let palette = theme::get(ui.ctx());
    // The entries speak the chip language: solid side colour, dark ink —
    // the same grammar as the price chips in the gutter, because the button
    // and the entry line it will create are one statement. Both wear one
    // fixed size in every state, the face truncating rather than growing,
    // so opening a position never slides SELL out from under the pointer.
    let entry = |ui: &mut egui::Ui, label: &str, fill: egui::Color32, enabled: bool| {
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    egui::vec2(TRADE_ENTRY_WIDTH_PX, 24.0),
                    egui::Button::new(
                        egui::RichText::new(label)
                            .color(if !enabled && !palette.resolved.dark {
                                palette.text_primary()
                            } else {
                                crate::paper_chrome::trade_action_ink(&palette, fill)
                            })
                            .strong(),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .fill(if !enabled && !palette.resolved.dark {
                        palette.control()
                    } else {
                        fill
                    })
                    .stroke(egui::Stroke::NONE)
                    .rounding(egui::Rounding::same(3.0)),
                )
            })
            .inner;
        if !enabled && !palette.resolved.dark {
            crate::paper_chrome::disabled_action_label(
                ui,
                &response,
                label,
                egui::TextWrapMode::Truncate,
            );
        }
        response
    };
    let enabled = paper.blocked.is_none();
    let locked = paper.blocked.as_deref().unwrap_or_default();
    // Left-aligned after History: the exits open right of the entries,
    // which never move.
    let flat = 2.0 * TRADE_ENTRY_WIDTH_PX + TRADE_GAP_PX;
    let width = if paper.close.is_some() { W_TRADE } else { flat };
    let row = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(egui::vec2(width, 24.0), row, |ui| {
        ui.spacing_mut().item_spacing.x = TRADE_GAP_PX;
        let buy = entry(ui, &paper.buy.face, palette.buy(), enabled)
            .on_hover_text(&paper.buy.hover)
            .on_disabled_hover_text(locked);
        #[cfg(test)]
        ui.data_mut(|data| data.insert_temp(egui::Id::new("toolbar_buy_response"), buy.id));
        if buy.clicked() {
            actions.push(ToolbarAction::PaperBuy);
        }
        let sell = entry(ui, &paper.sell.face, palette.sell(), enabled)
            .on_hover_text(&paper.sell.hover)
            .on_disabled_hover_text(locked);
        if sell.clicked() {
            actions.push(ToolbarAction::PaperSell);
        }
        // The exit controls, on the same surface as the entries: an open
        // position must never require a dock dive to leave or flip (audit
        // pain 1, B2). Outlined, not filled — leaving is deliberate. Fixed
        // widths too, so a flip never slides the reverse. Close is never
        // locked; the reverse opens a position, so the entries' lock holds it.
        if let Some(close) = &paper.close {
            let outlined = |ui: &mut egui::Ui, text: String, width: f32, enabled: bool| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        egui::vec2(width, 24.0),
                        egui::Button::new(
                            egui::RichText::new(text)
                                .color(palette.text_primary())
                                .strong(),
                        )
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .fill(palette.control())
                        .stroke(egui::Stroke::new(1.0_f32, palette.border()))
                        .rounding(egui::Rounding::same(3.0)),
                    )
                })
                .inner
            };
            let button = outlined(ui, format!("× {close}"), TRADE_CLOSE_WIDTH_PX, true)
                .on_hover_text(format!(
                    "{close}: exit the open simulated position at the next print (market)"
                ));
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
    });
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
        if !plan.panels_inline
            && ui
                .button(format!("{} Show/hide panels", icons::TRAY))
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
                .on_hover_text(model.history.status.button_hover(model.history.main))
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

    use super::*;

    #[test]
    fn trade_actions_keep_readable_labels() {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(quantick_appearance::LIGHT).unwrap();
        theme::sync(&ctx, &appearance);
        for ready in [false, true] {
            for state in 0..3 {
                let hovered = state == 1;
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
                let mut actions = Vec::new();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 100.0),
                        )),
                        events: if hovered {
                            vec![egui::Event::PointerMoved(egui::pos2(40.0, 20.0))]
                        } else if state == 2 {
                            vec![
                                egui::Event::PointerGone,
                                egui::Event::Key {
                                    key: egui::Key::Tab,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ]
                        } else {
                            vec![egui::Event::PointerGone]
                        },
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| draw_trade(ui, &flat_trade(ready), &mut actions));
                    },
                );
                assert!(actions.is_empty());
                if state == 2 {
                    let id = ctx
                        .data(|data| {
                            data.get_temp::<egui::Id>(egui::Id::new("toolbar_buy_response"))
                        })
                        .unwrap();
                    assert_eq!(
                        ctx.memory(|memory| memory.has_focus(id)),
                        ready,
                        "BUY focus follows enabled state"
                    );
                }

                let buy = output
                    .shapes
                    .iter()
                    .rev()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "BUY 1" => Some(text),
                        _ => None,
                    })
                    .expect("BUY label");
                let ink = buy
                    .override_text_color
                    .unwrap_or_else(|| buy.galley.rows[0].visuals.mesh.vertices[0].color);
                let fill = output
                    .shapes
                    .iter()
                    .rev()
                    .find_map(|paint| match &paint.shape {
                        egui::Shape::Rect(rect)
                            if rect.rect.contains(buy.visual_bounding_rect().center())
                                && rect.fill.a() == 255 =>
                        {
                            Some(rect.fill)
                        }
                        _ => None,
                    })
                    .expect("painted button surface");
                let a = theme::relative_luminance(ink);
                let b = theme::relative_luminance(fill);
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }

    /// The row's horizontal padding, which the plan never sees.
    const ROW_MARGINS: f32 = ROW_MARGIN.left + ROW_MARGIN.right;

    /// What `draw` takes off for REC: the button and the gap before it.
    const REC_WIDTH: f32 = crate::deal_recording_ui::REC_BUTTON_MIN_WIDTH_PX + MARKET_REC_GAP_PX;

    /// Every group fits a 1280 px window and a 1920 px one with REC, at
    /// TRADE's reserved width: the tray stays at the right edge.
    #[test]
    fn nothing_folds_at_the_common_window_widths() {
        for available in [
            1280.0 - ROW_MARGINS,
            1920.0 - ROW_MARGINS,
            1920.0 - ROW_MARGINS - REC_WIDTH,
        ] {
            assert_eq!(
                collapse_plan(available, W_TRADE),
                CollapsePlan::FULL,
                "{available} px"
            );
        }
    }

    /// The 1100 px first-launch window, and 1280 px with REC, no longer hold
    /// the full row since the picker and the separators grew: PANELS and
    /// HISTORY fold into `⋯` together and TRADE stays inline.
    #[test]
    fn a_narrow_window_folds_panels_and_history_but_keeps_trade() {
        for available in [1100.0 - ROW_MARGINS, 1280.0 - ROW_MARGINS - REC_WIDTH] {
            let plan = collapse_plan(available, W_TRADE);
            assert!(
                !plan.panels_inline && !plan.history_inline,
                "{available} px"
            );
            assert!(plan.trade_inline, "{available} px");
        }
    }

    /// Separator air is exact on both sides of every separator: the gap a
    /// group draws inside it never trails into the air after it.
    #[test]
    fn every_separator_has_its_air_on_both_sides() {
        let shapes = toolbar_shapes(flat_trade(true), 1920.0);
        let rules: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.rect.width() == SEPARATOR_WIDTH_PX
                        && rect.rect.height() == TOOLBAR_HEIGHT - 1.0 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rules.len(), 5, "five full-height separators: {rules:?}");
        let (buy, _) = entry_rects(&shapes);
        assert_eq!(
            rules[2].right() + SEPARATOR_AIR_PX,
            buy.expect("BUY").left()
        );
        let history = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.rect.left() > rules[1].right()
                        && rect.rect.right() < rules[2].left() =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .map(|rect| rect.left())
            .fold(f32::INFINITY, f32::min);
        let market_right = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.rect.left() > rules[0].right()
                        && rect.rect.right() < rules[1].left() =>
                {
                    Some(rect.rect.right())
                }
                _ => None,
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(
            rules[1].left() - market_right,
            SEPARATOR_AIR_PX,
            "air before the History rule"
        );
        assert_eq!(
            history - rules[1].right(),
            SEPARATOR_AIR_PX,
            "air after the History rule"
        );
    }

    /// The row draws the list door, market, history, trade, capsule and
    /// panels, left to right: asserted on the painted shapes of a full-width
    /// toolbar, the capsule by its workspace name.
    #[test]
    fn the_row_draws_door_market_history_trade_capsule_panels() {
        let shapes = toolbar_shapes(flat_trade(true), 1920.0);
        let (buy, sell) = entry_rects(&shapes);
        let (buy, sell) = (buy.expect("BUY inline"), sell.expect("SELL inline"));
        let text_x = |needle: &str| {
            shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text().contains(needle) => {
                        Some(text.pos.x)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{needle} was not painted"))
        };
        let order = [
            text_x(icons::LIST),
            text_x("BTCUSDT"),
            text_x("History"),
            buy.left(),
            sell.left(),
            text_x("Default"),
            text_x(icons::TRAY),
        ];
        assert!(
            order.is_sorted(),
            "door, market, history, BUY, SELL, capsule, tray: {order:?}"
        );
        assert_eq!(sell.left() - buy.right(), TRADE_GAP_PX);
        assert_eq!(buy.size(), egui::vec2(TRADE_ENTRY_WIDTH_PX, 24.0));
        assert!(
            sell.right() < 1920.0 / 2.0,
            "TRADE stands left, after History"
        );
        assert!(order[6] > 1920.0 - 40.0, "the tray sits at the right edge");
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
        let mut history_menu_rect = None;
        for replaying in [false, true] {
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let mut capsule_rects = capsule::CapsuleRects::default();
                    let mut model = ToolbarModel {
                        window_menu: WindowMenuDoor {
                            open: false,
                            hover: String::new(),
                            rect: &mut None,
                        },
                        capsule: capsule::tests::model(),
                        capsule_rects: &mut capsule_rects,
                        market: ('B', "BTCUSDT \u{b7} Binance"),
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
                            transport: true,
                            book_capture: !replaying,
                            history_paging: !replaying,
                            history_boundary: None,
                            traded_volume: true,
                            deal_counter: false,
                            ohlcv_history: !replaying,
                            ohlcv_generation: 0,
                            ohlcv_daily_generation: 0,
                            ohlcv_aggressor_split: false,
                            native_candle_intervals: false,
                            liquidations: false,
                            open_interest: false,
                            funding: false,
                            spot_perp_pair: false,
                        },
                        dock_visible: true,
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
        let mut history_menu_rect = None;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                let mut capsule_rects = capsule::CapsuleRects::default();
                let mut model = ToolbarModel {
                    window_menu: WindowMenuDoor {
                        open: false,
                        hover: String::new(),
                        rect: &mut None,
                    },
                    capsule: capsule::tests::model(),
                    capsule_rects: &mut capsule_rects,
                    market: ('B', "US500 \u{b7} MetaTrader 5"),
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
                        transport: true,
                        book_capture: false,
                        history_paging: false,
                        history_boundary: None,
                        traded_volume: false,
                        deal_counter: false,
                        ohlcv_history: false,
                        ohlcv_generation: 0,
                        ohlcv_daily_generation: 0,
                        ohlcv_aggressor_split: false,
                        native_candle_intervals: false,
                        liquidations: false,
                        open_interest: false,
                        funding: false,
                        spot_perp_pair: false,
                    },
                    dock_visible: true,
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
                let mut capsule_rects = capsule::CapsuleRects::default();
                let mut model = ToolbarModel {
                    window_menu: WindowMenuDoor {
                        open: false,
                        hover: String::new(),
                        rect: &mut None,
                    },
                    capsule: capsule::tests::model(),
                    capsule_rects: &mut capsule_rects,
                    market: ('B', "BTCUSDT \u{b7} Binance"),
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
                        transport: true,
                        book_capture: true,
                        history_paging: true,
                        history_boundary: None,
                        traded_volume: true,
                        deal_counter: false,
                        ohlcv_history: true,
                        ohlcv_generation: 0,
                        ohlcv_daily_generation: 0,
                        ohlcv_aggressor_split: false,
                        native_candle_intervals: false,
                        liquidations: false,
                        open_interest: false,
                        funding: false,
                        spot_perp_pair: false,
                    },
                    dock_visible: true,
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
                    // Left to right, as in the toolbar's left run.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
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
                    let row = egui::Layout::left_to_right(egui::Align::Min);
                    ui.with_layout(row, |ui| draw_trade(ui, paper, &mut actions));
                });
            });
        }
        actions
    }
}
