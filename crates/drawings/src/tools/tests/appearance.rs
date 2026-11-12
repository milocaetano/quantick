use super::*;
use quantick_appearance::{ResolvedTheme, Token};

struct Metrics;
impl TextMetrics for Metrics {
    fn size(&self, text: &str, font: FontSpec) -> Px {
        vec2(text.len() as f32 * font.size_px * 0.6, font.size_px)
    }
}
fn scene(
    theme: &ResolvedTheme,
    id: &str,
    payload: &dyn DrawingPayload,
    selected: bool,
) -> MarkScene {
    scene_in(
        theme,
        id,
        payload,
        selected,
        PremultipliedRgba::from_rgb(71, 92, 123),
    )
}

fn scene_in(
    theme: &ResolvedTheme,
    id: &str,
    payload: &dyn DrawingPayload,
    selected: bool,
    color: PremultipliedRgba,
) -> MarkScene {
    let scale = PriceScale::from_range(0.0, 100.0, 0.0, 500.0);
    let anchors = [
        ChartPoint {
            bar: 1.0,
            price: 20.0,
            time_ms: None,
        },
        ChartPoint {
            bar: 5.0,
            price: 80.0,
            time_ms: None,
        },
    ];
    let style = DrawingStyle {
        color,
        ..DrawingStyle::default()
    };
    let ctxt = DrawContext {
        appearance: theme,
        payload,
        anchors: &anchors,
        scale: &scale,
        px_per_bar: 10.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style,
        selected,
        halo: false,
        content_editing: false,
        text: &Metrics,
    };
    let mut result = MarkScene::default();
    tool(id).paint(
        &mut result,
        PxRect::from_min_max(Px::ZERO, pos2(900.0, 500.0)),
        style,
        &[pos2(100.0, 400.0), pos2(500.0, 100.0)],
        &ctxt,
        selected,
    );
    result
}

#[test]
fn selection_changes_ambient_handles_without_repainting_authored_stroke() {
    let dark = ResolvedTheme::bundled_dark();
    let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
    assert_eq!(
        dark.color(Token::DrawingSelectionHalo),
        SELECTION_HALO_COLOR
    );
    assert_eq!(dark.color(Token::Canvas), SELECTED_ANCHOR_FILL);
    for theme in [dark, &light] {
        let painted = scene(theme, "trend-line", &NoPayload, true);
        assert!(
            painted
                .shapes
                .iter()
                .any(|shape| matches!(shape, MarkShape::Circle { stroke, .. }
            if stroke.color == theme.color(Token::Accent)))
        );
        assert!(
            painted
                .shapes
                .iter()
                .any(|shape| matches!(shape, MarkShape::Circle { fill, .. }
            if *fill == theme.color(Token::Canvas)))
        );
        assert!(
            painted
                .shapes
                .iter()
                .any(|shape| matches!(shape, MarkShape::Line { stroke, .. }
            if stroke.color == PremultipliedRgba::from_rgb(71, 92, 123)))
        );
    }
}

#[test]
fn empty_note_uses_ambient_ink_but_authored_note_keeps_its_font_and_color() {
    let theme = ResolvedTheme::parse("version = 1\nname = 'Contrast'\n[colors]\ntext_muted = '#12345680'\n[typography.drawing]\nsize = 40").unwrap();
    for text in ["", "Authored"] {
        let payload = TextPayload {
            text: text.into(),
            size_px: 17.0,
            family: FontFamily::Monospace,
        };
        let painted = scene(&theme, "text", &payload, false);
        assert!(painted.shapes.iter().any(|shape| matches!(shape, MarkShape::Text { color, font, .. }
            if *color == if text.is_empty() { theme.color(Token::TextMuted) } else { PremultipliedRgba::from_rgb(71, 92, 123) }
            && font.size_px == 17.0 && font.family == FontFamily::Monospace)));
    }
}

#[test]
fn ruler_uses_the_same_configured_font_for_plate_measurement_and_paint() {
    let theme = ResolvedTheme::parse("version = 1\nname = 'Large ruler'\n[colors]\nbuy = '#005500'\ndrawing_readout_plate = '#F0F0F0EE'\n[typography.monospace]\nsize = 36\nfamily = 'proportional'").unwrap();
    let painted = scene(&theme, "measure", &NoPayload, false);
    assert!(painted.shapes.iter().any(|shape| matches!(shape, MarkShape::Text { color, font, .. }
        if *color == theme.color(Token::Buy) && font.size_px == 30.0 && font.family == FontFamily::Proportional)));
    assert!(
        painted
            .shapes
            .iter()
            .any(|shape| matches!(shape, MarkShape::Rect { fill, .. }
        if *fill == theme.color(Token::DrawingReadoutPlate)))
    );
}

/// Light paints a measurement's readout on the paper plate the trader picked
/// for the ruler: no other drawing label has a plate on Light, but the ruler's
/// readout does.
#[test]
fn light_ruler_readout_sits_on_the_paper_plate() {
    let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
    let plate = light.color(Token::DrawingReadoutPlate);
    let painted = scene(&light, "measure", &NoPayload, false);
    assert!(
        painted
            .shapes
            .iter()
            .any(|shape| matches!(shape, MarkShape::Text { .. }))
    );
    assert!(
        painted
            .shapes
            .iter()
            .any(|shape| matches!(shape, MarkShape::Rect { fill, .. } if *fill == plate))
    );
}

#[test]
fn light_enlarged_fib_labels_follow_ambient_font_without_changing_levels() {
    use crate::tools::fib::{FibKind, FibPayload};
    let payload = FibPayload::new(FibKind::Retracement);
    let before = payload.export_preset();
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(&format!(
        "{}\n[typography.monospace]\nsize=24.0",
        quantick_appearance::LIGHT
    ))
    .unwrap();
    assert_eq!(
        light
            .typography
            .get(quantick_appearance::FontRole::Monospace)
            .size,
        24.0
    );
    let small = scene(&dark, "fib-retracement", &payload, false);
    let large = scene(&light, "fib-retracement", &payload, false);
    let fonts = |scene: &MarkScene| {
        scene
            .shapes
            .iter()
            .filter_map(|shape| {
                if let MarkShape::Text { font, .. } = shape {
                    Some(font.size_px)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    let (small, large) = (fonts(&small), fonts(&large));
    assert!(!small.is_empty());
    assert_eq!(small.len(), large.len());
    assert!(small.iter().zip(large).all(|(a, b)| b == a * 2.0));
    assert_eq!(payload.export_preset(), before);
}

#[test]
fn light_enlarged_profile_status_uses_ambient_font_and_ink() {
    use quantick_anchored_studies::{ProfileInputs, ProfileRequest, RangeProfile};
    use quantick_engine::{BarSpec, Side, Trade, bar_registry::BarConfiguration};
    let mut builder = BarConfiguration::from(BarSpec::Tick(1)).build();
    let mut bars = Vec::new();
    builder.push_into(
        &Trade {
            agg_id: 1,
            timestamp_ms: 0,
            price: 50.into(),
            quantity: 1.into(),
            side: Side::Buy,
        },
        &mut bars,
    );
    let mut payload = crate::tools::fixed_range_profile::FrvpPayload {
        show_labels: true,
        ..Default::default()
    };
    RangeProfile::refresh(
        &mut payload.cache,
        ProfileRequest {
            min_bar: 0.0,
            max_bar: 1.0,
            extend_right: true,
            approximate_history: false,
            value_area_pct: 70,
        },
        &ProfileInputs {
            closed: &bars,
            ladders: &[],
            prefix: &[],
            partial_ladder: None,
            group: 1.into(),
            series_revision: 0,
            partial_version: 0,
            blocked: true,
            side_inferred: false,
            partial_bucket_slot: None,
            budget: 1500,
        },
    );
    let theme = ResolvedTheme::parse(&format!(
        "{}\n[typography.body]\nsize=25.0",
        quantick_appearance::LIGHT
    ))
    .unwrap();
    assert_eq!(
        theme
            .typography
            .get(quantick_appearance::FontRole::Body)
            .size,
        25.0
    );
    assert!(
        payload.cache.is_some(),
        "refresh must install the blocked cache"
    );
    for (appearance, expected_size) in [(&ResolvedTheme::default(), 10.0), (&theme, 20.0)] {
        let painted = scene(appearance, "fixed-range-profile", &payload, false);
        assert!(
            painted.shapes.iter().any(|shape| matches!(shape,
                MarkShape::Text { text, font, color, .. }
                if text == "feed reports no traded volume"
                    && font.size_px == expected_size
                    && *color == appearance.color(Token::TextMuted)
            )),
            "expected visible status at {expected_size}px: {painted:?}"
        );
    }
}

/// Every colour a scene paints with, strokes, fills and text alike.
fn painted_colors(scene: &MarkScene) -> Vec<PremultipliedRgba> {
    scene
        .shapes
        .iter()
        .flat_map(|shape| match shape {
            MarkShape::Line { stroke, .. } | MarkShape::DashedLine { stroke, .. } => {
                vec![stroke.color]
            }
            MarkShape::Path { fill, stroke, .. }
            | MarkShape::Rect { fill, stroke, .. }
            | MarkShape::Circle { fill, stroke, .. } => vec![*fill, stroke.color],
            MarkShape::Text { color, .. } => vec![*color],
        })
        .collect()
}

#[test]
fn a_style_saved_on_dark_keeps_its_colour_on_dark_and_is_shaded_on_light() {
    // The trader's saved presets: every tool in #f3f7ff, chosen on Dark.
    let near_white = PremultipliedRgba::from_rgb(0xF3, 0xF7, 0xFF);
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
    let shaded = light.legible_drawing(near_white);
    for tool in DRAWING_TOOLS {
        let payload = tool.default_payload();
        let on_dark = painted_colors(&scene_in(&dark, tool.id(), &*payload, false, near_white));
        let on_light = painted_colors(&scene_in(&light, tool.id(), &*payload, false, near_white));
        assert!(
            !on_light.contains(&near_white),
            "{} paints white on white",
            tool.id()
        );
        assert_eq!(
            on_dark.contains(&near_white),
            on_light.contains(&shaded),
            "{} swaps exactly the authored colour",
            tool.id()
        );
    }
    let ruler = painted_colors(&scene_in(&light, "measure", &NoPayload, false, near_white));
    assert!(ruler.contains(&shaded), "the ruler leg reads on Light");
    let dark_ruler = painted_colors(&scene_in(&dark, "measure", &NoPayload, false, near_white));
    assert!(
        dark_ruler.contains(&near_white),
        "Dark paints the saved colour unchanged"
    );
}

#[test]
fn a_flat_readout_reads_against_its_plate() {
    let near_white = PremultipliedRgba::from_rgb(0xF3, 0xF7, 0xFF);
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
    let text = |theme: &ResolvedTheme| {
        scene_in(theme, "date-range", &NoPayload, false, near_white)
            .shapes
            .into_iter()
            .filter_map(|shape| match shape {
                MarkShape::Text { color, .. } => Some(color),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let (on_dark, on_light) = (text(&dark), text(&light));
    assert!(!on_dark.is_empty() && on_dark.iter().all(|color| *color == near_white));
    // Light writes a readout with no direction in its primary ink.
    assert!(!on_light.is_empty());
    assert!(
        on_light
            .iter()
            .all(|color| *color == light.color(Token::TextPrimary))
    );
}

/// A level ruler (Shift copies the price) has no move to ink: Light writes
/// its bars and time in the primary ink inside the grey edge.
#[test]
fn light_level_ruler_reads_in_the_primary_ink() {
    let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
    let (texts, edges) = ruler_readout(&light, [50.0, 50.0]);
    assert_eq!(texts.len(), 1, "bars and time only: {texts:?}");
    assert!(texts[0].0.contains("bars"), "{texts:?}");
    assert_eq!(texts[0].1, light.color(Token::TextPrimary));
    assert_eq!(
        edges,
        vec![Stroke::new(1.0, light.color(Token::ReadoutEdge))]
    );
}

/// The ruler's texts and the stroke of its plate's edge, measured from
/// `prices[0]` at bar 1 to `prices[1]` at bar 5.
fn ruler_readout(
    theme: &ResolvedTheme,
    prices: [f64; 2],
) -> (Vec<(String, PremultipliedRgba)>, Vec<Stroke>) {
    let scale = PriceScale::from_range(0.0, 100.0, 0.0, 500.0);
    let point = |bar, price| ChartPoint {
        bar,
        price,
        time_ms: None,
    };
    let anchors = [point(1.0, prices[0]), point(5.0, prices[1])];
    let style = DrawingStyle::default();
    let ctxt = DrawContext {
        appearance: theme,
        payload: &NoPayload,
        anchors: &anchors,
        scale: &scale,
        px_per_bar: 10.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style,
        selected: false,
        halo: false,
        content_editing: false,
        text: &Metrics,
    };
    let mut painted = MarkScene::default();
    tool("measure").paint(
        &mut painted,
        PxRect::from_min_max(Px::ZERO, pos2(900.0, 500.0)),
        style,
        &[pos2(100.0, 400.0), pos2(500.0, 100.0)],
        &ctxt,
        false,
    );
    let texts = painted
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            MarkShape::Text { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();
    let edges = painted
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            MarkShape::Rect { stroke, .. } if stroke.width > 0.0 => Some(*stroke),
            _ => None,
        })
        .collect();
    (texts, edges)
}

/// Light's ruler readout is a white plate edged in neutral grey: the points
/// and percent line in the chart's gain or loss ink, the bars and time in
/// the primary ink. Dark keeps its unedged plate, every line in the move's
/// colour.
#[test]
fn light_ruler_readout_is_edged_and_inks_only_the_signed_line() {
    let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
    let dark = ResolvedTheme::bundled_dark();
    for rising in [true, false] {
        let prices = if rising { [20.0, 80.0] } else { [80.0, 20.0] };
        let (texts, edges) = ruler_readout(&light, prices);
        assert_eq!(texts.len(), 2, "{texts:?}");
        let (price, time) = (&texts[0], &texts[1]);
        assert!(price.0.contains("pts"), "{price:?}");
        let signed = if rising {
            Token::BuyInk
        } else {
            Token::LossInk
        };
        assert_eq!(price.1, light.color(signed), "rising {rising}");
        assert_eq!(time.1, light.color(Token::TextPrimary));
        assert_eq!(
            edges,
            vec![Stroke::new(1.0, light.color(Token::ReadoutEdge))],
            "one grey edge"
        );

        let (texts, edges) = ruler_readout(dark, prices);
        let side = if rising { Token::Buy } else { Token::Sell };
        assert!(texts.iter().all(|(_, color)| *color == dark.color(side)));
        assert!(edges.is_empty(), "Dark draws no readout edge");
    }
}
