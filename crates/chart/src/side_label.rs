//! What a flow-pane side says about its data: how far it reaches and, on the tape,
//! whether more is coming. The tape wears it as a plate inside the chart; the trail
//! says it in its header plate's tooltip.
//!
//! The clock is the market's, told by the caller, and the status is the word
//! [`crate::connection_state::Link`] speaks, so a blip the other surfaces keep
//! quiet about stays quiet here too. The window only paints the result.

use quantick_civil::TzOffset;

use crate::connection_state::Link;
use crate::geometry::{TimeLabelFormat, time_label};
use crate::market_fold::last_print_ms;
use crate::pane_header::CapsuleSide;
use crate::state::ChartState;

/// The room the tape's plate leaves before its side's right end: the capsules
/// dock in the header now, so nothing else shares its line.
pub const SIDE_LABEL_GAP_PX: f32 = 8.0;
/// How far the plate stands off its side's left edge.
pub const SIDE_LABEL_INSET_LEFT_PX: f32 = 8.0;
/// How far the plate stands below the side's header.
pub const SIDE_LABEL_INSET_TOP_PX: f32 = 6.0;
/// The plate's padding across.
pub const SIDE_LABEL_PADDING_PX: f32 = 7.0;
/// The plate's height.
pub const SIDE_LABEL_HEIGHT_PX: f32 = 18.0;

/// How far below the side's header the tape's plate ends: what stands under it
/// starts here. The trail wears no plate inside the chart.
pub const SIDE_LABEL_BOTTOM_PX: f32 = SIDE_LABEL_INSET_TOP_PX + SIDE_LABEL_HEIGHT_PX;
/// Where the tape's Buy/Sell key starts, below the side's plate.
pub const TAPE_KEY_TOP_PX: f32 = SIDE_LABEL_BOTTOM_PX + 4.0;

const SEPARATOR: &str = " · ";

/// How a run of a label's text is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelInk {
    Muted,
    Amber,
    /// Faint, breathing while an attempt runs; still under reduced motion.
    FaintPulse,
    Warn,
}

/// The tape's status word, for a [`Link`] that has one. A source with no
/// transport has none, since nothing could be up or down.
#[must_use]
pub fn status_of(link: Link) -> Option<(&'static str, LabelInk)> {
    match link {
        Link::Live => Some(("live", LabelInk::Muted)),
        Link::Quiet => Some(("quiet", LabelInk::Amber)),
        Link::Attempting => Some(("reconnecting", LabelInk::FaintPulse)),
        Link::Offline => Some(("offline", LabelInk::Warn)),
        Link::NoTransport => None,
    }
}

/// How much of a label a side has room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelFit {
    Whole,
    /// The clock goes first: the status still reads.
    NoClock,
    None,
}

impl LabelFit {
    /// The fit for a label this wide, or this wide without its clock.
    #[must_use]
    pub fn of(whole_px: f32, short_px: f32, room_px: f32) -> Self {
        if whole_px <= room_px {
            Self::Whole
        } else if short_px <= room_px {
            Self::NoClock
        } else {
            Self::None
        }
    }
}

/// One side's label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideLabel {
    clock: String,
    status: Option<(&'static str, LabelInk)>,
}

impl SideLabel {
    /// The label of a flow-pane side, reading how far its data reaches: the
    /// newest bar's close on the trail, the newest print's time on the tape. A
    /// time pane has none.
    #[must_use]
    pub fn of(
        side: CapsuleSide,
        state: &ChartState,
        tz: TzOffset,
        link: Option<Link>,
    ) -> Option<Self> {
        match side {
            CapsuleSide::Candles => {
                let bar = state.partial().or_else(|| state.bars().last());
                Some(Self::trail(bar.map(|bar| bar.close_time), tz))
            }
            CapsuleSide::Tape => Some(Self::tape(last_print_ms(state), tz, link)),
            CapsuleSide::Time => None,
        }
    }

    /// The candles side, its bars reaching `last_ms`.
    #[must_use]
    pub fn trail(last_ms: Option<i64>, tz: TzOffset) -> Self {
        Self {
            clock: clock("bars", last_ms, tz),
            status: None,
        }
    }

    /// The tape, its prints reaching `last_ms`, the feed speaking `link`.
    #[must_use]
    pub fn tape(last_ms: Option<i64>, tz: TzOffset, link: Option<Link>) -> Self {
        Self {
            clock: clock("prints", last_ms, tz),
            status: link.and_then(status_of),
        }
    }

    /// The label's runs at this fit, left to right.
    pub fn runs(&self, fit: LabelFit) -> impl Iterator<Item = (&str, LabelInk)> {
        let whole = fit == LabelFit::Whole;
        let status = self.status.filter(|_| fit != LabelFit::None);
        [
            whole.then_some((self.clock.as_str(), LabelInk::Muted)),
            status
                .filter(|_| whole)
                .map(|_| (SEPARATOR, LabelInk::Muted)),
            status,
        ]
        .into_iter()
        .flatten()
    }

    /// The label as one string, at this fit.
    #[must_use]
    pub fn text(&self, fit: LabelFit) -> String {
        self.runs(fit).map(|(text, _)| text).collect()
    }
}

/// `bars to HH:MM:SS`, or the plain fact that there is nothing yet.
fn clock(what: &str, last_ms: Option<i64>, tz: TzOffset) -> String {
    last_ms.map_or_else(
        || format!("no {what}"),
        |ms| format!("{what} to {}", time_label(ms, tz, TimeLabelFormat::Full)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTC: TzOffset = TzOffset::new(0);
    const AT: Option<i64> = Some(3_723_000);

    #[test]
    fn the_trail_label_says_its_clock() {
        assert_eq!(
            SideLabel::trail(AT, UTC).text(LabelFit::Whole),
            "bars to 01:02:03"
        );
        assert_eq!(
            SideLabel::trail(AT, TzOffset::new(-180)).text(LabelFit::Whole),
            "bars to 22:02:03",
            "in the chart's own timezone"
        );
    }

    /// Each `Link` has one word in one ink, and a source with no transport has
    /// none: a replay says nothing about a connection it does not have.
    #[test]
    fn the_tape_label_says_what_the_link_says() {
        for (link, text, ink) in [
            (
                Link::Live,
                "prints to 01:02:03 · live",
                Some(LabelInk::Muted),
            ),
            (
                Link::Quiet,
                "prints to 01:02:03 · quiet",
                Some(LabelInk::Amber),
            ),
            (
                Link::Attempting,
                "prints to 01:02:03 · reconnecting",
                Some(LabelInk::FaintPulse),
            ),
            (
                Link::Offline,
                "prints to 01:02:03 · offline",
                Some(LabelInk::Warn),
            ),
            (Link::NoTransport, "prints to 01:02:03", None),
        ] {
            let label = SideLabel::tape(AT, UTC, Some(link));
            assert_eq!(label.text(LabelFit::Whole), text);
            assert_eq!(status_of(link).map(|(_, ink)| ink), ink, "{link:?}");
        }
    }

    /// A session opening speaks no link yet, and a tape with nothing on it says
    /// so rather than inventing a time.
    #[test]
    fn the_label_invents_nothing() {
        assert_eq!(
            SideLabel::tape(AT, UTC, None).text(LabelFit::Whole),
            "prints to 01:02:03"
        );
        assert_eq!(
            SideLabel::tape(None, UTC, Some(Link::Quiet)).text(LabelFit::Whole),
            "no prints · quiet"
        );
        assert_eq!(SideLabel::trail(None, UTC).text(LabelFit::Whole), "no bars");
    }

    /// The clock goes before the status, and the status last.
    #[test]
    fn a_narrow_side_drops_the_clock_then_the_label() {
        assert_eq!(LabelFit::of(200.0, 90.0, 200.0), LabelFit::Whole);
        assert_eq!(LabelFit::of(200.0, 90.0, 199.0), LabelFit::NoClock);
        assert_eq!(LabelFit::of(200.0, 90.0, 90.0), LabelFit::NoClock);
        assert_eq!(LabelFit::of(200.0, 90.0, 89.0), LabelFit::None);
        let label = SideLabel::tape(AT, UTC, Some(Link::Offline));
        assert_eq!(label.text(LabelFit::NoClock), "offline");
        assert_eq!(label.text(LabelFit::None), "");
    }
}
