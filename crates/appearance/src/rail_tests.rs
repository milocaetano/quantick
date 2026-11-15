//! The folded pane's rail in both bundled themes: the lit band of the
//! designers' mock (option 1), its inks at rest and under the pointer, and a
//! stored theme written before the rail tokens existed.
use crate::rail::rail_inks;
use crate::{DARK, LIGHT, PremultipliedRgba, ResolvedTheme, ThemeDocument, Token};

const TOKENS: [Token; 6] = [
    Token::RailBand,
    Token::RailEdge,
    Token::RailGrip,
    Token::RailBandHover,
    Token::RailEdgeHover,
    Token::RailGripHover,
];

fn rgb(hex: u32) -> PremultipliedRgba {
    let [_, r, g, b] = hex.to_be_bytes();
    PremultipliedRgba::from_rgb(r, g, b)
}

/// The trader's pick ("Option 2 · Band"): band, edge and marks, then the
/// same three under the pointer, a clear step lighter in Dark and darker in
/// Light.
#[test]
fn both_bundled_themes_paint_the_rail_as_the_mock_draws_it() {
    let dark = [0x38425C, 0x4A5573, 0xC8D0DC, 0x4A5A80, 0x63739C, 0xFFFFFF];
    let light = [0xD0D0D0, 0x7C7C7C, 0x3A4049, 0xB0B5BC, 0x5F6670, 0x000000];
    for (source, hexes) in [(DARK, dark), (LIGHT, light)] {
        let theme = ResolvedTheme::parse(source).unwrap();
        for (token, hex) in TOKENS.into_iter().zip(hexes) {
            assert_eq!(
                theme.color(token),
                rgb(hex),
                "{}: {}",
                theme.name,
                token.key()
            );
        }
    }
}

/// In the workspace's mark order, band, edge and grip, each in its hover
/// token while the pointer is on the rail's hit area.
#[test]
fn the_rail_inks_are_band_edge_grip_at_rest_and_under_the_pointer() {
    assert_eq!(rail_inks(false), TOKENS[..3]);
    assert_eq!(rail_inks(true), TOKENS[3..]);
}

/// A stored theme is a frozen copy of the bundled file it was picked from:
/// one saved before the rail tokens lacks every one, and still paints the
/// rail in its own mode's bundled colours, never Dark's on a Light copy.
#[test]
fn a_frozen_copy_from_before_the_rail_takes_its_own_modes_rail() {
    for source in [DARK, LIGHT] {
        let frozen: String = source
            .lines()
            .filter(|line| !line.starts_with("rail_"))
            .map(|line| format!("{line}\n"))
            .collect();
        let document = ThemeDocument::parse(&frozen).unwrap();
        for token in TOKENS {
            assert!(
                !document.colors.contains_key(token.key()),
                "{}",
                token.key()
            );
        }
        let bundled = ResolvedTheme::parse(source).unwrap();
        let theme = document.resolve().unwrap();
        for token in TOKENS {
            let key = token.key();
            assert_eq!(
                theme.color(token),
                bundled.color(token),
                "{}: {key}",
                bundled.name
            );
        }
    }
}
