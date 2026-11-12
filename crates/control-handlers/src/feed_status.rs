//! The feed's status chip and the recovery popup it opens, as the scene names
//! them: which controls are on screen, in the order they are painted, and the
//! capability each one calls.
//!
//! Beside both families it reads rather than inside either: the recovery pair
//! and the contract roll know nothing of the chip, so neither module reaches
//! back into the other through it.

use quantick_control_schema::contract_roll::CONTRACT_ROLL_CAPABILITY_ID;
use quantick_control_schema::scene::{
    FEED_RECONNECT_CONTROL_ID, FEED_RELOAD_CONTROL_ID, FEED_ROLL_CONTROL_ID,
};
use quantick_sources::recovery::{Recovery, roll_label, roll_refused_sentence};

use crate::contract_roll::RollRefusal;
use crate::recovery::capability_id;

/// A control on the feed's status chip or in the popup it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedStatusControl {
    /// The roll off a contract the venue no longer lists.
    Roll,
    Recover(Recovery),
}

impl FeedStatusControl {
    /// The scene's stable name for it.
    #[must_use]
    pub const fn control_id(self) -> &'static str {
        match self {
            Self::Roll => FEED_ROLL_CONTROL_ID,
            Self::Recover(Recovery::Reconnect) => FEED_RECONNECT_CONTROL_ID,
            Self::Recover(Recovery::Reload) => FEED_RELOAD_CONTROL_ID,
        }
    }

    /// The capability a press calls.
    #[must_use]
    pub const fn capability_id(self) -> &'static str {
        match self {
            Self::Roll => CONTRACT_ROLL_CAPABILITY_ID,
            Self::Recover(recovery) => capability_id(recovery),
        }
    }
}

/// What the feed's status chip and the popup it opens show this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedStatusView<'a> {
    /// The roll off the contract the venue no longer lists: the target on
    /// offer, or the reason it is refused. `None` while there is none.
    pub roll: Option<Result<&'a str, &'a str>>,
    /// The recovery the popup fills and paints first for this stall.
    pub primary: Recovery,
    pub popup_open: bool,
    /// A one-click act stands outside the popup: the overlay's or the chip's.
    pub offered: bool,
}

/// One feed-status control as the screen paints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedStatusEntry {
    pub control: FeedStatusControl,
    pub label: String,
    /// What the popup says about it beyond the label: a refused roll's reason.
    pub description: Option<String>,
    /// The precondition a press would fail on, while it is refused.
    pub refused: Option<&'static str>,
}

/// The label of a roll whose target a refusal keeps unnamed.
pub const REFUSED_ROLL_LABEL: &str = "Roll to the next contract";

/// The code of a refused roll whose sentence no [`RollRefusal`] words.
pub const ROLL_REFUSED_CODE: &str = "roll_refused";

impl FeedStatusEntry {
    fn new(control: FeedStatusControl, label: String) -> Self {
        Self {
            control,
            label,
            description: None,
            refused: None,
        }
    }

    /// The roll as the popup shows it: a button to `target`, or the reason
    /// it cannot run, said under the next step.
    fn roll(roll: Result<&str, &str>) -> Self {
        match roll {
            Ok(target) => Self::new(FeedStatusControl::Roll, roll_label(target)),
            Err(reason) => Self {
                description: Some(roll_refused_sentence(reason)),
                refused: Some(
                    RollRefusal::from_reason(reason)
                        .map_or(ROLL_REFUSED_CODE, RollRefusal::precondition),
                ),
                ..Self::new(FeedStatusControl::Roll, REFUSED_ROLL_LABEL.to_owned())
            },
        }
    }

    fn recover(recovery: Recovery) -> Self {
        Self::new(
            FeedStatusControl::Recover(recovery),
            recovery.label().to_owned(),
        )
    }
}

/// The feed-status controls on screen, in painted order.
///
/// Inside the popup: the roll — offered, or refused with its reason — then
/// the stall's primary recovery, then the other. Outside it the one-click act
/// stands alone when `offered`: the roll on offer, else Reconnect, as the chip
/// chooses. So the scene names what the screen shows, in the order it shows it.
#[must_use]
pub fn feed_status_controls(view: FeedStatusView<'_>) -> Vec<FeedStatusEntry> {
    if view.popup_open {
        let pair = [view.primary, view.primary.other()].map(FeedStatusEntry::recover);
        view.roll
            .map(FeedStatusEntry::roll)
            .into_iter()
            .chain(pair)
            .collect()
    } else if view.offered {
        vec![match view.roll {
            Some(Ok(target)) => FeedStatusEntry::roll(Ok(target)),
            _ => FeedStatusEntry::recover(Recovery::Reconnect),
        }]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "feed_status_tests.rs"]
mod feed_status_tests;
