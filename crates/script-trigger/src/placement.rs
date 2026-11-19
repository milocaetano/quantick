//! Where a script instance may ring: any placement the kernel knows.

use quantick_engine::{Bar, Side};
use quantick_strategy::{Anywhere, FixedRange, LineCross, Placement, Reach, Region};

/// The placement an instance was armed with. Only a drawn region needs the
/// chart to resolve it — per bar, since a rectangle has a time span — so it
/// carries the drawing's key; the rest are fixed and always active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPlacement<D> {
    /// The rectangle drawing `D` marks.
    Region(D),
    /// A fixed price band, both edges inside; tested as a [`FixedRange`].
    Range(Region),
    /// A horizontal line the body must cross one way.
    Line(LineCross),
    /// No price condition.
    Anywhere,
}

/// A placement resolved for one bar, as the kernel tests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedPlacement {
    Region(Region),
    Range(FixedRange),
    Line(LineCross),
    Anywhere,
}

impl<D: Copy> ScriptPlacement<D> {
    /// The drawing a region placement rides, if this is one.
    #[must_use]
    pub fn drawing(&self) -> Option<D> {
        match self {
            Self::Region(drawing) => Some(*drawing),
            Self::Range(_) | Self::Line(_) | Self::Anywhere => None,
        }
    }

    /// This placement for one bar, and whether it is active there: a region
    /// through `region` (`None` when its drawing cannot be read there), the
    /// others as they are, always active.
    pub fn resolve(
        &self,
        region: impl FnOnce(D) -> Option<(Region, bool)>,
    ) -> Option<(ResolvedPlacement, bool)> {
        match *self {
            Self::Region(drawing) => {
                region(drawing).map(|(band, active)| (ResolvedPlacement::Region(band), active))
            }
            Self::Range(band) => Some((
                ResolvedPlacement::Range(FixedRange::new(band.low(), band.high())),
                true,
            )),
            Self::Line(line) => Some((ResolvedPlacement::Line(line), true)),
            Self::Anywhere => Some((ResolvedPlacement::Anywhere, true)),
        }
    }
}

impl Placement for ResolvedPlacement {
    fn reach(&self, side: Side, bar: &Bar) -> Reach {
        match self {
            Self::Region(region) => region.reach(side, bar),
            Self::Range(range) => range.reach(side, bar),
            Self::Line(line) => line.reach(side, bar),
            Self::Anywhere => Anywhere.reach(side, bar),
        }
    }
}
