//! The capsule's workspace menu (designer B, "workspace first"): which token
//! each of its parts wears. Dark and Light name different tokens for the
//! menu's ground and its header and footer blocks, so the choice is made
//! here, once, and graded headless; the painter only reads it.

use crate::Token;
use crate::contrast::ACTIVE_TINT_ALPHA;

/// Light's Save tint: 0.16 of the accent, as designer B's `--tint`.
const LIGHT_TINT_ALPHA: u8 = 41;

/// The token of each part of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuChrome {
    /// Behind the list.
    pub ground: Token,
    /// The header and footer blocks, and the rule between each and the list.
    pub block: Token,
    pub block_edge: Token,
    /// The menu's 1 px border.
    pub edge: Token,
    /// A button at rest, and its edge.
    pub control: Token,
    pub control_edge: Token,
    pub hover: Token,
    /// The tab's own workspace in the list.
    pub current: Token,
    pub ink: Token,
    /// A summary line and the switch's state words.
    pub support: Token,
    /// The caption, hints, and a miniature's time panes.
    pub faint: Token,
    /// The check, the unsaved dots, Save's edge and a miniature's flow pane.
    pub accent: Token,
    /// The filter field.
    pub inset: Token,
    /// Save's accent fill while it holds edits, out of 255.
    pub tint: u8,
}

/// The menu as `dark` or Light paints it.
#[must_use]
pub const fn chrome(dark: bool) -> MenuChrome {
    // Designer B's tint: the pressed accent's in Dark, lighter on Light's white.
    let tint = if dark {
        ACTIVE_TINT_ALPHA
    } else {
        LIGHT_TINT_ALPHA
    };
    let (ground, block, block_edge) = if dark {
        (Token::Chrome, Token::PanelBlock, Token::PanelBlockEdge)
    } else {
        (
            Token::Control,
            Token::LedgerSession,
            Token::LedgerSessionEdge,
        )
    };
    MenuChrome {
        ground,
        block,
        block_edge,
        edge: Token::PanelBlockEdge,
        control: Token::Control,
        control_edge: Token::ControlEdge,
        hover: Token::HoverFill,
        current: Token::LedgerSelection,
        ink: Token::TextPrimary,
        support: Token::TextSupport,
        faint: Token::TextFaint,
        accent: Token::Accent,
        inset: Token::Inset,
        tint,
    }
}
