//! The context toolbar: *what am I looking at* on the left, *what is drawn
//! on it* on the right (`docs/ux/ui-design-model.md` §6).
//!
//! Left: SOURCE (feed + symbol, or the amber session label while replaying),
//! BARS (kind + one parameter), HISTORY (a `+ older ▾` split button whose
//! menu holds the page size). Right: LAYERS (one icon toggle per visual
//! layer; right-click opens its dock tab), LOOK (appearance dialog), PANELS
//! (dock show/hide). Between HISTORY and the right groups sits TRADE — the
//! simulated BUY/SELL market buttons (`docs/ux/paper-trading.md`). The
//! toolbar never wraps: [`collapse_plan`] folds groups into the `⋯` overflow
//! menu in the §6 order — LOOK → PANELS → HISTORY → TRADE → bar parameter
//! merges into the kind combo → the feed name shrinks to its initial. The
//! symbol and LAYERS never fold.
//!
//! The widgets edit the app's state through [`ToolbarModel`]'s borrows;
//! anything with a side effect beyond a field write comes back as a
//! [`ToolbarAction`] for the app to carry out.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::chart_layers::{ChartLayer, LayerBlock};
use crate::config::FeedCapabilities;
use crate::dock::DockTab;
use crate::state::{BarKind, ImbalanceUnit};
use crate::theme;
use crate::widgets::{IconButton, TOOLBAR_ICON};

/// Height of the toolbar, in pixels (§5 zone 2).
pub const TOOLBAR_HEIGHT: f32 = 44.0;

// The LAYERS group's glyphs. Named here, and asserted as a set by
// `the_layer_toggles_speak_one_visual_language`, because a test that spells
// the literal itself proves nothing: revert the button and it still passes.
// The two layers that own a dock tab take the tab's glyph instead of a
// constant here, so a toggle and the panel behind it cannot drift apart.
/// The indicators menu: a plotted line.
const LAYER_INDICATORS_ICON: &str = icons::CHART_LINE;
/// The live strip: a sideways histogram, which is what it draws.
const LAYER_STRIP_ICON: &str = icons::CHART_BAR_HORIZONTAL;
/// The footprint: the grid of per-price cells the layer draws inside each
/// candle. Same alphabet rule as its neighbours — the glyph is the shape on
/// the chart, never a metaphor for it.
const LAYER_FOOTPRINT_ICON: &str = icons::GRID_NINE;

// Width estimates for the overflow rule, in pixels. egui sizes widgets while
// drawing them, so the plan is decided up front from these; they only need to
// be right enough that folding starts before clipping does.
/// Margins, separators and inter-group spacing.
const W_SLACK: f32 = 90.0;
/// SOURCE with the full feed name.
const W_SOURCE_FULL: f32 = 350.0;
/// SOURCE with the feed shrunk to its initial.
const W_SOURCE_SHRUNK: f32 = 260.0;
/// BARS without the parameter widget.
const W_BARS: f32 = 160.0;
/// The bar-parameter label + drag value (every kind but time).
const W_BAR_PARAM: f32 = 150.0;
/// The time kind's parameter row: four preset chips plus the custom drag.
const W_TIME_PARAM: f32 = 260.0;
/// The imbalance kind's parameter row: three unit chips plus the target
/// label and drag. Underestimating this makes the collapse plan draw a row
/// wider than it budgeted instead of folding it.
const W_IMBALANCE_PARAM: f32 = 330.0;
/// The `+ older ▾` split button.
const W_HISTORY: f32 = 100.0;
/// The LAYERS icon group (bubbles, heatmap, live strip, indicators).
const W_LAYERS: f32 = 128.0;
/// One 28 px icon button (LOOK, PANELS, or the overflow `⋯`).
const W_ICON: f32 = 28.0;
/// Estimated width of one glyph in a TRADE button label, in pixels.
const TRADE_GLYPH_PX: f32 = 7.0;
/// One TRADE button's own horizontal padding, in pixels.
const TRADE_BUTTON_PAD_PX: f32 = 16.0;
/// The close button's `× ` prefix, in pixels.
const TRADE_CLOSE_PREFIX_PX: f32 = 14.0;

/// Which groups stay inline at the current width. Everything folded is
/// reachable through the `⋯` overflow menu instead — never hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollapsePlan {
    /// LOOK renders as its own icon.
    pub look_inline: bool,
    /// PANELS renders as its own icon.
    pub panels_inline: bool,
    /// HISTORY renders as the split button.
    pub history_inline: bool,
    /// TRADE renders as the BUY/SELL pair.
    pub trade_inline: bool,
    /// The bar parameter renders next to the kind combo.
    pub param_inline: bool,
    /// The feed combo shows its full display name.
    pub feed_full_name: bool,
}

impl CollapsePlan {
    /// Nothing folded.
    const FULL: Self = Self {
        look_inline: true,
        panels_inline: true,
        history_inline: true,
        trade_inline: true,
        param_inline: true,
        feed_full_name: true,
    };

    /// Whether the `⋯` menu is needed to reach a folded group. A shrunk feed
    /// name loses no affordance, so it alone needs no overflow.
    #[must_use]
    pub fn overflow_needed(self) -> bool {
        !(self.look_inline
            && self.panels_inline
            && self.history_inline
            && self.trade_inline
            && self.param_inline)
    }

    /// Estimated width of the toolbar under this plan. The `⋯` slot is
    /// always reserved: it appears exactly when something folds, so counting
    /// it conditionally would make the first fold width-neutral and leave
    /// the planner skipping straight past it. `trade_width` and
    /// `param_width` come in as parameters: the TRADE group's state-aware
    /// labels grow and shrink with the position, and the time kind's chip
    /// row is wider than the other kinds' single drag.
    fn width(self, trade_width: f32, param_width: f32) -> f32 {
        let mut width = W_SLACK + W_ICON + W_BARS + W_LAYERS;
        width += if self.feed_full_name {
            W_SOURCE_FULL
        } else {
            W_SOURCE_SHRUNK
        };
        if self.param_inline {
            width += param_width;
        }
        if self.history_inline {
            width += W_HISTORY;
        }
        if self.trade_inline {
            width += trade_width;
        }
        if self.look_inline {
            width += W_ICON;
        }
        if self.panels_inline {
            width += W_ICON;
        }
        width
    }
}

/// Decide what folds at `available` width, following the §6 collapse order.
/// The last plan is accepted even when it still overflows — there is nothing
/// left to fold, and the symbol and LAYERS are never candidates.
#[must_use]
pub fn collapse_plan(available: f32, trade_width: f32, param_width: f32) -> CollapsePlan {
    let mut plan = CollapsePlan::FULL;
    let steps: [fn(&mut CollapsePlan); 6] = [
        |plan| plan.look_inline = false,
        |plan| plan.panels_inline = false,
        |plan| plan.history_inline = false,
        |plan| plan.trade_inline = false,
        |plan| plan.param_inline = false,
        |plan| plan.feed_full_name = false,
    ];
    for fold in steps {
        if plan.width(trade_width, param_width) <= available {
            return plan;
        }
        fold(&mut plan);
    }
    plan
}

/// Estimated TRADE width for the collapse plan. The state-aware labels
/// (`SELL 5 (reverses to short 4)`, the close button) change with the
/// position, so the fold decision must read them rather than a constant.
#[must_use]
pub fn trade_width(paper: &PaperTradeModel) -> f32 {
    let button = |label: &str| TRADE_BUTTON_PAD_PX + label.chars().count() as f32 * TRADE_GLYPH_PX;
    let mut width = button(&paper.buy_label) + button(&paper.sell_label);
    if let Some(close) = &paper.close_label {
        width += button(close) + TRADE_CLOSE_PREFIX_PX;
    }
    width
}

/// Estimated width of the selected kind's parameter row, for the collapse
/// plan: the time kind carries the preset chips (audit QW1), so its row is
/// wider than the single label + drag every other kind shows.
#[must_use]
pub fn param_width(kind: BarKind) -> f32 {
    match kind {
        BarKind::Time => W_TIME_PARAM,
        BarKind::Imbalance => W_IMBALANCE_PARAM,
        _ => W_BAR_PARAM,
    }
}

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
    /// The selected bar kind.
    pub kind: &'a mut BarKind,
    /// Parameter for [`BarKind::Tick`].
    pub tick_n: &'a mut u64,
    /// Parameter for [`BarKind::Volume`].
    pub volume_units: &'a mut f64,
    /// Parameter for [`BarKind::Dollar`].
    pub dollar_notional: &'a mut f64,
    /// Parameter for [`BarKind::Time`].
    pub time_interval_ms: &'a mut i64,
    /// Parameter for [`BarKind::Imbalance`].
    pub imbalance_target: &'a mut u64,
    /// What θ accumulates for [`BarKind::Imbalance`]: trades, volume or
    /// dollar (López de Prado's TIB/VIB/DIB).
    pub imbalance_unit: &'a mut ImbalanceUnit,
    /// Trades pulled per "+ older" click.
    pub history_step: &'a mut usize,
    /// Trades backfilled so far, for the history menu readout.
    pub history_trades: usize,
    /// What the active source's backend can do.
    pub capabilities: FeedCapabilities,
    /// The LAYERS group's four lamps, in [`LayerToggle::ALL`] order: whether
    /// each layer is drawn, and what blocks it where something does. Both
    /// come from `ChartPane::layer_blocked` / `layer_visible` through
    /// [`crate::tab::Tab::layer_toggle_state`], which the semantic scene reads
    /// too — so a button cannot tell the trader one thing and an assistant
    /// another.
    pub layers: [LayerToggleState; 4],
    /// Whether the dock (strip included) is shown.
    pub dock_visible: bool,
    /// Whether the appearance dialog is open.
    pub appearance_open: bool,
    /// The TRADE group: readiness, state-aware entry labels and the close
    /// button, all computed by the paper host (`PaperTrading`).
    pub paper: PaperTradeModel,
    /// Active indicators, for the INDICATORS menu (add order).
    pub indicators: Vec<IndicatorMenuEntry>,
    /// Loadable script names, embedded first (the INDICATORS menu's "add"
    /// section; indices map straight to [`ToolbarAction::AddScriptIndicator`]).
    pub scripts: Vec<String>,
}

/// What the TRADE group shows this frame, computed by the paper host so the
/// toolbar stays decoupled from the simulator's types. The labels disclose
/// what the press would do to the open position (`SELL 1 (closes)`), because
/// the quantity deciding it lives in a tab the toolbar never shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperTradeModel {
    /// Whether the simulator has seen a price — the buttons disable
    /// themselves (with the reason) until it has.
    pub ready: bool,
    /// The buy button's state-aware label.
    pub buy_label: String,
    /// The sell button's state-aware label.
    pub sell_label: String,
    /// The buy button's hover text (quantity and bracket disclosure).
    pub buy_hover: String,
    /// The sell button's hover text.
    pub sell_hover: String,
    /// `Close 1 LONG` while a position is open; `None` removes the button.
    pub close_label: Option<String>,
}

impl PaperTradeModel {
    /// A flat, ready TRADE group — the tests' baseline.
    #[cfg(test)]
    fn flat(ready: bool) -> Self {
        Self {
            ready,
            buy_label: "BUY 1".to_owned(),
            sell_label: "SELL 1".to_owned(),
            buy_hover: "simulated market buy".to_owned(),
            sell_hover: "simulated market sell".to_owned(),
            close_label: None,
        }
    }
}

/// One active indicator as the INDICATORS menu shows it. Raw slot numbers
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
    /// Fetch and prepend one page of older trades.
    LoadOlder,
    /// Show or hide the L2 depth map. Display-only: the recorder keeps
    /// running, so reopening the map brings its history back whole.
    SetHeatmap(bool),
    /// Turn the aggression layer on or off.
    SetBubbles(bool),
    /// Show or hide the live strip (the book's current depth beside the
    /// price axis). Display-only: capture is untouched.
    SetLiveStrip(bool),
    /// Show or hide the candle footprint — the per-price ladder inside the
    /// bars. Display-only: the ladders keep accumulating either way.
    SetFootprint(bool),
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
    /// Add the native EMA overlay (M1's hardcoded entry; the script library
    /// browser replaces this menu in M2).
    AddEmaIndicator,
    /// Add the native CVD pane.
    AddCvdIndicator,
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
            let plan = collapse_plan(
                ui.available_width(),
                trade_width(&model.paper),
                param_width(*model.kind),
            );
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                draw_source(ui, model, plan);
                ui.separator();
                draw_bars(ui, model, plan);
                if plan.history_inline {
                    ui.separator();
                    draw_history(ui, model, &mut actions);
                }
                if plan.trade_inline {
                    ui.separator();
                    draw_trade(ui, model, &mut actions);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if plan.overflow_needed() {
                        draw_overflow(ui, model, plan, &mut actions);
                    }
                    if plan.panels_inline {
                        let panels = IconButton::new(icons::SIDEBAR_SIMPLE, TOOLBAR_ICON)
                            .active(model.dock_visible)
                            .hover_text("show or hide the panels dock (Ctrl+B)")
                            .show(ui);
                        if panels.clicked() {
                            actions.push(ToolbarAction::ToggleDock);
                        }
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
fn draw_source(ui: &mut egui::Ui, model: &mut ToolbarModel, plan: CollapsePlan) {
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
}

/// BARS: the kind combo, with the parameter beside it or merged into its
/// selected text when folded.
fn draw_bars(ui: &mut egui::Ui, model: &mut ToolbarModel, plan: CollapsePlan) {
    ui.label(egui::RichText::new("bars").color(theme::TEXT_MUTED));
    let selected = if plan.param_inline {
        model.kind.label().to_owned()
    } else {
        format!("{} · {}", model.kind.label(), param_summary(model))
    };
    let traded_volume = model.capabilities.traded_volume;
    egui::ComboBox::from_id_salt("bar_kind")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for kind in BarKind::ALL {
                // A rule that counts traded size is offered only where size is
                // real. On a quote-driven feed it would silently become a tick
                // bar under another name.
                let usable = traded_volume || !kind.needs_traded_volume();
                ui.add_enabled_ui(usable, |ui| {
                    let item = ui.selectable_value(model.kind, kind, kind.label());
                    if !usable {
                        item.on_disabled_hover_text(
                            "this source quotes prices but prints no traded volume",
                        );
                    }
                });
            }
        });
    if plan.param_inline {
        draw_bar_param(ui, model);
    }
}

/// The one parameter of the selected bar kind. Lives beside the combo, or in
/// the overflow menu when the plan folded it.
fn draw_bar_param(ui: &mut egui::Ui, model: &mut ToolbarModel) {
    match model.kind {
        BarKind::Tick => {
            ui.label("N trades");
            ui.add(egui::DragValue::new(model.tick_n).range(1.0..=5000.0));
        }
        BarKind::Volume => {
            ui.label("units");
            ui.add(
                egui::DragValue::new(model.volume_units)
                    .range(0.1..=1000.0)
                    .speed(0.1),
            );
        }
        BarKind::Dollar => {
            ui.label("notional");
            ui.add(
                egui::DragValue::new(model.dollar_notional)
                    .range(1000.0..=1_000_000_000.0)
                    .speed(1000.0),
            );
        }
        BarKind::Time => {
            // The same four presets the time pane's header offers — one
            // list, two surfaces (§11) — with the drag as the custom escape
            // hatch. `bars → time` must not require knowing that a minute is
            // 60000 (audit QW1).
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (label, preset_ms) in crate::time_header::PRESETS {
                    let selected = *model.time_interval_ms == preset_ms;
                    // The selected timeframe wears the chip language (solid
                    // accent, dark ink) — the unselected ones stay quiet.
                    let (fill, ink) = if selected {
                        (theme::ACCENT, theme::CHIP_INK)
                    } else {
                        (theme::CONTROL, theme::TEXT_MUTED)
                    };
                    let chip = ui.add(
                        egui::Button::new(egui::RichText::new(label).color(ink).small())
                            .fill(fill)
                            .stroke(egui::Stroke::NONE)
                            .rounding(egui::Rounding::same(9.0))
                            .min_size(egui::vec2(28.0, 18.0)),
                    );
                    if chip.clicked() && !selected {
                        *model.time_interval_ms = preset_ms;
                    }
                }
                ui.add(
                    egui::DragValue::new(model.time_interval_ms)
                        .range(
                            crate::state::MIN_TIME_INTERVAL_MS as f64
                                ..=crate::state::MAX_TIME_INTERVAL_MS as f64,
                        )
                        .speed(crate::state::TIME_INTERVAL_DRAG_SPEED)
                        .suffix(" ms"),
                )
                .on_hover_text("custom interval, in milliseconds");
            });
        }
        BarKind::Imbalance => {
            // The unit picks what θ accumulates; the target counts trades in
            // every unit. Size-measuring units are offered only where the
            // venue prints a real size, mirroring the kind combo's gate.
            let traded_volume = model.capabilities.traded_volume;
            for unit in ImbalanceUnit::ALL {
                // The chip label is the unit's own spec token, so what the
                // trader clicks is the word the spec string says.
                let hover = match unit {
                    ImbalanceUnit::Trades => "θ sums ±1 per trade — tick imbalance bars",
                    ImbalanceUnit::Volume => "θ sums ±quantity — volume imbalance bars",
                    ImbalanceUnit::Dollar => "θ sums ±(price × quantity) — dollar imbalance bars",
                };
                let usable = traded_volume || unit == ImbalanceUnit::Trades;
                ui.add_enabled_ui(usable, |ui| {
                    let chip = ui
                        .selectable_value(model.imbalance_unit, unit, unit.as_str())
                        .on_hover_text(hover);
                    if !usable {
                        chip.on_disabled_hover_text(
                            "this source quotes prices but prints no traded volume",
                        );
                    }
                });
            }
            ui.label("target trades");
            // A promise the engine now keeps, so the tooltip can state it
            // plainly. `E[T]` used to be an EWMA seeded with this number
            // while the threshold was linear in it — a feedback loop that
            // parked on a clamp and delivered `3 * target` or a fraction of
            // it instead, which is what made a trader set 1500, then 2000,
            // and get two unrecognisably different charts.
            // `quantick_engine`'s `a_bar_is_about_the_target_long_in_balanced_flow`
            // is what keeps the sentence honest.
            ui.add(
                egui::DragValue::new(model.imbalance_target)
                    .range(2.0..=1_000_000.0)
                    .speed(25.0),
            )
            .on_hover_text(
                "expected trades per bar in balanced flow — a real \
                 expectation, not a fixed length: one-sided aggression closes \
                 a bar well short of it, which is what this bar type is for",
            );
        }
    }
}

/// Short parameter readout for the merged kind combo, e.g. `tick · 50` or
/// `time · 1m` — the chips' own vocabulary, never raw milliseconds (QW3).
fn param_summary(model: &ToolbarModel) -> String {
    match model.kind {
        BarKind::Tick => model.tick_n.to_string(),
        BarKind::Volume => format!("{:.1}", model.volume_units),
        BarKind::Dollar => format!("{:.0}", model.dollar_notional),
        BarKind::Time => crate::state::fmt_time_interval(*model.time_interval_ms),
        BarKind::Imbalance => match *model.imbalance_unit {
            ImbalanceUnit::Trades => model.imbalance_target.to_string(),
            unit => format!("{} {}", unit.as_str(), model.imbalance_target),
        },
    }
}

/// HISTORY: the `+ older ▾` split button. The page size lives in the caret
/// menu; the whole group gates on the `history_paging` capability.
fn draw_history(ui: &mut egui::Ui, model: &mut ToolbarModel, actions: &mut Vec<ToolbarAction>) {
    let paging = model.capabilities.history_paging;
    let load = ui
        .add_enabled(paging, egui::Button::new(format!("{} older", icons::PLUS)))
        .on_hover_text("fetch older trades and prepend them")
        .on_disabled_hover_text(
            "no older trades to fetch: this feed only streams forward, or its \
             history is already all here",
        );
    if load.clicked() {
        actions.push(ToolbarAction::LoadOlder);
    }
    ui.add_enabled_ui(paging, |ui| {
        ui.menu_button(icons::CARET_DOWN, |ui| {
            draw_history_menu(ui, model);
        });
    });
}

/// The history caret/overflow menu body: page size and the running total.
fn draw_history_menu(ui: &mut egui::Ui, model: &mut ToolbarModel) {
    ui.label("page size (trades per load)");
    ui.add(
        egui::DragValue::new(model.history_step)
            .range(500.0..=50_000.0)
            .speed(100.0),
    );
    ui.small(format!("{} trades backfilled so far", model.history_trades));
}

/// TRADE: the simulated market entries and, while a position is open, its
/// exit (`docs/ux/paper-trading.md`). Quantity and brackets live in the
/// Trading dock tab; the labels disclose them. Gated on the simulator having
/// seen a price, never on the provider — paper trading works on any feed.
fn draw_trade(ui: &mut egui::Ui, model: &ToolbarModel, actions: &mut Vec<ToolbarAction>) {
    let paper = &model.paper;
    // The entries speak the chip language: solid side colour, dark ink —
    // the same grammar as the price chips in the gutter, because the button
    // and the entry line it will create are one statement.
    let entry = |ui: &mut egui::Ui, label: &str, fill: egui::Color32, enabled: bool| {
        ui.add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(label).color(theme::CHIP_INK).strong())
                .fill(fill)
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(3.0))
                .min_size(egui::vec2(52.0, 24.0)),
        )
    };
    let buy = entry(ui, &paper.buy_label, theme::BUY, paper.ready)
        .on_hover_text(&paper.buy_hover)
        .on_disabled_hover_text("waiting for the first print - there is no market yet");
    if buy.clicked() {
        actions.push(ToolbarAction::PaperBuy);
    }
    let sell = entry(ui, &paper.sell_label, theme::SELL, paper.ready)
        .on_hover_text(&paper.sell_hover)
        .on_disabled_hover_text("waiting for the first print - there is no market yet");
    if sell.clicked() {
        actions.push(ToolbarAction::PaperSell);
    }
    // The exit control, on the same surface as the entries: an open position
    // must never require a dock dive to leave (audit pain 1, B2). Outlined,
    // not filled — leaving is deliberate, never the loudest thing on the bar.
    if let Some(close) = &paper.close_label {
        let button = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("× {close}"))
                        .color(theme::TEXT_PRIMARY)
                        .strong(),
                )
                .fill(theme::CONTROL)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(egui::Rounding::same(3.0))
                .min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text("exit the open simulated position at the next print (market)");
        if button.clicked() {
            actions.push(ToolbarAction::PaperClose);
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
    pub const ALL: [Self; 4] = [
        Self::Bubbles,
        Self::Heatmap,
        Self::Footprint,
        Self::LiveStrip,
    ];

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
                "aggression bubbles: confirmed executions from the trade stream — \
                 right-click for settings"
            }
            Self::Heatmap => {
                "L2 heatmap: show the recorded depth map — recording never stops, so hiding it \
                 loses nothing. Right-click for settings"
            }
            Self::Footprint => {
                "candle footprint: the buy/sell split per price inside each bar — detail follows \
                 the zoom. Right-click for style and thresholds"
            }
            Self::LiveStrip => {
                "live strip: the book's resting depth and the forming bar's aggression, \
                 beside the price axis — right-click for settings"
            }
        }
    }

    /// This toggle's slot in [`ToolbarModel::layers`].
    #[must_use]
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Bubbles => 0,
            Self::Heatmap => 1,
            Self::Footprint => 2,
            Self::LiveStrip => 3,
        }
    }

    #[must_use]
    fn toggle_action(self, on: bool) -> ToolbarAction {
        match self {
            Self::Bubbles => ToolbarAction::SetBubbles(on),
            Self::Heatmap => ToolbarAction::SetHeatmap(on),
            Self::Footprint => ToolbarAction::SetFootprint(on),
            Self::LiveStrip => ToolbarAction::SetLiveStrip(on),
        }
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
    draw_indicators_menu(ui, model, actions);

    for layer in LayerToggle::ALL {
        let state = model.layers[layer.index()];
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

/// INDICATORS: add the M1 native indicators and manage the active ones —
/// status dot, eye toggle, remove. M2 swaps the two hardcoded add entries
/// for the script library browser; the entry list stays.
fn draw_indicators_menu(ui: &mut egui::Ui, model: &ToolbarModel, actions: &mut Vec<ToolbarAction>) {
    let any_active = !model.indicators.is_empty();
    let icon = egui::RichText::new(LAYER_INDICATORS_ICON)
        .size(16.0)
        .color(if any_active {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        });
    ui.menu_button(icon, |ui| {
        if ui
            .button(format!("{} Add EMA(9) on close", icons::PLUS))
            .clicked()
        {
            actions.push(ToolbarAction::AddEmaIndicator);
            ui.close_menu();
        }
        if ui.button(format!("{} Add CVD pane", icons::PLUS)).clicked() {
            actions.push(ToolbarAction::AddCvdIndicator);
            ui.close_menu();
        }
        if !model.scripts.is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new("scripts")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            for (index, name) in model.scripts.iter().enumerate() {
                if ui.button(format!("{} {name}", icons::FILE_CODE)).clicked() {
                    actions.push(ToolbarAction::AddScriptIndicator(index));
                    ui.close_menu();
                }
            }
        }
        if any_active {
            ui.separator();
        }
        for entry in &model.indicators {
            ui.horizontal(|ui| {
                // Status dot: honest state at a glance — errored beats hidden.
                let (dot, color) = if entry.errored {
                    (icons::WARNING_CIRCLE, theme::SELL)
                } else if entry.stale {
                    // Running, but the file on disk has errors.
                    (icons::WARNING, theme::ACCENT)
                } else if entry.hidden {
                    (icons::EYE_SLASH, theme::TEXT_MUTED)
                } else {
                    (icons::CIRCLE, theme::BUY)
                };
                ui.label(egui::RichText::new(dot).color(color));
                ui.label(&entry.label);
                if ui
                    .small_button(if entry.hidden {
                        icons::EYE
                    } else {
                        icons::EYE_SLASH
                    })
                    .on_hover_text("hide/show without removing (no recompute)")
                    .clicked()
                {
                    actions.push(ToolbarAction::ToggleIndicatorHidden(entry.slot));
                }
                if ui
                    .small_button(icons::GEAR)
                    .on_hover_text("settings (applying recomputes from scratch)")
                    .clicked()
                {
                    actions.push(ToolbarAction::OpenIndicatorSettings(entry.slot));
                    // The menu closes so it cannot occlude the dialog it
                    // just spawned (egui paints menus above windows) —
                    // audit M3.
                    ui.close_menu();
                }
                if ui
                    .small_button(icons::TRASH)
                    .on_hover_text("remove this indicator")
                    .clicked()
                {
                    actions.push(ToolbarAction::RemoveIndicator(entry.slot));
                    // Closing beats redrawing the menu around a vanished
                    // row. The eye deliberately keeps the menu open:
                    // toggling several indicators is one errand.
                    ui.close_menu();
                }
            });
        }
    })
    .response
    .on_hover_text("indicators: add or manage overlay and pane indicators");
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
                    egui::Button::new(format!("{} Load older", icons::PLUS)),
                )
                .on_disabled_hover_text(
                    "no older trades to fetch: this feed only streams forward, or \
                     its history is already all here",
                );
            if load.clicked() {
                actions.push(ToolbarAction::LoadOlder);
                ui.close_menu();
            }
            if paging {
                draw_history_menu(ui, model);
            }
        }
        if !plan.trade_inline {
            ui.separator();
            let buy = ui
                .add_enabled(
                    model.paper.ready,
                    egui::Button::new(format!("{} at market (SIM)", model.paper.buy_label)),
                )
                .on_disabled_hover_text("waiting for the first print - there is no market yet");
            if buy.clicked() {
                actions.push(ToolbarAction::PaperBuy);
                ui.close_menu();
            }
            let sell = ui
                .add_enabled(
                    model.paper.ready,
                    egui::Button::new(format!("{} at market (SIM)", model.paper.sell_label)),
                )
                .on_disabled_hover_text("waiting for the first print - there is no market yet");
            if sell.clicked() {
                actions.push(ToolbarAction::PaperSell);
                ui.close_menu();
            }
            if let Some(close) = &model.paper.close_label
                && ui.button(format!("× {close} (SIM)")).clicked()
            {
                actions.push(ToolbarAction::PaperClose);
                ui.close_menu();
            }
        }
        if !plan.param_inline {
            ui.separator();
            ui.label(egui::RichText::new("bar parameter").color(theme::TEXT_MUTED));
            draw_bar_param(ui, model);
        }
    });
}

#[cfg(test)]
mod tests {
    /// Four unblocked lamps from their on/off flags, in [`LayerToggle::ALL`]
    /// order. A fixture that wants a blocked lamp builds the array itself.
    fn layer_states(on: [bool; 4]) -> [LayerToggleState; 4] {
        on.map(|on| LayerToggleState { on, blocked: None })
    }

    use super::*;

    /// The flat TRADE pair's width — the old `W_TRADE` constant, kept as the
    /// tests' baseline now that the group is measured from its labels.
    const FLAT_TRADE_W: f32 = 110.0;

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
            LAYER_INDICATORS_ICON,   // indicators: a plotted line
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

    /// How many §6 collapse steps a plan has taken. `None` when the plan is
    /// not one of the seven canonical states — folding out of order.
    fn stage(plan: CollapsePlan) -> Option<usize> {
        let states = [
            (true, true, true, true, true, true),
            (false, true, true, true, true, true),
            (false, false, true, true, true, true),
            (false, false, false, true, true, true),
            (false, false, false, false, true, true),
            (false, false, false, false, false, true),
            (false, false, false, false, false, false),
        ];
        states.iter().position(|&state| {
            state
                == (
                    plan.look_inline,
                    plan.panels_inline,
                    plan.history_inline,
                    plan.trade_inline,
                    plan.param_inline,
                    plan.feed_full_name,
                )
        })
    }

    #[test]
    fn a_wide_toolbar_folds_nothing() {
        let plan = collapse_plan(10_000.0, FLAT_TRADE_W, W_BAR_PARAM);
        assert_eq!(stage(plan), Some(0));
        assert!(!plan.overflow_needed());
    }

    #[test]
    fn a_hopeless_width_ends_fully_folded() {
        let plan = collapse_plan(0.0, FLAT_TRADE_W, W_BAR_PARAM);
        assert_eq!(stage(plan), Some(6));
        assert!(plan.overflow_needed());
    }

    #[test]
    fn folding_follows_the_documented_order_and_never_skips() {
        let mut last_stage = 6;
        // Sweep from narrow to wide: the collapse stage must only decrease,
        // and every plan must be one of the canonical §6 states.
        let mut width = 100.0;
        while width <= 2000.0 {
            let plan = collapse_plan(width, FLAT_TRADE_W, W_BAR_PARAM);
            let stage = stage(plan).expect("plans fold strictly in the §6 order");
            assert!(
                stage <= last_stage,
                "widening from {width} px must never fold more"
            );
            last_stage = stage;
            width += 10.0;
        }
        assert_eq!(last_stage, 0, "the sweep must reach the unfolded plan");
    }

    #[test]
    fn look_folds_first_and_alone_at_a_mildly_tight_width() {
        // One pixel short of the full plan: exactly LOOK moves to overflow.
        let full_width = CollapsePlan::FULL.width(FLAT_TRADE_W, W_BAR_PARAM);
        let plan = collapse_plan(full_width - 1.0, FLAT_TRADE_W, W_BAR_PARAM);
        assert_eq!(stage(plan), Some(1));
        assert!(plan.overflow_needed());
    }

    /// An open position widens TRADE (state-aware labels + the close
    /// button), and the plan reads that width: what exactly fits flat folds
    /// once a position opens.
    #[test]
    fn an_open_position_widens_trade_and_folds_sooner() {
        let flat = PaperTradeModel::flat(true);
        let open = PaperTradeModel {
            buy_label: "BUY 1 (adds to 2)".to_owned(),
            sell_label: "SELL 1 (closes 1 of 2)".to_owned(),
            close_label: Some("Close 2 LONG".to_owned()),
            ..PaperTradeModel::flat(true)
        };
        let flat_w = trade_width(&flat);
        let open_w = trade_width(&open);
        assert!(open_w > flat_w, "{open_w} vs {flat_w}");
        let exactly_flat = CollapsePlan::FULL.width(flat_w, W_BAR_PARAM);
        assert_eq!(
            stage(collapse_plan(exactly_flat, flat_w, W_BAR_PARAM)),
            Some(0)
        );
        assert!(
            stage(collapse_plan(exactly_flat, open_w, W_BAR_PARAM)).expect("canonical") > 0,
            "the wider TRADE must fold something at the same window width"
        );
    }

    /// The time kind's chip row widens the parameter slot, and the plan
    /// reads that width: what exactly fits a drag-only kind folds once the
    /// chips are on screen.
    #[test]
    fn the_time_kind_widens_the_param_slot_and_folds_sooner() {
        assert!(param_width(BarKind::Time) > param_width(BarKind::Tick));
        let exactly_drag = CollapsePlan::FULL.width(FLAT_TRADE_W, param_width(BarKind::Tick));
        assert_eq!(
            stage(collapse_plan(
                exactly_drag,
                FLAT_TRADE_W,
                param_width(BarKind::Tick)
            )),
            Some(0)
        );
        assert!(
            stage(collapse_plan(
                exactly_drag,
                FLAT_TRADE_W,
                param_width(BarKind::Time)
            ))
            .expect("canonical")
                > 0,
            "the wider chip row must fold something at the same window width"
        );
    }

    #[test]
    fn a_shrunk_feed_name_alone_never_requires_the_overflow_menu() {
        let plan = CollapsePlan {
            feed_full_name: false,
            ..CollapsePlan::FULL
        };
        assert!(!plan.overflow_needed());
    }

    /// Lay the toolbar out for real, off-screen, live and replaying — a
    /// duplicated widget id or a panicking widget fails here instead of on
    /// the chart, and an un-clicked frame must ask nothing of the app.
    #[test]
    fn the_toolbar_lays_out_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut feed_id = "binance".to_owned();
        let mut symbol = "BTCUSDT".to_owned();
        let mut kind = BarKind::Tick;
        let mut tick_n = 50_u64;
        let mut volume_units = 5.0_f64;
        let mut dollar_notional = 500_000.0_f64;
        let mut time_interval_ms = 1_000_i64;
        let mut imbalance_target = 100_u64;
        let mut imbalance_unit = ImbalanceUnit::Trades;
        let mut history_step = 2_000_usize;
        for replaying in [false, true] {
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let mut model = ToolbarModel {
                        feeds: vec![("binance".to_owned(), "Binance".to_owned())],
                        feed_id: &mut feed_id,
                        feed_display_name: "Binance".to_owned(),
                        symbols: vec!["BTCUSDT".to_owned()],
                        symbol: &mut symbol,
                        replay: replaying.then(|| ReplaySource {
                            label: "BTCUSDT · 2026-03-16".to_owned(),
                            hover: "Replaying a recorded session".to_owned(),
                        }),
                        kind: &mut kind,
                        tick_n: &mut tick_n,
                        volume_units: &mut volume_units,
                        dollar_notional: &mut dollar_notional,
                        time_interval_ms: &mut time_interval_ms,
                        imbalance_target: &mut imbalance_target,
                        imbalance_unit: &mut imbalance_unit,
                        history_step: &mut history_step,
                        history_trades: 1_000,
                        capabilities: FeedCapabilities {
                            book_capture: !replaying,
                            history_paging: !replaying,
                            traded_volume: true,
                            ohlcv_history: !replaying,
                            ohlcv_generation: 0,
                        },
                        layers: layer_states([true, false, false, false]),
                        dock_visible: true,
                        appearance_open: false,
                        paper: PaperTradeModel::flat(true),
                        // Non-empty on purpose: the entry rows (status dot,
                        // eye, trash) are only drawn when the menu has
                        // something in it, and an empty vec never exercises
                        // them.
                        indicators: vec![
                            IndicatorMenuEntry {
                                slot: 0,
                                label: "EMA(9, close)".to_owned(),
                                hidden: false,
                                errored: false,
                                stale: false,
                            },
                            IndicatorMenuEntry {
                                slot: 1,
                                label: "CVD".to_owned(),
                                hidden: true,
                                errored: true,
                                stale: true,
                            },
                        ],
                        scripts: vec!["ema.pine".to_owned(), "zigzag.pine".to_owned()],
                    };
                    let actions = draw(ctx, &mut model);
                    assert!(actions.is_empty(), "no clicks, no actions");
                });
            }
        }
    }

    /// With the time kind selected, the preset chips reach actual toolbar
    /// pixels — the audit's pain 2 began with `bars → time` offering a bare
    /// millisecond drag (QW1), and the summary speaks the chips' vocabulary
    /// (QW3).
    #[test]
    fn the_time_kind_paints_the_preset_chips() {
        let ctx = egui::Context::default();
        let mut feed_id = "binance".to_owned();
        let mut symbol = "BTCUSDT".to_owned();
        let mut kind = BarKind::Time;
        let mut tick_n = 50_u64;
        let mut volume_units = 5.0_f64;
        let mut dollar_notional = 500_000.0_f64;
        let mut time_interval_ms = 60_000_i64;
        let mut imbalance_target = 100_u64;
        let mut imbalance_unit = ImbalanceUnit::Trades;
        let mut history_step = 2_000_usize;
        // Wide enough that the §6 plan folds nothing — the point is the
        // inline chip row, not the overflow menu.
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1900.0, 400.0),
            )),
            ..Default::default()
        };
        let mut painted = String::new();
        for _ in 0..2 {
            let output = ctx.run(input(), |ctx| {
                let mut model = ToolbarModel {
                    feeds: vec![("binance".to_owned(), "Binance".to_owned())],
                    feed_id: &mut feed_id,
                    feed_display_name: "Binance".to_owned(),
                    symbols: vec!["BTCUSDT".to_owned()],
                    symbol: &mut symbol,
                    replay: None,
                    kind: &mut kind,
                    tick_n: &mut tick_n,
                    volume_units: &mut volume_units,
                    dollar_notional: &mut dollar_notional,
                    time_interval_ms: &mut time_interval_ms,
                    imbalance_target: &mut imbalance_target,
                    imbalance_unit: &mut imbalance_unit,
                    history_step: &mut history_step,
                    history_trades: 1_000,
                    capabilities: FeedCapabilities {
                        book_capture: true,
                        history_paging: true,
                        traded_volume: true,
                        ohlcv_history: true,
                        ohlcv_generation: 0,
                    },
                    layers: layer_states([false, false, false, false]),
                    dock_visible: true,
                    appearance_open: false,
                    paper: PaperTradeModel::flat(true),
                    indicators: Vec::new(),
                    scripts: Vec::new(),
                };
                let actions = draw(ctx, &mut model);
                assert!(actions.is_empty(), "no clicks, no actions");
            });
            painted.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(text) = shape.shape {
                    painted.push_str(text.galley.text());
                    painted.push(' ');
                }
            }
        }
        for chip in ["1m", "5m", "15m", "1h"] {
            assert!(
                painted.contains(chip),
                "the {chip} preset never reached the toolbar; painted: {painted}"
            );
        }
        assert_eq!(kind, BarKind::Time, "an un-clicked frame changes nothing");
        assert_eq!(time_interval_ms, 60_000);
    }

    /// The same layout for a venue that prints no volume — the shape a live
    /// Tickmill US500 session produces. Every size-measuring affordance is
    /// disabled rather than absent, so the toolbar must still lay out whole.
    #[test]
    fn a_quote_driven_feed_lays_the_toolbar_out_with_its_volume_widgets_disabled() {
        let ctx = egui::Context::default();
        let mut feed_id = "metatrader".to_owned();
        let mut symbol = "US500".to_owned();
        let mut tick_n = 50_u64;
        let mut volume_units = 5.0_f64;
        let mut dollar_notional = 500_000.0_f64;
        let mut time_interval_ms = 1_000_i64;
        let mut imbalance_target = 100_u64;
        let mut imbalance_unit = ImbalanceUnit::Trades;
        let mut history_step = 2_000_usize;
        // Every kind, including the two the feed cannot back: selecting one is
        // still possible from config or a previous session, and the toolbar
        // must draw it rather than panic.
        for mut kind in BarKind::ALL {
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let mut model = ToolbarModel {
                        feeds: vec![("metatrader".to_owned(), "MetaTrader 5".to_owned())],
                        feed_id: &mut feed_id,
                        feed_display_name: "MetaTrader 5".to_owned(),
                        symbols: vec!["US500".to_owned()],
                        symbol: &mut symbol,
                        replay: None,
                        kind: &mut kind,
                        tick_n: &mut tick_n,
                        volume_units: &mut volume_units,
                        dollar_notional: &mut dollar_notional,
                        time_interval_ms: &mut time_interval_ms,
                        imbalance_target: &mut imbalance_target,
                        imbalance_unit: &mut imbalance_unit,
                        history_step: &mut history_step,
                        history_trades: 200_000,
                        capabilities: FeedCapabilities {
                            book_capture: false,
                            history_paging: false,
                            traded_volume: false,
                            ohlcv_history: false,
                            ohlcv_generation: 0,
                        },
                        layers: layer_states([false, false, false, false]),
                        dock_visible: true,
                        appearance_open: false,
                        // Not yet ready: the TRADE pair must lay out in its
                        // disabled state without asking anything of the app.
                        paper: PaperTradeModel::flat(false),
                        indicators: Vec::new(),
                        scripts: Vec::new(),
                    };
                    let actions = draw(ctx, &mut model);
                    assert!(actions.is_empty(), "no clicks, no actions");
                });
            }
        }
    }

    /// With a position open, the exit control reaches actual toolbar pixels
    /// — the audit's pain 1 was precisely this button not existing anywhere
    /// the user looks.
    #[test]
    fn an_open_position_paints_the_close_button() {
        let ctx = egui::Context::default();
        let mut feed_id = "binance".to_owned();
        let mut symbol = "BTCUSDT".to_owned();
        let mut kind = BarKind::Tick;
        let mut tick_n = 50_u64;
        let mut volume_units = 5.0_f64;
        let mut dollar_notional = 500_000.0_f64;
        let mut time_interval_ms = 1_000_i64;
        let mut imbalance_target = 100_u64;
        let mut imbalance_unit = ImbalanceUnit::Trades;
        let mut history_step = 2_000_usize;
        let mut painted = String::new();
        // Wide enough that the §6 plan folds nothing — the point is the
        // inline button, not the overflow menu.
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1900.0, 400.0),
            )),
            ..Default::default()
        };
        for _ in 0..2 {
            let output = ctx.run(input(), |ctx| {
                let mut model = ToolbarModel {
                    feeds: vec![("binance".to_owned(), "Binance".to_owned())],
                    feed_id: &mut feed_id,
                    feed_display_name: "Binance".to_owned(),
                    symbols: vec!["BTCUSDT".to_owned()],
                    symbol: &mut symbol,
                    replay: None,
                    kind: &mut kind,
                    tick_n: &mut tick_n,
                    volume_units: &mut volume_units,
                    dollar_notional: &mut dollar_notional,
                    time_interval_ms: &mut time_interval_ms,
                    imbalance_target: &mut imbalance_target,
                    imbalance_unit: &mut imbalance_unit,
                    history_step: &mut history_step,
                    history_trades: 1_000,
                    capabilities: FeedCapabilities {
                        book_capture: true,
                        history_paging: true,
                        traded_volume: true,
                        ohlcv_history: true,
                        ohlcv_generation: 0,
                    },
                    layers: layer_states([false, false, false, false]),
                    dock_visible: true,
                    appearance_open: false,
                    paper: PaperTradeModel {
                        buy_label: "BUY 1 (adds to 2)".to_owned(),
                        sell_label: "SELL 1 (closes 1 of 2)".to_owned(),
                        close_label: Some("Close 2 LONG".to_owned()),
                        ..PaperTradeModel::flat(true)
                    },
                    indicators: Vec::new(),
                    scripts: Vec::new(),
                };
                let actions = draw(ctx, &mut model);
                assert!(actions.is_empty(), "no clicks, no actions");
            });
            painted.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(text) = shape.shape {
                    painted.push_str(text.galley.text());
                    painted.push(' ');
                }
            }
        }
        assert!(
            painted.contains("Close 2 LONG"),
            "the exit control never reached the toolbar; painted: {painted}"
        );
        assert!(
            painted.contains("SELL 1 (closes 1 of 2)"),
            "the state-aware label never reached the toolbar; painted: {painted}"
        );
    }
}
