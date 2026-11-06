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
        color: PremultipliedRgba::from_rgb(71, 92, 123),
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
