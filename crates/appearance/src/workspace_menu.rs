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

/// A button filled in a colour, its label in an ink that reaches 4.5:1 on
/// it in both themes: the accent's for the Enter default (Swap, Apply,
/// Save), the error red's for Delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilledButton {
    pub fill: Token,
    pub ink: Token,
}

impl FilledButton {
    /// Swap, Apply and Save.
    #[must_use]
    pub const fn primary() -> Self {
        Self {
            fill: Token::Accent,
            ink: Token::ChipInk,
        }
    }

    /// Delete.
    #[must_use]
    pub const fn danger() -> Self {
        Self {
            fill: Token::ErrorText,
            ink: Token::ChipInk,
        }
    }
}

/// How far a workspace row's fill moves from the list's ground toward the
/// row's own colour: at rest, under the pointer or the keys, and on the
/// tab's own workspace (twice the rest). The capsule's 12% was the start;
/// Dark's support ink on Dark Ink's tint fell under 4.5:1 above 7%, so every
/// row takes these lower shares, never one per preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowTint {
    Rest,
    Hover,
    Current,
}

impl RowTint {
    /// The share of the colour in the fill.
    #[must_use]
    pub const fn share(self) -> f32 {
        match self {
            Self::Rest => 0.035,
            Self::Hover => 0.05,
            Self::Current => 0.07,
        }
    }
}

/// A workspace row's fill: the list's ground as `dark` paints it, `tint`'s
/// share of the way toward `colour`. Menu B and Manage both fill with this.
#[must_use]
pub fn row_fill(colour: [u8; 3], dark: bool, tint: RowTint) -> [u8; 3] {
    let ground = crate::ResolvedTheme::bundled(dark)
        .color(chrome(dark).ground)
        .to_array();
    let share = tint.share();
    let mix = |at: usize| {
        let (from, to) = (f32::from(ground[at]), f32::from(colour[at]));
        (from + (to - from) * share).round().clamp(0.0, 255.0) as u8
    };
    [mix(0), mix(1), mix(2)]
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

#[cfg(test)]
#[path = "workspace_menu_tests.rs"]
mod tests;
