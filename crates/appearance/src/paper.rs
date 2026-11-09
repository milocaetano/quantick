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

/// The ink of a signed amount written on `surface` (a panel's inset, the
/// chart canvas): Dark's candle colours and a muted zero; a light theme's
/// buy ink and loss ink where the surface is pale, and its bright inks where
/// it is dark, such as its slate flow pane.
#[must_use]
pub fn pnl_ink(theme: &ResolvedTheme, surface: Token, sign: Ordering) -> Token {
    if theme.dark {
        return match sign {
            Ordering::Greater => Token::Buy,
            Ordering::Less => Token::Sell,
            Ordering::Equal => Token::TextMuted,
        };
    }
    let pale = relative_luminance(theme.color(surface)) > 0.5;
    match sign {
        Ordering::Greater if pale => Token::BuyInk,
        Ordering::Less if pale => Token::LossInk,
        Ordering::Greater => Token::BrightGainInk,
        Ordering::Less => Token::BrightLossInk,
        Ordering::Equal => Token::TextPrimary,
    }
}

/// The opacity of a plate's close glyph: dimmed at rest, the plate's full
/// ink under the pointer, so the glyph still answers a hover.
#[must_use]
pub const fn close_glyph_alpha(hovered: bool) -> u8 {
    if hovered { 255 } else { 150 }
}

/// One of the trade ledger's row kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A civil day's band; a folded one dims its words.
    Day { folded: bool },
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
        RowKind::Day { folded } => RowLook {
            fill: Some(Token::Control),
            edge: Some(Token::TextPrimary),
            ink: if folded {
                Token::TextMuted
            } else {
                Token::TextPrimary
            },
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
    /// The band under the row: the hover fill while `lit`, when it has one.
    #[must_use]
    pub fn fill_at(&self, lit: bool) -> Option<Token> {
        self.hover_fill.filter(|_| lit).or(self.fill)
    }

    /// The outline a `lit` row adds.
    #[must_use]
    pub fn outline_at(&self, lit: bool) -> Option<Token> {
        self.hover_edge.filter(|_| lit)
    }

    /// The corner radius: a ruled row is square, a plate rounded.
    #[must_use]
    pub fn rounding(&self) -> f32 {
        if self.edge.is_some() { 0.0 } else { 3.0 }
    }

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
