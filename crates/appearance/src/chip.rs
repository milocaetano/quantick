//! A bare header chip's colour decisions — the timeframe presets — so the
//! chosen one reads as chosen in every theme, not only by its ink.
use crate::Token;

/// The fill behind a chip: the selected one stands on a control's own fill
/// (Light's white control is the canvas, so there it takes the tag grey), a
/// hovered one lifts, the rest stay bare on the header.
#[must_use]
pub const fn chip_fill(selected: bool, hovered: bool, dark: bool) -> Option<Token> {
    match (selected, hovered) {
        (true, _) if dark => Some(Token::Control),
        (true, _) => Some(Token::TagBg),
        (false, true) => Some(Token::BareHover),
        (false, false) => None,
    }
}

/// A chip's label: the selected one in the primary ink, the rest muted.
#[must_use]
pub const fn chip_ink(selected: bool) -> Token {
    if selected {
        Token::TextPrimary
    } else {
        Token::TextMuted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LIGHT, ResolvedTheme, contrast::contrast_ratio};

    #[test]
    fn the_selected_chip_wears_the_control_fill_and_a_hovered_one_the_hover() {
        for dark in [true, false] {
            let selected = if dark { Token::Control } else { Token::TagBg };
            assert_eq!(chip_fill(true, false, dark), Some(selected));
            assert_eq!(chip_fill(true, true, dark), Some(selected));
            assert_eq!(chip_fill(false, true, dark), Some(Token::BareHover));
            assert_eq!(chip_fill(false, false, dark), None);
        }
        assert_eq!(chip_ink(true), Token::TextPrimary);
        assert_eq!(chip_ink(false), Token::TextMuted);
    }

    /// In dark and light alike the selected fill reads apart from the canvas
    /// and the chrome a header sits on, and its label stays readable.
    #[test]
    fn the_selected_fill_stands_out_and_stays_readable_in_both_themes() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for theme in [&dark, &light] {
            let fill = theme.color(chip_fill(true, false, theme.dark).unwrap());
            for under in [Token::Canvas, Token::Chrome] {
                let apart = contrast_ratio(fill, theme.color(under));
                assert!(apart >= 1.1, "{} on {}: {apart}", theme.name, under.key());
            }
            let ratio = contrast_ratio(theme.color(chip_ink(true)), fill);
            assert!(ratio >= 4.5, "{}: {ratio}", theme.name);
        }
    }
}
