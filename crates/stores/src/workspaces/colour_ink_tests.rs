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
/// colour: at rest, hovered and current. The name and the summary, in the
/// ink [`row_ink`] chooses for the fills the row can wear, read 4.5:1 on
/// each, and the current row stands apart from a row of the same colour at
/// rest.
#[test]
fn every_row_tint_keeps_its_words_at_four_and_a_half_to_one() {
    use quantick_appearance::workspace_menu::{RowTint, chrome, row_fill, row_ink};
    for dark in [true, false] {
        let theme = ResolvedTheme::bundled(dark);
        let menu = chrome(dark);
        for colour in presets() {
            let shade = |tint| row_fill(colour.rgb(dark), dark, tint);
            let worn = [
                vec![shade(RowTint::Rest), shade(RowTint::Hover)],
                vec![shade(RowTint::Current)],
            ];
            for fills in &worn {
                for token in [menu.ink, menu.support] {
                    let ink = row_ink(fills, dark, token).map_or_else(|| theme.color(token), rgb);
                    for fill in fills {
                        let ratio = contrast_ratio(ink, rgb(*fill));
                        assert!(
                            ratio >= 4.5,
                            "{} dark {dark} {token:?}: {ratio}",
                            colour.name()
                        );
                    }
                }
            }
            let (now, rest) = (shade(RowTint::Current), shade(RowTint::Rest));
            let apart = (0..3).map(|at| now[at].abs_diff(rest[at])).max();
            assert!(
                apart >= Some(8),
                "{} dark {dark}: current stands apart",
                colour.name()
            );
        }
    }
}

/// The trader asked to see each row's colour, as the selected row shows
/// it: the rest tint is at least 12%, hover and current stronger.
#[test]
fn every_row_wears_its_colour_visibly() {
    use quantick_appearance::workspace_menu::RowTint;
    assert!(RowTint::Rest.share() >= 0.12);
    assert!(RowTint::Hover.share() > RowTint::Rest.share());
    assert!(RowTint::Current.share() > RowTint::Hover.share());
}
