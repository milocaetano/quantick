//! Shared text budgets for footprint level selection and painting.
use super::{GLYPH_EM, LADDER_MIN_FONT_PX, QUANTITY_GLYPHS, RowFloors};
use quantick_appearance::{FontFamily, FontRole, Typography};

#[derive(Debug, Clone, Copy)]
pub struct FootprintText {
    scale: f32,
    glyph_em: f32,
}
impl FootprintText {
    pub fn new(typography: &Typography) -> Self {
        Self {
            scale: typography.scale(FontRole::Footprint),
            // A conservative budget when the configured family has variable digits.
            glyph_em: match typography.get(FontRole::Footprint).family {
                FontFamily::Monospace => GLYPH_EM,
                FontFamily::Proportional => 1.0,
            },
        }
    }
    pub fn width_scale(self) -> f32 {
        self.scale * self.glyph_em / GLYPH_EM
    }
    pub fn row_floors(self, mut floors: RowFloors) -> RowFloors {
        floors.compact *= self.scale;
        floors.detailed *= self.scale;
        floors
    }
    /// Fit the font within both dimensions, with the same legibility floor as LOD.
    pub fn fitted_size(self, row_height: f32, width: f32) -> f32 {
        (row_height - 2.0)
            .min(width / (QUANTITY_GLYPHS * self.glyph_em))
            .clamp(LADDER_MIN_FONT_PX * self.scale, 13.0 * self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_appearance::ResolvedTheme;
    #[test]
    fn original_metrics_are_exact_and_larger_fonts_raise_the_same_floors() {
        let dark = FootprintText::new(&Typography::default());
        let larger =
            ResolvedTheme::parse("version = 1\nname = 'Large'\n[typography.footprint]\nsize = 20")
                .unwrap();
        let large = FootprintText::new(&larger.typography);
        let floors = RowFloors::new(4.0, 12.0);
        assert_eq!(dark.row_floors(floors), floors);
        assert_eq!(dark.width_scale(), 1.0);
        assert_eq!(dark.fitted_size(14.0, 60.0), 12.0);
        assert_eq!(
            large.row_floors(floors),
            RowFloors {
                profile: 4.0,
                compact: 22.0,
                detailed: 24.0
            }
        );
        assert_eq!(large.width_scale(), 2.0);
        assert_eq!(large.fitted_size(28.0, 60.0), 20.0);
    }
    #[test]
    fn proportional_digits_pay_for_their_wider_budget() {
        let theme = ResolvedTheme::parse(
            "version = 1\nname = 'Variable'\n[typography.footprint]\nfamily = 'proportional'",
        )
        .unwrap();
        let text = FootprintText::new(&theme.typography);
        assert!(text.width_scale() > 1.0);
        assert_eq!(text.fitted_size(30.0, 50.0), 10.0);
    }
}
