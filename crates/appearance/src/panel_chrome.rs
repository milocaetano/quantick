//! The settings panels' block structure: the order-flow settings panel and
//! the TRADES · SIM dock share one ground, one title plate, white blocks
//! under graphite header bands, uppercase subgroup captions and one footer
//! plate. Every painter of them reads its colours and sizes here, so the two
//! panels cannot drift apart and a test grades the answer headless.
use std::cmp::Ordering;

use crate::Token;

/// A section block's corner radius and its border width.
pub const BLOCK_RADIUS_PX: f32 = 4.0;
pub const BLOCK_EDGE_PX: f32 = 1.0;
/// What stands between two blocks.
pub const BLOCK_GAP_PX: f32 = 8.0;
/// A block's padding around its body.
pub const BLOCK_PAD_PX: f32 = 8.0;
/// A header band's padding: across, then above and below its words.
pub const BAND_PAD_X_PX: f32 = 8.0;
pub const BAND_PAD_Y_PX: f32 = 6.0;
/// A full-bleed plate's padding: across, then above and below its words.
pub const PLATE_PAD_X_PX: f32 = 10.0;
pub const PLATE_PAD_Y_PX: f32 = 7.0;
/// A subgroup caption's size, and the rule under it.
pub const CAPTION_SIZE: f32 = 10.5;
pub const CAPTION_RULE_PX: f32 = 1.0;
/// A trade card's left stripe in its result colour.
pub const STRIPE_PX: f32 = 3.0;
/// The panel's own margin around its blocks, which a full-bleed plate
/// reaches back across: across, then above and below.
pub const PANEL_MARGIN_X_PX: f32 = 10.0;
pub const PANEL_MARGIN_Y_PX: f32 = 8.0;
/// The space above a subgroup caption, where a separator used to stand.
pub const CAPTION_GAP_PX: f32 = 6.0;
/// A caption's extra space between letters.
pub const CAPTION_TRACKING_PX: f32 = 0.7;
/// The fold triangle's side on a band or a folding caption.
pub const FOLD_ICON_PX: f32 = 9.0;

/// A filled shape and the words written on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    pub fill: Token,
    pub ink: Token,
}

/// Each part of a panel, as the theme paints it.
pub const GROUND: Surface = Surface {
    fill: Token::PanelGround,
    ink: Token::TextPrimary,
};
pub const TITLE_PLATE: Surface = Surface {
    fill: Token::PanelPlate,
    ink: Token::PanelPlateInk,
};
pub const BLOCK: Surface = Surface {
    fill: Token::PanelBlock,
    ink: Token::TextPrimary,
};
pub const BAND: Surface = Surface {
    fill: Token::PanelBand,
    ink: Token::PanelPlateInk,
};
/// The state words on a band's right.
pub const BAND_STATE: Surface = Surface {
    fill: Token::PanelBand,
    ink: Token::PanelBandState,
};
pub const CAPTION: Surface = Surface {
    fill: Token::PanelBlock,
    ink: Token::PanelCaption,
};
pub const FOOTER: Surface = Surface {
    fill: Token::PanelFooter,
    ink: Token::PanelStrongInk,
};
pub const FILTER_BAR: Surface = Surface {
    fill: Token::PanelFilterBar,
    ink: Token::TextPrimary,
};
pub const LIVE_ROW: Surface = Surface {
    fill: Token::PanelLiveRow,
    ink: Token::TextPrimary,
};
/// THIS SESSION / EARLIER SESSIONS, bold on the ground.
pub const SESSION_CAPTION: Surface = Surface {
    fill: Token::PanelGround,
    ink: Token::PanelStrongInk,
};

/// Every surface that carries words, for the contrast tests.
pub const WORDED: [Surface; 10] = [
    GROUND,
    TITLE_PLATE,
    BLOCK,
    BAND,
    BAND_STATE,
    CAPTION,
    FOOTER,
    FILTER_BAR,
    LIVE_ROW,
    SESSION_CAPTION,
];

/// The border of a block, and the rule under the filter bar.
pub const BLOCK_EDGE: Token = Token::PanelBlockEdge;
/// The rule under a subgroup caption.
pub const CAPTION_RULE: Token = Token::PanelCaptionRule;

/// A subgroup caption's words: the name in capitals.
#[must_use]
pub fn caption_words(name: &str) -> String {
    name.to_uppercase()
}

/// A trade card's stripe for a result of `sign`: the theme's buy ink for a
/// win, the loss stripe for a loss, and the block's edge for a flat trade.
#[must_use]
pub const fn result_stripe(sign: Ordering) -> Token {
    match sign {
        Ordering::Greater => Token::BuyInk,
        Ordering::Less => Token::PanelStripeLoss,
        Ordering::Equal => Token::PanelBlockEdge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResolvedTheme, contrast::contrast_ratio};

    /// Text contrast for normal-size words (WCAG AA).
    const TEXT: f32 = 4.5;
    /// Contrast for a non-text mark: a border, a rule, a stripe.
    const MARK: f32 = 3.0;

    fn both() -> [&'static ResolvedTheme; 2] {
        [ResolvedTheme::bundled(false), ResolvedTheme::bundled(true)]
    }

    #[test]
    fn every_panel_word_reads_on_its_surface_in_both_themes() {
        for theme in both() {
            for surface in WORDED {
                let ratio = contrast_ratio(theme.color(surface.ink), theme.color(surface.fill));
                assert!(
                    ratio >= TEXT,
                    "{} {:?} on {:?}: {ratio:.2}",
                    theme.name,
                    surface.ink,
                    surface.fill
                );
            }
        }
    }

    #[test]
    fn light_blocks_stand_apart_from_the_ground_and_the_band_from_its_block() {
        let light = ResolvedTheme::bundled(false);
        let edge = light.color(BLOCK_EDGE);
        assert!(contrast_ratio(edge, light.color(Token::PanelBlock)) >= 2.0);
        assert!(
            contrast_ratio(
                light.color(Token::PanelBand),
                light.color(Token::PanelBlock)
            ) >= MARK
        );
        assert!(
            contrast_ratio(
                light.color(Token::PanelPlate),
                light.color(Token::PanelGround)
            ) >= MARK
        );
    }

    #[test]
    fn light_wears_the_picked_graphite_hexes() {
        let light = ResolvedTheme::bundled(false);
        for (token, rgb) in [
            (Token::PanelGround, [0xDF, 0xE2, 0xE6]),
            (Token::PanelPlate, [0x2B, 0x30, 0x37]),
            (Token::PanelPlateInk, [0xF2, 0xF4, 0xF7]),
            (Token::PanelBlock, [0xFF, 0xFF, 0xFF]),
            (Token::PanelBlockEdge, [0x9A, 0xA1, 0xAB]),
            (Token::PanelBand, [0x4A, 0x50, 0x59]),
            (Token::PanelBandState, [0xD5, 0xDA, 0xE1]),
            (Token::PanelCaption, [0x4A, 0x50, 0x59]),
            (Token::PanelCaptionRule, [0xC9, 0xCE, 0xD6]),
            (Token::PanelFooter, [0xC9, 0xCE, 0xD6]),
            (Token::PanelStrongInk, [0x2B, 0x30, 0x37]),
            (Token::PanelFilterBar, [0xCD, 0xD2, 0xD9]),
            (Token::PanelLiveRow, [0xE9, 0xEC, 0xEF]),
            (Token::PanelStripeLoss, [0xB3, 0x26, 0x1E]),
        ] {
            let [r, g, b, _] = light.color(token).to_array();
            assert_eq!([r, g, b], rgb, "{token:?}");
        }
    }

    /// Dark gains the structure only: every panel colour is one Dark already
    /// paints with.
    #[test]
    fn dark_panel_colours_are_dark_colours_already_there() {
        let dark = ResolvedTheme::bundled(true);
        let existing: Vec<_> = Token::ALL
            .into_iter()
            .filter(|token| !token.key().starts_with("panel_"))
            .map(|token| dark.color(token))
            .collect();
        for token in Token::ALL
            .into_iter()
            .filter(|t| t.key().starts_with("panel_"))
        {
            assert!(
                existing.contains(&dark.color(token)),
                "{token:?} is a new Dark colour"
            );
        }
    }

    #[test]
    fn a_trade_stripe_reads_against_its_white_card() {
        for theme in both() {
            for sign in [Ordering::Greater, Ordering::Less] {
                let stripe = theme.color(result_stripe(sign));
                let ratio = contrast_ratio(stripe, theme.color(Token::PanelBlock));
                assert!(ratio >= MARK, "{} {sign:?}: {ratio:.2}", theme.name);
            }
        }
    }

    #[test]
    fn captions_are_capitals() {
        assert_eq!(caption_words("size & placement"), "SIZE & PLACEMENT");
    }
}
