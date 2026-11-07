//! Bubble colours after the panel's overrides are laid over the theme.
//!
//! Generic over the painter's colour type, as [`Palette`] is: the decision
//! is headless, the colour type is the painter's.

use quantick_engine::Side;
use quantick_orderbook::BookSide;

use super::BubbleStyle;
use super::crown::CROWN_WHITE_MIX;
use super::theme::{Palette, mix_rgb};

/// Resolved per draw call and never written back into [`Palette`]: the same
/// theme colours keep driving the liquidity-response layer, which the bubble
/// panel does not own.
#[derive(Debug, Clone, Copy)]
pub struct BubbleColors<C> {
    pub buy: C,
    pub sell: C,
    pub front: C,
    pub trail: C,
    pub text: C,
    /// The crown's colour per side. Derived from the side colour rather than
    /// from [`front`](Self::front), so consumption reads as the same event
    /// hotter instead of introducing a third hue — unless the panel explicitly
    /// overrode the consumption colour, which stays the one door to change it.
    pub crown_backing: C,
    pub separator: C,
    pub crown_buy: C,
    pub crown_sell: C,
}

impl<C: Copy> BubbleColors<C> {
    /// The panel's overrides over `palette`; `opaque` builds an opaque colour
    /// from RGB and `rgb` reads one back.
    pub fn resolve(
        palette: &Palette<C>,
        bubbles: &BubbleStyle,
        opaque: impl Fn([u8; 3]) -> C,
        rgb: impl Fn(C) -> [u8; 3],
    ) -> Self {
        let front = bubbles.front_color.map_or(palette.consumption, &opaque);
        let buy = bubbles.buy_color.map_or(palette.buy, &opaque);
        let sell = bubbles.sell_color.map_or(palette.sell, &opaque);
        let crown_of = |side: C| match bubbles.front_color {
            Some(color) => opaque(color),
            None => opaque(mix_rgb(rgb(side), [255, 255, 255], CROWN_WHITE_MIX)),
        };
        Self {
            buy,
            sell,
            front,
            // The trail is the front's own glow, so it follows it by default.
            trail: bubbles.trail_color.map_or(front, &opaque),
            text: bubbles.label_color.map_or(palette.bubble_text, &opaque),
            crown_backing: palette.crown_backing,
            separator: palette.separator,
            crown_buy: crown_of(buy),
            crown_sell: crown_of(sell),
        }
    }

    /// The fill of a print on `side`.
    pub fn for_side(self, side: Side) -> C {
        match side {
            Side::Buy => self.buy,
            Side::Sell => self.sell,
        }
    }

    /// The colour of the resting `side`: a bid in the buy colour, an ask in
    /// the sell colour, as the prints that would hit them.
    pub fn for_resting(self, side: BookSide) -> C {
        match side {
            BookSide::Bid => self.buy,
            BookSide::Ask => self.sell,
        }
    }

    /// The crown of a print on `side`.
    pub fn crown_for_side(self, side: Side) -> C {
        match side {
            Side::Buy => self.crown_buy,
            Side::Sell => self.crown_sell,
        }
    }
}
