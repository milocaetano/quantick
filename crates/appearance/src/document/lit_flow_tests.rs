use super::ChartSurface;
use crate::contrast::{contrast_ratio, over, relative_luminance};
use crate::{Appearance, DARK, LIGHT, PremultipliedRgba, ResolvedTheme, ThemeDocument, Token};

const VWAP: (&str, &str) = ("VWAP (cumulative)", "vwap");
/// Text at the body sizes the legend and the lane labels use.
const TEXT: f32 = 4.5;
/// A mark that is not text: a rim, a lane line, a glyph.
const MARK: f32 = 3.0;

fn rgba(bytes: [u8; 4]) -> PremultipliedRgba {
    PremultipliedRgba::from_premultiplied(bytes)
}

fn light() -> ResolvedTheme {
    ResolvedTheme::parse(LIGHT).unwrap()
}

#[test]
fn only_light_offers_the_lights_and_a_surface_falls_back_to_the_island_without_them() {
    let (dark, light) = (ResolvedTheme::parse(DARK).unwrap(), light());
    assert!(light.has_lit_flow());
    assert!(!dark.has_lit_flow());
    for surface in [ChartSurface::Flow, ChartSurface::LitFlow] {
        assert!(std::ptr::eq(dark.surface(surface), dark.bookmap()));
    }
    assert!(std::ptr::eq(light.surface(ChartSurface::Paper), &light));
    assert!(std::ptr::eq(
        light.surface(ChartSurface::Flow),
        light.bookmap()
    ));
    assert!(!std::ptr::eq(
        light.surface(ChartSurface::LitFlow),
        light.bookmap()
    ));
    assert_eq!(ChartSurface::pane(None), ChartSurface::Paper);
    assert_eq!(ChartSurface::pane(Some(false)), ChartSurface::Flow);
    assert_eq!(ChartSurface::pane(Some(true)), ChartSurface::LitFlow);
}

#[test]
fn the_lit_pane_is_paper_where_it_says_nothing_never_the_island() {
    let light = light();
    let lit = light.surface(ChartSurface::LitFlow);
    let island = light.bookmap();
    for token in [
        Token::DrawingDefault,
        Token::DrawingBuy,
        Token::DrawingSell,
        Token::DrawingVwap,
        Token::DrawingProfile,
        Token::ChartCanvas,
        Token::Canvas,
        Token::TextPrimary,
        Token::CandleWick,
        Token::AxisInk,
    ] {
        assert_eq!(lit.color(token), light.color(token), "{}", token.key());
        // The island's own overrides; the profile and the axes it shares with paper.
        if !matches!(token, Token::DrawingProfile | Token::AxisInk) {
            assert_ne!(lit.color(token), island.color(token), "{}", token.key());
        }
    }
    assert_eq!(lit.plot_ink(VWAP.0, VWAP.1), light.plot_ink(VWAP.0, VWAP.1));
    assert!(lit.plot_ink(VWAP.0, VWAP.1).is_some());
    assert_eq!(lit.candle_fill_opacity, light.candle_fill_opacity);
    assert_eq!(lit.footprint_heat, light.footprint_heat);
    assert_eq!(lit.color(Token::FlowSurface).to_array(), [255; 4]);
    assert!(lit.orderflow.surface_bound);
    assert_eq!(lit.candle_casing_px, Some(1.5));
    assert_eq!(lit.color(Token::Casing).to_array()[3], 0xE6);
    let dark = ResolvedTheme::parse(DARK).unwrap();
    for plain in [&light, island, &dark, dark.bookmap(), light.title_strip()] {
        assert_eq!(
            plain.candle_casing_px, None,
            "only the lit pane cases candles"
        );
    }
    assert!(!light.orderflow.surface_bound && !island.orderflow.surface_bound);
}

#[test]
fn the_lit_ramp_climbs_grey_to_a_plum_wall_on_its_own_stops() {
    let lit = light().surface(ChartSurface::LitFlow).orderflow;
    assert_eq!(lit.ramp_stops, [0, 150, 400, 620, 800, 870, 900, 950, 1000]);
    assert_eq!(lit.ramp[0], [255, 255, 255]);
    assert_eq!(lit.ramp[6], [0x8A, 0x2C, 0x7C]);
    assert_eq!(lit.ramp[8], [0x3A, 0x0A, 0x33]);
    let luminance = lit
        .ramp
        .map(|[r, g, b]| relative_luminance(PremultipliedRgba::from_rgb(r, g, b)));
    assert!(
        luminance.windows(2).all(|pair| pair[1] < pair[0]),
        "more liquidity is always darker ink: {luminance:?}"
    );
    for [r, g, b] in lit.ramp {
        // Grey, or plum: never the gold, ochre or orange family.
        assert!(r <= b || g < b, "{r} {g} {b}");
    }
    // The wall stands off the paper as text would.
    let white = PremultipliedRgba::from_rgb(255, 255, 255);
    let [r, g, b] = lit.ramp[6];
    assert!(contrast_ratio(PremultipliedRgba::from_rgb(r, g, b), white) >= TEXT);
}

#[test]
fn every_lit_order_flow_token_reads_on_white() {
    let lit = light().surface(ChartSurface::LitFlow).clone();
    let flow = lit.orderflow;
    let paper = lit.color(Token::ChartCanvas);
    assert_eq!(paper.to_array(), [255; 4]);
    let on_paper = |bytes: [u8; 4]| over(rgba(bytes), paper);
    let legend = on_paper(flow.legend_background);
    assert!(contrast_ratio(rgba(flow.legend_text), legend) >= TEXT);
    assert!(contrast_ratio(rgba(flow.muted_text), paper) >= TEXT);
    for mark in [flow.lane_now, flow.gap_boundary, flow.legend_border] {
        assert!(contrast_ratio(on_paper(mark), paper) >= 1.5, "{mark:?}");
    }
    assert!(contrast_ratio(on_paper(flow.lane_now), paper) >= MARK);
    assert!(contrast_ratio(on_paper(flow.gap_boundary), paper) >= MARK);
    // A bubble's label: the large-text floor, as labels draw only on big bubbles.
    for side in [flow.buy, flow.sell] {
        assert!(contrast_ratio(rgba(flow.bubble_text), rgba(side)) >= MARK);
        assert!(contrast_ratio(rgba(side), paper) >= MARK);
    }
    let [buy_rim, sell_rim] = flow.flat_rims.expect("the lit pane rims its bubbles");
    for (rim, side) in [(buy_rim, flow.buy), (sell_rim, flow.sell)] {
        assert!(contrast_ratio(rgba(rim), paper) >= TEXT);
        assert!(relative_luminance(rgba(rim)) < relative_luminance(rgba(side)));
    }
    // The white hair lifts a bubble off the wall it sits on.
    let [r, g, b] = flow.ramp[8];
    assert!(contrast_ratio(rgba(flow.separator), PremultipliedRgba::from_rgb(r, g, b)) >= TEXT);
    assert!(contrast_ratio(rgba(flow.consumption), paper) >= TEXT);
    assert!(contrast_ratio(on_paper(flow.crown_backing), paper) >= MARK);
    assert!(contrast_ratio(on_paper(flow.preview_path), paper) >= MARK);
    // The tape's path and the trail's candles on white.
    assert!(contrast_ratio(on_paper(lit.color(Token::TapePath).to_array()), paper) >= MARK);
    for outline in [Token::CandleBullOutline, Token::CandleBearOutline] {
        assert!(contrast_ratio(lit.color(outline), paper) >= TEXT);
    }
    // The outline carries a candle's edge; its body need only stand apart.
    for fill in [Token::Buy, Token::Sell] {
        assert!(contrast_ratio(lit.color(fill), paper) >= 2.5);
    }
    assert!(contrast_ratio(lit.color(Token::FlowDivider), paper) >= 2.0);
}

#[test]
fn the_lit_bulb_reads_on_the_band_apart_from_the_resting_gear_and_is_cool() {
    let light = light();
    let band = light.color(Token::FlowBand);
    let (lit, rest) = (
        light.color(Token::FlowBulbLit),
        light.color(Token::FlowGearInk),
    );
    assert!(contrast_ratio(lit, band) >= TEXT);
    assert!(contrast_ratio(lit, band) > contrast_ratio(rest, band));
    assert_ne!(lit, rest);
    let [r, _, b, _] = lit.to_array();
    assert!(b >= r, "a cool white, never gold: {lit:?}");
}

#[test]
fn an_invalid_lit_surface_rejects_the_whole_theme() {
    let mut state = Appearance::default();
    state.apply(LIGHT).unwrap();
    let before = state.clone();
    for (invalid, field) in [
        (
            "[lit_flow.colors]\nunknown='#123456'",
            "lit_flow.colors.unknown",
        ),
        (
            "[lit_flow.orderflow.colors]\nbuy='#12345680'",
            "lit_flow.orderflow.colors.buy",
        ),
        (
            "[lit_flow.orderflow.colors]\nbuy_rim='#123456'",
            "lit_flow.orderflow.colors.buy_rim",
        ),
        (
            "[lit_flow.orderflow]\nramp_stops=[0.0,0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.9]",
            "lit_flow.orderflow.ramp_stops",
        ),
        (
            "[lit_flow.orderflow]\nramp_stops=[0.0,0.2,0.1,0.3,0.4,0.5,0.6,0.7,1.0]",
            "lit_flow.orderflow.ramp_stops",
        ),
        (
            "[lit_flow.orderflow]\nramp_stops=[0.0,0.1,0.2,0.3,0.4,0.5,0.6,nan,1.0]",
            "lit_flow.orderflow.ramp_stops",
        ),
        ("[lit_flow]\nwidth=3", "document"),
        (
            "[lit_flow]\ncandle_casing_px=9.0",
            "lit_flow.candle_casing_px",
        ),
        (
            "[lit_flow]\ncandle_casing_px=nan",
            "lit_flow.candle_casing_px",
        ),
        (
            "[lit_flow.orderflow]\nheat_gamma=0.05",
            "lit_flow.orderflow.heat_gamma",
        ),
        (
            "[lit_flow.orderflow]\nheat_gamma=nan",
            "lit_flow.orderflow.heat_gamma",
        ),
        (
            "[lit_flow.orderflow]\nheat_fade_below=0.0",
            "lit_flow.orderflow.heat_fade_below",
        ),
        (
            "[lit_flow.orderflow]\nheat_fade_below=1.5",
            "lit_flow.orderflow.heat_fade_below",
        ),
    ] {
        let error = state
            .apply(&format!("version=1\nname='Invalid lights'\n{invalid}"))
            .unwrap_err();
        assert_eq!(error.field, field, "{invalid}");
        assert_eq!(state.resolved(), before.resolved());
        assert_eq!(state.revision(), before.revision());
    }
}

#[test]
fn only_the_lit_pane_lifts_and_fills_its_heat() {
    let (dark, light) = (ResolvedTheme::parse(DARK).unwrap(), light());
    let lit = &light.surface(ChartSurface::LitFlow).orderflow;
    assert_eq!(lit.heat_gamma(), Some(0.7));
    assert_eq!(lit.heat_fade_below(), Some(0.1));
    lit.validate().unwrap();
    for plain in [&light, light.bookmap(), &dark, dark.bookmap()] {
        assert_eq!(plain.orderflow.heat_gamma, None);
        assert_eq!(plain.orderflow.heat_fade_below, None);
    }
    let mut saved = *lit;
    saved.heat_gamma = Some(9_000);
    assert_eq!(saved.validate().unwrap_err().field, "orderflow.heat_gamma");
    let mut saved = *lit;
    saved.heat_fade_below = Some(0);
    assert_eq!(
        saved.validate().unwrap_err().field,
        "orderflow.heat_fade_below"
    );
}

#[test]
fn the_lights_change_nothing_else_a_theme_resolves() {
    let mut document = ThemeDocument::parse(LIGHT).unwrap();
    let mut full = document.resolve().unwrap();
    document.lit_flow = None;
    let without = document.resolve().unwrap();
    assert!(!without.has_lit_flow());
    full.lit_flow = None;
    assert_eq!(full, without);
}
