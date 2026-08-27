//! The human half of the setup: a price region the machine never invents.

use quantick_engine::{Bar, Side};
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
    /// The whole bar is taken rather than two prices because the argument
    /// order would otherwise carry the entire meaning of the verdict:
    /// transposing open and close turns a bar that cut nothing into one
    /// that rests a limit order, and two adjacent `Decimal`s cannot be
    /// transposed wrongly if they are never passed apart. The bar's `high`
    /// and `low` are deliberately not read — that is the whole point of
    /// the rule.
    ///
    /// The edge is inclusive on both tests, exactly as [`Self::contains`]
    /// reads it: a price resting on the drawn line is inside the region the
    /// human drew. So a bar opening *on* the low and closing below it did
    /// leave the band, and cuts.
    #[must_use]
    pub fn body_cut(&self, side: Side, bar: &Bar) -> BodyCut {
        if self.contains(bar.close) {
            return BodyCut::ClosedInside;
        }
        // The edge a trade leaves through: a sell exits the band downward
        // at the low, a buy upward at the high. `open_on_region_side` is
        // not "the open was inside the band" — an open far above a sell
        // region is outside it and still on the region's side of the low,
        // which is exactly what makes the bar's travel a crossing.
        let (edge, closed_beyond, open_on_region_side) = match side {
            Side::Sell => (self.low, bar.close < self.low, bar.open >= self.low),
            Side::Buy => (self.high, bar.close > self.high, bar.open <= self.high),
        };
        if !closed_beyond {
            return BodyCut::ClosedAway;
        }
        if open_on_region_side {
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

    /// A bar with no shadow beyond its body: open, close, and nothing else.
    fn body(open: i64, close: i64) -> Bar {
        wicked(open, close, open.max(close), open.min(close))
    }

    /// A bar whose shadows reach past its body — the case the whole rule
    /// exists for.
    fn wicked(open: i64, close: i64, high: i64, low: i64) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: dec(open),
            high: dec(high),
            low: dec(low),
            close: dec(close),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    /// The trader's rule, walked on a sell zone: crossing is open-to-close,
    /// and a body that never crossed the edge is not a cut however far
    /// beyond it the bar finished.
    #[test]
    fn a_sell_only_cuts_the_low_when_the_body_crossed_it() {
        let region = Region::new(dec(100), dec(110));
        // Closed inside — the open is irrelevant, from above or below.
        assert_eq!(
            region.body_cut(Side::Sell, &body(115, 105)),
            BodyCut::ClosedInside
        );
        assert_eq!(
            region.body_cut(Side::Sell, &body(95, 105)),
            BodyCut::ClosedInside
        );
        // Opened above the low (over the band or inside it) and closed
        // below: the body cut the lower edge.
        assert_eq!(
            region.body_cut(Side::Sell, &body(115, 95)),
            BodyCut::CutThrough { edge: dec(100) }
        );
        assert_eq!(
            region.body_cut(Side::Sell, &body(105, 95)),
            BodyCut::CutThrough { edge: dec(100) }
        );
        // The whole body below the low: nothing was cut. This is the bug
        // that put a resting limit under a bar that never touched the band.
        assert_eq!(region.body_cut(Side::Sell, &body(99, 95)), BodyCut::NoCut);
        // Closed above the band: the far side, nothing to sell into.
        assert_eq!(
            region.body_cut(Side::Sell, &body(105, 115)),
            BodyCut::ClosedAway
        );
        assert_eq!(
            region.body_cut(Side::Sell, &body(120, 115)),
            BodyCut::ClosedAway
        );
    }

    /// A buy zone is the same rule mirrored around the high.
    #[test]
    fn a_buy_only_cuts_the_high_when_the_body_crossed_it() {
        let region = Region::new(dec(100), dec(110));
        assert_eq!(
            region.body_cut(Side::Buy, &body(95, 105)),
            BodyCut::ClosedInside
        );
        assert_eq!(
            region.body_cut(Side::Buy, &body(115, 105)),
            BodyCut::ClosedInside
        );
        assert_eq!(
            region.body_cut(Side::Buy, &body(95, 115)),
            BodyCut::CutThrough { edge: dec(110) }
        );
        assert_eq!(
            region.body_cut(Side::Buy, &body(105, 115)),
            BodyCut::CutThrough { edge: dec(110) }
        );
        assert_eq!(region.body_cut(Side::Buy, &body(111, 115)), BodyCut::NoCut);
        assert_eq!(
            region.body_cut(Side::Buy, &body(105, 95)),
            BodyCut::ClosedAway
        );
        assert_eq!(
            region.body_cut(Side::Buy, &body(90, 95)),
            BodyCut::ClosedAway
        );
    }

    /// The sentence the whole rule was written for: a shadow reaching deep
    /// into the band is the level being probed and refused, not cut. The
    /// verdict must not move when only the wicks do.
    #[test]
    fn wicks_never_change_the_verdict() {
        let region = Region::new(dec(100), dec(110));
        // Sell: body wholly under the band, upper shadow well inside it.
        assert_eq!(
            region.body_cut(Side::Sell, &wicked(99, 95, 108, 94)),
            BodyCut::NoCut
        );
        // The same body with no shadow at all answers the same.
        assert_eq!(region.body_cut(Side::Sell, &body(99, 95)), BodyCut::NoCut);
        // Buy: body wholly above the band, lower shadow inside it.
        assert_eq!(
            region.body_cut(Side::Buy, &wicked(111, 115, 116, 102)),
            BodyCut::NoCut
        );
        // And a genuine cut keeps its edge however far the shadows run.
        assert_eq!(
            region.body_cut(Side::Sell, &wicked(105, 95, 130, 70)),
            BodyCut::CutThrough { edge: dec(100) }
        );
    }

    /// The drawn edge belongs to the region on both tests, exactly as
    /// [`Region::contains`] reads it: an open resting on the line opened
    /// inside the band the human drew, so the bar that leaves it cuts.
    #[test]
    fn the_drawn_edge_counts_as_inside_for_the_open_too() {
        let region = Region::new(dec(100), dec(110));
        assert_eq!(
            region.body_cut(Side::Sell, &body(100, 95)),
            BodyCut::CutThrough { edge: dec(100) }
        );
        assert_eq!(
            region.body_cut(Side::Buy, &body(110, 115)),
            BodyCut::CutThrough { edge: dec(110) }
        );
        // A close on the line is inside, so it is a market entry, not a cut.
        assert_eq!(
            region.body_cut(Side::Sell, &body(115, 100)),
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
