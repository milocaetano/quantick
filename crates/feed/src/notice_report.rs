//! Feed recovery reports and popup transitions, independent of a renderer.
use crate::{
    FeedNotice,
    stall::{Recovery, Stall},
};

/// What the person watching asked of the popup this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeAction {
    /// Nothing was clicked.
    #[default]
    None,
    /// Respawn the transport and keep the timeline.
    Reconnect,
    /// Throw the timeline away and rebuild it.
    Reload,
}

impl From<Recovery> for NoticeAction {
    fn from(recovery: Recovery) -> Self {
        match recovery {
            Recovery::Reconnect => Self::Reconnect,
            Recovery::Reload => Self::Reload,
        }
    }
}

/// How much of the interface a report is entitled to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something is under way and nothing is wrong yet: a first connection, a
    /// history block still arriving. The wait has surfaces of its own.
    Progress,
    /// The chart is not being fed, whatever the socket believes: a transport
    /// that never landed, one that dropped, one that stayed open while the thing
    /// behind it went quiet, or a reason the provider named itself.
    Offline,
}

/// What the interface has to tell the trader about one feed this frame —
/// however it was arrived at.
///
/// Both sources of words end up here: the feed's own [`FeedNotice`], and the
/// [`Stall`] this application infers when a notice stops being progress, so
/// there is one set of words and one pair of buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report<'a> {
    /// One line: what is happening, or what was observed.
    pub headline: &'a str,
    /// The single next step, when there is one to give.
    pub next_step: Option<&'a str>,
    /// This needs a person, it is not just quiet.
    pub needs_user: bool,
    /// Whether this is worth a corner at all.
    pub severity: Severity,
    /// The control offered first. The other is always beside it.
    pub primary: Recovery,
}

impl Report<'_> {
    /// Whether the corner should carry this report.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        matches!(self.severity, Severity::Offline)
    }
}

/// Build the report for a feed, or `None` when there is nothing to say.
///
/// `stall` is this application's own judgement, already decided against the
/// clock by [`crate::stall::assess`]; it wins over a progress notice,
/// because a progress notice that has been escalated is exactly the one whose
/// words stopped being true.
#[must_use]
pub fn report<'a>(notice: &'a FeedNotice, stall: Option<&'a Stall>) -> Option<Report<'a>> {
    // The provider named a specific reason. Nothing inferred improves on it,
    // and `assess` already stands down in this case.
    if let FeedNotice::Attention {
        headline,
        next_step,
    } = notice
    {
        return Some(Report {
            headline,
            next_step: Some(next_step),
            needs_user: true,
            severity: Severity::Offline,
            primary: Recovery::Reconnect,
        });
    }
    if let Some(stall) = stall {
        return Some(Report {
            headline: &stall.headline,
            next_step: Some(&stall.next_step),
            needs_user: stall.needs_attention,
            severity: Severity::Offline,
            primary: stall.primary,
        });
    }
    match notice {
        FeedNotice::Connected | FeedNotice::Clear | FeedNotice::Attention { .. } => None,
        FeedNotice::Reconnecting { headline } | FeedNotice::Working { headline } => Some(Report {
            headline,
            next_step: None,
            needs_user: false,
            severity: Severity::Progress,
            primary: Recovery::Reconnect,
        }),
    }
}

/// Whether the recovery popup is still up after a frame.
///
/// The status word is its only door, in both directions. Everything else here
/// closes it: a click that landed somewhere else, a control that was pressed and
/// is now under way, and a feed that recovered on its own while the trader was
/// reading about it. With no door there is nothing to draw, in every combination.
#[must_use]
pub fn popup_still_open(
    was_open: bool,
    chip_clicked: bool,
    offline: bool,
    dismissed: bool,
    action: NoticeAction,
) -> bool {
    if !offline {
        return false;
    }
    if chip_clicked {
        return !was_open;
    }
    was_open && !dismissed && action == NoticeAction::None
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The report for a bare notice, with nothing inferred on top.
    fn plain(notice: &FeedNotice) -> Option<Report<'_>> {
        report(notice, None)
    }

    /// A feed with nothing of its own to say, so the stall is the whole story.
    const CLEAR: FeedNotice = FeedNotice::Clear;

    /// The report an inferred stall produces on such a feed.
    fn from_stall(stall: &Stall) -> Report<'_> {
        report(&CLEAR, Some(stall)).expect("a stall speaks")
    }

    /// The stall the tests below escalate into.
    fn silent_stall() -> Stall {
        Stall {
            headline: "MetaTrader 5 — B3 has sent nothing for 4 min".to_owned(),
            next_step: "If the market is open, check the terminal.".to_owned(),
            primary: Recovery::Reload,
            needs_attention: false,
        }
    }

    #[test]
    fn a_clear_notice_says_nothing() {
        assert!(plain(&FeedNotice::Clear).is_none());
        assert!(plain(&FeedNotice::Connected).is_none());
    }
    #[test]
    fn progress_never_reaches_the_corner() {
        let working = FeedNotice::working("starting the MetaTrader bridge");
        let report = plain(&working).expect("progress is worth saying");
        assert_eq!(
            report.severity,
            Severity::Progress,
            "a first connection is not an outage"
        );
        assert!(
            !report.is_offline(),
            "the loading overlay owns a wait; the corner would say it twice"
        );

        let reconnecting = FeedNotice::reconnecting("Binance disconnected — reconnecting");
        let report = plain(&reconnecting).expect("a reconnect is worth saying");
        assert!(!report.is_offline());
    }
    #[test]
    fn every_fault_lands_in_the_corner() {
        let attention = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        let named = plain(&attention).expect("a named reason is always worth saying");
        assert!(
            named.is_offline(),
            "the reason the provider named is an outage like any other"
        );
        assert!(named.needs_user, "and it needs a person");

        let silent = silent_stall();
        let inferred = from_stall(&silent);
        assert!(
            inferred.is_offline(),
            "so is a tape that stopped without anyone saying why"
        );
    }
    #[test]
    fn a_stall_wins_over_the_progress_it_escalated() {
        let notice = FeedNotice::working("connecting to MetaTrader 5");
        let stall = Stall {
            headline: "MetaTrader 5 — B3 has not connected in 30 s".to_owned(),
            next_step: "Check that MetaTrader 5 is running.".to_owned(),
            primary: Recovery::Reconnect,
            needs_attention: true,
        };
        let report = report(&notice, Some(&stall)).expect("a stall is worth saying");
        assert_eq!(report.headline, stall.headline);
        assert_eq!(report.next_step, Some(stall.next_step.as_str()));
        assert!(
            report.is_offline(),
            "an escalated progress notice is no longer progress"
        );
    }
    #[test]
    fn a_quiet_tape_gets_controls_and_not_an_alarm() {
        let stall = silent_stall();
        let report = from_stall(&stall);
        assert!(!report.needs_user);
        assert_eq!(report.primary, Recovery::Reload);
    }
    #[test]
    fn every_way_the_popup_closes_and_the_one_way_it_opens() {
        let shut = |chip, offline, dismissed, action| {
            popup_still_open(false, chip, offline, dismissed, action)
        };
        let open = |chip, offline, dismissed, action| {
            popup_still_open(true, chip, offline, dismissed, action)
        };

        assert!(
            shut(true, true, false, NoticeAction::None),
            "the chip opens it"
        );
        assert!(
            !shut(false, true, false, NoticeAction::None),
            "and nothing else does"
        );
        assert!(
            !open(true, true, false, NoticeAction::None),
            "the chip closes it again"
        );
        assert!(
            open(false, true, false, NoticeAction::None),
            "an untouched frame leaves it up"
        );
        assert!(
            !open(false, true, true, NoticeAction::None),
            "a click somewhere else puts it away"
        );
        assert!(
            !open(false, false, false, NoticeAction::None),
            "so does a feed that came back on its own"
        );
        for action in [NoticeAction::Reconnect, NoticeAction::Reload] {
            assert!(
                !open(false, true, false, action),
                "a control that was pressed is under way, not still being read: {action:?}"
            );
        }
        // No chip, no popup, whatever else is true on the frame. A click on a
        // chip that was never drawn cannot reach this today; the rule holds it
        // anyway rather than resting on that.
        assert!(!shut(true, false, false, NoticeAction::None));
        assert!(!open(true, false, false, NoticeAction::None));
    }
}
