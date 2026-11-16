//! The workspace menu's filled buttons and the capsule's miniature, graded on
//! the bundled themes.
use super::*;
use crate::contrast::contrast_ratio;
use crate::{PremultipliedRgba, ResolvedTheme};

/// Every filled button's label reaches 4.5:1 on its fill in both themes: the
/// accent's Swap, Apply and Save and the error red's Delete.
#[test]
fn every_filled_button_label_reads_on_its_fill_in_both_themes() {
    for dark in [true, false] {
        let theme = ResolvedTheme::bundled(dark);
        let buttons = [
            ("Delete", FilledButton::danger()),
            ("Swap", FilledButton::primary()),
            ("Apply", FilledButton::primary()),
            ("Save", FilledButton::primary()),
        ];
        for (label, button) in buttons {
            let ratio = contrast_ratio(theme.color(button.ink), theme.color(button.fill));
            assert!(ratio >= 4.5, "{label} dark {dark}: {ratio}");
        }
    }
}

/// The capsule's layout miniature is a neutral grey, not the accent's blue.
#[test]
fn the_capsule_miniature_is_neutral_grey_in_both_themes() {
    let grey = |dark| ResolvedTheme::bundled(dark).color(Token::CapsuleMiniature);
    assert_eq!(grey(true), PremultipliedRgba::from_rgb(0x8A, 0x93, 0xA1));
    assert_eq!(grey(false), PremultipliedRgba::from_rgb(0x7C, 0x7C, 0x7C));
}
