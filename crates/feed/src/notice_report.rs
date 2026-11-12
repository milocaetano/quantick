//! Feed recovery reports and popup transitions, independent of a renderer.
use crate::{
    FeedNotice,
    stall::{Recovery, Stall},
};

/// What the person watching asked of the popup this frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NoticeAction {
    /// Nothing was clicked.
    #[default]
    None,
    /// Respawn the transport and keep the timeline.
    Reconnect,
    /// Throw the timeline away and rebuild it.
    Reload,
    /// Roll the tab off a contract the venue no longer lists, onto the one
    /// its feed config names next: the contract roll, drawings carried.
    Roll(String),
}

impl NoticeAction {
    /// The word on the control that asks for this.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Roll(target) => roll_label(target),
            Self::Reload => Recovery::Reload.label().to_owned(),
            Self::None | Self::Reconnect => Recovery::Reconnect.label().to_owned(),
        }
    }

    /// What it costs, on the control's own hover.
    #[must_use]
    pub fn cost(&self) -> &'static str {
        match self {
            Self::Roll(_) => ROLL_CAPTION,
            Self::Reload => Recovery::Reload.cost(),
            Self::None | Self::Reconnect => Recovery::Reconnect.cost(),
        }
    }
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
    /// What a contract roll can do about a contract the venue no longer
    /// lists; `None` leaves the report exactly as it was without one. An
    /// offered roll comes before the pair, which stays whole.
    pub roll: Option<RollNote<'a>>,
}

/// The contract roll a report about an unlisted contract carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollNote<'a> {
    /// The configured next contract: offered first, ahead of the pair.
    To(&'a str),
    /// Why the configured roll cannot run now, said under the next step.
    Refused(&'a str),
}

impl<'a> Report<'a> {
    /// Whether the corner should carry this report.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        matches!(self.severity, Severity::Offline)
    }

    /// The same report, carrying `roll`.
    #[must_use]
    pub fn with_roll(self, roll: Option<RollNote<'a>>) -> Self {
        Self { roll, ..self }
    }

    /// The roll this report offers ahead of Reconnect and Reload, if any.
    #[must_use]
    pub fn roll_target(&self) -> Option<&'a str> {
        match self.roll {
            Some(RollNote::To(target)) => Some(target),
            _ => None,
        }
    }

    /// The next step as the popup words it: a refused roll's reason follows
    /// the provider's own step, which stays word for word.
    #[must_use]
    pub fn next_step_text(&self) -> Option<String> {
        let step = self.next_step?;
        Some(match self.roll {
            Some(RollNote::Refused(reason)) => format!("{step}\n{ROLL_REFUSED_PREFIX}{reason}."),
            _ => step.to_owned(),
        })
    }
}

impl<'a> From<Result<&'a str, &'a str>> for RollNote<'a> {
    /// The roll on offer, or the reason it is refused.
    fn from(roll: Result<&'a str, &'a str>) -> Self {
        match roll {
            Ok(target) => Self::To(target),
            Err(reason) => Self::Refused(reason),
        }
    }
}

/// What [`Recovery::Reload`] costs, said on the popup rather than behind a
/// confirmation dialog.
///
/// A modal between a trader and a stalled chart is one more thing in the way at
/// the worst moment, and this act is recoverable in the sense that matters —
/// the position it closes is journaled, with its reason, by
/// `PaperTrading::on_timeline_reset`. What is not acceptable is the trader
/// learning about it afterwards, which is what happened while the only button
/// on offer said "Try again" and quietly reset the timeline.
pub const RELOAD_CAPTION: &str = "Reload rebuilds the chart from scratch: it closes any open paper position and \
     disarms every strategy.";

/// What a roll off an unlisted contract does, on the hover of the control
/// that runs it.
pub const ROLL_CAPTION: &str = "Roll starts the chart on the next contract: it keeps the workspace and \
     drawings, and disarms the tab's strategies.";

/// What heads a refused roll's reason under the next step.
pub const ROLL_REFUSED_PREFIX: &str = "Cannot roll to the next contract: ";

/// The label of the control that rolls to `target`.
#[must_use]
pub fn roll_label(target: &str) -> String {
    format!("Roll to {target}")
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
        ..
    } = notice
    {
        return Some(Report {
            headline,
            next_step: Some(next_step),
            needs_user: true,
            severity: Severity::Offline,
            primary: Recovery::Reconnect,
            roll: None,
        });
    }
    if let Some(stall) = stall {
        return Some(Report {
            headline: &stall.headline,
            next_step: Some(&stall.next_step),
            needs_user: stall.needs_attention,
            severity: Severity::Offline,
            primary: stall.primary,
            roll: None,
        });
    }
    match notice {
        FeedNotice::Connected
        | FeedNotice::Clear
        | FeedNotice::Attention { .. }
        | FeedNotice::Rolled { .. } => None,
        FeedNotice::Reconnecting { headline } | FeedNotice::Working { headline } => Some(Report {
            headline,
            next_step: None,
            needs_user: false,
            severity: Severity::Progress,
            primary: Recovery::Reconnect,
            roll: None,
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
    fn a_roll_offer_comes_first_and_the_pair_stays_whole() {
        let notice = FeedNotice::attention("MetaTrader does not list WINV26", "Add it.")
            .with_unlisted(Some("WINV26".to_owned()));
        let bare = plain(&notice).expect("an attention speaks");
        assert_eq!(bare.roll, None, "the notice alone offers no roll");
        assert_eq!(bare.next_step_text().as_deref(), Some("Add it."));

        let offered = bare.with_roll(Some(RollNote::To("WINZ26")));
        assert_eq!(offered.roll_target(), Some("WINZ26"));
        assert_eq!(roll_label("WINZ26"), "Roll to WINZ26");
        assert_eq!(offered.next_step_text().as_deref(), Some("Add it."));
        assert_eq!(offered.headline, bare.headline, "today's words stay");
        assert_eq!(
            (offered.primary, offered.primary.other()),
            (Recovery::Reconnect, Recovery::Reload),
            "Reconnect and Reload both stay reachable beside the roll"
        );
        let roll = NoticeAction::Roll("WINZ26".to_owned());
        assert_eq!(roll.label(), "Roll to WINZ26");
        assert_eq!(roll.cost(), ROLL_CAPTION);
        assert_eq!(NoticeAction::Reconnect.label(), "Reconnect");
        assert_eq!(NoticeAction::Reconnect.cost(), Recovery::Reconnect.cost());
        assert_eq!(NoticeAction::Reload.label(), "Reload");

        let refused = bare.with_roll(Some(RollNote::Refused("Finish the drawing gesture first")));
        assert_eq!(
            refused.roll_target(),
            None,
            "a refused roll offers no button"
        );
        assert_eq!(
            refused.next_step_text().as_deref(),
            Some("Add it.\nCannot roll to the next contract: Finish the drawing gesture first.")
        );
        assert_eq!(refused.primary, Recovery::Reconnect);
        assert_eq!(
            RollNote::from(Err("Finish the drawing gesture first")),
            RollNote::Refused("Finish the drawing gesture first")
        );
        assert_eq!(RollNote::from(Ok("WINZ26")), RollNote::To("WINZ26"));
    }

    #[test]
    fn a_roll_note_reports_nothing_to_recover() {
        let note = FeedNotice::Rolled {
            from: "WINV26".to_owned(),
            to: "WINZ26".to_owned(),
        };
        assert_eq!(plain(&note), None, "no alarm, no controls");
        assert_eq!(note.summary(), ("info", true, false));
        assert_eq!(
            note.rolled_text().as_deref(),
            Some("WINV26 is no longer listed; rolled to WINZ26")
        );
    }
    /// The consequence of the destructive control is on the popup, so a trader
    /// learns it before pressing rather than from a journal afterwards.
    #[test]
    fn the_popup_says_what_reload_costs() {
        assert!(
            RELOAD_CAPTION.contains("paper position") && RELOAD_CAPTION.contains("strateg"),
            "the caption has to name both things Reload ends: {RELOAD_CAPTION}"
        );
    }
    /// A roll is a market switch, which disarms the tab's strategies; the
    /// control says so before it is pressed, as Reload's does.
    #[test]
    fn the_roll_says_it_disarms_the_strategies() {
        assert!(
            ROLL_CAPTION.contains("drawings") && ROLL_CAPTION.contains("disarms the tab's strateg"),
            "the caption has to name what Roll keeps and what it ends: {ROLL_CAPTION}"
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
        for action in [
            NoticeAction::Reconnect,
            NoticeAction::Reload,
            NoticeAction::Roll("WINZ26".to_owned()),
        ] {
            assert!(
                !open(false, true, false, action.clone()),
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
