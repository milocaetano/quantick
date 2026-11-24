//! Cached tab identities and intent validation. The arrangement remains authoritative.
use quantick_appearance::tab_strip::FeedMark;
use quantick_session::tab_chip::{FaceCache, FaceLayout, FaceSource, Squeeze};
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
}

/// A tab's icon and quote.
pub type MarketFace = (TabIcon, TabQuote);

/// A tab's icon and quote: `symbol`'s icon on its `feed`, and its newest
/// print `last` priced in the decimals
/// [`price_decimals`](quantick_session::tab_quote::price_decimals) picks and
/// read against the `venue`'s day reference. The chip and the scene both
/// read a tab through this.
#[must_use]
pub fn market_face(
    feed: Option<&quantick_stores::config::FeedConfig>,
    symbol: &str,
    last: Option<&quantick_engine::Trade>,
    venue: &quantick_sources::day_reference::VenueQuote,
) -> MarketFace {
    use quantick_session::tab_quote::{price_decimals, quote};
    let tick = feed.and_then(|feed| feed.widget_tick(symbol));
    let icon = TabIcon::of(symbol, feed.and_then(|feed| feed.asset_icon(symbol)));
    // A tab whose prints all came as history has folded none of them.
    let seen = last.map_or(venue.tape_decimals, |last| {
        venue.tape_decimals_with(last.price)
    });
    let decimals = price_decimals(tick, venue.decimals, seen);
    (icon, quote(last, venue.day.as_ref(), decimals))
}

/// How far the strip squeezes, and each chip's face at that stage, the open
/// chip (`open`) on one line. The count's stage over `room` first
/// ([`Squeeze::of_strip`]); further while the faces, each taking `per_tab`
/// beside its own width, overrun `tabs_room` ([`Squeeze::fitting`]).
/// `width` measures a text at a size, at `scale` pixels per point.
pub fn squeezed_faces(
    chips: &[Chip],
    open: Option<usize>,
    (room, tabs_room, per_tab): (f32, f32, f32),
    scale: f32,
    cache: &mut FaceCache,
    width: impl Fn(&str, f32) -> f32,
) -> (Squeeze, Vec<FaceLayout>) {
    cache.keep(chips.iter().map(|chip| chip.id));
    let mut faces_at = |stage: Squeeze| -> Vec<FaceLayout> {
        let face = |(i, chip): (usize, &Chip)| {
            let source = chip.face_source(stage.for_tab(open == Some(i)), scale);
            cache.layout(chip.id, &source, &width)
        };
        chips.iter().enumerate().map(face).collect()
    };
    let taken = |faces: &[FaceLayout]| faces.iter().map(|face| face.width + per_tab).sum();
    let count = Squeeze::of_strip(chips.len(), room);
    let stage = count.fitting(tabs_room, |stage| taken(&faces_at(stage)));
    (stage, faces_at(stage))
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

    /// A chip measures its face from its own symbol, code and quote.
    #[test]
    fn a_chip_measures_its_face_from_its_symbol_and_quote() {
        let quote = quantick_session::tab_quote::quote(None, None, 0);
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
        chip.quote.price = Some("172455".into());
        chip.quote.change = Some(quantick_session::tab_quote::format_change(
            "0.41".parse().unwrap(),
        ));
        let source = chip.face_source(Squeeze::NoPrice, 1.5);
        assert_eq!((source.symbol, source.code), ("WINZ26", "MT5"));
        assert_eq!(
            (source.price, source.change),
            (Some("172455"), Some("+0.41%"))
        );
    }
    /// Long faces squeeze the strip past the stage its tab count names.
    #[test]
    fn the_strip_squeezes_past_the_count_when_the_faces_do_not_fit() {
        let chip = |id| Chip {
            id,
            label: "BTCUSDT \u{b7} Binance".into(),
            feed: TabFeed::Live,
            workspace: "Default".into(),
            venue_code: "BIN".into(),
            symbol_ink: None,
            tag_fill: None,
            icon: TabIcon::of("BTCUSDT", None),
            quote: TabQuote {
                price: Some("76200.00".into()),
                change: None,
            },
        };
        let chips = [chip(1), chip(2), chip(3)];
        let width = |text: &str, pt: f32| text.len() as f32 * pt;
        let mut cache = FaceCache::default();
        let mut fit = |tabs_room| {
            let rooms = (3_000.0, tabs_room, 24.0);
            squeezed_faces(&chips, Some(0), rooms, 1.0, &mut cache, width)
        };
        let (roomy, wide) = fit(3_000.0);
        assert_eq!((roomy, wide.len()), (Squeeze::OneLine, 3));
        let one_line: f32 = wide.iter().map(|face| face.width + 24.0).sum();
        let (tight, faces) = fit(one_line - 1.0);
        assert_ne!(tight, Squeeze::OneLine);
        assert_eq!(faces[0], wide[0], "the open tab stays on one line");
        assert!(faces[1].width < wide[1].width);
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
