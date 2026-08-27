//! The human half of the setup: a price region the machine never invents.

use quantick_engine::Side;
use rust_decimal::Decimal;

/// A price band, drawn by a human on the chart (or fixed by a backtest
/// config). The kernel only ever *tests* against it; deciding where it sits
/// is the judgement this crate exists to leave with the trader.
///
/// Time validity is deliberately not here: whether the drawing still covers
/// "now" is a question about the drawing's anchors, which the caller answers
/// per closed bar (`region_active`). Price containment is the half that is
/// identical for chart and backtest, so it lives in the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    low: Decimal,
    high: Decimal,
}

/// How a trigger bar's *body* met the region, read from the point of view
/// of a trade on one side.
///
/// The bar's wicks are deliberately not consulted. A shadow poking into the
/// band is the market probing the level and being refused; the trader's
/// rule is that crossing means **open to close**, so that is what this
/// answers. The state machine turns the answer into an order (or into a
/// named reason not to place one); the geometry lives here, once, for the
/// chart and the backtest both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyCut {
    /// The close landed inside the band. Whatever the open did — above it,
    /// below it, inside it — the bar finished in the zone the human drew,
    /// and that is the market entry.
    ClosedInside,
    /// The body crossed the edge the trade pushes towards and finished
    /// past it: the open sat on the region's side of `edge`, the close
    /// beyond it. `edge` is the price the tape must come back to for the
    /// retest.
    CutThrough { edge: Decimal },
    /// The close sits past that same edge — but so did the open. The body
    /// travelled entirely beyond the region without ever crossing into it,
    /// so there is no cut and no edge to rest an order on.
    NoCut,
    /// The close sits past the *opposite* edge, the side the trade would be
    /// pushing away from. A sell bar closing above a sell zone cut nothing
    /// downward.
    ClosedAway,
}

impl Region {
    /// Build from any two edge prices; the order is normalised so a
    /// rectangle drawn upward and one drawn downward mean the same band.
    #[must_use]
    pub fn new(a: Decimal, b: Decimal) -> Self {
        Self {
            low: a.min(b),
            high: a.max(b),
        }
    }

    #[must_use]
    pub fn low(&self) -> Decimal {
        self.low
    }

    #[must_use]
    pub fn high(&self) -> Decimal {
        self.high
    }

    /// Inclusive containment: a close sitting exactly on the drawn edge is
    /// inside the region the human drew.
    #[must_use]
    pub fn contains(&self, price: Decimal) -> bool {
        price >= self.low && price <= self.high
    }

    /// Judge one trigger bar's body against the band, for a trade on
    /// `side`. See [`BodyCut`] for what each answer means.
    ///
    /// The edge is inclusive on both tests, exactly as [`Self::contains`]
    /// reads it: a price resting on the drawn line is inside the region the
    /// human drew. So a bar opening *on* the low and closing below it did
    /// leave the band, and cuts.
    #[must_use]
    pub fn body_cut(&self, side: Side, open: Decimal, close: Decimal) -> BodyCut {
        if self.contains(close) {
            return BodyCut::ClosedInside;
        }
        // The edge a trade leaves through: a sell exits the band downward
        // at the low, a buy upward at the high.
        let (edge, closed_beyond, opened_inside) = match side {
            Side::Sell => (self.low, close < self.low, open >= self.low),
            Side::Buy => (self.high, close > self.high, open <= self.high),
        };
        if !closed_beyond {
            return BodyCut::ClosedAway;
        }
        if opened_inside {
            BodyCut::CutThrough { edge }
        } else {
            BodyCut::NoCut
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(v: i64) -> Decimal {
        Decimal::from(v)
    }

    /// The trader's rule, walked on a sell zone: crossing is open-to-close,
    /// and a body that never crossed the edge is not a cut however far
    /// beyond it the bar finished.
    #[test]
    fn a_sell_only_cuts_the_low_when_the_body_crossed_it() {
        let region = Region::new(dec(100), dec(110));
        // Closed inside — the open is irrelevant, from above or below.
        assert_eq!(
            region.body_cut(Side::Sell, dec(115), dec(105)),
            BodyCut::ClosedInside
        );
        assert_eq!(
            region.body_cut(Side::Sell, dec(95), dec(105)),
            BodyCut::ClosedInside
        );
        // Opened above the low (inside the band or over it) and closed
        // below: the body cut the lower edge.
        assert_eq!(
            region.body_cut(Side::Sell, dec(115), dec(95)),
            BodyCut::CutThrough { edge: dec(100) }
        );
        assert_eq!(
            region.body_cut(Side::Sell, dec(105), dec(95)),
            BodyCut::CutThrough { edge: dec(100) }
        );
        // The whole body below the low: nothing was cut. This is the bug
        // that put a resting limit under a bar that never touched the band.
        assert_eq!(
            region.body_cut(Side::Sell, dec(99), dec(95)),
            BodyCut::NoCut
        );
        // Closed above the band: the far side, nothing to sell into.
        assert_eq!(
            region.body_cut(Side::Sell, dec(105), dec(115)),
            BodyCut::ClosedAway
        );
        assert_eq!(
            region.body_cut(Side::Sell, dec(120), dec(115)),
            BodyCut::ClosedAway
        );
    }

    /// A buy zone is the same rule mirrored around the high.
    #[test]
    fn a_buy_only_cuts_the_high_when_the_body_crossed_it() {
        let region = Region::new(dec(100), dec(110));
        assert_eq!(
            region.body_cut(Side::Buy, dec(95), dec(105)),
            BodyCut::ClosedInside
        );
        assert_eq!(
            region.body_cut(Side::Buy, dec(95), dec(115)),
            BodyCut::CutThrough { edge: dec(110) }
        );
        assert_eq!(
            region.body_cut(Side::Buy, dec(105), dec(115)),
            BodyCut::CutThrough { edge: dec(110) }
        );
        assert_eq!(
            region.body_cut(Side::Buy, dec(111), dec(115)),
            BodyCut::NoCut
        );
        assert_eq!(
            region.body_cut(Side::Buy, dec(105), dec(95)),
            BodyCut::ClosedAway
        );
    }

    /// The drawn edge belongs to the region on both tests, exactly as
    /// [`Region::contains`] reads it: an open resting on the line opened
    /// inside the band the human drew, so the bar that leaves it cuts.
    #[test]
    fn the_drawn_edge_counts_as_inside_for_the_open_too() {
        let region = Region::new(dec(100), dec(110));
        assert_eq!(
            region.body_cut(Side::Sell, dec(100), dec(95)),
            BodyCut::CutThrough { edge: dec(100) }
        );
        assert_eq!(
            region.body_cut(Side::Buy, dec(110), dec(115)),
            BodyCut::CutThrough { edge: dec(110) }
        );
        // A close on the line is inside, so it is a market entry, not a cut.
        assert_eq!(
            region.body_cut(Side::Sell, dec(115), dec(100)),
            BodyCut::ClosedInside
        );
    }

    #[test]
    fn edges_are_inside_and_order_does_not_matter() {
        let drawn_down = Region::new(dec(110), dec(100));
        let drawn_up = Region::new(dec(100), dec(110));
        assert_eq!(drawn_down, drawn_up);
        assert!(drawn_up.contains(dec(100)));
        assert!(drawn_up.contains(dec(110)));
        assert!(drawn_up.contains(dec(105)));
        assert!(!drawn_up.contains(dec(99)));
        assert!(!drawn_up.contains(dec(111)));
    }
}
