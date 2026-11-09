//! A theme's ink for an indicator plot nobody restyled, keyed
//! `"<indicator title>.<plot title>"`. The table belongs to the surface that
//! authors it: a derived surface (the order-flow pane, the title strip) never
//! inherits it, so a plot there keeps the colour its script declared.
use std::collections::BTreeMap;

use crate::{ColorValue, PremultipliedRgba, ResolvedTheme, ThemeError};

/// One resolved row: the indicator title, the plot title and its opaque ink.
pub(crate) type PlotInk = (String, String, PremultipliedRgba);

pub(crate) fn resolve(authored: &BTreeMap<String, ColorValue>) -> Result<Vec<PlotInk>, ThemeError> {
    authored
        .iter()
        .map(|(key, value)| {
            let field = format!("plot_ink.{key}");
            let (indicator, plot) = key
                .rsplit_once('.')
                .filter(|(indicator, plot)| !indicator.trim().is_empty() && !plot.trim().is_empty())
                .filter(|_| !key.chars().any(char::is_control))
                .ok_or_else(|| {
                    ThemeError::new(&field, "expected \"<indicator title>.<plot title>\"")
                })?;
            let ink = value.resolve(&field)?;
            if ink.to_array()[3] != 255 {
                return Err(ThemeError::new(
                    field,
                    "must be opaque: indicator plot styles use RGB",
                ));
            }
            Ok((indicator.to_owned(), plot.to_owned(), ink))
        })
        .collect()
}

impl ResolvedTheme {
    /// The ink this surface gives `plot` of `indicator` when the trader set no
    /// colour; `None` keeps the colour the indicator declared. Opaque, so its
    /// bytes are straight RGB too.
    pub fn plot_ink(&self, indicator: &str, plot: &str) -> Option<PremultipliedRgba> {
        self.plot_ink
            .iter()
            .find(|(name, title, _)| name == indicator && title == plot)
            .map(|&(_, _, ink)| ink)
    }
}

#[cfg(test)]
mod tests {
    use crate::{DARK, LIGHT, ResolvedTheme};

    const VWAP: (&str, &str) = ("VWAP (cumulative)", "vwap");

    #[test]
    fn light_inks_the_paper_vwap_and_leaves_dark_and_its_derived_surfaces_alone() {
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        let ink = light.plot_ink(VWAP.0, VWAP.1).map(|ink| ink.to_array());
        assert_eq!(ink, Some([0x2B, 0x36, 0x40, 255]));
        assert_eq!(light.plot_ink(VWAP.0, "other"), None);
        assert_eq!(light.plot_ink("Other", VWAP.1), None);
        // The slate flow pane keeps the script's purple: no inherited table.
        assert_eq!(light.bookmap().plot_ink(VWAP.0, VWAP.1), None);
        assert_eq!(light.title_strip().plot_ink(VWAP.0, VWAP.1), None);
        let dark = ResolvedTheme::parse(DARK).unwrap();
        assert_eq!(dark.plot_ink(VWAP.0, VWAP.1), None);
        assert_eq!(dark.bookmap().plot_ink(VWAP.0, VWAP.1), None);
        // The key names the bundled script's own titles: a rename breaks here.
        let script = include_str!("../../../app/scripts/vwap_cumulative.pine");
        assert!(script.contains(&format!("indicator(\"{}\"", VWAP.0)));
        assert!(script.contains(&format!("title=\"{}\"", VWAP.1)));
    }

    #[test]
    fn a_document_without_the_table_loads_and_a_surface_may_author_its_own() {
        let bare = ResolvedTheme::parse("version=1\nname='Bare'\ndark=false").unwrap();
        assert_eq!(bare.plot_ink(VWAP.0, VWAP.1), None);
        let source = "version=1\nname='Own'\n[plot_ink]\n'A.b.c' = '#010203'\n\
                      [bookmap.plot_ink]\n'VWAP (cumulative).vwap' = '#0A0B0C'";
        let theme = ResolvedTheme::parse(source).unwrap();
        // The last dot splits the key: an indicator title may hold dots.
        let ink = theme.plot_ink("A.b", "c").map(|ink| ink.to_array());
        assert_eq!(ink, Some([1, 2, 3, 255]));
        assert_eq!(theme.plot_ink(VWAP.0, VWAP.1), None);
        let flow = theme.bookmap().plot_ink(VWAP.0, VWAP.1);
        assert_eq!(flow.map(|ink| ink.to_array()), Some([10, 11, 12, 255]));
    }

    #[test]
    fn malformed_rows_and_unknown_sections_are_rejected() {
        for (table, field) in [
            ("[plot_ink]\nvwap='#123456'", "plot_ink.vwap"),
            ("[plot_ink]\n'.vwap'='#123456'", "plot_ink..vwap"),
            ("[plot_ink]\n'VWAP.'='#123456'", "plot_ink.VWAP."),
            ("[plot_ink]\n'VWAP.vwap'='nope'", "plot_ink.VWAP.vwap"),
            ("[plot_ink]\n'VWAP.vwap'='#12345680'", "plot_ink.VWAP.vwap"),
            (
                "[bookmap.plot_ink]\nvwap='#123456'",
                "bookmap.plot_ink.vwap",
            ),
            ("[plot_inks]\n'VWAP.vwap'='#123456'", "document"),
        ] {
            let error =
                ResolvedTheme::parse(&format!("version=1\nname='Bad'\n{table}")).expect_err(table);
            assert_eq!(error.field, field, "{table}: {error}");
        }
    }
}
