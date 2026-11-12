//! The paper-trading surfaces' colour decisions: the chart's order plates,
//! the ledger's nested cards and the report's equity paint, so every
//! painter of them reads the same answer and a test grades it headless.
//!
//! Light writes its chart levels on one near-black plate and lets only the
//! ink say what a level is, except a level under the hand or aimed, which
//! wears the white paper plate; Dark keeps its accent and side colours. Light's
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

/// What a paper-trading level on the chart is doing, which decides how its
/// tag and price-axis chip are painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelState {
    /// In the book and left alone: a working order, a live leg.
    Resting,
    /// A protective leg riding an unfilled order: it arms on the fill.
    Pending,
    /// Under the hand: a leg being dragged, or a stop or target the aim
    /// carries before the click.
    Moving,
    /// The aim itself (the Alt ghost): the order the next click sends.
    Aimed,
}

/// Whether a level's tag is outlined rather than filled. A pending leg is
/// outlined in both themes; a light chart also writes a level under the
/// hand on its paper plate (white, edged in the side's tag colour, only the
/// money coloured) instead of the near-black order plate. Dark fills it.
#[must_use]
pub fn outlined_tag(theme: &ResolvedTheme, state: LevelState) -> bool {
    match state {
        LevelState::Resting => false,
        LevelState::Pending => true,
        LevelState::Moving | LevelState::Aimed => plates_on_chart(theme),
    }
}

/// The paper plate: its fill, its one-pixel edge, the words on it, and
/// whether the edge is dashed. The edge is dashed exactly while the order
/// is not sent (the aim); a solid edge is an order in the book or a level
/// that moves with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaperPlate {
    pub fill: Token,
    pub edge: Token,
    pub ink: Token,
    pub dashed: bool,
}

/// The dash and gap of a paper plate's unsent edge, in logical pixels.
pub const PAPER_DASH_PX: f32 = 3.0;
pub const PAPER_GAP_PX: f32 = 2.0;

/// The paper plate a light chart lays under a level drawn in `line` while it
/// is `state`, or `None` where it keeps its own look (Dark, and every
/// resting level). The edge is the side's tag colour, the one its filled
/// price-axis chip wears ([`level_chip`]), so each side has one colour; an
/// entry's is the order plate. A pending leg's plate here is the one
/// `chip_tag` outlines.
#[must_use]
pub fn paper_plate(
    theme: &ResolvedTheme,
    line: PremultipliedRgba,
    state: LevelState,
) -> Option<PaperPlate> {
    let plate = plate_for(theme, line)?;
    let edge = match plate.ink {
        Token::PlateGain => Token::PaperTagGain,
        Token::PlateLoss => Token::PaperTagLoss,
        _ => Token::OrderPlate,
    };
    (state != LevelState::Resting).then_some(PaperPlate {
        fill: Token::Inset,
        edge,
        ink: Token::TextPrimary,
        dashed: state == LevelState::Aimed,
    })
}

/// The price-axis chip of a level drawn in `line`: on a light chart a level
/// under the hand or aimed is filled in its side's tag colour with white
/// digits, so it reads as the paper plate's twin; every other level keeps
/// [`plate_for`] (`None` in Dark: the side colour and `chip_ink`).
#[must_use]
pub fn level_chip(
    theme: &ResolvedTheme,
    line: PremultipliedRgba,
    state: LevelState,
) -> Option<Plate> {
    let plate = plate_for(theme, line)?;
    let fill = match plate.ink {
        Token::PlateGain => Token::PaperTagGain,
        Token::PlateLoss => Token::PaperTagLoss,
        _ => return Some(plate),
    };
    Some(match state {
        LevelState::Moving | LevelState::Aimed => Plate {
            fill,
            ink: Token::ChipInk,
        },
        LevelState::Resting | LevelState::Pending => plate,
    })
}

/// How a light chart writes a measurement's readout: a grey edge round its
/// plate, the signed price line in the gain or loss ink and every other line
/// in the primary ink. `None` keeps the surface's own readout (Dark: no
/// edge, every line in the move's colour).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaperReadout {
    pub edge: Token,
    pub ink: Token,
    pub gain: Token,
    pub loss: Token,
}

/// The readout look of `theme`'s chart; see [`PaperReadout`].
#[must_use]
pub fn paper_readout(theme: &ResolvedTheme) -> Option<PaperReadout> {
    plates_on_chart(theme).then(|| PaperReadout {
        edge: Token::ReadoutEdge,
        ink: Token::TextPrimary,
        gain: pnl_ink(theme, Token::ChartCanvas, Ordering::Greater),
        loss: pnl_ink(theme, Token::ChartCanvas, Ordering::Less),
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

/// One of the trade ledger's row kinds. Each day is a card, each session a
/// box inside its card, each trade a row inside its box: the borders say
/// what holds what, in both themes, and only the tokens' values differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A civil day's header, the top of its card.
    Day,
    /// A Live or Replay session's header, the top of its box.
    Session,
    /// A closed or open trade.
    Trade { long: bool },
}

/// How a ledger row is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowLook {
    /// The row's own plate; `None` leaves its card or box showing.
    pub fill: Option<Token>,
    /// The rule down the row's left edge.
    pub edge: Option<Token>,
    /// The leading words: the date, the session, the side and size.
    pub ink: Token,
    /// The words behind them: prices, the detail line, counts.
    pub detail: Token,
    /// Gain and loss inks of the row's signed amounts.
    pub pnl: [Token; 2],
    /// The fill a hovered or focused row takes instead of its own.
    pub hover_fill: Option<Token>,
    /// The fill a selected row takes.
    pub selected_fill: Option<Token>,
    /// The ink a row's marks (the source glyph) take instead of their own.
    pub mark: Option<Token>,
    /// The corner radius of the row's plate or hover fill.
    pub radius: f32,
}

/// Corner radius of a day card, in logical pixels.
pub const CARD_RADIUS_PX: f32 = 6.0;
/// Corner radius of a session box.
pub const SESSION_RADIUS_PX: f32 = 5.0;
/// Corner radius of a trade row's plate.
pub const TRADE_RADIUS_PX: f32 = 3.0;
/// Width of a trade row's side rule.
pub const TRADE_EDGE_PX: f32 = 3.0;
/// The space under each trade row's plate.
pub const TRADE_GAP_PX: f32 = 3.0;
/// The space above each day card.
pub const CARD_GAP_PX: f32 = 8.0;
/// How far a session box (or a trade no session holds) stands in from its
/// card's sides, and the space under the box inside the card.
pub const SESSION_INSET_PX: f32 = 8.0;
/// The box's own padding under its last row.
pub const SESSION_PAD_PX: f32 = 4.0;
/// How far a trade row stands in from its session box's sides.
pub const TRADE_INSET_PX: f32 = 5.0;
/// The fold caret's size: larger than the words it leads.
pub const CARET_PX: f32 = 14.0;
/// The size of a row's leading line, and of its detail line.
pub const TITLE_PX: f32 = 11.0;
pub const DETAIL_PX: f32 = 10.0;
/// The space between a trade's two lines.
pub const LINE_GAP_PX: f32 = 2.0;
/// The space between a line's left words and its right-hand figure.
pub const SIDE_GAP_PX: f32 = 8.0;
/// A day header's words inside its card.
pub const DAY_PAD: Margin = Margin {
    left: 10.0,
    right: 10.0,
    top: 9.0,
    bottom: 9.0,
};
/// A session header's words inside its box.
pub const SESSION_HEAD_PAD: Margin = Margin {
    left: 9.0,
    right: 9.0,
    top: 7.0,
    bottom: 5.0,
};
/// A trade's words inside its plate, the left counted from its side rule.
pub const TRADE_PAD: Margin = Margin {
    left: 9.0,
    right: 10.0,
    top: 6.0,
    bottom: 6.0,
};

/// The look of one ledger row kind. Light and Dark share the structure; a
/// loss is Dark's soft sell ink and Light's own loss ink, and Light marks a
/// replay in its violet replay ink, never Dark's amber.
#[must_use]
pub const fn ledger_row(dark: bool, row: RowKind) -> RowLook {
    let base = RowLook {
        fill: None,
        edge: None,
        ink: Token::TextPrimary,
        detail: Token::TextMuted,
        pnl: [
            Token::BuyInk,
            if dark { Token::SellInk } else { Token::LossInk },
        ],
        hover_fill: Some(Token::HoverFill),
        selected_fill: None,
        mark: None,
        radius: CARD_RADIUS_PX,
    };
    match row {
        RowKind::Day => base,
        RowKind::Session => RowLook {
            mark: if dark { None } else { Some(Token::ReplayInk) },
            radius: SESSION_RADIUS_PX,
            ..base
        },
        RowKind::Trade { long } => RowLook {
            fill: Some(Token::LedgerCard),
            edge: Some(if long { Token::Buy } else { Token::Sell }),
            selected_fill: Some(Token::LedgerSelection),
            mark: if dark { None } else { Some(Token::ReplayInk) },
            radius: TRADE_RADIUS_PX,
            ..base
        },
    }
}

impl RowLook {
    /// The plate under the row: the selection, else the hover fill while
    /// `lit`, else its own.
    #[must_use]
    pub fn fill_at(&self, lit: bool, selected: bool) -> Option<Token> {
        self.selected_fill
            .filter(|_| selected)
            .or(self.hover_fill.filter(|_| lit))
            .or(self.fill)
    }

    /// The secondary words' ink: a selected row lifts them to its primary
    /// ink, which keeps 4.5:1 on the selection where the muted ink would not.
    #[must_use]
    pub fn detail_at(&self, selected: bool) -> Token {
        if selected && self.selected_fill.is_some() {
            self.ink
        } else {
            self.detail
        }
    }

    /// The ink of a signed amount on this row.
    #[must_use]
    pub const fn pnl_ink(&self, sign: Ordering) -> Token {
        match sign {
            Ordering::Greater => self.pnl[0],
            Ordering::Less => self.pnl[1],
            Ordering::Equal => self.ink,
        }
    }
}

/// Where a row stands in one container, a day card or a session box, as
/// `[opens, closes]`: its first row opens the container, its last closes
/// it. The ledger's nesting (`paper::report::ledger_rows::RowNest`) hands
/// these over as they are.
pub type Ends = [bool; 2];

/// Space around a rectangle, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Margin {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// One container's slice behind one row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameLayer {
    pub fill: Token,
    pub stroke: Token,
    pub radius: f32,
    /// The slice, inset from the row's rectangle.
    pub inset: Margin,
    /// Whether this row opens or closes the container: only those ends are
    /// rounded and stroked across.
    pub ends: Ends,
}

impl FrameLayer {
    /// The slice's top and bottom corner radii: rounded only at the ends
    /// this row closes.
    #[must_use]
    pub fn corners(&self) -> [f32; 2] {
        self.ends
            .map(|closes| if closes { self.radius } else { 0.0 })
    }

    /// How far the container's outline runs past the slice above and below:
    /// past an end this row leaves open, so a clip to the row keeps only
    /// its sides there.
    #[must_use]
    pub fn reach(&self) -> [f32; 2] {
        self.ends
            .map(|closes| if closes { 0.0 } else { self.radius + 1.0 })
    }
}

/// What a row paints behind its content, outermost first, and where its
/// content stands inside its rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowFrame {
    pub layers: [Option<FrameLayer>; 2],
    pub content: Margin,
}

/// The frame of a row inside `day`'s card and, when it has one, `session`'s
/// box. A row in neither (a caption, the "show older" control) is bare.
#[must_use]
pub fn ledger_frame(day: Option<Ends>, session: Option<Ends>) -> RowFrame {
    let mut frame = RowFrame {
        layers: [None, None],
        content: Margin::default(),
    };
    let Some(day) = day else {
        return frame;
    };
    let [day_opens, day_closes] = day;
    let top = if day_opens { CARD_GAP_PX } else { 0.0 };
    frame.layers[0] = Some(FrameLayer {
        fill: Token::LedgerCard,
        stroke: Token::LedgerCardEdge,
        radius: CARD_RADIUS_PX,
        inset: Margin {
            top,
            ..Margin::default()
        },
        ends: day,
    });
    frame.content.top = top;
    let side = match session {
        Some(session) => {
            let [opens, closes] = session;
            let below = if closes { SESSION_INSET_PX } else { 0.0 };
            frame.layers[1] = Some(FrameLayer {
                fill: Token::LedgerSession,
                stroke: Token::LedgerSessionEdge,
                radius: SESSION_RADIUS_PX,
                inset: Margin {
                    left: SESSION_INSET_PX,
                    right: SESSION_INSET_PX,
                    top,
                    bottom: below,
                },
                ends: session,
            });
            frame.content.bottom = if closes { below + SESSION_PAD_PX } else { 0.0 };
            // The session's header spans its box; its trades stand in.
            if opens {
                SESSION_INSET_PX
            } else {
                SESSION_INSET_PX + TRADE_INSET_PX
            }
        }
        // The day's header spans its card.
        None if day_opens => 0.0,
        // A trade no session holds stands where a box would.
        None => {
            if day_closes {
                frame.content.bottom = SESSION_INSET_PX;
            }
            SESSION_INSET_PX
        }
    };
    frame.content.left = side;
    frame.content.right = side;
    frame
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
