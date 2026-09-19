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
pub(crate) fn draw_risk_block(ui: &mut eframe::egui::Ui, mut block: RiskBlock<'_>) -> bool {
    use crate::paper_chrome::caption;

    ui.add_space(4.0);
    ui.label(caption("RISK PER TRADE"));
    let mut changed = block.basis_pills(ui);

    let declared = block.book.get(block.symbol).cloned();
    let currency_code = declared.as_ref().map_or_else(
        || block.currency_text.trim().to_uppercase(),
        |money| money.currency.code().to_owned(),
    );
    match block.settings.basis {
        RiskBasis::Off => {}
        RiskBasis::Amount => changed |= block.amount_limit(ui, &currency_code),
        RiskBasis::PercentOfCapital => changed |= block.percent_limit(ui, &currency_code),
    }
    if block.settings.basis != RiskBasis::Off {
        changed |= block.lock_toggle(ui);
        changed |= InstrumentMoneySection {
            symbol: block.symbol,
            book: block.book,
            declared: declared.as_ref(),
            currency_code: &currency_code,
            point_value_text: block.point_value_text,
            size_step_text: block.size_step_text,
            currency_text: block.currency_text,
        }
        .show(ui);
    }
    changed
}

impl RiskBlock<'_> {
    /// The three bases as pills; choosing another one is a change.
    fn basis_pills(&mut self, ui: &mut eframe::egui::Ui) -> bool {
        use crate::paper_chrome::pill_toggle;

        let mut changed = false;
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
                let on = self.settings.basis == basis;
                if pill_toggle(ui, label, on, hover).clicked() && !on {
                    self.settings.basis = basis;
                    changed = true;
                }
            }
        });
        changed
    }

    /// "Lose at most N <currency>" — the fixed-amount limit.
    fn amount_limit(&mut self, ui: &mut eframe::egui::Ui, currency_code: &str) -> bool {
        use eframe::egui;

        use crate::theme;

        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Lose at most")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            if edit_decimal(ui, self.amount_text, &mut self.settings.amount, 64.0) {
                changed = true;
            }
            ui.label(
                egui::RichText::new(if currency_code.is_empty() {
                    "per trade"
                } else {
                    currency_code
                })
                .color(theme::TEXT_FAINT)
                .small(),
            );
        });
        changed
    }

    /// "Lose at most N % of C <currency>" — the share of declared capital,
    /// and the capital itself, kept per currency.
    fn percent_limit(&mut self, ui: &mut eframe::egui::Ui, currency_code: &str) -> bool {
        use eframe::egui;

        use crate::theme;

        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Lose at most")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            if edit_decimal(ui, self.percent_text, &mut self.settings.percent, 44.0) {
                changed = true;
            }
            ui.label(egui::RichText::new("% of").color(theme::TEXT_FAINT).small());
            let mut capital_value = currency_code
                .is_empty()
                .then_some(Decimal::ZERO)
                .or_else(|| self.capital.get(currency_code).copied())
                .unwrap_or(Decimal::ZERO);
            if edit_decimal(ui, self.capital_text, &mut capital_value, 72.0)
                && !currency_code.is_empty()
            {
                if capital_value > Decimal::ZERO {
                    self.capital.insert(currency_code.to_owned(), capital_value);
                } else {
                    self.capital.remove(currency_code);
                }
                changed = true;
            }
            ui.label(
                egui::RichText::new(if currency_code.is_empty() {
                    "capital"
                } else {
                    currency_code
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
        changed
    }

    /// Whether an entry over the limit is refused or only sized.
    fn lock_toggle(&mut self, ui: &mut eframe::egui::Ui) -> bool {
        use eframe::egui;

        let mut lock = self.settings.lock;
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
            self.settings.lock = lock;
            return true;
        }
        false
    }
}

/// The two facts no feed reports, and the currency they are in.
///
/// Declared per symbol, never derived: a wrong point value is a wrong
/// position size, and unlike a wrong row on a chart it is invisible until
/// it fills.
struct InstrumentMoneySection<'a> {
    symbol: &'a str,
    book: &'a mut InstrumentBook,
    /// The declaration on file before this frame's edits.
    declared: Option<&'a InstrumentMoney>,
    currency_code: &'a str,
    point_value_text: &'a mut String,
    size_step_text: &'a mut String,
    currency_text: &'a mut String,
}

impl InstrumentMoneySection<'_> {
    fn show(self, ui: &mut eframe::egui::Ui) -> bool {
        use eframe::egui;

        use crate::theme;

        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if self.symbol.is_empty() {
                    "instrument"
                } else {
                    self.symbol
                })
                .color(theme::TEXT_MUTED)
                .small(),
            );
            ui.label(
                egui::RichText::new("point value")
                    .color(theme::TEXT_FAINT)
                    .small(),
            );
            let mut point_value = self
                .declared
                .map_or(Decimal::ZERO, |money| money.point_value);
            let point_changed = edit_decimal(ui, self.point_value_text, &mut point_value, 56.0);
            ui.label(egui::RichText::new("step").color(theme::TEXT_FAINT).small());
            let mut size_step = self.declared.map_or(Decimal::ZERO, |money| money.size_step);
            let step_changed = edit_decimal(ui, self.size_step_text, &mut size_step, 56.0);
            let currency_changed = edit_currency(ui, self.currency_text, self.currency_code);
            if point_changed || step_changed || currency_changed {
                match declaration(self.currency_text, point_value, size_step, self.declared) {
                    Some(money) => {
                        self.book.insert(self.symbol.to_owned(), money);
                    }
                    // Half a declaration is no declaration. Dropping it is
                    // what keeps the ticket saying "nothing here knows what
                    // one point is worth" instead of sizing off a stray
                    // number the trader was still typing.
                    None => {
                        self.book.remove(self.symbol);
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
        changed
    }
}

/// The instrument money the three fields declare, or `None` while any of
/// them is missing.
fn declaration(
    currency_text: &str,
    point_value: Decimal,
    size_step: Decimal,
    previous: Option<&InstrumentMoney>,
) -> Option<InstrumentMoney> {
    let currency = Currency::new(currency_text)?;
    if point_value <= Decimal::ZERO || size_step <= Decimal::ZERO {
        return None;
    }
    Some(InstrumentMoney {
        point_value,
        size_step,
        // A minimum the trader declared is theirs to keep: a venue whose
        // smallest order is not one increment exists, and overwriting it
        // here made the record's field write-only.
        min_size: previous.map_or(size_step, |money| money.min_size),
        max_size: previous.and_then(|money| money.max_size),
        currency,
        source: quantick_sim::MoneySource::Declared,
    })
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
