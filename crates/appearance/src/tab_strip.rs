//! The market tab strip's colour decisions, so every painter of a tab reads
//! the same answer (`docs/ux/layout-appearance-2026-11-06`).
use crate::Token;

/// The mark beside a tab's symbol. Which one a tab wears is the caller's
/// rule (`quantick_workspace`'s tab feed state); this is only how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedMark {
    Live,
    /// Connected while the venue reports a failure.
    Attention,
    Replay,
    /// A live session was lost.
    Lost,
    Stopped,
}

/// The mark's shape: the state survives without its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkShape {
    Dot,
    Diamond,
    Triangle,
    Ring,
}

/// The mark's shape: attention's filled diamond is neither live's dot nor
/// lost's hollow ring, though it shares the ring's ink.
#[must_use]
pub const fn feed_shape(mark: FeedMark) -> MarkShape {
    match mark {
        FeedMark::Live => MarkShape::Dot,
        FeedMark::Attention => MarkShape::Diamond,
        FeedMark::Replay => MarkShape::Triangle,
        FeedMark::Lost | FeedMark::Stopped => MarkShape::Ring,
    }
}

/// Amber is not here: on a tab it means unsaved edits.
#[must_use]
pub const fn feed_token(mark: FeedMark) -> Token {
    match mark {
        FeedMark::Live => Token::BuyInk,
        FeedMark::Replay => Token::ReplayInk,
        FeedMark::Attention | FeedMark::Lost => Token::Warn,
        FeedMark::Stopped => Token::TextFaint,
    }
}

/// The fill behind a tab: the active one is the toolbar's own sheet, every
/// other one a block lifted off the strip. A hovered or focused idle tab adds
/// the hover wash over its block ([`Token::BareHover`]).
#[must_use]
pub const fn tab_fill(selected: bool) -> Token {
    if selected {
        Token::ToolbarSheet
    } else {
        Token::TabIdleFill
    }
}

/// The glow behind the Home Q, by distance from its centre as a fraction of
/// its radius: a Gaussian that is 1 at the centre and reaches 0 at the rim, so
/// the edge never shows. Outside the radius it is 0.
#[must_use]
pub fn glow_falloff(distance: f32) -> f32 {
    if !(0.0..1.0).contains(&distance) {
        return 0.0;
    }
    // A sigma of a third of the radius, renormalised so the rim is exactly 0.
    let gaussian = |d: f32| (-4.5 * d * d).exp();
    let rim = gaussian(1.0);
    (gaussian(distance) - rim) / (1.0 - rim)
}

/// The Home glow's diameter in points.
pub const HOME_GLOW_DIAMETER_PT: f32 = 28.0;

/// The Home glow as triangles around its centre: each vertex's offset in
/// points and its strength (0 to 1), and the triangles' vertex indices.
#[derive(Debug, Clone, PartialEq)]
pub struct GlowMesh {
    pub vertices: Vec<([f32; 2], f32)>,
    pub indices: Vec<u32>,
}

/// A disc [`HOME_GLOW_DIAMETER_PT`] across in rings, shaded by [`glow_falloff`],
/// built once.
#[must_use]
pub fn glow_mesh() -> &'static GlowMesh {
    static MESH: std::sync::OnceLock<GlowMesh> = std::sync::OnceLock::new();
    MESH.get_or_init(build_glow_mesh)
}

fn build_glow_mesh() -> GlowMesh {
    const RINGS: u32 = 14;
    const SEGMENTS: u32 = 48;
    let radius = HOME_GLOW_DIAMETER_PT / 2.0;
    let mut vertices = vec![([0.0, 0.0], glow_falloff(0.0))];
    for ring in 1..=RINGS {
        let distance = ring as f32 / RINGS as f32;
        for segment in 0..SEGMENTS {
            let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
            let at = [angle.cos(), angle.sin()].map(|unit| unit * radius * distance);
            vertices.push((at, glow_falloff(distance)));
        }
    }
    let vertex = |ring: u32, segment: u32| 1 + (ring - 1) * SEGMENTS + segment % SEGMENTS;
    let mut indices = Vec::new();
    for segment in 0..SEGMENTS {
        indices.extend([0, vertex(1, segment), vertex(1, segment + 1)]);
        for ring in 1..RINGS {
            let (a, b) = (vertex(ring, segment), vertex(ring, segment + 1));
            let (c, d) = (vertex(ring + 1, segment), vertex(ring + 1, segment + 1));
            indices.extend([a, c, b, b, c, d]);
        }
    }
    GlowMesh { vertices, indices }
}

/// The active tab's accent band along its top edge, and a tab's top corner
/// radius, in points.
pub const TAB_BAND_PT: f32 = 2.0;
pub const TAB_CORNER_PT: f32 = 6.0;

/// One rounded-top layer of a market tab's shape, bottom to top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabLayer {
    /// How far below the tab's top edge the layer starts.
    pub inset: f32,
    /// Its top corners' radius, less the inset so nested corners follow.
    pub corner_inset: f32,
    pub fill: Token,
    /// A 1 pt edge.
    pub outline: Option<Token>,
    /// How tall the layer is, or to the tab's floor when `None`.
    pub height: Option<f32>,
}

/// A market tab's layers, in the theme's own palette: the active one an accent
/// band under the toolbar sheet, inset by [`TAB_BAND_PT`] so the band shows
/// along its top; an idle one an edged block. Light's idle edge is
/// transparent: its warm grey block needs none. A hovered or focused tab lifts
/// its ink, not its block, so its text keeps the block's contrast.
#[must_use]
pub fn tab_layers(selected: bool) -> Vec<TabLayer> {
    let layer = |inset, fill, outline| TabLayer {
        inset,
        corner_inset: inset,
        fill,
        outline,
        height: None,
    };
    if selected {
        // The band ends where the sheet's corners do: the sheet's
        // anti-aliased straight sides never sit over it.
        let band = TabLayer {
            height: Some(TAB_CORNER_PT),
            ..layer(0.0, Token::TabBand, None)
        };
        return vec![band, layer(TAB_BAND_PT, tab_fill(true), None)];
    }
    vec![layer(0.0, tab_fill(false), Some(Token::TabIdleOutline))]
}

/// A market tab's text and glyphs: its symbol and the ×.
#[must_use]
pub const fn tab_ink(lit: bool) -> Token {
    if lit {
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
    fn each_feed_state_has_its_own_shape_or_colour() {
        let all = [
            FeedMark::Live,
            FeedMark::Attention,
            FeedMark::Replay,
            FeedMark::Lost,
            FeedMark::Stopped,
        ];
        // Attention is told from live and from lost by shape alone.
        for other in [FeedMark::Live, FeedMark::Lost] {
            assert_ne!(feed_shape(FeedMark::Attention), feed_shape(other));
        }
        for (index, a) in all.iter().enumerate() {
            for b in &all[index + 1..] {
                assert!(
                    feed_shape(*a) != feed_shape(*b) || feed_token(*a) != feed_token(*b),
                    "{a:?} {b:?}"
                );
                assert_ne!(feed_token(*a), Token::Amber, "amber means unsaved");
            }
        }
    }

    #[test]
    fn the_active_tab_wears_the_toolbar_sheet_and_the_rest_a_block() {
        assert_eq!(tab_fill(true), Token::ToolbarSheet);
        assert_eq!(tab_fill(false), Token::TabIdleFill);
        assert_eq!(tab_ink(true), Token::TextPrimary);
        assert_eq!(tab_ink(false), Token::TextMuted);
    }

    /// The three layout tokens differ between the themes. An idle tab's inks
    /// (the strip's palette) stay readable on the strip and on its block, the
    /// active tab's (the theme's own) on the toolbar sheet, in both themes.
    #[test]
    fn dark_and_light_resolve_distinct_readable_strip_tokens() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for (token, want) in [
            (Token::TabStrip, [[0x10, 0x14, 0x1D], [0x25, 0x2F, 0x37]]),
            (Token::TabBand, [[0x3A, 0xF0, 0xB2], [0x12, 0xC6, 0x8C]]),
            (Token::TabIdleFill, [[0x1B, 0x20, 0x2D], [0xCF, 0xCC, 0xC7]]),
            (
                Token::ToolbarSheet,
                [[0x17, 0x1B, 0x26], [0xFF, 0xFC, 0xF7]],
            ),
        ] {
            for (theme, rgb) in [(&dark, want[0]), (&light, want[1])] {
                let palette = [theme, theme.title_strip()][usize::from(token == Token::TabStrip)];
                assert_eq!(palette.color(token).to_array()[..3], rgb, "{}", token.key());
            }
        }
        let outline = |theme: &ResolvedTheme| theme.color(Token::TabIdleOutline).to_array();
        assert_eq!(outline(&dark), [0x42, 0x4B, 0x60, 255]);
        assert_eq!(outline(&light)[3], 0, "Light's idle tab has no edge");
        // Text at 4.5:1; the feed marks are graphics, 3:1. The wordmark is a
        // logotype, which has no contrast floor (WCAG 1.4.3, 1.4.11).
        let text = [(Token::TextMuted, 4.5), (Token::ReplayInk, 4.5)];
        let marks = [
            FeedMark::Live,
            FeedMark::Attention,
            FeedMark::Lost,
            FeedMark::Stopped,
        ];
        let graphics = marks.map(|mark| (feed_token(mark), 3.0));
        for theme in [&dark, &light] {
            // Market tabs wear the theme's inks on both their faces.
            for under in [tab_fill(false), tab_fill(true)].map(|fill| theme.color(fill)) {
                for (ink, floor) in text.into_iter().chain(graphics) {
                    let ratio = contrast_ratio(theme.color(ink), under);
                    assert!(ratio >= floor, "{} {}: {ratio}", theme.name, ink.key());
                }
            }
            // The strip's own inks (window buttons, menu) on the strip.
            let strip = theme.title_strip();
            let ratio = contrast_ratio(strip.color(Token::TextMuted), strip.color(Token::TabStrip));
            assert!(ratio >= 4.5, "{} strip: {ratio}", theme.name);
        }
    }

    #[test]
    fn the_active_tab_is_a_band_under_the_sheet_and_an_idle_one_an_edged_block() {
        let active = tab_layers(true);
        assert_eq!(
            active.iter().map(|l| l.fill).collect::<Vec<_>>(),
            [Token::TabBand, Token::ToolbarSheet]
        );
        assert_eq!(active[1].inset, TAB_BAND_PT);
        assert!(active.iter().all(|layer| layer.outline.is_none()));
        assert_eq!(active[0].height, Some(TAB_CORNER_PT));
        assert_eq!(active[1].height, None);
        let idle = tab_layers(false);
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].outline, Some(Token::TabIdleOutline));
        assert_eq!(idle[0].fill, Token::TabIdleFill);
    }

    #[test]
    fn the_glow_mesh_is_a_closed_disc_brightest_at_its_centre() {
        let mesh = glow_mesh();
        assert_eq!(mesh.vertices[0], ([0.0, 0.0], 1.0));
        let radius = HOME_GLOW_DIAMETER_PT / 2.0;
        for &([x, y], strength) in &mesh.vertices {
            assert!(x.hypot(y) <= radius + 1e-3);
            assert!((0.0..=1.0).contains(&strength));
        }
        assert_eq!(mesh.indices.len() % 3, 0);
        let count = mesh.vertices.len() as u32;
        assert!(mesh.indices.iter().all(|&index| index < count));
    }

    #[test]
    fn the_home_glow_peaks_at_its_centre_and_fades_out_at_its_rim() {
        assert_eq!(glow_falloff(0.0), 1.0);
        assert!(glow_falloff(1.0).abs() < f32::EPSILON);
        assert_eq!(glow_falloff(1.5), 0.0);
        assert_eq!(glow_falloff(-0.1), 0.0);
        let samples: Vec<f32> = (0..10)
            .map(|step| glow_falloff(step as f32 / 10.0))
            .collect();
        assert!(
            samples.windows(2).all(|pair| pair[0] > pair[1]),
            "{samples:?}"
        );
    }
}
