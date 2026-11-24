//! Cached tab identities and intent validation. The arrangement remains authoritative.
use quantick_appearance::tab_strip::FeedMark;
use quantick_session::tab_chip::FaceSource;
use quantick_workspace::arrangement::strip::{StripSurface, TabFeed, chip_parts};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
    New,
    Home,
}
pub use TabAction as Action;

/// One tab as the strip paints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub id: u64,
    /// `SYMBOL · venue`, composed by the tab when its market last changed.
    pub label: String,
    /// The feed's mark: none while healthy and live.
    pub feed: TabFeed,
    /// The workspace, for the hover, and the venue's code (`BIN`) for its tag.
    pub workspace: String,
    pub venue_code: String,
    /// The open tab's symbol in its workspace's colour, where that reads on
    /// the tab (`quantick_stores::workspaces::symbol_ink`); `None` keeps the
    /// tab's normal ink.
    pub symbol_ink: Option<[u8; 3]>,
    /// The venue tag filled in the route's colour while this tab's orders go
    /// to (or are open at) a broker account; `None` keeps the outline.
    pub tag_fill: Option<[u8; 3]>,
    /// The workspace's colour for the theme, painted in every tab's top-left
    /// corner, open or closed; `None` only before the book has settled one.
    pub corner: Option<[u8; 3]>,
}
impl Chip {
    /// What the tab's face is measured from at `scale` pixels per point.
    #[must_use]
    pub fn face_source(&self, scale: f32) -> FaceSource<'_> {
        FaceSource {
            symbol: chip_parts(&self.label).0,
            code: &self.venue_code,
            marked: self.feed.marked(),
            word: self.feed.word(),
            scale,
        }
    }
}

/// The workspace's feed state as the theme's mark.
#[must_use]
pub const fn feed_mark(feed: TabFeed) -> FeedMark {
    match feed {
        TabFeed::Live => FeedMark::Live,
        TabFeed::Attention => FeedMark::Attention,
        TabFeed::Replay => FeedMark::Replay,
        TabFeed::Disconnected => FeedMark::Lost,
        TabFeed::Stopped => FeedMark::Stopped,
    }
}

/// The scene's words for the colour a tab painted in its corner.
#[must_use]
pub fn corner_description(rgb: [u8; 3]) -> String {
    format!("Workspace corner {}", quantick_stores::workspaces::hex(rgb))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub chips: Vec<Chip>,
    pub active: usize,
    pub surface: StripSurface,
    pub allow_close_last: bool,
}
#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
}
pub enum Intent {
    Refresh(Snapshot),
    Action(Action),
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects(pub Option<Action>);
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Refresh(snapshot) => {
                if state.snapshot.as_ref() != Some(&snapshot) {
                    state.snapshot = Some(snapshot);
                }
                Effects::default()
            }
            Intent::Action(action) => Effects(
                state
                    .snapshot
                    .as_ref()
                    .filter(|s| match action {
                        Action::Activate(index) => index < s.chips.len(),
                        Action::Close(index) => {
                            index < s.chips.len() && (s.chips.len() > 1 || s.allow_close_last)
                        }
                        Action::Home | Action::New => true,
                    })
                    .map(|_| action),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;

    /// The corner's words name the colour as the workspace menu writes it.
    #[test]
    fn the_corner_description_names_the_colour_in_hex() {
        assert_eq!(
            corner_description([0x3A, 0x7B, 0xD5]),
            "Workspace corner #3A7BD5"
        );
    }
    #[test]
    fn tab_actions_reject_stale_indices_and_last_tab_close() {
        let mut state = State::default();
        let snapshot = Snapshot {
            chips: vec![Chip {
                id: 7,
                label: "BTC".into(),
                feed: TabFeed::Live,
                workspace: "Default".into(),
                venue_code: "BIN".into(),
                symbol_ink: None,
                tag_fill: None,
                corner: Some([163, 179, 204]),
            }],
            active: 0,
            surface: StripSurface::default(),
            allow_close_last: false,
        };
        Model::update(&mut state, Intent::Refresh(snapshot.clone()));
        for action in [Action::Activate(1), Action::Close(1), Action::Close(0)] {
            assert_eq!(
                Model::update(&mut state, Intent::Action(action)),
                Effects(None)
            );
        }
        for action in [Action::Activate(0), Action::Home, Action::New] {
            assert_eq!(
                Model::update(&mut state, Intent::Action(action)),
                Effects(Some(action))
            );
        }
        Model::update(
            &mut state,
            Intent::Refresh(Snapshot {
                allow_close_last: true,
                ..snapshot
            }),
        );
        assert_eq!(
            Model::update(&mut state, Intent::Action(Action::Close(0))),
            Effects(Some(Action::Close(0)))
        );
    }
}
