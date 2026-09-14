//! CMD entry: the modifier-and-click order the chart takes without a form.
//!
//! Holding the configured modifier turns the pointer into an order: the
//! preview says what a press would place and at what price, and the press
//! places exactly that. The preview and the press read the same
//! [`CmdPreview`], which is why what the trader was shown is what the venue
//! is told.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::{Bracket, EntryKind};
use rust_decimal::Decimal;

use super::paint_ctx::clamp_tag_center;
use super::{
    CMD_LABEL_CURSOR_GAP_PX, CMD_LABEL_WIDTH_PX, CMD_LINE_MIN_PX, ChartInput, CmdEntryKind,
    CmdModifier, CmdTradingSettings, PaperTrading, TAG_HEIGHT_PX, modifier_is_down,
};
use crate::paper_chrome::{caption, fmt_decimal, pill_toggle};
use crate::theme;

impl PaperTrading {
    /// Install cmd-trading settings — the app's fan-out on boot and on a
    /// change made in any tab (one gesture, one meaning, everywhere).
    pub fn set_cmd_trading(&mut self, settings: CmdTradingSettings) {
        self.account.cmd_trading = settings;
        if !settings.enabled {
            self.cmd_preview = None;
        }
    }

    /// Drop the frame's preview — the pane calls this when a drawing tool
    /// owns the hand, so a stale line never keeps painting.
    pub fn clear_cmd_preview(&mut self) {
        self.cmd_preview = None;
    }

    pub(super) fn compute_cmd_preview(&self, input: &ChartInput<'_>) -> Option<CmdPreview> {
        if !self.account.cmd_trading.enabled || !input.layer_visible {
            return None;
        }
        // A drawing a press would grab, or the canvas's own chrome. The
        // buy modifier is Shift by default — the very key that levels a
        // channel corner — so sweeping across a drawn line blinks the aim
        // off for its grab band, in step with the move cursor the drawings
        // put up.
        if input.canvas_claimed {
            return None;
        }
        // An armed limit/stop is an intent already stated, with its own
        // hint on screen; a modifier resting under the hand must not turn
        // that click into a different order and leave the ticket armed.
        if self.account.armed.is_some() {
            return None;
        }
        let scale = input.scale?;
        let (pointer, side, forced) = match self.cmd_preview_force {
            // The harness has no hand; park the pointer mid-chart, or at
            // the x the hook stated — which is the whole point of a run
            // capturing where the label rides, so it wins over a stray
            // real pointer that in such a run is nobody's aim.
            Some(force) => {
                let pointer = match force.x_fraction {
                    Some(fraction) => egui::pos2(
                        input.chart.left() + input.chart.width() * fraction,
                        input
                            .pointer
                            .map_or_else(|| input.chart.center().y, |pointer| pointer.y),
                    ),
                    None => input.pointer.unwrap_or(input.chart.center()),
                };
                (pointer, force.side, true)
            }
            None => {
                let pointer = input.pointer?;
                let buy = modifier_is_down(self.account.cmd_trading.buy, input.modifiers);
                let sell = modifier_is_down(self.account.cmd_trading.sell, input.modifiers);
                let side = match (buy, sell) {
                    (true, false) => Side::Buy,
                    (false, true) => Side::Sell,
                    _ => return None,
                };
                (pointer, side, false)
            }
        };
        if !input.chart.contains(pointer) {
            return None;
        }
        // This module's own furniture outranks the aim, the same way an
        // annotation does: an ✕ or a bracket handle under the pointer, and
        // any line a press would grab. Otherwise holding the modifier
        // while reaching for a stop would rest a new order on top of it,
        // with the hand cursor promising exactly that.
        if self.control_at(pointer, input.chart, scale).is_some()
            || self.line_at(pointer, scale).is_some()
        {
            return None;
        }
        let mark = self.account.venue.mark_price()?;
        let raw_price = scale.price_at(pointer.y);
        let price = self.account.snap(raw_price);
        // The context menu's own validity table, plus the trader's stated
        // kind. `None` stands the aim down rather than substituting the
        // other kind — see `resolve_cmd_kind`.
        let kind = resolve_cmd_kind(self.account.cmd_trading.kind, side, price, mark)?;
        let quantity = self.quantity_preview().unwrap_or(Decimal::ONE);
        let ticket = self.ticket_bracket(side, price);
        Some(CmdPreview {
            side,
            kind,
            price,
            raw_price,
            pointer,
            forced,
            bracket: self.account.aim_bracket(
                side,
                price,
                quantity,
                ticket,
                &self.account_env(side, price),
            ),
            ruler_ticks: self.ruler_notches,
        })
    }

    /// The cmd-trading block of the ticket: the enable pill and the two
    /// key bindings. Returns whether anything changed, so the host can
    /// persist and fan out.
    pub(super) fn draw_cmd_trading_settings(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.add_space(4.0);
        ui.label(caption("CMD TRADING"));
        ui.horizontal(|ui| {
            if pill_toggle(
                ui,
                "Enabled",
                self.account.cmd_trading.enabled,
                "hold a key over the chart: a dashed line shows exactly where the order \
                 will rest, and the click places it",
            )
            .clicked()
            {
                self.account.cmd_trading.enabled = !self.account.cmd_trading.enabled;
                changed = true;
            }
            for (word, slot) in [("Buy", true), ("Sell", false)] {
                ui.label(egui::RichText::new(word).color(theme::TEXT_MUTED).small());
                let current = if slot {
                    self.account.cmd_trading.buy
                } else {
                    self.account.cmd_trading.sell
                };
                egui::ComboBox::from_id_salt(("cmd_trading_modifier", word))
                    .width(64.0)
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        for modifier in CmdModifier::ALL {
                            if ui
                                .selectable_label(current == modifier, modifier.label())
                                .clicked()
                                && current != modifier
                            {
                                if slot {
                                    self.account.cmd_trading.buy = modifier;
                                } else {
                                    self.account.cmd_trading.sell = modifier;
                                }
                                changed = true;
                            }
                        }
                    });
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Place")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            let current = self.account.cmd_trading.kind;
            egui::ComboBox::from_id_salt("cmd_trading_entry_kind")
                .width(64.0)
                .selected_text(current.label())
                .show_ui(ui, |ui| {
                    for kind in CmdEntryKind::ALL {
                        if ui.selectable_label(current == kind, kind.label()).clicked()
                            && current != kind
                        {
                            self.account.cmd_trading.kind = kind;
                            changed = true;
                        }
                    }
                })
                .response
                .on_hover_text(
                    "which order the aim places - auto takes whichever kind can rest at \
                     the price, limit and stop place only that one and show nothing where \
                     it cannot rest",
                );
            ui.label(egui::RichText::new("Step").color(theme::TEXT_MUTED).small());
            // The empty field shows this instrument's own default as a hint,
            // so blank reads as "follows the instrument" rather than as
            // nothing. A tick is what the ladder speaks, and saying what one
            // is worth here is the only place the ticket ever does.
            let derived = self.account.derived_ruler_step();
            let tick = self.account.tick();
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.ruler_step_text)
                    .desired_width(52.0)
                    .hint_text(fmt_decimal(derived)),
            );
            if response.changed() {
                let typed = self.ruler_step_text.trim();
                let step = if typed.is_empty() {
                    None
                } else {
                    typed.parse::<Decimal>().ok()
                };
                self.set_ruler_step(step);
                changed = true;
            }
            response.on_hover_text(format!(
                "how far one wheel notch walks the aim's stop and target, in points of \
                 this instrument. One tick here is {}, so this instrument defaults to \
                 {} a notch. Empty follows that default; saved per symbol.",
                fmt_decimal(tick),
                fmt_decimal(derived),
            ));
            ui.label(egui::RichText::new("pts").color(theme::TEXT_FAINT).small());
        });
        if self.account.cmd_trading.enabled && self.account.cmd_trading.kind != CmdEntryKind::Auto {
            // A stated kind is valid on one side of the market only, so the
            // aim is silent on the other half of the chart. Said here, or a
            // trader spends a minute wondering why the gesture died.
            ui.label(
                egui::RichText::new(format!(
                    "the aim shows only where a {} can rest: {} the market",
                    self.account.cmd_trading.kind.label(),
                    if self.account.cmd_trading.kind == CmdEntryKind::Limit {
                        "below it to buy, above it to sell"
                    } else {
                        "above it to buy, below it to sell"
                    },
                ))
                .color(theme::TEXT_SUPPORT)
                .small(),
            );
        }
        if self.account.cmd_trading.enabled
            && self.account.cmd_trading.buy == self.account.cmd_trading.sell
        {
            // A shared key is ambiguous, so the gesture shows nothing —
            // said here rather than discovered over the chart.
            ui.label(
                egui::RichText::new(
                    "buy and sell share a key - the gesture stays hidden until they differ",
                )
                .color(theme::AMBER)
                .small(),
            );
        } else if self.account.cmd_trading.enabled {
            // The gesture is invisible until a key is held; its one line
            // of instructions lives where the toggle does, not in a
            // tooltip a newcomer never hovers.
            ui.label(
                egui::RichText::new(format!(
                    "hold {} over the chart to buy, {} to sell - the dashed line shows \
                     where, and the click places it. Roll the wheel while holding to walk \
                     a stop and target out from the aim; roll back to zero, or press the \
                     wheel, to leave the aim as it was",
                    self.account.cmd_trading.buy.label(),
                    self.account.cmd_trading.sell.label(),
                ))
                .color(theme::TEXT_SUPPORT)
                .small(),
            );
        }
        changed
    }
}

/// The kind the aim places at `price`, or `None` where nothing may rest
/// there.
///
/// [`CmdEntryKind::Auto`] reads the market: above the mark a buy stops in,
/// below it a buy waits at a limit; a sell mirrors. On the mark exactly,
/// nothing can rest — a resting order there would fill on the next print,
/// which is a market order wearing the wrong name.
///
/// A stated kind is honoured only where it is valid. Returning `None`
/// instead of the other kind is the point: the aim's promise is that the
/// label can never advertise an order the press will not make, and a
/// silent substitution would break it in the most expensive way — placing
/// a breakout stop for a trader who came to buy a pullback.
#[must_use]
pub(super) fn resolve_cmd_kind(
    choice: CmdEntryKind,
    side: Side,
    price: Decimal,
    mark: Decimal,
) -> Option<EntryKind> {
    let available = match (price > mark, price < mark, side) {
        (true, _, Side::Buy) | (_, true, Side::Sell) => EntryKind::Stop,
        (true, _, Side::Sell) | (_, true, Side::Buy) => EntryKind::Limit,
        _ => return None,
    };
    match choice {
        CmdEntryKind::Auto => Some(available),
        CmdEntryKind::Limit => (available == EntryKind::Limit).then_some(EntryKind::Limit),
        CmdEntryKind::Stop => (available == EntryKind::Stop).then_some(EntryKind::Stop),
    }
}

/// The frame's cmd-trading preview: computed by `handle_chart_input`,
/// painted by `draw_layer`, clicked through the same geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CmdPreview {
    pub(super) side: Side,
    pub(super) kind: EntryKind,
    /// Snapped price, for the label and the gutter chip.
    pub(super) price: Decimal,
    /// Raw pointer price — what a click hands to `place_resting`, which
    /// snaps for itself (the armed-click path's contract).
    pub(super) raw_price: f64,
    /// The aiming pointer, both coordinates: y is the price, x is where
    /// the label rides. Stored whole so paint and press lay out from the
    /// very same position.
    pub(super) pointer: egui::Pos2,
    /// This aim was invented by the capture hook, not by a held key. It
    /// paints, so a screenshot has something to show, and it never places:
    /// a run with nobody at the keyboard is holding no modifier, and a
    /// stray click during one must not write orders into a journal.
    pub(super) forced: bool,
    /// The protection this order would carry: a strategy's ladder, the
    /// ruler's symmetric pair, or the ticket's typed offsets. Empty when the
    /// order would rest bare.
    ///
    /// One value, computed once, painted by the projection and placed by the
    /// click - a preview that promised one bracket while the order took
    /// another is the worst bug this surface can have.
    pub(super) bracket: Bracket,
    /// How many ticks the ruler stands at; zero means it is not in use.
    pub(super) ruler_ticks: u32,
}

/// The `QUANTICK_CMD_PREVIEW` hook, parsed: which side to aim and, when
/// stated, where along the band to park the virtual pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CmdPreviewForce {
    pub(super) side: Side,
    /// 0.0 is the band's left edge, 1.0 its right; `None` leaves the
    /// pointer mid-band, which is what the hook meant before the label
    /// followed it.
    pub(super) x_fraction: Option<f32>,
}

impl CmdPreviewForce {
    /// `buy`, `sell`, `buy@0.15`. An unparseable fraction degrades to the
    /// mid-band park rather than killing the whole preview — a capture run
    /// that paints nothing is the hardest failure to read.
    pub(super) fn parse(value: &str) -> Option<Self> {
        let (side, fraction) = match value.split_once('@') {
            Some((side, fraction)) => (side, Some(fraction)),
            None => (value, None),
        };
        let side = match side.trim().to_ascii_lowercase().as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return None,
        };
        Some(Self {
            side,
            x_fraction: fraction
                .and_then(|text| text.trim().parse::<f32>().ok())
                // `"NaN"` and `"inf"` parse, and `clamp` passes NaN
                // straight through — which would poison the pointer's x
                // and paint nothing at all, the one outcome this fallback
                // exists to rule out.
                .filter(|fraction: &f32| fraction.is_finite())
                .map(|fraction| fraction.clamp(0.0, 1.0)),
        })
    }
}

/// The cmd preview's geometry from the interactive band and the pointer:
/// the dashed line under the cursor running out to the right edge, and the
/// clickable label riding beside the cursor. One function for paint and
/// press alike, so a painted label and its hit-test can never disagree
/// (the overlay-controls rule).
///
/// The label follows the pointer rather than parking against the right
/// edge: aiming at a price on the left of the plot used to mean crossing
/// the whole chart to click the thing you were already pointing at. It
/// sits *beside* the cursor, never under it, so the crosshair and the
/// candle it rests on stay readable. Left is the preferred side (the line
/// and the price chip run off to the right, so the label completes the
/// sentence from its start); it flips right when the left edge leaves no
/// room, and in a band too narrow for either — well under any window this
/// app opens — it parks against the closer edge.
pub(super) fn cmd_preview_layout(
    band: egui::Rect,
    axis_x: f32,
    pointer: egui::Pos2,
) -> (egui::Pos2, egui::Pos2, egui::Rect) {
    let need = CMD_LABEL_CURSOR_GAP_PX + CMD_LABEL_WIDTH_PX;
    let left = if pointer.x - band.left() >= need {
        pointer.x - need
    } else if band.right() - pointer.x >= need {
        pointer.x + CMD_LABEL_CURSOR_GAP_PX
    } else {
        // Narrower than the label and its gap on either side: the two
        // cannot both hold, so it parks at the left edge and the cursor
        // may cross it. Reaching this needs a band under 260 px — no
        // window this app opens is that small.
        band.left()
    };
    let center_y = clamp_tag_center(pointer.y, band.top(), band.bottom());
    let half = TAG_HEIGHT_PX / 2.0;
    let label = egui::Rect::from_min_max(
        egui::pos2(left, center_y - half),
        egui::pos2(left + CMD_LABEL_WIDTH_PX, center_y + half),
    );
    // The line starts under the cursor and reaches the axis, which is what
    // ties the label beside the hand to the price on the gutter. Close to
    // that edge it starts further left instead, so there is always a line
    // to read.
    let start = egui::pos2(
        pointer
            .x
            .min(band.right() - CMD_LINE_MIN_PX)
            .max(band.left()),
        pointer.y,
    );
    // The *band* stops at the live lane's divider, because that is where a
    // click can still be pressed; the line does not, because it is a read
    // and not a control. Stopping it there left the tape lane — the widest
    // thing on the chart — as a blank gap between the aim and its own price
    // on the axis, so the one place a trader watches the order arrive was
    // the one place the order was invisible. Every other level here already
    // spans to the axis (`level_line`); this now says the same.
    (
        start,
        egui::pos2(axis_x.max(band.right()), pointer.y),
        label,
    )
}
