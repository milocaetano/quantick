//! The feed's status chip and the recovery popup it opens, as the scene names
//! them: which controls are on screen, in the order they are painted, and the
//! capability each one calls.
//!
//! Beside both families it reads rather than inside either: the recovery pair
//! and the contract roll know nothing of the chip, so neither module reaches
//! back into the other through it.

use quantick_control_host::wire::{available, bounds_not_recorded, unavailable};
use quantick_control_schema::contract_roll::CONTRACT_ROLL_CAPABILITY_ID;
use quantick_control_schema::scene::{
    FEED_CHIP_CONTROL_ID, FEED_RECONNECT_CONTROL_ID, FEED_RELOAD_CONTROL_ID, FEED_ROLL_CONTROL_ID,
    FEED_STATUS_OWNER_ID, SceneBoundsSnapshot, SceneControlSnapshot, SceneOwnerKindDto,
    SceneOwnerSnapshot, SceneRoleDto,
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
    /// The status word, while it is drawn as the popup's door.
    pub chip: Option<FeedChip<'a>>,
    /// Where the tape cell's Reconnect was drawn, as the scene reports it.
    pub reconnect_drawn: Option<&'a SceneBoundsSnapshot>,
}

/// The status word as the scene names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedChip<'a> {
    /// The word the status bar paints, not one assembled here.
    pub word: &'a str,
    /// Where it was painted; `None` for a rectangle no wire number carries.
    pub bounds: Option<&'a SceneBoundsSnapshot>,
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
    /// Where it was drawn; `None` where no rectangle is recorded.
    pub bounds: Option<SceneBoundsSnapshot>,
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
            bounds: None,
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
///
/// Only the tape cell's Reconnect has a recorded rectangle, and only while it
/// stands alone: with the popup closed and no status word beside it.
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
        let entry = match view.roll {
            Some(Ok(target)) => FeedStatusEntry::roll(Ok(target)),
            _ => FeedStatusEntry::recover(Recovery::Reconnect),
        };
        let drawn = view.reconnect_drawn.filter(|_| view.chip.is_none());
        vec![FeedStatusEntry {
            bounds: drawn.cloned(),
            ..entry
        }]
    } else {
        Vec::new()
    }
}

/// The feed status as the scene lists it, under its own owner: the status word
/// while it is drawn, then [`feed_status_controls`].
///
/// The word is a toggle with no capability. Opening the popup is a gesture,
/// not a call: everything behind it is reachable directly, so a capability
/// whose only effect is to show a human something would be a second way to say
/// the same thing.
#[must_use]
pub fn feed_status_scene(view: FeedStatusView<'_>) -> Vec<SceneControlSnapshot> {
    let owner = || SceneOwnerSnapshot {
        kind: SceneOwnerKindDto::FeedStatus,
        id: FEED_STATUS_OWNER_ID.to_owned(),
    };
    let chip = view.chip.map(|chip| SceneControlSnapshot {
        control_id: FEED_CHIP_CONTROL_ID.to_owned(),
        label: chip.word.to_owned(),
        description: None,
        role: SceneRoleDto::Toggle,
        owner: owner(),
        selected: view.popup_open,
        availability: available(),
        bounds_availability: match chip.bounds {
            Some(_) => available(),
            None => unavailable("the_chips_rectangle_is_not_a_reportable_number"),
        },
        bounds: chip.bounds.cloned(),
        capability_id: None,
    });
    let controls = feed_status_controls(view).into_iter().map(|entry| {
        let control = entry.control;
        SceneControlSnapshot {
            control_id: control.control_id().to_owned(),
            label: entry.label,
            description: entry.description,
            role: SceneRoleDto::Action,
            owner: owner(),
            // Which one leads is the application's judgement about *this*
            // stall, and it is a matter of emphasis rather than of state:
            // neither control is chosen until it is pressed.
            selected: false,
            availability: entry.refused.map_or_else(available, unavailable),
            bounds_availability: match entry.bounds {
                Some(_) => available(),
                None => bounds_not_recorded(),
            },
            bounds: entry.bounds,
            capability_id: Some(control.capability_id().to_owned()),
        }
    });
    chip.into_iter().chain(controls).collect()
}

#[cfg(test)]
#[path = "feed_status_tests.rs"]
mod feed_status_tests;
