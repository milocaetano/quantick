//! The window's title strip (logo, market tabs, `+`, menu and window
//! buttons), resolved once beside the application theme. A theme may dress it
//! in other inks than its own: Light keeps the order-flow slate with Dark's.
use crate::{ColorValue, ResolvedTheme, ThemeDocument, ThemeError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TitleStripOverride {
    #[serde(default)]
    pub colors: BTreeMap<String, ColorValue>,
}

impl TitleStripOverride {
    pub(crate) fn resolve(&self, parent: &ResolvedTheme) -> Result<ResolvedTheme, ThemeError> {
        ThemeDocument {
            version: 1,
            name: parent.name.clone(),
            dark: parent.dark,
            candle_fill_opacity: None,
            candle_outline_width: None,
            candle_outline_opacity: None,
            candle_wick_opacity: None,
            chart_time_grid: None,
            chart_axes: None,
            colors: self.colors.clone(),
            typography: BTreeMap::new(),
            footprint_heat: Default::default(),
            orderflow: Default::default(),
            bookmap: None,
            title_strip: None,
        }
        .resolve_with(Some(parent))
        .map_err(|error| ThemeError::new(format!("title_strip.{}", error.field), error.message))
    }
}

impl ResolvedTheme {
    /// The strip's palette; a theme without the section paints it in its own.
    pub fn title_strip(&self) -> &Self {
        self.title_strip.as_deref().unwrap_or(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::{DARK, LIGHT, ResolvedTheme, Token};

    #[test]
    fn light_dresses_the_strip_in_slate_with_darks_inks_and_dark_keeps_its_own() {
        let dark = ResolvedTheme::parse(DARK).unwrap();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        assert!(
            std::ptr::eq(dark.title_strip(), &dark),
            "Dark has no section"
        );
        let strip = light.title_strip();
        assert_eq!(
            strip.color(Token::TabStrip).to_array(),
            [0x25, 0x2F, 0x37, 255]
        );
        // The logo and its wordmark read exactly as they do in Dark.
        for token in [Token::TextPrimary, Token::Brand] {
            assert_eq!(strip.color(token), dark.color(token), "{}", token.key());
        }
        // The active tab's face stays the theme's own: it is the toolbar sheet.
        assert_eq!(
            strip.color(Token::ToolbarSheet),
            light.color(Token::ToolbarSheet)
        );
        assert_eq!(strip.color(Token::TabBand), light.color(Token::TabBand));
        assert_ne!(
            light.color(Token::TextPrimary),
            dark.color(Token::TextPrimary)
        );
    }

    #[test]
    fn an_authored_light_file_without_the_table_paints_its_own_strip() {
        let authored = "version=1\nname='Mine'\ndark=false\n[colors]\ntab_strip='#E0E0E0'";
        let authored = ResolvedTheme::parse(authored).unwrap();
        assert!(std::ptr::eq(authored.title_strip(), &authored));
        let strip = authored.title_strip().color(Token::TabStrip).to_array();
        assert_eq!(strip, [0xE0, 0xE0, 0xE0, 255]);
    }

    #[test]
    fn an_invalid_strip_color_names_its_section() {
        let source = "version=1\nname='Bad strip'\n[title_strip.colors]\ntab_strip='nope'";
        let error = ResolvedTheme::parse(source).unwrap_err();
        assert!(
            error.field.starts_with("title_strip.colors."),
            "{}",
            error.field
        );
    }
}
