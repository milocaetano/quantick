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
/// colour over the list's ground: at rest, hovered and current. The name
/// and the summary, in the ink [`RowWords::ink`] chooses for the fills the
/// row wears, read 4.5:1 on each, and the current row stands apart from a
/// row of the same colour at rest.
#[test]
fn every_row_tint_keeps_its_words_at_four_and_a_half_to_one() {
    use quantick_appearance::workspace_menu::{RowTint, RowWords, chrome, mix};
    for dark in [true, false] {
        let theme = ResolvedTheme::bundled(dark);
        let menu = chrome(dark);
        let solid = |token| {
            let [r, g, b, _] = theme.color(token).to_array();
            [r, g, b]
        };
        let ground = solid(menu.ground);
        for colour in presets() {
            for current in [false, true] {
                let row = RowWords {
                    ground: menu.ground,
                    colour: colour.rgb(dark),
                    current,
                };
                for token in [menu.ink, menu.support] {
                    let ink = row.ink(ground, solid(token), solid(Token::TextPrimary));
                    for fill in row.fills(ground) {
                        let ratio = contrast_ratio(rgb(ink), rgb(fill));
                        assert!(
                            ratio >= 4.5,
                            "{} dark {dark} {token:?}: {ratio}",
                            colour.name()
                        );
                    }
                }
            }
            let shade = |tint: RowTint| mix(ground, colour.rgb(dark), tint.share());
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

/// A theme of the trader's own paints its own ground: the row's fill mixes
/// from that ground, and its words are chosen against it.
#[test]
fn a_row_follows_the_ground_the_theme_paints() {
    use quantick_appearance::workspace_menu::{RowTint, RowWords, chrome, mix};
    let menu = chrome(true);
    let sky = WorkspaceColour::preset(2).unwrap().rgb(true);
    let row = RowWords {
        ground: menu.ground,
        colour: sky,
        current: false,
    };
    let (bundled, own) = ([0x17, 0x1B, 0x26], [0x3A, 0x40, 0x4C]);
    assert_eq!(row.fills(own)[0], mix(own, sky, RowTint::Rest.share()));
    assert_ne!(
        row.fills(own),
        row.fills(bundled),
        "the mix follows the ground"
    );
    let support = [0x86, 0x92, 0xA4];
    let ink = row.ink(own, support, [0xD2, 0xDA, 0xE2]);
    for fill in row.fills(own) {
        assert!(
            contrast_ratio(rgb(ink), rgb(fill)) >= 4.5,
            "chosen on the theme's own ground"
        );
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
