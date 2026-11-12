//! The market tab strip's colour decisions, so every painter of a tab reads
//! the same answer (`docs/ux/layout-appearance-2026-11-06`).
use crate::{PremultipliedRgba, Token, contrast::contrast_ratio};

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

/// The Home glow as the approved mock draws it: its diameter in points (a
/// ~28 pt light, blurred), its centre's offset from the Q's centre (behind
/// the Q's tail and the "u"), and its strength while the pointer is over Home.
pub const HOME_GLOW_DIAMETER_PT: f32 = 60.0;
pub const HOME_GLOW_OFFSET_PT: [f32; 2] = [11.0, 1.0];
pub const HOME_GLOW_HOVER: f32 = 0.5;

/// How strong the Home glow is: full while Home is shown, half while the
/// pointer is over it, none otherwise.
#[must_use]
pub fn home_glow_strength(shown: bool, hovered: bool) -> Option<f32> {
    if shown {
        Some(1.0)
    } else {
        hovered.then_some(HOME_GLOW_HOVER)
    }
}

/// The Home glow as triangles: each vertex's offset from the Q's centre in
/// points (the glow's own centre is [`HOME_GLOW_OFFSET_PT`] away) and its
/// strength (0 to 1), and the triangles' vertex indices.
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
    let mut vertices = vec![(HOME_GLOW_OFFSET_PT, glow_falloff(0.0))];
    for ring in 1..=RINGS {
        let distance = ring as f32 / RINGS as f32;
        for segment in 0..SEGMENTS {
            let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
            let [x, y] = [angle.cos(), angle.sin()].map(|unit| unit * radius * distance);
            let at = [HOME_GLOW_OFFSET_PT[0] + x, HOME_GLOW_OFFSET_PT[1] + y];
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

/// The workspace colours in `quantick_workspace`'s palette order: slot `i`
/// paints in `WORKSPACE_TOKENS[i]`.
pub const WORKSPACE_TOKENS: [Token; 8] = [
    Token::WorkspaceSteel,
    Token::WorkspaceViolet,
    Token::WorkspaceCyan,
    Token::WorkspaceMagenta,
    Token::WorkspaceAzure,
    Token::WorkspaceIndigo,
    Token::WorkspaceChalk,
    Token::WorkspaceCobalt,
];

/// The least a bar or ring must contrast with what it sits on, in the
/// graphics floor of WCAG 1.4.11.
pub const WORKSPACE_EDGE_CONTRAST: f32 = 3.0;

/// Below this contrast with the strip a bar reads as no bar at all, as
/// Light's Chalk (about 1.04:1) does. The picked palette keeps every other
/// bar unoutlined (Light's next lowest, Indigo, is about 1.76:1).
pub const WORKSPACE_BAR_LOST_CONTRAST: f32 = 1.5;

/// Whether a workspace's bar needs a 1 px outline along its strip-facing
/// edges: it falls below [`WORKSPACE_BAR_LOST_CONTRAST`] against the strip,
/// as Light's Chalk does. The outline is the strip's `TextPrimary`.
#[must_use]
pub fn bar_needs_outline(bar: [u8; 4], strip: [u8; 4]) -> bool {
    let [bar, strip] = [bar, strip].map(PremultipliedRgba::from_premultiplied);
    contrast_ratio(bar, strip) < WORKSPACE_BAR_LOST_CONTRAST
}

/// The ink of the current swatch's ring: `TextPrimary` unless that falls short
/// of [`WORKSPACE_EDGE_CONTRAST`] on the swatch, then the theme's tab-strip
/// ground, the other primary ink.
#[must_use]
pub fn swatch_ring_token(swatch: [u8; 4], text_primary: [u8; 4]) -> Token {
    let [swatch, ink] = [swatch, text_primary].map(PremultipliedRgba::from_premultiplied);
    if contrast_ratio(swatch, ink) < WORKSPACE_EDGE_CONTRAST {
        Token::TabStrip
    } else {
        Token::TextPrimary
    }
}

/// The bar's strip-facing edges as a polyline, in points: up the left side of
/// a tab whose box is `left`, `top` to `bottom`, then round its top-left curve
/// of `radius`, inset half a point so a 1 px stroke stays inside the tab.
#[must_use]
pub fn bar_edge_points(left: f32, top: f32, bottom: f32, right: f32, radius: f32) -> Vec<[f32; 2]> {
    let (left, top) = (left + 0.5, top + 0.5);
    let arc = (0..=6).map(|step| {
        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
        [
            left + radius - radius * angle.cos(),
            top + radius - radius * angle.sin(),
        ]
    });
    // A bar wider than the corner keeps a straight top edge on the strip.
    let rest = (right > left + radius).then_some([right, top]);
    std::iter::once([left, bottom])
        .chain(arc)
        .chain(rest)
        .collect()
}

/// The polyline of a bar's outline when it needs one against the strip (see
/// [`bar_needs_outline`]): `[left, top, bottom, right]` is the bar's box, its
/// corner the tab's [`TAB_CORNER_PT`].
#[must_use]
pub fn bar_outline(
    bar: [u8; 4],
    strip: [u8; 4],
    [left, top, bottom, right]: [f32; 4],
) -> Option<Vec<[f32; 2]>> {
    bar_needs_outline(bar, strip).then(|| bar_edge_points(left, top, bottom, right, TAB_CORNER_PT))
}

/// The line in the workspace's colour under the toolbar capsule, and the bar
/// at a workspace menu row's left edge with the indent its name takes past
/// it, in points.
pub const WORKSPACE_UNDERLINE_PT: f32 = 3.0;
/// The capsule's bottom band the underline fills: its 1 pt border and the line.
pub const WORKSPACE_UNDERLINE_BAND_PT: f32 = 1.0 + WORKSPACE_UNDERLINE_PT;

/// How far the capsule's content rises so it centres in the room above its
/// underline: half the line when the workspace has a colour, else none.
#[must_use]
pub fn underline_lift(coloured: bool) -> f32 {
    if coloured {
        WORKSPACE_UNDERLINE_PT / 2.0
    } else {
        0.0
    }
}
pub const WORKSPACE_ROW_BAR_PT: f32 = 7.0;
pub const WORKSPACE_ROW_INDENT_PT: f32 = 8.0;
/// The workspace menu's colour swatches: 22 pt squares 8 pt apart, one held
/// by another workspace dimmed to this alpha under its owner's initials.
pub const WORKSPACE_SWATCH_PT: f32 = 22.0;
pub const WORKSPACE_SWATCH_GAP_PT: f32 = 8.0;
pub const WORKSPACE_SWATCH_USED_ALPHA: f32 = 0.38;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LIGHT, ResolvedTheme};

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
                [[0x17, 0x1B, 0x26], [0xED, 0xED, 0xED]],
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

    /// Each workspace colour is a graphic (3:1) on the idle tab's block and on
    /// the active tab's sheet, in both themes, and the eight are distinct.
    #[test]
    fn every_workspace_colour_reads_on_both_tab_faces_in_both_themes() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for theme in [&dark, &light] {
            let mut seen = Vec::new();
            for token in WORKSPACE_TOKENS {
                let colour = theme.color(token);
                assert!(!seen.contains(&colour), "{} {}", theme.name, token.key());
                seen.push(colour);
                for face in [tab_fill(false), tab_fill(true)] {
                    let ratio = contrast_ratio(colour, theme.color(face));
                    let at = (theme.name.as_str(), token.key(), face.key());
                    assert!(ratio >= 3.0, "{at:?}: {ratio}");
                }
            }
        }
    }

    /// No green, red, gold or orange: those already mean buy, sell, the POC
    /// and unsaved edits. A tinted colour's hue stays between cyan and
    /// magenta; a near-grey one (steel, chalk) has no hue to speak of.
    #[test]
    fn no_workspace_colour_is_green_red_gold_or_orange() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for theme in [&dark, &light] {
            for token in WORKSPACE_TOKENS {
                let [r, g, b, _] = theme.color(token).to_array().map(f32::from);
                let (max, min) = (r.max(g).max(b), r.min(g).min(b));
                if (max - min) / max.max(1.0) < 0.2 {
                    continue;
                }
                let hue = if max == r {
                    60.0 * ((g - b) / (max - min))
                } else if max == g {
                    60.0 * ((b - r) / (max - min) + 2.0)
                } else {
                    60.0 * ((r - g) / (max - min) + 4.0)
                };
                let hue = hue.rem_euclid(360.0);
                assert!((180.0..=330.0).contains(&hue), "{}: {hue}", token.key());
            }
        }
    }

    /// Only a bar lost on the strip is outlined, in an ink that reads there:
    /// of both themes' palettes that is Light's Chalk (1.04:1) alone.
    #[test]
    fn a_bar_lost_on_the_strip_is_outlined_in_an_ink_that_reads_there() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for theme in [&dark, &light] {
            let strip = theme.title_strip();
            let ground = strip.color(Token::TabStrip);
            for token in WORKSPACE_TOKENS {
                let bar = theme.color(token);
                let lost = bar_needs_outline(bar.to_array(), ground.to_array());
                let ratio = contrast_ratio(strip.color(Token::TextPrimary), ground);
                assert!(
                    !lost || ratio >= WORKSPACE_EDGE_CONTRAST,
                    "{} {}: {ratio}",
                    theme.name,
                    token.key()
                );
            }
        }
        let chalk = light.color(Token::WorkspaceChalk);
        assert!(bar_needs_outline(
            chalk.to_array(),
            light.title_strip().color(Token::TabStrip).to_array()
        ));
        let mut outlined = Vec::new();
        for theme in [&dark, &light] {
            let ground = theme.title_strip().color(Token::TabStrip).to_array();
            for token in WORKSPACE_TOKENS {
                if bar_needs_outline(theme.color(token).to_array(), ground) {
                    outlined.push((theme.name.clone(), token.key()));
                }
            }
        }
        assert_eq!(outlined.len(), 1, "{outlined:?}");
        assert_eq!(outlined[0].1, Token::WorkspaceChalk.key());
    }

    /// The outline climbs the bar's left edge, rounds its corner and runs on
    /// along the top to the bar's right edge when the bar is wider than the
    /// corner.
    #[test]
    fn a_bar_outline_follows_the_corner_and_the_whole_top_edge() {
        let points = bar_edge_points(10.0, 0.0, 30.0, 19.0, 6.0);
        assert_eq!(points.first(), Some(&[10.5, 30.0]));
        assert_eq!(points[1], [10.5, 6.5]);
        assert!(points[1..8].iter().all(|[x, y]| *x <= 16.5 && *y <= 6.5));
        assert_eq!(points.last(), Some(&[19.0, 0.5]));
        let narrow = bar_edge_points(10.0, 0.0, 30.0, 14.0, 6.0);
        assert_eq!(narrow.len(), 8, "a bar inside the corner ends on the arc");
    }

    /// The current swatch's ring reads on every colour in both themes.
    #[test]
    fn the_current_swatch_ring_reads_on_every_colour_in_both_themes() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for theme in [&dark, &light] {
            let ink = theme.color(Token::TextPrimary);
            for token in WORKSPACE_TOKENS {
                let swatch = theme.color(token);
                let ring = theme.color(swatch_ring_token(swatch.to_array(), ink.to_array()));
                let ratio = contrast_ratio(ring, swatch);
                assert!(
                    ratio >= WORKSPACE_EDGE_CONTRAST,
                    "{} {}: {ratio}",
                    theme.name,
                    token.key()
                );
            }
        }
        let chalk = dark.color(Token::WorkspaceChalk);
        assert_eq!(
            swatch_ring_token(chalk.to_array(), dark.color(Token::TextPrimary).to_array()),
            Token::TabStrip
        );
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
    fn home_glows_in_full_when_shown_and_by_half_when_hovered() {
        assert_eq!(home_glow_strength(true, false), Some(1.0));
        assert_eq!(home_glow_strength(true, true), Some(1.0));
        assert_eq!(home_glow_strength(false, true), Some(HOME_GLOW_HOVER));
        assert_eq!(home_glow_strength(false, false), None);
    }

    #[test]
    fn the_glow_mesh_is_a_closed_disc_brightest_at_its_centre() {
        let mesh = glow_mesh();
        assert_eq!(mesh.vertices[0], (HOME_GLOW_OFFSET_PT, 1.0));
        let radius = HOME_GLOW_DIAMETER_PT / 2.0;
        let [cx, cy] = HOME_GLOW_OFFSET_PT;
        for &([x, y], strength) in &mesh.vertices {
            assert!((x - cx).hypot(y - cy) <= radius + 1e-3);
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
