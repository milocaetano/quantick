//! The paper-trading surfaces' colour decisions: the chart's order plates,
//! the ledger's three row kinds and the report's equity paint, so every
//! painter of them reads the same answer and a test grades it headless.
//!
//! Light writes its chart levels on one near-black plate and lets only the
//! ink say what a level is; Dark keeps its accent and side colours. Light's
//! slate flow pane is a dark surface, so it keeps Dark's look too.
use std::cmp::Ordering;

use crate::{PremultipliedRgba, ResolvedTheme, Token, contrast::relative_luminance};

/// A filled shape and the words written on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plate {
    pub fill: Token,
    pub ink: Token,
}

/// Whether `theme` lays plates under its chart's levels: a light theme over
/// a light chart.
#[must_use]
pub fn plates_on_chart(theme: &ResolvedTheme) -> bool {
    !theme.dark && relative_luminance(theme.color(Token::ChartCanvas)) > 0.5
}

/// The plate a light chart lays under a level whose line Dark draws in
/// `line` (the accent for an entry order, the side colour for a stop, a
/// target or the position), its words in the plate's white and its stop,
/// target and position prices in the loss or gain ink. `None` keeps the
/// surface's own colours. The line itself is drawn in the plate's fill.
#[must_use]
pub fn plate_for(theme: &ResolvedTheme, line: PremultipliedRgba) -> Option<Plate> {
    let ink = if line == theme.color(Token::Buy) {
        Token::PlateGain
    } else if line == theme.color(Token::Sell) {
        Token::PlateLoss
    } else {
        Token::OrderPlateInk
    };
    plates_on_chart(theme).then_some(Plate {
        fill: Token::OrderPlate,
        ink,
    })
}

/// A light theme's loss ink on its pale panels.
const LIGHT_LOSS_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(136, 24, 30);
/// A light theme's gain and loss inks where its surface is dark.
const BRIGHT_GAIN_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(100, 220, 160);
const BRIGHT_LOSS_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(255, 145, 145);

/// The ink of a signed amount on `theme`'s panels: Dark's candle colours and
/// a muted zero; a light theme's buy ink and dark red on its pale panels, and
/// bright ones on a dark surface such as its slate flow pane.
#[must_use]
pub fn pnl_ink(theme: &ResolvedTheme, sign: Ordering) -> PremultipliedRgba {
    if theme.dark {
        return theme.color(match sign {
            Ordering::Greater => Token::Buy,
            Ordering::Less => Token::Sell,
            Ordering::Equal => Token::TextMuted,
        });
    }
    let pale = relative_luminance(theme.color(Token::Inset)) > 0.5;
    match sign {
        Ordering::Greater if pale => theme.color(Token::BuyInk),
        Ordering::Less if pale => LIGHT_LOSS_INK,
        Ordering::Greater => BRIGHT_GAIN_INK,
        Ordering::Less => BRIGHT_LOSS_INK,
        Ordering::Equal => theme.color(Token::TextPrimary),
    }
}

/// One of the trade ledger's row kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A civil day's band.
    Day,
    /// A Live or Replay session inside its day.
    Session,
    /// A closed or open trade.
    Trade { long: bool },
}

/// How a ledger row is painted: three kinds, three drawings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowLook {
    /// The band behind the row; `None` leaves the panel showing.
    pub fill: Option<Token>,
    /// The rule down the row's left edge.
    pub edge: Option<Token>,
    pub ink: Token,
    /// Gain and loss inks of a row that writes its own; `None` takes the
    /// panel's signed-amount inks.
    pub pnl: Option<[Token; 2]>,
    /// The fill a hovered or focused row takes instead of its own.
    pub hover_fill: Option<Token>,
    /// The outline a hovered or focused row adds.
    pub hover_edge: Option<Token>,
    /// The ink a row's marks (the source glyph) take instead of their own.
    pub mark: Option<Token>,
}

/// The width of a day band's left rule, in logical pixels.
pub const DAY_EDGE_PX: f32 = 3.0;

/// The look of one ledger row kind in a theme of the given mode.
#[must_use]
pub const fn ledger_row(dark: bool, row: RowKind) -> RowLook {
    let hover = Token::HoverFill;
    match row {
        RowKind::Day => RowLook {
            fill: Some(Token::Control),
            edge: Some(Token::TextPrimary),
            ink: Token::TextPrimary,
            pnl: None,
            hover_fill: Some(hover),
            hover_edge: None,
            mark: None,
        },
        RowKind::Session if dark => RowLook {
            fill: Some(Token::Control),
            edge: None,
            ink: Token::TextPrimary,
            pnl: Some([Token::BuyInk, Token::SellInk]),
            hover_fill: Some(hover),
            hover_edge: None,
            mark: None,
        },
        // A plate cannot lift onto a paler fill without losing its inks, so
        // a hovered one is outlined instead.
        RowKind::Session => RowLook {
            fill: Some(Token::ReplayPlate),
            edge: None,
            ink: Token::ReplayPlateInk,
            pnl: Some([Token::PlateGain, Token::PlateLoss]),
            hover_fill: None,
            hover_edge: Some(Token::Accent),
            // Replay's amber play glyph sinks into the near-black plate.
            mark: Some(Token::ReplayPlateInk),
        },
        RowKind::Trade { long } => RowLook {
            fill: None,
            edge: Some(if long { Token::Buy } else { Token::Sell }),
            ink: Token::TextPrimary,
            pnl: None,
            hover_fill: Some(hover),
            hover_edge: None,
            mark: None,
        },
    }
}

impl RowLook {
    /// The ink of a signed amount on this row, when the row writes its own.
    #[must_use]
    pub const fn pnl_ink(&self, sign: Ordering) -> Option<Token> {
        match (self.pnl, sign) {
            (Some([gain, _]), Ordering::Greater) => Some(gain),
            (Some([_, loss]), Ordering::Less) => Some(loss),
            (Some(_), Ordering::Equal) => Some(self.ink),
            (None, _) => None,
        }
    }
}

/// The report's equity area above (`gain`) or below its baseline. Light's
/// gains take the readable buy ink and its losses the warning red, where
/// Dark keeps its candle colours.
#[must_use]
pub const fn equity_area(dark: bool, gain: bool) -> Token {
    match (dark, gain) {
        (true, true) => Token::Buy,
        (true, false) => Token::Sell,
        (false, true) => Token::BuyInk,
        (false, false) => Token::Warn,
    }
}

/// The chip that prints the deepest drawdown on the equity curve.
#[must_use]
pub const fn drawdown_chip(dark: bool) -> Plate {
    if dark {
        Plate {
            fill: Token::Sell,
            ink: Token::ChipInk,
        }
    } else {
        Plate {
            fill: Token::OrderPlate,
            ink: Token::OrderPlateInk,
        }
    }
}

#[cfg(test)]
#[path = "paper_tests.rs"]
mod tests;
