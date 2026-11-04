//! What a flow-pane side says about its data: how far it reaches and, on the tape,
//! whether more is coming. Each side says it in its header plate's tooltip; nothing
//! is printed inside the chart.
//!
//! The clock is the market's, told by the caller, and the status is the word
//! [`crate::connection_state::Link`] speaks, so a blip the other surfaces keep
//! quiet about stays quiet here too. The window only shows the result.

use quantick_civil::TzOffset;

use crate::connection_state::Link;
use crate::geometry::{TimeLabelFormat, time_label};
use crate::market_fold::last_print_ms;
use crate::pane_header::CapsuleSide;
use crate::state::ChartState;

/// The padding across the plates the tape's seam wears.
pub const SIDE_LABEL_PADDING_PX: f32 = 7.0;

const SEPARATOR: &str = " · ";

/// The tape's status word, for a [`Link`] that has one. A source with no
/// transport has none, since nothing could be up or down.
#[must_use]
pub fn status_of(link: Link) -> Option<&'static str> {
    match link {
        Link::Live => Some("live"),
        Link::Quiet => Some("quiet"),
        Link::Attempting => Some("reconnecting"),
        Link::Offline => Some("offline"),
        Link::NoTransport => None,
    }
}

/// One side's label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideLabel {
    clock: String,
    status: Option<&'static str>,
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

    /// The label as one string: the clock, then the status word.
    #[must_use]
    pub fn text(&self) -> String {
        match self.status {
            Some(status) => format!("{}{SEPARATOR}{status}", self.clock),
            None => self.clock.clone(),
        }
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
        assert_eq!(SideLabel::trail(AT, UTC).text(), "bars to 01:02:03");
        assert_eq!(
            SideLabel::trail(AT, TzOffset::new(-180)).text(),
            "bars to 22:02:03",
            "in the chart's own timezone"
        );
    }

    /// Each `Link` has one word, and a source with no transport has none: a
    /// replay says nothing about a connection it does not have.
    #[test]
    fn the_tape_label_says_what_the_link_says() {
        for (link, text) in [
            (Link::Live, "prints to 01:02:03 · live"),
            (Link::Quiet, "prints to 01:02:03 · quiet"),
            (Link::Attempting, "prints to 01:02:03 · reconnecting"),
            (Link::Offline, "prints to 01:02:03 · offline"),
            (Link::NoTransport, "prints to 01:02:03"),
        ] {
            assert_eq!(SideLabel::tape(AT, UTC, Some(link)).text(), text);
        }
    }

    /// A session opening speaks no link yet, and a tape with nothing on it says
    /// so rather than inventing a time.
    #[test]
    fn the_label_invents_nothing() {
        assert_eq!(SideLabel::tape(AT, UTC, None).text(), "prints to 01:02:03");
        assert_eq!(
            SideLabel::tape(None, UTC, Some(Link::Quiet)).text(),
            "no prints · quiet"
        );
        assert_eq!(SideLabel::trail(None, UTC).text(), "no bars");
    }
}
