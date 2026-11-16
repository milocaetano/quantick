//! Each workspace colour where it is painted, against the theme it is painted
//! on: the capsule's name on its tint, the open tab's symbol on its sheet.
use super::*;
use quantick_appearance::contrast::contrast_ratio;
use quantick_appearance::{LIGHT, PremultipliedRgba, ResolvedTheme, Token};

fn themes() -> [ResolvedTheme; 2] {
    [
        ResolvedTheme::default(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    ]
}

fn rgb([r, g, b]: Rgb) -> PremultipliedRgba {
    PremultipliedRgba::from_rgb(r, g, b)
}

fn presets() -> impl Iterator<Item = WorkspaceColour> {
    (0..16).filter_map(WorkspaceColour::preset)
}

/// The tint moves the capsule's fill a fixed share toward the colour and
/// keeps the name at 4.5:1 on every preset in both themes.
#[test]
fn the_capsule_name_reads_on_every_preset_tint_in_both_themes() {
    for theme in themes() {
        assert_eq!(
            capsule_fill(WorkspaceColour::Custom([255; 3]), &theme)[3],
            255
        );
        let ink = theme.color(Token::TextPrimary);
        for colour in presets() {
            let fill = PremultipliedRgba::from_premultiplied(capsule_fill(colour, &theme));
            let ratio = contrast_ratio(ink, fill);
            assert!(ratio >= 4.5, "{} {}: {ratio}", theme.name, colour.name());
        }
    }
}

/// Every preset, in both themes, paints the open tab's symbol in its own
/// colour at 3:1 or more on the tab's sheet, and the capsule's border at 3:1
/// on the toolbar around it and on the capsule's tint: no preset falls back.
#[test]
fn no_preset_falls_back_on_the_open_tab_or_the_capsule_in_either_theme() {
    for theme in themes() {
        let sheet = theme.color(Token::ToolbarSheet);
        for colour in presets() {
            let painted = colour.rgb(theme.dark);
            let name = format!("{} {}", theme.name, colour.name());
            assert_eq!(symbol_ink(colour, &theme), Some(painted), "{name}");
            let ratio = contrast_ratio(rgb(painted), sheet);
            assert!(ratio >= 3.0, "{name} symbol and border: {ratio}");
            let tint = PremultipliedRgba::from_premultiplied(capsule_fill(colour, &theme));
            let ratio = contrast_ratio(rgb(painted), tint);
            assert!(ratio >= 3.0, "{name} border on its tint: {ratio}");
        }
    }
}

/// Only a custom colour can fall short on the open tab: below 3:1 its symbol
/// keeps the tab's normal ink.
#[test]
fn a_custom_colour_below_the_floor_falls_back_to_the_tabs_ink() {
    let pale = WorkspaceColour::Custom([0xEE, 0xEE, 0xEE]);
    let [dark, light] = themes();
    assert_eq!(symbol_ink(pale, &dark), Some([0xEE; 3]));
    assert_eq!(symbol_ink(pale, &light), None);
}

/// Every workspace row, in menu B and in Manage, is tinted toward its own
/// colour: at rest, hovered and current (about twice the rest). The name and
/// the summary read 4.5:1 on each, and the current row stands apart from a
/// row of the same colour at rest.
#[test]
fn every_row_tint_keeps_its_words_at_four_and_a_half_to_one() {
    use quantick_appearance::workspace_menu::{RowTint, row_fill};
    for dark in [true, false] {
        let theme = ResolvedTheme::bundled(dark);
        let inks = [Token::TextPrimary, Token::TextSupport].map(|token| theme.color(token));
        for colour in presets() {
            let shade = |tint| row_fill(colour.rgb(dark), dark, tint);
            for tint in [RowTint::Rest, RowTint::Hover, RowTint::Current] {
                for ink in inks {
                    let ratio = contrast_ratio(ink, rgb(shade(tint)));
                    assert!(ratio >= 4.5, "{} dark {dark} {tint:?}: {ratio}", colour.name());
                }
            }
            let apart = contrast_ratio(rgb(shade(RowTint::Current)), rgb(shade(RowTint::Rest)));
            assert!(apart > 1.05, "{} dark {dark}: current reads apart ({apart})", colour.name());
        }
    }
}
