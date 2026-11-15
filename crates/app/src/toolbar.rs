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

#[cfg(test)]
use crate::config::FeedCapabilities;
use quantick_chart::toolbar_collapse::{
    CollapsePlan, SEPARATOR_AIR_PX, SEPARATOR_WIDTH_PX, W_PICKER, collapse_plan,
};
pub(crate) use quantick_ui_model::chrome_toolbar::available_bar_inputs;
pub(crate) mod bar_parameters;
pub mod capsule;
pub mod layout_popover;
use crate::theme;
use crate::widgets::{IconButton, RAIL_ICON, TOOLBAR_ICON};
use quantick_feed::history_reach::HistoryReach;
#[cfg(test)]
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

pub use quantick_ui_model::chrome_toolbar::{HistoryButton, ReplaySource};

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

/// Borrow the registered projection; only geometry and popup mechanics are mutable.
pub struct ToolbarModel<'a> {
    pub snapshot: &'a quantick_ui_model::chrome_toolbar::Snapshot,
    pub window_menu: WindowMenuDoor<'a>,
    pub capsule: capsule::CapsuleModel<'a>,
    pub capsule_rects: &'a mut capsule::CapsuleRects,
    pub deal_recording_menu: bool,
    pub history_menu_rect: &'a mut Option<egui::Rect>,
}
impl std::ops::Deref for ToolbarModel<'_> {
    type Target = quantick_ui_model::chrome_toolbar::Snapshot;
    fn deref(&self) -> &Self::Target {
        self.snapshot
    }
}

/// What the TRADE group shows this frame: the paper desk's
/// [`TradeBar`](quantick_paper::desk::TradeBar), decided in one read by the
/// headless desk so the toolbar stays decoupled from the simulator's types.
/// The faces name side and size only (`SELL 1`) so the buttons never move;
/// the hovers disclose what the press would do to the open position.
pub type PaperTradeModel = quantick_paper::desk::TradeBar;

pub use quantick_ui_model::chrome_toolbar::Action as ToolbarAction;

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
            let fixed = fixed - crate::add_widget::BUTTON_PX - crate::add_widget::GAP_PX;
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
                    ui.add_space(crate::add_widget::GAP_PX);
                    let add = crate::add_widget::button(ui, model.capsule.add_open);
                    model.capsule_rects.add = Some(add.rect);
                    if add.clicked() {
                        actions.push(ToolbarAction::ToggleAddMenu);
                    }
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
    if draw_market(ui, (model.market.0, &model.market.1), plan.feed_full_name).clicked() {
        actions.push(ToolbarAction::SwitchMarket);
    }
    if let Some(view) = &model.deal_recording
        && let Some(action) = crate::deal_recording_ui::draw_button(
            ui,
            view,
            model.focused_bars.definition(),
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

/// The button's words: the target it loads, or the run it is making.
fn history_button_label(button: &HistoryButton) -> String {
    button.label(icons::CLOCK_COUNTER_CLOCKWISE, icons::X)
}

/// The main click repeats the target; while running it cancels.
fn history_button_action(button: &HistoryButton) -> ToolbarAction {
    button.action()
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
mod tests;
