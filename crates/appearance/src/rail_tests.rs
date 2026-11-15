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

/// The mock's `RAIL` table, keys ("dark", 1) and ("light", 1): band, edge
/// and grip, then the same three under the pointer.
#[test]
fn both_bundled_themes_paint_the_rail_as_the_mock_draws_it() {
    let dark = [0x283042, 0x3B4560, 0xAAB4C3, 0x343D54, 0x56627F, 0xEEF2F6];
    let light = [0xDADADA, 0x7C7C7C, 0x3A3A3A, 0xC9CDD3, 0x5F6670, 0x000000];
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

/// Back to front, as the workspace lays the marks out: the band, its two
/// edges and the grip, each in its hover token while the pointer is on the
/// rail's hit area.
#[test]
fn the_rail_inks_are_band_edge_edge_grip_at_rest_and_under_the_pointer() {
    assert_eq!(
        rail_inks(false),
        [
            Token::RailBand,
            Token::RailEdge,
            Token::RailEdge,
            Token::RailGrip
        ]
    );
    assert_eq!(
        rail_inks(true),
        [
            Token::RailBandHover,
            Token::RailEdgeHover,
            Token::RailEdgeHover,
            Token::RailGripHover
        ]
    );
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
