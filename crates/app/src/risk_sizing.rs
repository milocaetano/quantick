//! Risk per trade: the block the ticket draws it with.
//!
//! The policy — what one trade may lose, the lock, the capital, the
//! instrument money and the one sentence both the ticket and the control
//! plane read — lives in `quantick_paper::risk_sizing`, beside the account
//! that sizes entries with it. What stays here is the surface: the fields a
//! trader types into, rewritten from the model while they do not hold the
//! keyboard. Every policy name is re-exported so callers keep one address.

use quantick_sim::{Currency, InstrumentMoney};
use rust_decimal::Decimal;

pub(crate) use quantick_paper::risk_sizing::*;

/// Everything the risk block reads and writes, borrowed from the ticket.
///
/// A struct rather than nine arguments, and the whole surface lives here
/// rather than in the ticket, so the file that already carries the order
/// form does not absorb another feature. The ticket calls this once.
pub(crate) struct RiskBlock<'a> {
    pub symbol: &'a str,
    pub settings: &'a mut RiskSettings,
    pub capital: &'a mut Capital,
    pub book: &'a mut InstrumentBook,
    pub amount_text: &'a mut String,
    pub percent_text: &'a mut String,
    pub capital_text: &'a mut String,
    pub point_value_text: &'a mut String,
    pub size_step_text: &'a mut String,
    pub currency_text: &'a mut String,
}

/// Draw the risk-per-trade block. Returns whether anything changed, so the
/// caller persists exactly when a trader actually moved something.
///
/// Every field follows the same rule: while it does not hold the keyboard,
/// its text is rewritten from the model. That is what makes switching
/// symbols show the new instrument's numbers without a marker to keep in
/// step — and it is why a half-typed value is never clobbered mid-edit.
pub(crate) fn draw_risk_block(ui: &mut eframe::egui::Ui, block: RiskBlock<'_>) -> bool {
    use eframe::egui;

    use crate::paper_chrome::{caption, pill_toggle};
    use crate::theme;

    let mut changed = false;
    ui.add_space(4.0);
    ui.label(caption("RISK PER TRADE"));

    ui.horizontal(|ui| {
        for (basis, label, hover) in [
            (RiskBasis::Off, "off", "type the size yourself, as always"),
            (
                RiskBasis::Amount,
                "amount",
                "a fixed amount of money one trade may lose",
            ),
            (
                RiskBasis::PercentOfCapital,
                "% of capital",
                "a share of the capital you declared for this instrument's currency",
            ),
        ] {
            let on = block.settings.basis == basis;
            if pill_toggle(ui, label, on, hover).clicked() && !on {
                block.settings.basis = basis;
                changed = true;
            }
        }
    });

    let declared = block.book.get(block.symbol).cloned();
    let currency_code = declared.as_ref().map_or_else(
        || block.currency_text.trim().to_uppercase(),
        |money| money.currency.code().to_owned(),
    );

    match block.settings.basis {
        RiskBasis::Off => {}
        RiskBasis::Amount => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Lose at most")
                        .color(theme::TEXT_MUTED)
                        .small(),
                );
                if edit_decimal(ui, block.amount_text, &mut block.settings.amount, 64.0) {
                    changed = true;
                }
                ui.label(
                    egui::RichText::new(if currency_code.is_empty() {
                        "per trade"
                    } else {
                        currency_code.as_str()
                    })
                    .color(theme::TEXT_FAINT)
                    .small(),
                );
            });
        }
        RiskBasis::PercentOfCapital => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Lose at most")
                        .color(theme::TEXT_MUTED)
                        .small(),
                );
                if edit_decimal(ui, block.percent_text, &mut block.settings.percent, 44.0) {
                    changed = true;
                }
                ui.label(egui::RichText::new("% of").color(theme::TEXT_FAINT).small());
                let mut capital_value = currency_code
                    .is_empty()
                    .then_some(Decimal::ZERO)
                    .or_else(|| block.capital.get(&currency_code).copied())
                    .unwrap_or(Decimal::ZERO);
                if edit_decimal(ui, block.capital_text, &mut capital_value, 72.0)
                    && !currency_code.is_empty()
                {
                    if capital_value > Decimal::ZERO {
                        block.capital.insert(currency_code.clone(), capital_value);
                    } else {
                        block.capital.remove(&currency_code);
                    }
                    changed = true;
                }
                ui.label(
                    egui::RichText::new(if currency_code.is_empty() {
                        "capital"
                    } else {
                        currency_code.as_str()
                    })
                    .color(theme::TEXT_FAINT)
                    .small(),
                );
            });
            ui.label(
                egui::RichText::new(
                    "a share of the capital you declared, not of your session's result",
                )
                .color(theme::TEXT_FAINT)
                .small(),
            );
        }
    }

    if block.settings.basis != RiskBasis::Off {
        let mut lock = block.settings.lock;
        if ui
            .checkbox(
                &mut lock,
                egui::RichText::new("Refuse an entry over it").small(),
            )
            .on_hover_text(
                "on: an entry whose stop risks more than this does not go out. Turn it off to \
                 take one anyway.",
            )
            .changed()
        {
            block.settings.lock = lock;
            changed = true;
        }

        // The two facts no feed reports. Declared per symbol, never derived:
        // a wrong point value is a wrong position size, and unlike a wrong
        // row on a chart it is invisible until it fills.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if block.symbol.is_empty() {
                    "instrument"
                } else {
                    block.symbol
                })
                .color(theme::TEXT_MUTED)
                .small(),
            );
            ui.label(
                egui::RichText::new("point value")
                    .color(theme::TEXT_FAINT)
                    .small(),
            );
            let mut point_value = declared
                .as_ref()
                .map_or(Decimal::ZERO, |money| money.point_value);
            let point_changed = edit_decimal(ui, block.point_value_text, &mut point_value, 56.0);
            ui.label(egui::RichText::new("step").color(theme::TEXT_FAINT).small());
            let mut size_step = declared
                .as_ref()
                .map_or(Decimal::ZERO, |money| money.size_step);
            let step_changed = edit_decimal(ui, block.size_step_text, &mut size_step, 56.0);
            let currency_changed = edit_currency(ui, block.currency_text, &currency_code);
            if point_changed || step_changed || currency_changed {
                let currency = Currency::new(block.currency_text);
                match (
                    currency,
                    point_value > Decimal::ZERO,
                    size_step > Decimal::ZERO,
                ) {
                    (Some(currency), true, true) => {
                        block.book.insert(
                            block.symbol.to_owned(),
                            InstrumentMoney {
                                point_value,
                                size_step,
                                // A minimum the trader declared is theirs to
                                // keep: a venue whose smallest order is not
                                // one increment exists, and overwriting it
                                // here made the record's field write-only.
                                min_size: declared
                                    .as_ref()
                                    .map_or(size_step, |money| money.min_size),
                                max_size: declared.as_ref().and_then(|money| money.max_size),
                                currency,
                                source: quantick_sim::MoneySource::Declared,
                            },
                        );
                    }
                    // Half a declaration is no declaration. Dropping it is
                    // what keeps the ticket saying "nothing here knows what
                    // one point is worth" instead of sizing off a stray
                    // number the trader was still typing.
                    _ => {
                        block.book.remove(block.symbol);
                    }
                }
                changed = true;
            }
        });
        ui.label(
            egui::RichText::new(
                "what one point of price is worth per unit held, and the smallest size you can \
                 trade. No feed reports these; saved per symbol.",
            )
            .color(theme::TEXT_FAINT)
            .small(),
        );
    }

    changed
}

/// The currency field, following the same rule as the decimal ones: while it
/// does not hold the keyboard, its text is the model's.
///
/// Without this the field kept the previous instrument's currency after a
/// symbol switch, and the next nudge of the point value wrote *that* currency
/// onto the new instrument - a money value in a currency nobody chose.
fn edit_currency(ui: &mut eframe::egui::Ui, text: &mut String, declared: &str) -> bool {
    use eframe::egui;

    let response = ui
        .add(
            egui::TextEdit::singleline(text)
                .desired_width(48.0)
                .hint_text("BRL"),
        )
        .on_hover_text("the currency this point value is in - never converted");
    if !response.has_focus() && !response.changed() {
        if text.as_str() != declared {
            text.clear();
            text.push_str(declared);
        }
        return false;
    }
    response.changed()
}

/// A decimal field that follows the model while the trader is not typing in
/// it.
///
/// Returns whether the value changed. An unparsable or blank field leaves
/// the model alone and is not a change: a half-typed "0." must not reset a
/// point value to nothing between two keystrokes.
fn edit_decimal(
    ui: &mut eframe::egui::Ui,
    text: &mut String,
    value: &mut Decimal,
    width: f32,
) -> bool {
    use eframe::egui;

    let response = ui.add(egui::TextEdit::singleline(text).desired_width(width));
    if !response.has_focus() && !response.changed() {
        let shown = if *value > Decimal::ZERO {
            crate::paper_chrome::fmt_decimal(*value)
        } else {
            String::new()
        };
        if *text != shown {
            *text = shown;
        }
        return false;
    }
    if !response.changed() {
        return false;
    }
    // Strictly positive: a half-typed "0." is unparsable and therefore safe,
    // but a bare "0" parses - and as the first keystroke of a retype it used
    // to clear the whole instrument declaration, taking the size step and the
    // currency with it.
    match Decimal::from_str_exact(text.trim()) {
        Ok(parsed) if parsed > Decimal::ZERO && parsed != *value => {
            *value = parsed;
            true
        }
        _ => false,
    }
}
