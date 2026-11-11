//! Bubble colours after the panel's overrides are laid over the theme.
//!
//! Generic over the painter's colour type, as [`Palette`] is: the decision
//! is headless, the colour type is the painter's.

use quantick_engine::Side;
use quantick_orderbook::BookSide;

use super::BubbleStyle;
use super::crown::CROWN_WHITE_MIX;
use super::dressing::{FLAT_RIM_PX, RIM_ALPHA, lit_halo_width, separator_ring_width};
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
    /// Flat bubbles ringed in these, buy then sell, where the palette says so.
    pub flat_rims: Option<[C; 2]>,
}

/// A dressed bubble's edge: the separator ring outside it and, where the
/// palette rims its bubbles, the flat rim that replaces the look's shading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BubbleEdge<C> {
    /// Width of the separator ring laid just outside the disc.
    pub separator: f32,
    /// The darker rim, [`FLAT_RIM_PX`] wide;
    /// `Some` means the bubble is flat.
    pub flat_rim: Option<C>,
}

/// One ring stroked around a bubble: its radius, width, colour, and the
/// alpha the colour is drawn at.
pub type BubbleRing<C> = (f32, f32, C, f32);

impl<C: Copy> BubbleEdge<C> {
    /// The rings around a disc of `radius`, in paint order: the separator
    /// hair just outside it in `separator`, then the flat rim on its edge at
    /// [`RIM_ALPHA`].
    pub fn rings(self, radius: f32, separator: C) -> impl Iterator<Item = BubbleRing<C>> {
        let hair = (
            radius + self.separator / 2.0,
            self.separator,
            separator,
            1.0,
        );
        let rim = (self.flat_rim).map(|rim| (radius, FLAT_RIM_PX, rim, RIM_ALPHA));
        std::iter::once(hair).chain(rim)
    }
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
            flat_rims: palette.flat_rims,
        }
    }

    /// How a dressed bubble of `radius` whose dominant side is `side` is
    /// edged. A palette that rims its bubbles (the lit flow pane's) draws
    /// them flat, a darker rim of [`FLAT_RIM_PX`]
    /// inside a separator halo of [`lit_halo_width`]; any other keeps the
    /// look's shading and hair.
    pub fn edge(&self, side: Side, radius: f32) -> BubbleEdge<C> {
        match self.flat_rims {
            Some([buy, sell]) => BubbleEdge {
                separator: lit_halo_width(radius),
                flat_rim: Some(match side {
                    Side::Buy => buy,
                    Side::Sell => sell,
                }),
            },
            None => BubbleEdge {
                separator: separator_ring_width(radius),
                flat_rim: None,
            },
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

#[cfg(test)]
mod edge_tests {
    use super::*;

    fn colors(flat_rims: Option<[u8; 2]>) -> BubbleColors<u8> {
        BubbleColors {
            buy: 1,
            sell: 2,
            front: 0,
            trail: 0,
            text: 0,
            crown_backing: 0,
            separator: 0,
            crown_buy: 0,
            crown_sell: 0,
            flat_rims,
        }
    }

    /// A rimmed palette edges each side in its own rim inside the wider lit
    /// halo; any other keeps the separator hair and no rim.
    #[test]
    fn a_rimmed_palette_edges_flat_and_any_other_keeps_the_hair() {
        let rimmed = colors(Some([10, 20]));
        let buy = rimmed.edge(Side::Buy, 12.0);
        assert_eq!(buy.flat_rim, Some(10));
        assert_eq!(buy.separator, lit_halo_width(12.0));
        assert_eq!(rimmed.edge(Side::Sell, 12.0).flat_rim, Some(20));
        let plain = colors(None).edge(Side::Buy, 12.0);
        assert_eq!(plain.flat_rim, None);
        assert_eq!(plain.separator, separator_ring_width(12.0));
        let hair = separator_ring_width(12.0);
        let rings: Vec<_> = plain.rings(12.0, 7).collect();
        assert_eq!(rings, [(12.0 + hair / 2.0, hair, 7, 1.0)]);
        let lit: Vec<_> = buy.rings(12.0, 7).collect();
        let halo = lit_halo_width(12.0);
        let rim = (12.0, FLAT_RIM_PX, 10, RIM_ALPHA);
        assert_eq!(lit, [(12.0 + halo / 2.0, halo, 7, 1.0), rim]);
    }
}
