//! The asks a frame settles after every surface has drawn.
use crate::HostEffect;
use quantick_workspace::market_list::PaletteIntent;

/// The market palette's first ask and one undo, however many surfaces asked.
///
/// Two palette asks in one frame cannot both be right, so the first in draw
/// order wins. Undo is a flag: each Undo pops a drawing, so several asks in
/// one frame still undo once.
#[derive(Debug, Default, PartialEq)]
pub struct LateEffects {
    pub market: Option<PaletteIntent>,
    pub undo_drawing: bool,
}

impl LateEffects {
    /// Fold the effects in draw order; effects of other surfaces are ignored.
    pub fn fold(effects: impl IntoIterator<Item = HostEffect>) -> Self {
        let mut late = Self::default();
        for effect in effects {
            match effect {
                HostEffect::MarketPalette(asked) => late.market = late.market.or(asked),
                HostEffect::Toast(toast) => late.undo_drawing |= toast.undo_drawing,
                _ => {}
            }
        }
        late
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toast::Effects;

    fn remove(symbol: &str) -> PaletteIntent {
        PaletteIntent::Remove {
            feed_id: "binance".to_owned(),
            symbol: symbol.to_owned(),
        }
    }

    #[test]
    fn the_first_palette_ask_wins_and_undo_folds_to_one() {
        let late = LateEffects::fold([
            HostEffect::MarketPalette(None),
            HostEffect::Toast(Effects { undo_drawing: true }),
            HostEffect::MarketPalette(Some(remove("FIRST"))),
            HostEffect::Toast(Effects { undo_drawing: true }),
            HostEffect::MarketPalette(Some(remove("SECOND"))),
        ]);
        assert_eq!(
            late,
            LateEffects {
                market: Some(remove("FIRST")),
                undo_drawing: true,
            }
        );
    }

    #[test]
    fn a_quiet_toast_or_another_surface_asks_nothing() {
        let late = LateEffects::fold([
            HostEffect::Toast(Effects {
                undo_drawing: false,
            }),
            HostEffect::BarSwitch(None),
        ]);
        assert_eq!(late, LateEffects::default());
    }
}
