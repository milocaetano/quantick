//! A theme's ink for an indicator plot nobody restyled, authored as
//! `[plot_ink."<indicator title>"]` with `<plot title> = colour` rows. The
//! table belongs to the surface that authors it: a derived surface (the
//! order-flow pane, the title strip) never inherits it, so a plot there keeps
//! the colour its script declared.
use std::collections::BTreeMap;

use crate::{ColorValue, PremultipliedRgba, ResolvedTheme, ThemeError};

/// Authored rows: indicator title, then plot title, to a colour.
pub(crate) type PlotInkDocument = BTreeMap<String, BTreeMap<String, ColorValue>>;
/// Resolved rows: indicator title, then plot title, to an opaque ink.
pub(crate) type PlotInks = BTreeMap<String, BTreeMap<String, PremultipliedRgba>>;

pub(crate) fn resolve(authored: &PlotInkDocument) -> Result<PlotInks, ThemeError> {
    let named = |field: &str, name: &str| {
        if name.trim().is_empty() || name.chars().any(char::is_control) {
            Err(ThemeError::new(
                field,
                "titles must be non-empty without control characters",
            ))
        } else {
            Ok(name.to_owned())
        }
    };
    let mut inks = PlotInks::new();
    for (indicator, plots) in authored {
        let field = format!("plot_ink.{indicator}");
        let rows = inks.entry(named(&field, indicator)?).or_default();
        for (plot, value) in plots {
            let field = format!("{field}.{plot}");
            let ink = value.resolve(&field)?;
            if ink.to_array()[3] != 255 {
                return Err(ThemeError::new(
                    field,
                    "must be opaque: indicator plot styles use RGB",
                ));
            }
            rows.insert(named(&field, plot)?, ink);
        }
    }
    Ok(inks)
}

impl ResolvedTheme {
    /// The ink this surface gives `plot` of `indicator` when the trader set no
    /// colour; `None` keeps the colour the indicator declared. Opaque, so its
    /// bytes are straight RGB too.
    pub fn plot_ink(&self, indicator: &str, plot: &str) -> Option<PremultipliedRgba> {
        self.plot_ink.get(indicator)?.get(plot).copied()
    }

    /// Whether this surface authors any plot ink: a derived surface that would
    /// otherwise be this theme itself must then be resolved apart, ink-free.
    pub(crate) fn has_plot_ink(&self) -> bool {
        !self.plot_ink.is_empty()
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
    fn an_authored_ink_without_surface_sections_stays_off_the_derived_surfaces() {
        let source = "version=1\nname='Mine'\ndark=false\n\
                      [plot_ink.'VWAP (cumulative)']\nvwap='#0A0B0C'";
        let theme = ResolvedTheme::parse(source).unwrap();
        let ink = theme.plot_ink(VWAP.0, VWAP.1).map(|ink| ink.to_array());
        assert_eq!(ink, Some([10, 11, 12, 255]));
        assert_eq!(theme.bookmap().plot_ink(VWAP.0, VWAP.1), None);
        assert_eq!(theme.title_strip().plot_ink(VWAP.0, VWAP.1), None);
        // Apart from the ink, the derived surfaces paint exactly as the root.
        for surface in [theme.bookmap(), theme.title_strip()] {
            for token in crate::Token::ALL {
                assert_eq!(surface.color(token), theme.color(token), "{}", token.key());
            }
            assert_eq!(surface.candle_fill_opacity, theme.candle_fill_opacity);
            assert_eq!(surface.typography, theme.typography);
            assert_eq!(surface.orderflow, theme.orderflow);
        }
    }

    #[test]
    fn a_document_without_the_table_loads_and_a_surface_may_author_its_own() {
        let bare = ResolvedTheme::parse("version=1\nname='Bare'\ndark=false").unwrap();
        assert_eq!(bare.plot_ink(VWAP.0, VWAP.1), None);
        assert!(std::ptr::eq(bare.bookmap(), &bare));
        let source = "version=1\nname='Own'\n[plot_ink.'A.b']\n'c.d' = '#010203'\n\
                      [bookmap.plot_ink.'VWAP (cumulative)']\nvwap = '#0A0B0C'";
        let theme = ResolvedTheme::parse(source).unwrap();
        // Titles may hold dots: each is its own key, never split.
        let ink = theme.plot_ink("A.b", "c.d").map(|ink| ink.to_array());
        assert_eq!(ink, Some([1, 2, 3, 255]));
        assert_eq!(theme.plot_ink("A", "b.c.d"), None);
        assert_eq!(theme.plot_ink(VWAP.0, VWAP.1), None);
        let flow = theme.bookmap().plot_ink(VWAP.0, VWAP.1);
        assert_eq!(flow.map(|ink| ink.to_array()), Some([10, 11, 12, 255]));
    }

    #[test]
    fn malformed_rows_and_unknown_sections_are_rejected() {
        for (table, field) in [
            ("[plot_ink]\nvwap='#123456'", "document"),
            ("[plot_ink.' ']\nvwap='#123456'", "plot_ink. "),
            ("[plot_ink.VWAP]\n' '='#123456'", "plot_ink.VWAP. "),
            ("[plot_ink.VWAP]\nvwap='nope'", "plot_ink.VWAP.vwap"),
            ("[plot_ink.VWAP]\nvwap='#12345680'", "plot_ink.VWAP.vwap"),
            (
                "[bookmap.plot_ink.VWAP]\nvwap='nope'",
                "bookmap.plot_ink.VWAP.vwap",
            ),
            ("[plot_inks.VWAP]\nvwap='#123456'", "document"),
        ] {
            let error =
                ResolvedTheme::parse(&format!("version=1\nname='Bad'\n{table}")).expect_err(table);
            assert_eq!(error.field, field, "{table}: {error}");
        }
    }
}
