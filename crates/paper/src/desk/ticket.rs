//! The ticket's typed form: what the trader wrote in the boxes, and what it
//! says.
//!
//! Text in, values out. The boxes are drawn by the host; reading them —
//! the quantity, the two protective offsets, the stepper, the words the
//! entry buttons wear — is decided here, so a toolbar, a dock tab and a
//! second operator reading the same ticket can never read it two ways.

use quantick_engine::Side;
use quantick_sim::{Bracket, EntryKind, Position};
use rust_decimal::Decimal;

use crate::account::{TicketForm, side_word};
use crate::format::{fmt_decimal, side_word_upper};

/// The order-entry form, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// The quantity box.
    pub qty_text: String,
    /// The order type the entry buttons place.
    pub order_type: EntryKind,
    /// The protective stop's distance, in points; empty places none.
    pub stop_offset_text: String,
    /// The profit target's distance, in points; empty places none.
    pub profit_offset_text: String,
    /// What the trader typed for the fixed risk per trade.
    pub risk_amount_text: String,
    /// What the trader typed for the percentage of capital.
    pub risk_percent_text: String,
    /// What the trader typed for this instrument's point value.
    pub point_value_text: String,
    /// What the trader typed for this instrument's size step.
    pub size_step_text: String,
    /// What the trader typed for this instrument's currency code.
    pub currency_text: String,
    /// What the trader typed for the capital in this instrument's currency.
    pub capital_text: String,
}

impl Default for Ticket {
    fn default() -> Self {
        Self {
            qty_text: "1".to_owned(),
            order_type: EntryKind::Market,
            stop_offset_text: String::new(),
            profit_offset_text: String::new(),
            risk_amount_text: String::new(),
            risk_percent_text: String::new(),
            point_value_text: String::new(),
            size_step_text: String::new(),
            currency_text: String::new(),
            capital_text: String::new(),
        }
    }
}

impl Ticket {
    /// The form quantity, parsed without side effects — the label builders
    /// peek at it every frame and must not toast.
    #[must_use]
    pub fn quantity_preview(&self) -> Option<Decimal> {
        match self.qty_text.trim().parse::<Decimal>() {
            Ok(quantity) if quantity > Decimal::ZERO => Some(quantity),
            _ => None,
        }
    }

    /// The three typed boxes, read. The quantity carries its own complaint
    /// so that the account can raise it only if it ever reaches the box.
    #[must_use]
    pub fn form(&self) -> TicketForm {
        TicketForm {
            quantity: match self.qty_text.trim().parse::<Decimal>() {
                Ok(quantity) if quantity > Decimal::ZERO => Ok(quantity),
                _ => Err(format!(
                    "SIM: quantity must be a positive number - got `{}`",
                    self.qty_text.trim(),
                )),
            },
            // Both boxes or neither: one that does not parse fails the pair,
            // which is what `ticket_bracket`'s `?` did.
            offsets: match (
                parse_offset(&self.stop_offset_text),
                parse_offset(&self.profit_offset_text),
            ) {
                (Ok(stop), Ok(profit)) => Some((stop, profit)),
                _ => None,
            },
        }
    }

    /// The bracket the ticket's offsets describe, or the complaint about the
    /// text that does not parse - which the host shows beside the box.
    ///
    /// # Errors
    ///
    /// The sentence naming the offset that is not a positive number.
    pub fn parse_bracket(&self, side: Side, reference: Decimal) -> Result<Bracket, String> {
        let stop_offset = parse_offset(&self.stop_offset_text).map_err(|got| {
            format!("SIM: the stop offset must be a positive number of points - got `{got}`")
        })?;
        let profit_offset = parse_offset(&self.profit_offset_text).map_err(|got| {
            format!("SIM: the profit offset must be a positive number of points - got `{got}`")
        })?;
        let form = TicketForm {
            quantity: Ok(Decimal::ONE),
            offsets: Some((stop_offset, profit_offset)),
        };
        Ok(form.bracket(side, reference))
    }

    /// Walk the typed quantity by `notches` of `unit`, never below `floor`.
    ///
    /// The step is the instrument's, not one. A hard-coded 1 is already
    /// wrong on any instrument whose lot is fractional — a press moved a
    /// crypto size by a hundred thousand steps — and the floor is the
    /// instrument's minimum rather than "anything above zero", so the
    /// steppers can only ever land on a size the venue would take.
    pub fn step_quantity(&mut self, notches: Decimal, unit: Decimal, floor: Decimal) {
        let current = self
            .qty_text
            .trim()
            .parse::<Decimal>()
            .ok()
            .filter(|quantity| *quantity > Decimal::ZERO)
            .unwrap_or(floor);
        let next = current.saturating_add(notches.saturating_mul(unit));
        if next >= floor {
            self.qty_text = fmt_decimal(next);
        }
    }

    /// The entry buttons' hover text: the quantity and protective offsets
    /// the press will use, which the toolbar itself has no widgets for.
    #[must_use]
    pub fn entry_hover(&self, side: Side) -> String {
        let quantity = match self.quantity_preview() {
            Some(qty) => format!("quantity {}", fmt_decimal(qty)),
            None => "the quantity is not a positive number".to_owned(),
        };
        let bracket = match (
            parse_offset(&self.stop_offset_text),
            parse_offset(&self.profit_offset_text),
        ) {
            (Ok(None), Ok(None)) => "no protective bracket set".to_owned(),
            (Ok(stop), Ok(profit)) => {
                let mut parts = Vec::new();
                if let Some(stop) = stop {
                    parts.push(format!("stop {} pts", fmt_decimal(stop)));
                }
                if let Some(profit) = profit {
                    parts.push(format!("target {} pts", fmt_decimal(profit)));
                }
                format!("{} on fill", parts.join(" / "))
            }
            _ => "an offset field needs fixing".to_owned(),
        };
        let hotkey = match side {
            Side::Buy => "Shift+B",
            Side::Sell => "Shift+S",
        };
        format!(
            "simulated market {} - fills at the next print; {quantity}, {bracket} \
             (Trading tab) · {hotkey}",
            side_word(side),
        )
    }
}

/// State-aware entry label: what pressing this side's button would do to
/// the open `position` — `SELL 1 (closes)`, `SELL 5 (reverses to short 4)`.
/// Whether a press closes or flips hangs on a quantity field the toolbar
/// never shows, so the button itself must say. `quantity` is the size the
/// press would send; the bare side word stands in while there is none — the
/// click will toast the correction.
#[must_use]
pub fn entry_label(side: Side, quantity: Option<Decimal>, position: Option<&Position>) -> String {
    let word = side_word_upper(side);
    let Some(qty) = quantity else {
        return word.to_owned();
    };
    let qty_text = fmt_decimal(qty);
    let Some(position) = position else {
        return format!("{word} {qty_text}");
    };
    if position.side == side {
        return format!(
            "{word} {qty_text} (adds to {})",
            fmt_decimal(position.quantity.saturating_add(qty)),
        );
    }
    match qty.cmp(&position.quantity) {
        std::cmp::Ordering::Less => format!(
            "{word} {qty_text} (closes {qty_text} of {})",
            fmt_decimal(position.quantity),
        ),
        std::cmp::Ordering::Equal => format!("{word} {qty_text} (closes)"),
        std::cmp::Ordering::Greater => format!(
            "{word} {qty_text} (reverses to {} {})",
            match side {
                Side::Buy => "long",
                Side::Sell => "short",
            },
            fmt_decimal(qty.saturating_sub(position.quantity)),
        ),
    }
}

/// Empty means "none"; otherwise a strictly positive decimal.
///
/// # Errors
///
/// The trimmed text, when it is neither empty nor a positive number.
pub fn parse_offset(text: &str) -> Result<Option<Decimal>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<Decimal>() {
        Ok(value) if value > Decimal::ZERO => Ok(Some(value)),
        _ => Err(trimmed.to_owned()),
    }
}

/// A protective price the ticket's offset away from the average entry:
/// the losing side for a stop, the winning side for a target. A long's
/// stop and a short's target sit below the entry; the other two above.
#[must_use]
pub fn offset_price(position: &Position, offset: Decimal, stop: bool) -> Decimal {
    let below = (position.side == Side::Buy) == stop;
    if below {
        position.avg_price.saturating_sub(offset)
    } else {
        position.avg_price.saturating_add(offset)
    }
}
