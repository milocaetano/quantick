use super::*;
use color::{RgbaExt, WHITE};

#[test]
fn bundled_dark_is_complete_and_matches_historical_bytes() {
    let document = ThemeDocument::parse(DARK).unwrap();
    assert_eq!(document.colors.len(), Token::ALL.len());
    let resolved = document.resolve().unwrap();
    assert_eq!(resolved, ResolvedTheme::default());
    for token in Token::ALL {
        assert_eq!(resolved.color(token), token.dark(), "{}", token.key());
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
    for token in Token::ALL {
        assert_ne!(light.color(token), dark.color(token), "{}", token.key());
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
        FontRole::Small.dark()
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
