//! The market tab strip's colour decisions, so every painter of a tab reads
//! the same answer (`docs/ux/layout-appearance-2026-11-06`).
use std::cmp::Ordering;

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

/// The disc an asset the config names no icon for wears.
pub const ICON_NEUTRAL: Token = Token::TextFaint;
/// The darker of an icon's two glyph inks, for a light disc.
pub const ICON_GLYPH_DARK: [u8; 3] = [0x1E, 0x23, 0x29];
/// A disc this light (Rec. 709 luma, 0 to 255) or lighter takes the dark glyph.
pub const ICON_LIGHT_LUMA: f32 = 160.0;

/// The day's change: on the open tab the theme's tab gain ink up and loss
/// ink down, flat in the tab's ink; on a closed tab always the tab's ink.
#[must_use]
pub const fn change_token(sign: Ordering, open: bool, lit: bool) -> Token {
    match (open, sign) {
        (true, Ordering::Greater) => Token::TabChangeUp,
        (true, Ordering::Less) => Token::TabChangeDown,
        _ => tab_ink(lit),
    }
}

/// An icon's disc and glyph: the asset's colour on the open tab, grey of the
/// same lightness on the others; `None` is the [`ICON_NEUTRAL`] disc, under
/// a white glyph.
#[must_use]
pub fn icon_inks(rgb: Option<[u8; 3]>, open: bool) -> (Option<[u8; 3]>, [u8; 3]) {
    let disc = rgb.map(|rgb| if open { rgb } else { icon_grey(rgb) });
    (disc, disc.map_or([0xFF; 3], icon_glyph_ink))
}

/// A closed tab's icon disc: `rgb` in grey of the same luma.
#[must_use]
pub fn icon_grey(rgb: [u8; 3]) -> [u8; 3] {
    let grey = luma(rgb).round().clamp(0.0, 255.0) as u8;
    [grey; 3]
}

/// The glyph on a disc of `rgb`: white, or [`ICON_GLYPH_DARK`] on a light disc.
#[must_use]
pub fn icon_glyph_ink(rgb: [u8; 3]) -> [u8; 3] {
    if luma(rgb) >= ICON_LIGHT_LUMA {
        ICON_GLYPH_DARK
    } else {
        [0xFF; 3]
    }
}

/// Rec. 709 luma of straight sRGB, 0 to 255.
fn luma([r, g, b]: [u8; 3]) -> f32 {
    0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
}

/// How much of the workspace's colour tints the toolbar workspace buttons'
/// fill; their borders take the colour whole (the trader's pick of 2026-11-12, option 7).
pub const WORKSPACE_TINT: f32 = 0.12;
/// The tint a toolbar workspace button takes under the pointer or while its
/// popover is open (design A-3 of 2026-11-23).
pub const WORKSPACE_HOVER_TINT: f32 = 0.28;
/// The bar in the workspace's colour at the workspace button's left edge, cut
/// by its corner (the trader's ask of 2026-11-13).
pub const WORKSPACE_CAPSULE_BAR_PT: f32 = 4.0;
/// The least the open tab's symbol, inked in its workspace's colour, keeps
/// against the tab's sheet: the graphics floor of WCAG 1.4.11 for a heavy
/// 13 pt symbol. Below it the symbol keeps the tab's normal ink.
pub const WORKSPACE_SYMBOL_CONTRAST: f32 = 3.0;

/// The legs of the filled triangle every market tab paints in its workspace's
/// colour at its top-left corner (the trader's pick of 2026-11-23, "Canto
/// cheio"); the tab's face starts past it.
pub const WORKSPACE_CORNER_PT: f32 = 17.0;
/// How many straight pieces stand for the corner's quarter circle.
const CORNER_ARC_STEPS: u16 = 6;

/// The corner mark's outline from the sheet's top-left, clockwise: down the
/// left leg's end, round the sheet's corner of `radius`, out along the top to
/// the other leg's end; the hypotenuse closes it. Convex, so one fill.
#[must_use]
pub fn corner_mark(radius: f32) -> Vec<[f32; 2]> {
    let radius = radius.clamp(0.0, WORKSPACE_CORNER_PT);
    let arc = (0..=CORNER_ARC_STEPS).map(|step| {
        let turn = std::f32::consts::FRAC_PI_2 * f32::from(step) / f32::from(CORNER_ARC_STEPS);
        [radius * (1.0 - turn.cos()), radius * (1.0 - turn.sin())]
    });
    std::iter::once([0.0, WORKSPACE_CORNER_PT])
        .chain(arc)
        .chain(std::iter::once([WORKSPACE_CORNER_PT, 0.0]))
        .collect()
}

/// A workspace button's fill, `base`, tinted `share` of the way toward the
/// workspace's `colour`; both straight sRGB, the base's alpha kept.
#[must_use]
pub fn workspace_tint(base: [u8; 4], colour: [u8; 3], share: f32) -> [u8; 4] {
    let mix = |b: u8, c: u8| {
        let (b, c) = (f32::from(b), f32::from(c));
        (b + (c - b) * share).round().clamp(0.0, 255.0) as u8
    };
    [
        mix(base[0], colour[0]),
        mix(base[1], colour[1]),
        mix(base[2], colour[2]),
        base[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LIGHT, ResolvedTheme, contrast::contrast_ratio};

    /// The mark spans both legs, never leaves the triangle, and never crosses
    /// the sheet's rounded corner.
    #[test]
    fn the_corner_mark_fills_the_triangle_inside_the_rounded_corner() {
        let radius = TAB_CORNER_PT;
        let points = corner_mark(radius);
        assert_eq!(points.first(), Some(&[0.0, WORKSPACE_CORNER_PT]));
        assert_eq!(points.last(), Some(&[WORKSPACE_CORNER_PT, 0.0]));
        for &[x, y] in &points {
            assert!(x >= 0.0 && y >= 0.0 && x + y <= WORKSPACE_CORNER_PT + 1e-4);
            if x < radius && y < radius {
                let off = ((x - radius).powi(2) + (y - radius).powi(2)).sqrt();
                assert!((off - radius).abs() < 1e-4, "{x},{y} leaves the corner");
            }
        }
        assert_eq!(corner_mark(0.0)[1], [0.0, 0.0]);
    }

    /// The tint moves the fill a fixed share toward the colour, alpha kept.
    #[test]
    fn the_capsule_tint_moves_a_fixed_share_toward_the_colour() {
        assert_eq!(workspace_tint([255; 4], [255; 3], WORKSPACE_TINT), [255; 4]);
        assert_eq!(
            workspace_tint([0, 0, 0, 200], [100, 200, 0], WORKSPACE_TINT),
            [12, 24, 0, 200]
        );
        assert_eq!(
            workspace_tint([0, 0, 0, 200], [100, 200, 0], WORKSPACE_HOVER_TINT),
            [28, 56, 0, 200]
        );
    }

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

    #[test]
    fn the_change_is_green_or_red_on_the_open_tab_only() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        assert_eq!(change_token(Greater, true, true), Token::TabChangeUp);
        assert_eq!(change_token(Less, true, true), Token::TabChangeDown);
        assert_eq!(change_token(Equal, true, true), tab_ink(true));
        for sign in [Greater, Less, Equal] {
            assert_eq!(change_token(sign, false, false), tab_ink(false));
            assert_eq!(change_token(sign, false, true), tab_ink(true));
        }
    }

    /// The open tab's change wears the mock's green and red in both themes,
    /// not the ladder's inks (Light's sell ink is near-black); a stored copy
    /// frozen before the keys existed takes its own mode's pair.
    #[test]
    fn the_open_tabs_change_wears_the_mocks_gain_and_loss_inks() {
        let mocks = [
            ([0x3A, 0xF0, 0xB2], [0xFF, 0x6B, 0x78]),
            ([0x0B, 0x8A, 0x5C], [0xC6, 0x2F, 0x3A]),
        ];
        let frozen = |source: &str| {
            let lines = source
                .lines()
                .filter(|line| !line.starts_with("tab_change_"));
            ResolvedTheme::parse(&lines.collect::<Vec<_>>().join("\n")).unwrap()
        };
        for (source, (up, down)) in [crate::DARK, LIGHT].into_iter().zip(mocks) {
            for theme in [ResolvedTheme::parse(source).unwrap(), frozen(source)] {
                let ink = |token| theme.title_strip().color(token).to_array()[..3].to_vec();
                assert_eq!(ink(Token::TabChangeUp), up, "{}", theme.name);
                assert_eq!(ink(Token::TabChangeDown), down, "{}", theme.name);
            }
        }
    }

    #[test]
    fn a_closed_icon_is_grey_of_its_own_lightness_and_its_glyph_reads() {
        assert_eq!(icon_grey([0xF7, 0x93, 0x1A]), [160, 160, 160]);
        assert_eq!(icon_grey([0, 0, 0]), [0, 0, 0]);
        assert_eq!(icon_grey([255, 255, 255]), [255, 255, 255]);
        assert_eq!(icon_glyph_ink([0x3B, 0x42, 0x50]), [255, 255, 255]);
        assert_eq!(icon_glyph_ink([0xF7, 0x93, 0x1A]), [255, 255, 255]);
        assert_eq!(icon_glyph_ink([200, 200, 200]), ICON_GLYPH_DARK);
        let orange = Some([0xF7, 0x93, 0x1A]);
        assert_eq!(icon_inks(orange, true), (orange, [0xFF; 3]));
        assert_eq!(icon_inks(orange, false).0, Some([160, 160, 160]));
        assert_eq!(icon_inks(None, true), (None, [0xFF; 3]));
    }
}
