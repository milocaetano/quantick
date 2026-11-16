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
/// tab's own workspace, so each row wears its colour as the selected one
/// does. The words keep 4.5:1 by their ink ([`RowWords::ink`]), never by a
/// lower share.
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
            Self::Rest => 0.14,
            Self::Hover => 0.18,
            Self::Current => 0.24,
        }
    }
}

/// `share` of the way from `ground` toward `colour`: a workspace row's fill
/// over the ground the theme paints.
#[must_use]
pub fn mix(ground: [u8; 3], colour: [u8; 3], share: f32) -> [u8; 3] {
    let at = |at: usize| {
        let (from, to) = (f32::from(ground[at]), f32::from(colour[at]));
        (from + (to - from) * share).round().clamp(0.0, 255.0) as u8
    };
    [at(0), at(1), at(2)]
}

/// A workspace row's fill: `toward`'s colour and share over `ground` as
/// the painter resolves it (its alpha set aside).
#[must_use]
pub fn mixed([r, g, b, _]: [u8; 4], (colour, share): ([u8; 3], f32)) -> [u8; 3] {
    mix([r, g, b], colour, share)
}

/// Words on a workspace row's tint: the row's colour over the `ground`
/// token, and whether it is the tab's own. The painter chooses their ink
/// with the theme it paints, so a theme of the trader's own reads too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowWords {
    pub ground: Token,
    pub colour: [u8; 3],
    pub current: bool,
}

impl RowWords {
    /// The fills the row wears over `ground`: its rest and hover, or its
    /// current.
    #[must_use]
    pub fn fills(self, ground: [u8; 3]) -> Vec<[u8; 3]> {
        let worn: &[RowTint] = match self.current {
            true => &[RowTint::Current],
            false => &[RowTint::Rest, RowTint::Hover],
        };
        (worn.iter())
            .map(|tint| mix(ground, self.colour, tint.share()))
            .collect()
    }

    /// [`RowWords::ink`] for words in `own`, each token as `solid` resolves
    /// it (alpha set aside): the painter's live theme.
    #[must_use]
    pub fn ink_by(self, own: Token, solid: impl Fn(Token) -> [u8; 4]) -> [u8; 3] {
        let rgb = |token| {
            let [r, g, b, _] = solid(token);
            [r, g, b]
        };
        self.ink(rgb(self.ground), rgb(own), rgb(Token::TextPrimary))
    }

    /// The ink of words in `own` on those fills: `own` while it reads 4.5:1
    /// on each, else `primary`, else white or near-black, whichever reads
    /// more. Menu B and Manage both choose with this.
    #[must_use]
    pub fn ink(self, ground: [u8; 3], own: [u8; 3], primary: [u8; 3]) -> [u8; 3] {
        use crate::{PremultipliedRgba as Rgba, contrast::contrast_ratio};
        let fills = self.fills(ground);
        let rgba = |[r, g, b]: [u8; 3]| Rgba::from_rgb(r, g, b);
        let least = |ink: [u8; 3]| {
            let ratios = fills
                .iter()
                .map(|fill| contrast_ratio(rgba(ink), rgba(*fill)));
            ratios.fold(f32::MAX, f32::min)
        };
        let strong = [[255; 3], [17; 3]]
            .into_iter()
            .max_by(|a, b| least(*a).total_cmp(&least(*b)));
        let readable = [own, primary].into_iter().find(|ink| least(*ink) >= 4.5);
        readable.or(strong).unwrap_or(own)
    }
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
