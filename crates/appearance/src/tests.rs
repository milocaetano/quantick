use super::*;
use color::{RgbaExt, WHITE};

#[test]
fn light_profile_source_reproduces_reference_fill_without_reapplying_its_alpha() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let source = light.color(Token::DrawingProfile);
    assert_eq!(source.to_array(), [122, 142, 194, 255]);
    assert_eq!(light.bookmap().color(Token::DrawingProfile), source);
    assert_eq!(
        ResolvedTheme::default().color(Token::DrawingProfile),
        ResolvedTheme::default().color(Token::DrawingDefault)
    );
    // This earlier reference used #F5F5F5; retain its authored source color
    // and in-value-area row opacity when the canvas changes.
    let [r, g, b, a] = source.gamma_multiply(0.55).to_array();
    let background = 245.0 * (1.0 - f32::from(a) / 255.0);
    let composite = [r, g, b].map(|channel| (f32::from(channel) + background).round() as u8);
    assert_eq!(composite, [177, 188, 217]);
    let authored = ResolvedTheme::parse(
        "version=1\nname='Authored profile'\n[colors]\ndrawing_profile='#123456'",
    )
    .unwrap();
    assert_eq!(
        authored.color(Token::DrawingProfile).to_array(),
        [18, 52, 86, 255]
    );
}

#[test]
fn authored_generic_drawing_color_remains_the_default_profile_color_when_unspecified() {
    let source = "version=1\nname='Existing authored theme'\n[colors]\ndrawing_default='#12345680'\n[bookmap.colors]\ndrawing_default='#654321'";
    let theme = ResolvedTheme::parse(source).unwrap();
    assert_eq!(
        theme.color(Token::DrawingProfile),
        theme.color(Token::DrawingDefault)
    );
    assert_eq!(
        theme.bookmap().color(Token::DrawingProfile),
        theme.bookmap().color(Token::DrawingDefault)
    );
    let mut complete = ThemeDocument::parse(DARK).unwrap();
    complete.colors.remove("drawing_profile");
    assert_eq!(
        complete.resolve_complete().unwrap_err().field,
        "colors.drawing_profile"
    );
}

#[test]
fn profile_defaults_respect_surface_presence_and_explicit_profile_tokens() {
    let root = "version=1\nname='Profile surfaces'\n[colors]\ndrawing_default='#123456'\ndrawing_profile='#234567'";
    for (surface, expected) in [
        ("[bookmap.colors]\ncanvas='#654321'", [35, 69, 103, 255]),
        (
            "[bookmap.colors]\ndrawing_default='#345678'",
            [52, 86, 120, 255],
        ),
        (
            "[bookmap.colors]\ndrawing_default='#345678'\ndrawing_profile='#456789'",
            [69, 103, 137, 255],
        ),
    ] {
        let theme = ResolvedTheme::parse(&format!("{root}\n{surface}")).unwrap();
        assert_eq!(
            theme.color(Token::DrawingProfile).to_array(),
            [35, 69, 103, 255]
        );
        assert_eq!(
            theme.bookmap().color(Token::DrawingProfile).to_array(),
            expected
        );
    }
}

#[test]
fn candle_opacity_defaults_boundaries_and_complete_document_requirement() {
    assert_eq!(ResolvedTheme::default().candle_fill_opacity, 0.20);
    assert_eq!(
        ResolvedTheme::parse(LIGHT).unwrap().candle_fill_opacity,
        1.0
    );
    let partial = "version=1\nname='Independent fallback'\ndark=false";
    assert_eq!(
        ResolvedTheme::parse(partial).unwrap().candle_fill_opacity,
        0.20
    );
    for value in [0.0, 0.37, 1.0] {
        let theme =
            ResolvedTheme::parse(&format!("{partial}\ncandle_fill_opacity={value:?}")).unwrap();
        assert_eq!(theme.candle_fill_opacity, value);
    }
    let mut complete = ThemeDocument::parse(DARK).unwrap();
    complete.candle_fill_opacity = None;
    assert_eq!(
        complete.resolve_complete().unwrap_err().field,
        "candle_fill_opacity"
    );
}

#[test]
fn invalid_candle_opacity_rejects_the_entire_candidate_atomically() {
    let mut state = Appearance::default();
    state.apply(LIGHT).unwrap();
    let previous = state.clone();
    for value in ["-0.01", "1.01", "nan", "inf", "-inf"] {
        let source = format!(
            "version=1\nname='Invalid opacity'\ncandle_fill_opacity={value}\n[colors]\ncanvas='#123456'"
        );
        assert_eq!(
            state.apply(&source).unwrap_err().field,
            "candle_fill_opacity"
        );
        assert_eq!(state.resolved(), previous.resolved());
        assert_eq!(state.source(), previous.source());
        assert_eq!(state.revision(), previous.revision());
    }
}

#[test]
fn flow_identity_dark_parity_and_partial_theme_fallback() {
    let dark = ResolvedTheme::default();
    let partial =
        ResolvedTheme::parse("version=1\nname='Partial'\n[colors]\nflow_surface='#123456'")
            .unwrap();
    for (token, bytes) in [
        (Token::FlowSurface, [30, 36, 51, 255]),
        (Token::FlowDivider, [58, 67, 87, 255]),
        (Token::FlowKeyUpLip, [56, 56, 56, 56]),
        (Token::FlowKeyDownShade, [0, 0, 0, 179]),
        (Token::FlowKeyUpInk, [255, 255, 255, 255]),
    ] {
        assert_eq!(dark.color(token).to_array(), bytes);
        if token != Token::FlowSurface {
            assert_eq!(partial.color(token), dark.color(token));
        }
    }
    assert_eq!(
        partial.color(Token::FlowSurface).to_array(),
        [18, 52, 86, 255]
    );
    assert_eq!(
        partial.color(Token::CapsuleFill),
        dark.color(Token::CapsuleFill),
        "legacy accepted keys remain stable"
    );
}

#[test]
fn bundled_dark_is_complete_and_matches_historical_bytes() {
    let document = ThemeDocument::parse(DARK).unwrap();
    assert_eq!(document.colors.len(), Token::ALL.len());
    let resolved = document.resolve().unwrap();
    assert_eq!(resolved, ResolvedTheme::default());
    for token in Token::ALL {
        assert_eq!(
            resolved.color(token),
            ResolvedTheme::bundled_dark().color(token),
            "{}",
            token.key()
        );
    }
    assert_eq!(resolved.color(Token::Canvas).to_array(), [19, 23, 34, 255]);
    assert_eq!(
        resolved.color(Token::PaneMark).to_array(),
        [190, 156, 63, 217]
    );
    assert_eq!(
        resolved.color(Token::CapsuleFill).to_array(),
        [8, 10, 15, 189]
    );
    assert_eq!(resolved.color(Token::Casing).to_array(), [5, 7, 12, 235]);
    assert_eq!(
        resolved.color(Token::CandleBullOutline).to_array(),
        [104, 224, 201, 255]
    );
}

#[test]
fn bundled_light_changes_every_semantic_color_and_preserves_readable_text() {
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    assert!(!light.dark);
    // The order-flow band's keys and gear read the same on both bands, as the
    // trader chose them.
    let shared = [
        Token::FlowKeyLitDot,
        Token::FlowKeyUnlitDot,
        Token::FlowKeyUpInk,
        Token::FlowKeyUnlitInk,
        Token::FlowGearInk,
    ];
    for token in Token::ALL {
        let same = light.color(token) == dark.color(token);
        assert_eq!(same, shared.contains(&token), "{}", token.key());
    }
    for background in [Token::Canvas, Token::Chrome, Token::Inset] {
        for text in [Token::TextPrimary, Token::TextMuted, Token::TextSupport] {
            assert!(contrast::contrast_ratio(light.color(background), light.color(text)) >= 4.5);
        }
    }
}

#[test]
fn missing_fields_fall_back_to_dark_independently() {
    let theme = ResolvedTheme::parse(
        "version = 1\nname = 'Partial'\n[colors]\ncanvas = '#ffffff'\n[typography.body]\nsize = 17",
    )
    .unwrap();
    assert_eq!(theme.color(Token::Canvas), WHITE);
    assert_eq!(theme.color(Token::Buy), palette::BUY);
    assert_eq!(
        theme.typography.get(FontRole::Body),
        FontSpec {
            family: FontFamily::Proportional,
            size: 17.0
        }
    );
    assert_eq!(
        theme.typography.get(FontRole::Small),
        ResolvedTheme::bundled_dark()
            .typography
            .get(FontRole::Small)
    );
    assert!(theme.dark);
}

#[test]
fn straight_alpha_and_explicit_premultiplied_bytes_are_distinct_contracts() {
    let theme = ResolvedTheme::parse("version = 1\nname = 'Alpha'\n[colors]\naccent = '#8AB4F838'\ncasing = { premultiplied = [5, 7, 12, 235] }").unwrap();
    assert_eq!(
        theme.color(Token::Accent),
        color::rgba_unmultiplied(138, 180, 248, 56)
    );
    assert_eq!(theme.color(Token::Casing).to_array(), [5, 7, 12, 235]);
    assert_ne!(theme.color(Token::Accent).to_array(), [138, 180, 248, 56]);
}

#[test]
fn invalid_themes_report_the_field_and_leave_state_unchanged() {
    let mut state = Appearance::default();
    state.apply(LIGHT).unwrap();
    let previous = state.clone();
    for (source, field) in [
        ("version = 2\nname = 'Future'", "version"),
        ("version = 1\nname = ''", "name"),
        (
            "version = 1\nname = 'Bad'\n[colors]\ncanvas = '#FFFFFF'\ntext_primary = '#GGGGGG'",
            "colors.text_primary",
        ),
        (
            "version = 1\nname = 'Bad'\n[colors]\ncanavs = '#FFFFFF'",
            "colors.canavs",
        ),
        (
            "version = 1\nname = 'Bad'\n[typography.body]\nsize = nan",
            "typography.body.size",
        ),
        (
            "version = 1\nname = 'Bad'\n[typography.axis]\nsize = 12",
            "typography.axis",
        ),
    ] {
        let error = state.apply(source).unwrap_err();
        assert_eq!(error.field, field);
        assert_eq!(state.resolved(), previous.resolved());
        assert_eq!(state.revision(), previous.revision());
        assert_eq!(state.source(), previous.source());
    }
}

#[test]
fn malformed_and_unknown_document_fields_are_errors() {
    for source in [
        "version = 1\nname = 'Bad'\nextra = 42",
        "version = 1\nname = 'Bad'\n[colors]\naccent = { premultiplied = [1, 2, 3, 4], extra = 1 }",
        "version = 1\nname = 'Bad'\n[typography.body]\nfamily = 'remote-font'",
        "version = 1\nname = 'Bad'\n[colors]\naccent = { premultiplied = [256, 2, 3, 4] }",
        "not TOML",
    ] {
        assert!(ResolvedTheme::parse(source).is_err(), "{source}");
    }
}

#[test]
fn retries_and_color_changes_do_not_invalidate_typography() {
    let mut state = Appearance::default();
    assert_eq!(state.apply(DARK).unwrap(), ApplyOutcome::Unchanged);
    assert_eq!(state.revision(), 0);
    assert_eq!(
        state.apply(LIGHT).unwrap(),
        ApplyOutcome::Changed {
            typography_changed: false
        }
    );
    assert_eq!(state.revision(), 1);
    assert_eq!(state.apply(LIGHT).unwrap(), ApplyOutcome::Unchanged);
    assert_eq!(
        state
            .apply(&format!("{LIGHT}\n# equivalent retry"))
            .unwrap(),
        ApplyOutcome::Unchanged
    );
    assert_eq!(state.revision(), 1);
    assert_eq!(
        state
            .apply(&format!("{LIGHT}\n[typography.body]\nsize = 17"))
            .unwrap(),
        ApplyOutcome::Changed {
            typography_changed: true
        }
    );
    assert_eq!(state.revision(), 2);
}

#[test]
fn independent_applications_cannot_leak_themes_to_each_other() {
    let dark = Appearance::default();
    let mut light = dark.clone();
    light.apply(LIGHT).unwrap();
    assert_eq!(dark.resolved(), &ResolvedTheme::default());
    assert_eq!(dark.revision(), 0);
    assert_eq!(light.revision(), 1);
}

#[test]
fn explicit_color_equal_to_previous_default_remains_explicit() {
    let saved = palette::ACCENT;
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let resolve = || light.color(Token::Accent);
    assert_eq!(Inheritance::Explicit.resolve(saved, resolve), saved);
    assert_eq!(
        Inheritance::Theme.resolve(saved, resolve),
        light.color(Token::Accent)
    );
    assert_eq!(Inheritance::default(), Inheritance::Explicit);
}

#[test]
fn document_round_trip_preserves_the_configuration_and_resolution() {
    for source in [DARK, LIGHT] {
        let original = ThemeDocument::parse(source).unwrap();
        let saved = toml::to_string(&original).unwrap();
        let restored = ThemeDocument::parse(&saved).unwrap();
        assert_eq!(restored, original);
        assert_eq!(restored.resolve().unwrap(), original.resolve().unwrap());
    }
}

#[test]
fn ink_is_the_better_configured_choice_on_the_actual_background() {
    for theme in [
        ResolvedTheme::default(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    ] {
        let background = theme.color(Token::Canvas);
        let first = theme.color(Token::ChipInk);
        let second = theme.color(Token::TextPrimary);
        for fill in [
            palette::ACCENT,
            palette::WARN,
            WHITE,
            color::TRANSPARENT,
            PremultipliedRgba::from_rgb(127, 127, 127),
            WHITE.gamma_multiply(0.45),
        ] {
            let ink = contrast::ink_on(fill, background, first, second);
            let other = if ink == first { second } else { first };
            let visible = contrast::over(fill, background);
            assert!(
                contrast::contrast_ratio(visible, ink) >= contrast::contrast_ratio(visible, other)
            );
        }
    }
    assert_eq!(
        contrast::ink_on(
            WHITE.gamma_multiply(0.45),
            palette::CANVAS,
            palette::CHIP_INK,
            palette::TEXT_PRIMARY
        ),
        palette::CHIP_INK
    );
}

#[test]
fn invalid_hex_never_panics_on_unicode_or_wrong_lengths() {
    for color in [
        "#ééé",
        "#fffff",
        "ffffff",
        "#fffffff",
        "#1234567890",
        "#12345😀",
    ] {
        let source = format!("version = 1\nname = 'Bad'\n[colors]\naccent = '{color}'");
        assert_eq!(
            ResolvedTheme::parse(&source).unwrap_err().field,
            "colors.accent"
        );
    }
}

#[test]
fn configured_side_inks_preserve_dark_and_read_clearly_on_light() {
    assert_eq!(palette::BUY_INK, contrast::side_ink(palette::BUY));
    assert_eq!(palette::SELL_INK, contrast::side_ink(palette::SELL));
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let casing = contrast::over(light.color(Token::Casing), light.color(Token::Canvas));
    for token in [Token::BuyInk, Token::SellInk] {
        assert!(contrast::contrast_ratio(light.color(token), casing) >= 4.5);
    }
}

#[test]
fn complete_documents_never_consult_the_fallback() {
    let mut document = ThemeDocument::parse(DARK).unwrap();
    assert_eq!(
        document.resolve_complete().unwrap(),
        ResolvedTheme::default()
    );
    document.colors.remove("canvas");
    let error = document.resolve_complete().unwrap_err().to_string();
    assert!(error.contains("colors.canvas"), "{error}");
    assert_eq!(
        document.resolve().unwrap().color(Token::Canvas),
        ResolvedTheme::default().color(Token::Canvas)
    );
    let mut document = ThemeDocument::parse(DARK).unwrap();
    document.typography.get_mut("body").unwrap().family = None;
    assert!(
        document
            .resolve_complete()
            .unwrap_err()
            .to_string()
            .contains("typography.body.family")
    );
    let mut document = ThemeDocument::parse(DARK).unwrap();
    document.footprint_heat.ink = None;
    assert!(
        document
            .resolve_complete()
            .unwrap_err()
            .to_string()
            .contains("footprint_heat.ink")
    );
}

#[test]
fn malformed_heat_ramps_preserve_the_active_theme() {
    let mut state = Appearance::default();
    state.apply(LIGHT).unwrap();
    let before = state.clone();
    for source in [
        "version = 1\nname = 'Bad heat'\n[footprint_heat]\nbuy = ['#000000']",
        "version = 1\nname = 'Bad heat'\n[footprint_heat]\nink = ['#000000', '#000000', '#000000', 'oops', '#000000', '#000000']",
    ] {
        assert!(state.apply(source).is_err());
        assert_eq!(state.resolved(), before.resolved());
        assert_eq!(state.revision(), before.revision());
    }
}

#[test]
fn bundled_dark_preserves_every_historical_token() {
    let theme = ResolvedTheme::default();
    for (token, expected) in [
        (Token::Canvas, palette::CANVAS),
        (Token::Chrome, palette::CHROME),
        (Token::Inset, palette::INSET),
        (Token::Control, palette::CONTROL),
        (Token::Border, palette::BORDER),
        (Token::TextPrimary, palette::TEXT_PRIMARY),
        (Token::TextMuted, palette::TEXT_MUTED),
        (Token::TextFaint, palette::TEXT_FAINT),
        (Token::Buy, palette::BUY),
        (Token::Sell, palette::SELL),
        (Token::Poc, palette::POC),
        (Token::Accent, palette::ACCENT),
        (Token::Amber, palette::AMBER),
        (Token::PaneMark, palette::PANE_MARK),
        (Token::PaneAction, palette::PANE_ACTION),
        (Token::Off, palette::OFF),
        (Token::CapsuleFill, palette::CAPSULE_FILL),
        (Token::CapsuleBorder, palette::CAPSULE_BORDER),
        (Token::CapsuleShadow, palette::CAPSULE_SHADOW),
        (Token::BareHover, palette::BARE_HOVER),
        (Token::SeamLine, palette::SEAM_LINE),
        (Token::DayRule, palette::DAY_RULE),
        (Token::SeamLabel, palette::SEAM_LABEL),
        (Token::Warn, palette::WARN),
        (Token::Rec, palette::REC),
        (Token::GapLine, palette::GAP_LINE),
        (Token::GapLabel, palette::GAP_LABEL),
        (Token::TagBg, palette::TAG_BG),
        (Token::TextSupport, palette::TEXT_SUPPORT),
        (Token::ChipInk, palette::CHIP_INK),
        (Token::Casing, palette::CASING),
        (Token::DrawViolet, palette::DRAW_VIOLET),
        (Token::DrawCyan, palette::DRAW_CYAN),
        (Token::FloatShadow, palette::FLOAT_SHADOW),
        (Token::CandleBullOutline, palette::CANDLE_BULL_OUTLINE),
        (Token::CandleBearOutline, palette::CANDLE_BEAR_OUTLINE),
        (Token::CandleWick, palette::CANDLE_WICK),
        (Token::BuyInk, palette::BUY_INK),
        (Token::SellInk, palette::SELL_INK),
    ] {
        assert_eq!(theme.color(token), expected, "{}", token.key());
    }
}

#[test]
fn role_hierarchy_uses_fixed_units_and_configuration_controls_all_sizes() {
    let dark = ResolvedTheme::default();
    for role in FontRole::ALL {
        for size in [8.0, 9.5, 10.0, 11.0, 12.0, 14.0, 18.0] {
            assert_eq!(dark.typography.at_size(role, size).size, size);
        }
    }
    let partial = ResolvedTheme::parse(
        "version = 1\nname = 'Large'\n[typography.body]\nsize = 25\nfamily = 'monospace'",
    )
    .unwrap();
    let complete = ResolvedTheme::parse(&DARK.replace("size = 12.5", "size = 25.0")).unwrap();
    for theme in [&partial, &complete] {
        assert_eq!(theme.typography.at_size(FontRole::Body, 10.0).size, 20.0);
    }
    assert_eq!(
        partial.typography.at_size(FontRole::Body, 10.0).family,
        FontFamily::Monospace
    );
    let base = AxisMetrics::new(&dark.typography);
    assert_eq!(
        base,
        AxisMetrics {
            indicator_header: 18.0,
            gutter: 64.0,
            strip: 24.0
        }
    );
    let large = ResolvedTheme::parse(
        "version = 1\nname = 'Large axes'\n[typography.chart_axis]\nsize = 22",
    )
    .unwrap();
    assert_eq!(
        AxisMetrics::new(&large.typography),
        AxisMetrics {
            indicator_header: 36.0,
            gutter: 128.0,
            strip: 48.0
        }
    );
}

#[test]
fn presentation_opacity_preserves_opaque_bytes_and_composes_configured_alpha() {
    use color::{rgba_unmultiplied, with_opacity};
    for opacity in [0, 18, 56, 84, 140, 255] {
        let rgb = PremultipliedRgba::from_rgb(138, 180, 248);
        assert_eq!(
            with_opacity(rgb, opacity),
            rgba_unmultiplied(138, 180, 248, opacity)
        );
    }
    let translucent = rgba_unmultiplied(255, 255, 255, 128);
    let faded = with_opacity(translucent, 128);
    assert_eq!(faded.to_array()[3], 64);
    let expected = rgba_unmultiplied(255, 255, 255, 64).to_array();
    for (actual, expected) in faded.to_array().into_iter().zip(expected) {
        assert!(actual.abs_diff(expected) <= 1);
    }
    assert_eq!(with_opacity(color::TRANSPARENT, 128), color::TRANSPARENT);
}

#[test]
fn rgb_style_fields_reject_alpha_before_publication() {
    for token in [
        Token::Canvas,
        Token::Control,
        Token::ChartGrid,
        Token::Buy,
        Token::Sell,
        Token::CandleBullOutline,
        Token::CandleBearOutline,
        Token::CandleWick,
    ] {
        let mut appearance = Appearance::default();
        let before = appearance.resolved().clone();
        let error = appearance
            .apply(&format!(
                "version = 1\nname = 'invalid'\n[colors]\n{} = '#12345680'",
                token.key()
            ))
            .unwrap_err();
        assert!(error.to_string().contains(token.key()));
        assert_eq!(appearance.resolved(), &before);
        assert_eq!(appearance.revision(), 0);
    }
}

#[test]
fn flow_and_tape_defaults_preserve_exact_dark_bytes_and_alpha() {
    let dark = ResolvedTheme::bundled_dark();
    assert_eq!(dark.color(Token::FlowBuy).to_array(), [112, 185, 244, 255]);
    assert_eq!(dark.color(Token::FlowSell).to_array(), [232, 175, 99, 255]);
    assert_eq!(
        dark.color(Token::TapeLabel).to_array(),
        [142, 166, 177, 255]
    );
    assert_eq!(
        dark.color(Token::TapePath),
        color::rgba_unmultiplied(142, 166, 177, 95)
    );
    assert_eq!(dark.color(Token::QuantityShadow).to_array(), [0, 0, 0, 190]);
    let configured = ResolvedTheme::parse(
        "version = 1\nname = 'Alpha'\n[colors]\nflow_buy = '#12345680'\ntape_label = '#ABCDEF40'",
    )
    .unwrap();
    assert_eq!(configured.color(Token::FlowBuy).to_array()[3], 128);
    assert_eq!(configured.color(Token::TapeLabel).to_array()[3], 64);
}

#[test]
fn ambient_layout_scale_preserves_dark_and_tracks_the_largest_role() {
    use crate::{FontRole, ResolvedTheme};
    assert_eq!(ResolvedTheme::bundled_dark().typography.layout_scale(), 1.0);
    let large = ResolvedTheme::parse("version = 1\nname = 'Large'\n[typography.body]\nsize = 25\nfamily = 'monospace'\n[typography.monospace]\nsize = 36").unwrap();
    assert_eq!(large.typography.layout_scale(), 3.0);
    assert_eq!(
        large.typography.at_size(FontRole::Monospace, 10.0).size,
        30.0
    );
    assert_eq!(34.0 * large.typography.layout_scale(), 102.0);
}

#[test]
fn preview_and_selection_keep_exact_dark_and_complete_light_values() {
    let dark = ResolvedTheme::default();
    for (token, expected) in [
        (Token::TradeSelectionHalo, [40, 40, 40, 40]),
        (Token::RetainedTapeShade, [0, 0, 0, 90]),
        (Token::CandlePreviewBackground, [12, 15, 22, 255]),
    ] {
        assert_eq!(dark.color(token).to_array(), expected);
    }
    for (token, rgb) in [
        (Token::CandlePreviewBid, [20, 185, 230]),
        (Token::CandlePreviewAsk, [245, 70, 70]),
    ] {
        for alpha in [36, 54, 78, 48] {
            assert_eq!(
                color::with_opacity(dark.color(token), alpha),
                color::rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha)
            );
        }
    }
    for (token, rgb) in [
        (Token::CandlePreviewBuy, [55, 226, 176]),
        (Token::CandlePreviewSell, [255, 91, 105]),
    ] {
        assert_eq!(
            dark.color(token),
            color::rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 210)
        );
    }
    let light = ThemeDocument::parse(LIGHT).unwrap();
    assert_eq!(light.colors.len(), Token::ALL.len());
    assert_eq!(
        light.orderflow.colors.len(),
        ThemeDocument::parse(DARK).unwrap().orderflow.colors.len()
    );
}

#[test]
fn light_uses_a_warm_canvas_neutral_panels_green_bulls_and_black_bears_without_typography_changes()
{
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let dark = ResolvedTheme::default();
    for (token, rgba) in [
        (Token::Canvas, [255, 252, 247, 255]),
        (Token::Chrome, [237, 237, 237, 255]),
        (Token::Inset, [247, 247, 247, 255]),
        (Token::Control, [255, 255, 255, 255]),
        (Token::Border, [124, 124, 124, 255]),
        (Token::TextPrimary, [32, 32, 32, 255]),
        (Token::Buy, [76, 175, 80, 255]),
        (Token::Sell, [0, 0, 0, 255]),
        (Token::CandleBullOutline, [0, 0, 0, 255]),
        (Token::CandleBearOutline, [0, 0, 0, 255]),
        (Token::CandleWick, [0, 0, 0, 255]),
        // The chart area alone is TradingView's white; the chrome stays warm.
        (Token::ChartCanvas, [255, 255, 255, 255]),
        (Token::AxisInk, [19, 23, 34, 255]),
        (Token::PriceAxisRule, [0, 0, 0, 0]),
        (Token::TimeAxisRule, [224, 227, 235, 255]),
    ] {
        assert_eq!(light.color(token).to_array(), rgba, "{}", token.key());
    }
    assert_eq!(light.typography, dark.typography);
}

/// Text on an amber fill (the REPLAY tag, the chosen speed, the replay
/// actions) reads chip_ink, at 4.5:1 in both themes.
#[test]
fn chip_ink_reads_on_amber_in_both_themes() {
    for theme in [
        ResolvedTheme::default(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    ] {
        let ratio =
            contrast::contrast_ratio(theme.color(Token::ChipInk), theme.color(Token::Amber));
        assert!(ratio >= 4.5, "{}: {ratio}", theme.name);
    }
}

/// `ResolvedTheme::bundled(false)` resolves Light inside its own lock: a mode
/// token a bundled theme left out would re-enter that lock at runtime.
#[test]
fn bundled_themes_write_every_mode_token() {
    for source in [DARK, LIGHT] {
        let theme = ThemeDocument::parse(source).unwrap();
        for token in document::MODE_TOKENS {
            let key = token.key();
            assert!(theme.colors.contains_key(key), "{}: {key}", theme.name);
        }
    }
}

#[test]
fn a_drawing_colour_authored_on_dark_stays_on_dark_and_falls_back_on_light() {
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let near_white = PremultipliedRgba::from_rgb(0xF3, 0xF7, 0xFF);
    assert_eq!(dark.legible_drawing(near_white), near_white);
    assert_eq!(dark.legible_readout(near_white), near_white);
    assert_eq!(
        light.legible_drawing(near_white),
        light.color(Token::DrawingDefault)
    );
    assert_eq!(
        light.legible_readout(near_white),
        light.color(Token::DrawingDefault)
    );
    let faint = color::rgba_unmultiplied(0xF3, 0xF7, 0xFF, 128);
    assert_eq!(dark.legible_drawing(faint), faint);
    assert_eq!(
        light.legible_drawing(faint).a(),
        128,
        "the fallback keeps the authored alpha"
    );
}

#[test]
fn every_bundled_drawing_colour_is_painted_as_authored_on_its_own_theme() {
    for theme in [
        ResolvedTheme::default(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    ] {
        let tokens = [
            Token::DrawingDefault,
            Token::DrawingBuy,
            Token::DrawingSell,
            Token::DrawingVwap,
            Token::DrawingProfile,
        ];
        for color in tokens.map(|token| theme.color(token)) {
            assert_eq!(theme.legible_drawing(color), color);
        }
    }
    let dark = ResolvedTheme::default();
    for swatch in palette::DRAWING_SWATCHES {
        assert_eq!(dark.legible_drawing(swatch), swatch);
        assert_eq!(dark.legible_readout(swatch), swatch);
    }
}

#[test]
fn legibility_falls_back_only_below_the_threshold_and_only_to_a_better_ink() {
    let white = PremultipliedRgba::from_rgb(255, 255, 255);
    let ink = PremultipliedRgba::from_rgb(0x20, 0x20, 0x20);
    let invisible = PremultipliedRgba::from_rgb(0xF0, 0xF0, 0xF0);
    assert!(contrast::contrast_ratio(invisible, white) < contrast::MIN_DRAWING_CONTRAST);
    assert_eq!(contrast::legible(invisible, white, ink), ink);
    let soft = PremultipliedRgba::from_rgb(0x4D, 0xD0, 0xE1);
    assert!(contrast::contrast_ratio(soft, white) >= contrast::MIN_DRAWING_CONTRAST);
    assert_eq!(
        contrast::legible(soft, white, ink),
        soft,
        "a chosen soft hue is kept"
    );
    let worse = PremultipliedRgba::from_rgb(255, 255, 255);
    assert_eq!(contrast::legible(invisible, white, worse), invisible);
    let transparent = PremultipliedRgba::from_premultiplied([0, 0, 0, 0]);
    assert_eq!(contrast::legible(transparent, white, ink), transparent);
}

fn assert_contrast(theme: &ResolvedTheme, ink: Token, under: Token, least: f32) {
    let ratio = contrast::contrast_ratio(theme.color(ink), theme.color(under));
    assert!(
        ratio >= least,
        "{}: {} on {}: {ratio}",
        theme.name,
        ink.key(),
        under.key()
    );
}

/// The words a panel writes - amber provenance, gains, and the primary and
/// secondary text - read at 4.5:1 on every surface Light paints a panel
/// with, and its control edge at 3:1 on the chrome.
#[test]
fn light_panel_words_and_edges_keep_their_contrast() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let dark = ResolvedTheme::default();
    for theme in [&dark, &light] {
        for under in [Token::Canvas, Token::Chrome] {
            assert_contrast(theme, Token::Amber, under, 4.5);
        }
        for under in [Token::Chrome, Token::Control] {
            assert_contrast(theme, Token::BuyInk, under, 4.5);
        }
    }
    for under in [Token::Chrome, Token::Inset, Token::Control, Token::TagBg] {
        for text in [Token::TextPrimary, Token::TextMuted] {
            assert_contrast(&light, text, under, 4.5);
        }
    }
    assert_contrast(&light, Token::Border, Token::Chrome, 3.0);
}

/// Every drawing colour a fresh object can be born in stands at 3:1 on the
/// chart it is drawn over: the white time chart, and the slate flow pane.
#[test]
fn every_light_drawing_colour_reads_on_its_own_chart() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let drawing = [
        Token::DrawingDefault,
        Token::DrawingBuy,
        Token::DrawingSell,
        Token::DrawingVwap,
        Token::DrawingProfile,
        Token::DrawViolet,
        Token::DrawCyan,
    ];
    for surface in [&light, light.bookmap()] {
        for token in drawing {
            assert_contrast(surface, token, Token::ChartCanvas, 3.0);
        }
    }
    let flow = light.bookmap();
    let dark = ResolvedTheme::default();
    for token in [
        Token::DrawingBuy,
        Token::DrawingSell,
        Token::DrawViolet,
        Token::DrawCyan,
    ] {
        assert_eq!(flow.color(token), dark.color(token), "{}", token.key());
    }
    assert_eq!(
        flow.color(Token::DrawingVwap),
        dark.color(Token::Accent),
        "the flow pane's VWAP is Dark's accent blue"
    );
}

/// Light's white controls stand off the chrome by their edge, a hovered one
/// reads apart from its rest with its words at 4.5:1, and the connection
/// screen's action reads on the chart; Dark keeps its border hover, unedged
/// controls and meta-grey action.
#[test]
fn light_controls_are_edged_and_their_hover_keeps_the_words_readable() {
    let dark = ResolvedTheme::default();
    assert_eq!(dark.color(Token::HoverFill), dark.color(Token::Border));
    assert_eq!(dark.color(Token::ControlEdge).a(), 0);
    assert_eq!(
        dark.color(Token::ConnectionAction),
        dark.color(Token::LoadingMeta)
    );
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    assert_contrast(&light, Token::ControlEdge, Token::Chrome, 3.0);
    assert!(
        contrast::contrast_ratio(light.color(Token::HoverFill), light.color(Token::Control)) >= 1.1
    );
    for ink in [Token::TextPrimary, Token::TextMuted, Token::BuyInk] {
        assert_contrast(&light, ink, Token::HoverFill, 4.5);
    }
    assert_contrast(&light, Token::ConnectionAction, Token::ChartCanvas, 4.5);
}

#[test]
fn drawing_primaries_are_every_additive_mix_of_red_green_and_blue() {
    // Each channel fully off or fully on, black excluded: the three primaries,
    // their three pairwise mixes and white, each exactly once.
    let mut mixes: Vec<[u8; 4]> = palette::DRAWING_PRIMARIES
        .iter()
        .map(|color| color.to_array())
        .collect();
    mixes.sort_unstable();
    let mut expected: Vec<[u8; 4]> = (1u8..8)
        .map(|bits| {
            let channel = |bit: u8| if bits & bit == 0 { 0 } else { 255 };
            [channel(1), channel(2), channel(4), 255]
        })
        .collect();
    expected.sort_unstable();
    assert_eq!(mixes, expected);
    assert_eq!(
        palette::DRAWING_PRIMARIES[..3]
            .iter()
            .map(|color| color.to_array())
            .collect::<Vec<_>>(),
        [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]],
        "the row reads red, green, blue first"
    );
}

/// A drawing still in its role's root default paints the surface's own colour
/// for that role; an authored colour keeps the legibility fallback alone.
#[test]
fn a_default_drawing_colour_takes_the_surface_role_and_an_authored_one_does_not() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let flow = light.bookmap();
    let born = light.color(Token::DrawingVwap);
    assert_eq!(born.to_array(), [0x2B, 0x36, 0x40, 255]);
    let on_flow = flow.legible_drawing_as(born, Token::DrawingVwap);
    assert_eq!(on_flow.to_array(), [0x8A, 0xB4, 0xF8, 255]);
    assert_eq!(light.legible_drawing_as(born, Token::DrawingVwap), born);
    for role in [Token::DrawingBuy, Token::DrawingSell, Token::DrawingProfile] {
        let born = light.color(role);
        assert_eq!(flow.legible_drawing_as(born, role), flow.color(role));
        assert_eq!(light.legible_drawing_as(born, role), born);
    }
    // Authored: main's stored blue stays, a vanished slate falls back as before.
    let blue = PremultipliedRgba::from_rgb(0x24, 0x5D, 0xB1);
    let slate = PremultipliedRgba::from_rgb(0x2B, 0x36, 0x41);
    for authored in [blue, slate] {
        let painted = flow.legible_drawing_as(authored, Token::DrawingVwap);
        assert_eq!(painted, flow.legible_drawing(authored));
    }
    assert_eq!(
        flow.legible_drawing(slate),
        flow.color(Token::DrawingDefault)
    );
    // Dark has no flow surface of its own: nothing changes there.
    let dark = ResolvedTheme::default();
    for role in [Token::DrawingVwap, Token::DrawingBuy, Token::DrawingDefault] {
        let born = dark.color(role);
        assert_eq!(dark.bookmap().legible_drawing_as(born, role), born);
    }
}

/// A document that predates `flow_band` keeps its band on its own
/// `flow_surface`, under its own inks; an authored band wins, and a document
/// that authors neither takes the fallback's band with the fallback's inks.
#[test]
fn the_flow_band_defaults_to_the_documents_own_flow_surface() {
    let band = |colors: &str| {
        let source = format!("version = 1\nname = \"Custom\"\ndark = false\n[colors]\n{colors}");
        let theme = ResolvedTheme::parse(&source).unwrap();
        theme.color(Token::FlowBand).to_array()
    };
    assert_eq!(
        band("flow_surface = \"#123456\"\n"),
        [0x12, 0x34, 0x56, 255]
    );
    assert_eq!(
        band("flow_surface = \"#123456\"\nflow_band = \"#654321\"\n"),
        [0x65, 0x43, 0x21, 255]
    );
    let dark = ResolvedTheme::default();
    assert_eq!(band(""), dark.color(Token::FlowBand).to_array());
}
