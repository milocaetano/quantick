//! Each workspace colour where it is painted, against the theme it is painted
//! on: the capsule's name on its tint, the open tab's symbol on its sheet.
use super::*;
use quantick_appearance::contrast::contrast_ratio;
use quantick_appearance::{LIGHT, PremultipliedRgba, ResolvedTheme, Token};

fn themes() -> [ResolvedTheme; 2] {
    [ResolvedTheme::default(), ResolvedTheme::parse(LIGHT).unwrap()]
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
        assert_eq!(capsule_fill(WorkspaceColour::Custom([255; 3]), &theme)[3], 255);
        let ink = theme.color(Token::TextPrimary);
        for colour in presets() {
            let fill = PremultipliedRgba::from_premultiplied(capsule_fill(colour, &theme));
            let ratio = contrast_ratio(ink, fill);
            assert!(ratio >= 4.5, "{} {}: {ratio}", theme.name, colour.name());
        }
    }
}

/// The open tab's symbol wears the colour at 3:1 or more on the tab's sheet;
/// below that it keeps the tab's normal ink. Of the presets only Light Ice
/// and Light Blossom fall back.
#[test]
fn the_open_tabs_symbol_wears_the_colour_only_where_it_reads() {
    let mut fallbacks = Vec::new();
    for theme in themes() {
        let sheet = theme.color(Token::ToolbarSheet);
        for colour in presets() {
            let painted = colour.rgb(theme.dark);
            match symbol_ink(colour, &theme) {
                Some(ink) => {
                    assert_eq!(ink, painted);
                    let ratio = contrast_ratio(rgb(ink), sheet);
                    assert!(ratio >= 3.0, "{} {}: {ratio}", theme.name, colour.name());
                }
                None => {
                    assert!(contrast_ratio(rgb(painted), sheet) < 3.0);
                    fallbacks.push(format!("{} {}", theme.name, colour.name()));
                }
            }
        }
    }
    assert_eq!(fallbacks, ["Light Ice", "Light Blossom"]);
    // A custom colour is held to the same floor.
    let pale = WorkspaceColour::Custom([0xEE, 0xEE, 0xEE]);
    let [dark, light] = themes();
    assert_eq!(symbol_ink(pale, &dark), Some([0xEE; 3]));
    assert_eq!(symbol_ink(pale, &light), None);
}
