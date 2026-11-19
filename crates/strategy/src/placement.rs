//! Where a setup may trade: the placement port.
//!
//! The trigger answers *when*; a placement answers *where*. Given a trigger
//! bar and the side an instance hunts, a placement says whether that bar's
//! body reached the place the human chose, and how the entry goes in. The
//! alarm and the order both read it through the same two gate functions in
//! `armed`, so a new placement cannot make them disagree.
//!
//! Every placement reads the bar's **body** — open to close, wicks ignored —
//! for the same reason the rectangle does ([`BodyCut`]): a shadow is the
//! level being probed and refused. Time validity stays with the caller, as
//! the `region_active` flag the kernel's entry points take; a placement is
//! price geometry only.

use quantick_engine::{Bar, Side};
use rust_decimal::Decimal;

use crate::region::{BodyCut, Region};

/// The verdict a placement gives one trigger bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The body finished where this placement enters at market.
    Market,
    /// The body crossed `edge` and finished beyond it: a retest limit may
    /// rest on `edge` when the instance's break policy allows one.
    /// `retest_off` is the refusal the badge shows when it does not.
    Cut {
        edge: Decimal,
        retest_off: &'static str,
    },
    /// The body did not reach this placement; the reason names it.
    Missed(&'static str),
}

/// Where an instance may trade, as price geometry tested against a trigger
/// bar's body. A new placement docks by implementing this; the state
/// machine never matches on a concrete kind.
pub trait Placement {
    /// How `bar`'s body met this placement, for a trade on `side`.
    fn reach(&self, side: Side, bar: &Bar) -> Reach;
}

/// The rectangle a human drew: the region test exactly as [`Region::body_cut`]
/// answers it, refusals worded as they always were.
impl Placement for Region {
    fn reach(&self, side: Side, bar: &Bar) -> Reach {
        band_reach(self, side, bar, &REGION_WORDS)
    }
}

/// A fixed price band that is not a drawing — set in config, valid on every
/// bar, so its caller passes it active. Same body geometry as a region; its
/// refusals say "range" so the badge names what was configured. Not the
/// drawn price-range tool: a drawing has a time span, this does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedRange(Region);

impl FixedRange {
    /// A band between two prices, in either order; both edges are inside.
    #[must_use]
    pub fn new(a: Decimal, b: Decimal) -> Self {
        Self(Region::new(a, b))
    }
}

impl Placement for FixedRange {
    fn reach(&self, side: Side, bar: &Bar) -> Reach {
        band_reach(&self.0, side, bar, &RANGE_WORDS)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossDirection {
    /// Open at or below the line, close above it.
    Up,
    /// Open at or above the line, close below it.
    Down,
}

/// A line the body must cross in one direction. `level` is the line's price
/// at the bar being judged: a sloped line is resolved per slot by the
/// caller, as a rectangle's time span is.
///
/// The cross itself is the entry, so it fires at market; there is no edge
/// left to retest, whatever the break policy says. The direction belongs
/// to the line, not to the instance's side. With the force trigger a
/// signal's side is its body's direction, so only the side that agrees with
/// the cross can ever fire; a fade needs a trigger that picks its side
/// another way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCross {
    pub level: Decimal,
    pub direction: CrossDirection,
}

impl Placement for LineCross {
    fn reach(&self, _side: Side, bar: &Bar) -> Reach {
        // An open resting on the line counts as its own side, exactly as a
        // region's drawn edge counts as inside; a close on the line has not
        // crossed it.
        let (finished_beyond, opened_short) = match self.direction {
            CrossDirection::Up => (bar.close > self.level, bar.open <= self.level),
            CrossDirection::Down => (bar.close < self.level, bar.open >= self.level),
        };
        match (finished_beyond, opened_short) {
            (true, true) => Reach::Market,
            (true, false) => Reach::Missed("the body never crossed the line"),
            (false, _) => Reach::Missed("did not close beyond the line"),
        }
    }
}

/// No price condition: every qualifying trigger bar enters at market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anywhere;

impl Placement for Anywhere {
    fn reach(&self, _side: Side, _bar: &Bar) -> Reach {
        Reach::Market
    }
}

/// What a bar is judged against when no placement could be resolved: it
/// admits nothing. The inactive gate already refuses first; this keeps the
/// stand-in closed should a caller ever read the geometry on its own.
pub(crate) struct Unresolved;

impl Placement for Unresolved {
    fn reach(&self, _side: Side, _bar: &Bar) -> Reach {
        Reach::Missed("placement not resolved")
    }
}

/// The refusals a band placement names, one set per kind.
struct BandWords {
    retest_off: &'static str,
    never_cut: &'static str,
    closed_away: &'static str,
}

const REGION_WORDS: BandWords = BandWords {
    retest_off: "cut the region, retest option off",
    never_cut: "the body never cut the region",
    closed_away: "closed outside region",
};

const RANGE_WORDS: BandWords = BandWords {
    retest_off: "cut the range, retest option off",
    never_cut: "the body never cut the range",
    closed_away: "closed outside range",
};

fn band_reach(band: &Region, side: Side, bar: &Bar, words: &BandWords) -> Reach {
    match band.body_cut(side, bar) {
        BodyCut::ClosedInside => Reach::Market,
        BodyCut::CutThrough { edge } => Reach::Cut {
            edge,
            retest_off: words.retest_off,
        },
        // Closed past the edge but opened past it too: it travelled beyond a
        // band it never crossed into. Resting a limit on that edge would put
        // an order in a band this bar has no claim on.
        BodyCut::NoCut => Reach::Missed(words.never_cut),
        BodyCut::ClosedAway => Reach::Missed(words.closed_away),
    }
}
