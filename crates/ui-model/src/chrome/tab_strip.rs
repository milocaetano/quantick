//! Cached tab identities and intent validation. The arrangement remains authoritative.
use quantick_appearance::tab_strip::FeedMark;
use quantick_session::tab_chip::{FaceSource, Squeeze};
use quantick_session::tab_quote::{TabIcon, TabQuote};
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
    /// The asset's icon, and the market's last price and day's change.
    pub icon: TabIcon,
    pub quote: TabQuote,
}

impl Chip {
    /// What this tab's face is measured from at `stage`, `scale` physical
    /// pixels per point.
    #[must_use]
    pub fn face_source(&self, stage: Squeeze, scale: f32) -> FaceSource<'_> {
        FaceSource {
            symbol: chip_parts(&self.label).0,
            code: &self.venue_code,
            marked: self.feed.marked(),
            word: self.feed.word(),
            price: self.quote.price.as_deref(),
            change: self
                .quote
                .change
                .as_ref()
                .map(|change| change.text.as_str()),
            stage,
            scale,
        }
    }

    /// What the scene says of the face beside its label: the price, the
    /// day's change and the stage, each named even when missing.
    #[must_use]
    pub fn scene_words(&self, stage: Squeeze) -> String {
        let price = self.quote.price.as_deref().unwrap_or("none");
        let change = self.quote.change.as_ref();
        let change = change.map_or("no venue reference", |change| change.text.as_str());
        format!("price {price}; day {change}; stage {}", stage.name())
    }
}

/// A tab's icon and quote: `symbol`'s icon on its `feed`, and its newest
/// print `last` priced in the feed's tick and read against the venue's `day`
/// reference.
#[must_use]
pub fn market_face(
    feed: Option<&quantick_stores::config::FeedConfig>,
    symbol: &str,
    last: Option<&quantick_engine::Trade>,
    day: Option<&quantick_sources::day_reference::DayReference>,
) -> (TabIcon, TabQuote) {
    let tick = feed.and_then(|feed| feed.widget_tick(symbol));
    let icon = TabIcon::of(symbol, feed.and_then(|feed| feed.asset_icon(symbol)));
    (icon, quantick_session::tab_quote::quote(last, day, tick))
}

/// The tab's feed state as the theme's mark.
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

    /// The scene names the price, the change and the stage the strip
    /// painted, and says so when one is missing.
    #[test]
    fn the_scene_names_price_change_and_stage() {
        let quote = quantick_session::tab_quote::quote(None, None, None);
        let mut chip = Chip {
            id: 1,
            label: "WINZ26 \u{b7} MetaTrader 5".into(),
            feed: TabFeed::Live,
            workspace: "Default".into(),
            venue_code: "MT5".into(),
            symbol_ink: None,
            tag_fill: None,
            icon: TabIcon::of("WINZ26", None),
            quote,
        };
        assert_eq!(
            chip.scene_words(Squeeze::Stacked),
            "price none; day no venue reference; stage stacked"
        );
        chip.quote.price = Some("172455".into());
        chip.quote.change = Some(quantick_session::tab_quote::format_change(
            "0.41".parse().unwrap(),
        ));
        assert_eq!(
            chip.scene_words(Squeeze::OneLine),
            "price 172455; day +0.41%; stage one_line"
        );
        let source = chip.face_source(Squeeze::NoPrice, 1.5);
        assert_eq!((source.symbol, source.code), ("WINZ26", "MT5"));
        assert_eq!(
            (source.price, source.change),
            (Some("172455"), Some("+0.41%"))
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
                icon: TabIcon::of("BTC", None),
                quote: TabQuote::default(),
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
