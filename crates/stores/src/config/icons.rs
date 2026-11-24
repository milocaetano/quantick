//! Each asset's tab icon (`symbol_icons`): a glyph and a disc colour, keyed
//! like `symbol_bubble_presets`. No symbol in code names an icon; an asset
//! the config does not name wears its first letter on a neutral disc.

use serde::Deserialize;

use super::{FeedConfig, declared};

/// The most characters an icon's glyph takes: one letter or sign fits a
/// 16 pt disc, two at a squeeze.
pub const ICON_GLYPH_MAX_CHARS: usize = 2;

/// One asset's icon.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetIcon {
    /// What the disc shows: `"B"`, `"Ξ"`, `"$"`.
    pub glyph: String,
    /// The disc's colour, `#RRGGBB`.
    pub colour: String,
}

impl AssetIcon {
    /// The colour as RGB, when it is `#RRGGBB`.
    #[must_use]
    pub fn rgb(&self) -> Option<[u8; 3]> {
        let hex = self.colour.strip_prefix('#')?;
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        Some([byte(0)?, byte(2)?, byte(4)?])
    }
}

impl FeedConfig {
    /// `symbol`'s icon on this feed, glyph and RGB: the exact key, else the
    /// longest family pattern; `None` when no key names it.
    #[must_use]
    pub fn asset_icon(&self, symbol: &str) -> Option<(&str, [u8; 3])> {
        let (_, icon) = declared(&self.symbol_icons, symbol)?;
        Some((icon.glyph.as_str(), icon.rgb()?))
    }

    pub(super) fn validate_icons(&self) -> Result<(), String> {
        for (symbol, icon) in &self.symbol_icons {
            let id = &self.id;
            if !self
                .symbols
                .iter()
                .any(|offered| super::key_matches(symbol, offered))
            {
                return Err(format!(
                    "feed '{id}' sets an icon for symbol '{symbol}', which matches none of \
                     the symbols it offers"
                ));
            }
            let chars = icon.glyph.chars().count();
            if !(1..=ICON_GLYPH_MAX_CHARS).contains(&chars) {
                return Err(format!(
                    "feed '{id}' gives '{symbol}' an icon glyph of {chars} characters; a glyph \
                     is 1 to {ICON_GLYPH_MAX_CHARS}"
                ));
            }
            if icon.rgb().is_none() {
                return Err(format!(
                    "feed '{id}' gives '{symbol}' the icon colour '{}', which is not #RRGGBB",
                    icon.colour
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(icons: &str) -> FeedConfig {
        toml::from_str(&format!(
            "id = \"b3\"\nname = \"B3\"\nprovider = \"metatrader\"\n\
             symbols = [\"WINZ26\", \"WDOZ26\", \"WIN$N\"]\nsymbol_icons = {icons}\n"
        ))
        .expect("fixture feed")
    }

    #[test]
    fn an_exact_key_wins_over_its_family_and_others_have_none() {
        let feed = feed(
            "{ \"WIN*\" = { glyph = \"W\", colour = \"#3B4250\" }, \
             \"WIN$N\" = { glyph = \"N\", colour = \"#102030\" } }",
        );
        assert_eq!(feed.asset_icon("WINZ26"), Some(("W", [0x3B, 0x42, 0x50])));
        assert_eq!(feed.asset_icon("WIN$N"), Some(("N", [0x10, 0x20, 0x30])));
        assert_eq!(feed.asset_icon("WDOZ26"), None);
        assert!(feed.validate_icons().is_ok());
    }

    #[test]
    fn a_bad_colour_a_long_glyph_or_a_key_matching_nothing_is_refused() {
        for (icons, refusal) in [
            (
                "{ \"WIN*\" = { glyph = \"W\", colour = \"3B4250\" } }",
                "#RRGGBB",
            ),
            (
                "{ \"WIN*\" = { glyph = \"W\", colour = \"#3B42ZZ\" } }",
                "#RRGGBB",
            ),
            (
                "{ \"WIN*\" = { glyph = \"WIN\", colour = \"#3B4250\" } }",
                "glyph",
            ),
            (
                "{ \"WIN*\" = { glyph = \"\", colour = \"#3B4250\" } }",
                "glyph",
            ),
            (
                "{ \"BTC\" = { glyph = \"B\", colour = \"#F7931A\" } }",
                "matches none",
            ),
        ] {
            let error = feed(icons).validate_icons().unwrap_err();
            assert!(error.contains(refusal), "{icons}: {error}");
        }
    }
}
