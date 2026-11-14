//! Application-wide recovery popup lifecycle; feed owns reports and recovery.
use quantick_workspace::arrangement::TabId as StableTabId;

#[derive(Debug, Default)]
pub struct State {
    open: Option<StableTabId>,
}
impl State {
    pub fn open(&self) -> Option<u64> {
        self.open.map(|tab| tab.0)
    }
}
#[derive(Debug)]
pub enum Intent {
    InitializeOpen(u64),
    Settle {
        tab: u64,
        chip_clicked: bool,
        offline: bool,
        dismissed: bool,
        action_taken: bool,
    },
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = ();
    fn update(state: &mut State, intent: Intent) {
        match intent {
            Intent::InitializeOpen(tab) => state.open = Some(StableTabId(tab)),
            Intent::Settle {
                tab,
                chip_clicked,
                offline,
                dismissed,
                action_taken,
            } => {
                state.open = popup_still_open(
                    state.open == Some(StableTabId(tab)),
                    chip_clicked,
                    offline,
                    dismissed,
                    action_taken,
                )
                .then_some(StableTabId(tab));
            }
        }
    }
}

/// Whether the recovery popup is still up after a frame.
///
/// The status word is its only door, in both directions. Everything else here
/// closes it: a click that landed somewhere else, a control that was pressed and
/// is now under way, and a feed that recovered on its own while the trader was
/// reading about it. With no door there is nothing to draw, in every combination.
#[must_use]
fn popup_still_open(
    was_open: bool,
    chip_clicked: bool,
    offline: bool,
    dismissed: bool,
    action_taken: bool,
) -> bool {
    if !offline {
        return false;
    }
    if chip_clicked {
        return !was_open;
    }
    was_open && !dismissed && !action_taken
}

#[cfg(test)]
#[path = "feed_notice/tests.rs"]
mod tests;
