//! A separately authored chart surface, resolved once beside the application theme.
use crate::{ColorValue, ResolvedTheme, ThemeDocument, ThemeError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BookmapOverride {
    pub candle_fill_opacity: Option<f32>,
    #[serde(default)]
    pub colors: BTreeMap<String, ColorValue>,
    #[serde(default)]
    pub footprint_heat: crate::heat::HeatOverride,
}

impl BookmapOverride {
    pub(crate) fn resolve(&self, parent: &ResolvedTheme) -> Result<ResolvedTheme, ThemeError> {
        // Reuse the root validation contract, including RGB-only style boundaries.
        // No nested surface is authored or resolved here.
        ThemeDocument {
            version: 1,
            name: parent.name.clone(),
            dark: parent.dark,
            candle_fill_opacity: self.candle_fill_opacity,
            colors: self.colors.clone(),
            typography: BTreeMap::new(),
            footprint_heat: self.footprint_heat.clone(),
            orderflow: Default::default(),
            bookmap: None,
        }
        .resolve_with(Some(parent))
        .map_err(|error| ThemeError::new(format!("bookmap.{}", error.field), error.message))
    }
}

impl ResolvedTheme {
    /// Missing surface overrides preserve version-1 authored themes' inheritance.
    pub fn bookmap(&self) -> &Self {
        self.bookmap.as_deref().unwrap_or(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Appearance, DARK, LIGHT, ResolvedTheme, ThemeDocument, Token};

    #[test]
    fn slate_light_bookmap_preserves_market_colors_and_keeps_time_canvas_light() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        assert_eq!(light.candle_fill_opacity, 1.0);
        assert_eq!(
            light.bookmap().candle_fill_opacity,
            dark.candle_fill_opacity
        );
        for token in Token::ALL {
            let expected = match token {
                Token::Canvas | Token::CandlePreviewBackground => [37, 47, 55, 255],
                Token::DrawingDefault
                | Token::DrawingBuy
                | Token::DrawingSell
                | Token::DrawingVwap => [181, 131, 43, 255],
                Token::DrawingProfile => [122, 142, 194, 255],
                Token::TextPrimary => [243, 244, 246, 255],
                Token::TextMuted | Token::TextSupport | Token::TapeLabel => [208, 215, 223, 255],
                Token::TextFaint => [187, 198, 208, 255],
                Token::CandleWick => [182, 201, 210, 255],
                Token::FootprintProfile => [197, 204, 214, 255],
                _ => dark.color(token).to_array(),
            };
            assert_eq!(
                light.bookmap().color(token).to_array(),
                expected,
                "{}",
                token.key()
            );
        }
        assert_eq!(light.color(Token::Canvas).to_array(), [255, 252, 247, 255]);
        assert_eq!(light.bookmap().orderflow, dark.orderflow);
        assert_eq!(light.bookmap().footprint_heat, dark.footprint_heat);
        assert_eq!(light.bookmap().typography, light.typography);
        assert_eq!(light.typography, dark.typography);
        assert!(std::ptr::eq(&dark, dark.bookmap()));
    }

    #[test]
    fn authored_surface_is_independent_and_missing_fields_inherit_parent() {
        let theme = ResolvedTheme::parse("version=1\nname='Independent'\ncandle_fill_opacity=0.7\n[colors]\ncanvas='#123456'\n[bookmap]\ncandle_fill_opacity=0.3\n[bookmap.colors]\nbuy='#ABCDEF'").unwrap();
        assert_eq!(
            theme.bookmap().color(Token::Canvas),
            theme.color(Token::Canvas)
        );
        assert_eq!(
            theme.bookmap().color(Token::Buy).to_array(),
            [171, 205, 239, 255]
        );
        assert_ne!(theme.bookmap().color(Token::Buy), theme.color(Token::Buy));
        assert_eq!(theme.candle_fill_opacity, 0.7);
        assert_eq!(theme.bookmap().candle_fill_opacity, 0.3);
        let mut document = ThemeDocument::parse(DARK).unwrap();
        document.bookmap = Some(Default::default());
        assert_eq!(
            document
                .resolve_complete()
                .unwrap()
                .bookmap()
                .candle_fill_opacity,
            0.2
        );
    }

    #[test]
    fn invalid_surface_rejects_candidate_atomically() {
        let mut state = Appearance::default();
        state.apply(LIGHT).unwrap();
        let before = state.clone();
        for invalid in [
            "[bookmap]\ncandle_fill_opacity=nan",
            "[bookmap]\ncandle_fill_opacity=-0.1",
            "[bookmap]\ncandle_fill_opacity=1.1",
            "[bookmap.colors]\ncanvas='#12345680'",
            "[bookmap.colors]\nbuy='#12345680'",
            "[bookmap.colors]\nunknown='#123456'",
        ] {
            let error = state
                .apply(&format!("version=1\nname='Invalid surface'\n{invalid}"))
                .unwrap_err();
            assert!(error.field.starts_with("bookmap."), "{error}");
            assert_eq!(state.resolved(), before.resolved());
            assert_eq!(state.source(), before.source());
            assert_eq!(state.revision(), before.revision());
        }
    }
}
