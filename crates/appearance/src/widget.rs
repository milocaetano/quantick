//! The toolkit widgets' colours that differ by mode, so the chrome and every
//! painter that imitates a widget read the same answer.
//!
//! Dark lifts a hovered control onto its border colour and leaves a resting
//! one unedged. Light's controls are white on a pale grey chrome: unedged
//! they vanish, and lifted onto its mid-grey border their words drop under
//! 4.5:1, so Light edges them with the border and hovers them on `tag_bg`.
use crate::Token;

/// The fill under a hovered control or row.
#[must_use]
pub const fn hover_fill(dark: bool) -> Token {
    if dark { Token::Border } else { Token::TagBg }
}

/// The edge of a framed control at rest; `None` keeps the toolkit's own.
#[must_use]
pub const fn resting_edge(dark: bool) -> Option<Token> {
    if dark { None } else { Some(Token::Border) }
}

/// A bare text action on a connection screen (`Reconnect`), and whether it
/// is struck bold. Dark keeps it in the screen's meta grey; Light writes the
/// one thing to press in the primary ink, so it is not the faintest word there.
#[must_use]
pub const fn text_action(dark: bool) -> (Token, bool) {
    if dark {
        (Token::LoadingMeta, false)
    } else {
        (Token::TextPrimary, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LIGHT, ResolvedTheme, contrast::contrast_ratio};

    #[test]
    fn dark_keeps_its_hover_unedged_controls_and_meta_grey_action() {
        assert_eq!(hover_fill(true), Token::Border);
        assert_eq!(resting_edge(true), None);
        assert_eq!(text_action(true), (Token::LoadingMeta, false));
    }

    #[test]
    fn light_writes_its_connection_action_bold_in_the_primary_ink() {
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        let (ink, bold) = text_action(false);
        assert!(bold);
        assert_eq!(ink, Token::TextPrimary);
        assert!(contrast_ratio(light.color(ink), light.color(Token::ChartCanvas)) >= 4.5);
    }

    /// Light's white controls stand off the chrome by their edge, and a
    /// hovered one still reads apart from its rest while its words keep 4.5:1.
    #[test]
    fn light_controls_are_edged_and_their_hover_keeps_the_words_readable() {
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        let edge = light.color(resting_edge(false).unwrap());
        assert!(contrast_ratio(edge, light.color(Token::Chrome)) >= 3.0);
        let hover = light.color(hover_fill(false));
        assert!(contrast_ratio(hover, light.color(Token::Control)) >= 1.1);
        for ink in [Token::TextPrimary, Token::TextMuted, Token::BuyInk] {
            let ratio = contrast_ratio(light.color(ink), hover);
            assert!(ratio >= 4.5, "{}: {ratio}", ink.key());
        }
    }
}
