//! The layout picker's tokens and colour clamp, graded on the bundled themes.
use super::*;

fn themes() -> [&'static ResolvedTheme; 2] {
    [ResolvedTheme::bundled(true), ResolvedTheme::bundled(false)]
}

/// Every word the picker paints reaches 4.5:1 where it stands: the captions,
/// hint and status on the ground and the header block, a label (in the ink)
/// on a hover or pressed plate too.
#[test]
fn every_word_reads_on_its_ground_in_both_themes() {
    for theme in themes() {
        let look = chrome(theme.dark);
        let grounds = [
            ("ground", theme.color(look.ground)),
            ("block", theme.color(look.block)),
            ("hover", theme.color(look.hover)),
            ("pressed", pressed(theme)),
        ];
        // Support words stand on the ground and the block only; a label or
        // the lit "was" words on a plate are in the ink.
        let support = &grounds[..2];
        for (ink_name, ink, under) in [
            ("ink", look.ink, &grounds[..]),
            ("support", look.support, support),
        ] {
            for &(ground_name, ground) in under {
                let ratio = contrast_ratio(theme.color(ink), ground);
                assert!(
                    ratio >= TEXT_CONTRAST + CAPTION_MARGIN,
                    "{} {ink_name} on {ground_name}: {ratio}",
                    theme.name
                );
            }
        }
    }
}

/// Hover, pressed and the focus ring differ from the ground and each other.
#[test]
fn hover_and_pressed_plates_differ_in_both_themes() {
    for theme in themes() {
        let look = chrome(theme.dark);
        let (ground, hover) = (theme.color(look.ground), theme.color(look.hover));
        assert_ne!(hover, ground, "{}", theme.name);
        assert_ne!(pressed(theme), hover, "{}", theme.name);
        assert_ne!(pressed(theme), ground, "{}", theme.name);
        let ring = contrast_ratio(theme.color(look.focus), ground);
        assert!(ring >= RING_CONTRAST, "{} focus ring: {ring}", theme.name);
    }
}

/// A colour that vanishes on the ground is shaded until its ring holds 3:1,
/// and a tint that cannot stand off the ground gives way to the wide edge.
#[test]
fn a_colour_equal_to_the_ground_is_clamped_and_loses_its_fill() {
    for theme in themes() {
        let ground = theme.color(chrome(theme.dark).ground);
        let [r, g, b, _] = ground.to_array();
        let inks = colour_inks([r, g, b], ground, theme.dark);
        let ratio = contrast_ratio(inks.ring, ground);
        assert!(ratio >= RING_CONTRAST, "{} ring: {ratio}", theme.name);
        assert_eq!(inks.fill, None, "{}", theme.name);
        assert_eq!(inks.flow_edge, FLOW_EDGE_BARE_PT, "{}", theme.name);
    }
}

/// A strong colour keeps its own value as the ring and gets a fill that
/// stands off the ground, the ✓ reading on the ring.
#[test]
fn a_strong_colour_keeps_its_value_and_fills_the_flow_pane() {
    let theme = ResolvedTheme::bundled(false);
    let ground = theme.color(chrome(false).ground);
    let inks = colour_inks([0x6A, 0x3F, 0xC4], ground, false);
    assert_eq!(inks.ring, PremultipliedRgba::from_rgb(0x6A, 0x3F, 0xC4));
    let fill = inks.fill.expect("violet tints the flow pane");
    assert!(contrast_ratio(fill, ground) >= FILL_CONTRAST);
    assert_eq!(inks.flow_edge, FLOW_EDGE_PT);
    assert!(contrast_ratio(inks.check, inks.ring) >= RING_CONTRAST);
    assert!(inks.time_edge.a() >= TIME_EDGE_ALPHA);
}

/// A timeframe pane's edge holds 3:1 on an empty chart cell once
/// composited: the colour at 60%, more opaque where 60% falls short.
#[test]
fn timeframe_edges_hold_three_to_one_on_the_cell() {
    for theme in themes() {
        let ground = theme.color(chrome(theme.dark).ground);
        for rgb in [[0x6A, 0x3F, 0xC4], [0x1F, 0x86, 0xBF], [0xA8, 0x87, 0xF2]] {
            let inks = colour_inks(rgb, ground, theme.dark);
            let seen = over(inks.time_edge, ground);
            let ratio = contrast_ratio(seen, ground);
            assert!(ratio >= RING_CONTRAST, "{} {rgb:?}: {ratio}", theme.name);
        }
    }
}

/// Ice in Dark: the flow fill stands off an empty cell at the target, not
/// only at the floor.
#[test]
fn a_pale_colour_fills_the_flow_pane_at_the_target() {
    let theme = ResolvedTheme::bundled(true);
    let ground = theme.color(chrome(true).ground);
    let inks = colour_inks([0xB3, 0xDB, 0xF5], ground, true);
    let fill = inks.fill.expect("ice tints the flow pane");
    let ratio = contrast_ratio(fill, ground);
    assert!(ratio >= FILL_TARGET, "{ratio}");
}

/// The focus ring is the theme's accent, never a workspace colour such as
/// Light Ink, which is near the ink itself.
#[test]
fn the_focus_ring_is_the_accent_and_stands_apart_from_ink() {
    for theme in themes() {
        assert_eq!(chrome(theme.dark).focus, Token::Accent);
    }
    let light = ResolvedTheme::bundled(false);
    let ground = light.color(chrome(false).ground);
    let ink = colour_inks([0x25, 0x2B, 0x36], ground, false).ring;
    assert_ne!(light.color(Token::Accent), ink);
}
