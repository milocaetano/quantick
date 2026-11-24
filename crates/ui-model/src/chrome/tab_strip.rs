//! Cached tab identities and intent validation, and what a chip paints from:
//! its face's source, its feed's mark and its corner's words. The arrangement
//! remains authoritative.
use quantick_appearance::tab_strip::FeedMark;
use quantick_session::tab_chip::{CloseTarget, FaceCache, FaceLayout, FaceSource, Squeeze};
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
    /// The workspace's colour for the theme, painted in every tab's top-left
    /// corner, open or closed; `None` only before the book has settled one.
    pub corner: Option<[u8; 3]>,
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
/// read against the `venue`'s day reference whose window covers it. The chip
/// and the scene both read a tab through this.
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
    let day = last.and_then(|last| venue.day_at(last.timestamp_ms));
    (icon, quote(last, day, decimals))
}

/// How far the strip squeezes, and each chip's face and close target at that
/// stage, the open chip (`open`) on one line. The count's stage over `room`
/// first ([`Squeeze::of_strip`]); further while the faces, each taking its
/// target's reserve and `gap` beside its own width, overrun `tabs_room`
/// ([`Squeeze::fitting`]): the room beside the labelled Home tab, which the
/// strip's fit measures against. `width` measures a text at a size, at
/// `scale` pixels per point.
pub fn squeezed_faces(
    chips: &[Chip],
    open: Option<usize>,
    (room, tabs_room, gap): (f32, f32, f32),
    scale: f32,
    cache: &mut FaceCache,
    width: impl Fn(&str, f32) -> f32,
) -> (Squeeze, Vec<(FaceLayout, CloseTarget)>) {
    cache.keep(chips.iter().map(|chip| chip.id));
    let mut faces_at = |stage: Squeeze| -> Vec<(FaceLayout, CloseTarget)> {
        let face = |(i, chip): (usize, &Chip)| {
            let worn = stage.for_tab(open == Some(i));
            let source = chip.face_source(worn, scale);
            let target = CloseTarget::of(open == Some(i), worn);
            (cache.layout(chip.id, &source, &width), target)
        };
        chips.iter().enumerate().map(face).collect()
    };
    let taken = |faces: &[(FaceLayout, CloseTarget)]| {
        let tab = |(face, target): &(FaceLayout, CloseTarget)| face.width + target.reserve();
        faces.iter().map(|face| tab(face) + gap).sum()
    };
    let count = Squeeze::of_strip(chips.len(), room);
    let stage = count.fitting(tabs_room, |stage| taken(&faces_at(stage)));
    (stage, faces_at(stage))
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

/// A drawn tab's scene words as it reads, left to right: the colour in its
/// `corner`, then its `quote`'s words.
#[must_use]
pub fn tab_words(corner: Option<[u8; 3]>, quote: String) -> String {
    match corner {
        Some(rgb) => format!("{}; {quote}", corner_description(rgb)),
        None => quote,
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
            corner: None,
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
            corner: None,
        };
        let chips = [chip(1), chip(2), chip(3)];
        let width = |text: &str, pt: f32| text.len() as f32 * pt;
        let mut cache = FaceCache::default();
        let mut fit = |tabs_room| {
            let rooms = (3_000.0, tabs_room, 4.0);
            squeezed_faces(&chips, Some(0), rooms, 1.0, &mut cache, width)
        };
        let (roomy, wide) = fit(3_000.0);
        assert_eq!((roomy, wide.len()), (Squeeze::OneLine, 3));
        let tab = |(face, target): &(FaceLayout, CloseTarget)| face.width + target.reserve();
        let one_line: f32 = wide.iter().map(|face| tab(face) + 4.0).sum();
        let (tight, faces) = fit(one_line - 1.0);
        assert_ne!(tight, Squeeze::OneLine);
        assert_eq!(faces[0], wide[0], "the open tab stays on one line");
        assert!(faces[1].0.width < wide[1].0.width);
    }

    /// The mock's tabs (`docs/label/tabs-final`), the fifth open, measured
    /// in the strip's font, on the strip of a 1,918-point window with the
    /// agent's pill shown and without: 6 on one line, 10 stacked, 14
    /// without a price, 26 the icon over the venue, every tab drawn.
    #[test]
    fn the_mocks_stages_fit_a_1918_point_window() {
        use quantick_session::tab_quote::format_change;
        const MOCK: [(&str, &str, &str, &str); 14] = [
            ("ETHUSDT", "BIN", "3241.50", "1.12"),
            ("BTCUSDT", "BIN", "76200.00", "-0.38"),
            ("ETHUSDT", "HL", "3241.80", "1.13"),
            ("BTCUSDT", "BIN", "76200.00", "-0.38"),
            ("WINZ26", "MT5", "172455", "-0.41"),
            ("WDOZ26", "MT5", "5412.5", "0.22"),
            ("SOLUSDT", "BIN", "182.40", "2.05"),
            ("BTCUSDT", "HL", "76195.00", "-0.39"),
            ("WINZ26", "MT5", "172455", "-0.41"),
            ("ETHUSDT", "BIN", "3241.50", "1.12"),
            ("SOLUSDT", "HL", "182.45", "2.08"),
            ("WDOZ26", "MT5", "5412.5", "0.22"),
            ("BTCUSDT", "BIN", "76200.00", "-0.38"),
            ("XRPUSDT", "BIN", "0.6142", "-1.20"),
        ];
        let chip = |id: usize| {
            let (symbol, code, price, change) = MOCK[id % MOCK.len()];
            Chip {
                id: id as u64,
                label: format!("{symbol} \u{b7} {code}"),
                feed: TabFeed::Live,
                workspace: "Default".into(),
                venue_code: code.into(),
                symbol_ink: None,
                tag_fill: None,
                icon: TabIcon::of(symbol, None),
                quote: TabQuote {
                    price: Some(price.into()),
                    change: Some(format_change(change.parse().unwrap())),
                },
                corner: None,
            }
        };
        // The app's proportional face (egui's default), em advances.
        let advance = |c: char| match c {
            'I' => 0.25,
            'J' => 0.49,
            'L' => 0.50,
            'F' => 0.52,
            'S' => 0.52,
            'E' | 'T' => 0.55,
            'Z' | 'Y' => 0.57,
            'P' => 0.59,
            'K' => 0.60,
            'C' | 'X' => 0.61,
            'R' | 'V' => 0.62,
            'B' => 0.63,
            'A' => 0.64,
            'G' => 0.66,
            'U' => 0.68,
            'H' => 0.69,
            'D' => 0.70,
            'N' => 0.71,
            'O' | 'Q' => 0.77,
            'M' => 0.86,
            'W' => 0.92,
            '.' => 0.25,
            '-' => 0.28,
            '%' => 0.83,
            _ => 0.56,
        };
        let width = |text: &str, pt: f32| (text.chars().map(advance).sum::<f32>() * pt).ceil();
        // 1,918 less the strip's edge (10), the window buttons (3 × 46),
        // the menu (36), the drag gap (1) and stretch (32), the overflow and
        // `+` (66) with their gap (4): 1,631, or 1,559 beside the agent's
        // pill (72). The labelled Home takes 105 of it.
        for room in [1_559.0, 1_631.0] {
            let tabs_room = room - 105.0;
            for (count, stage) in [
                (6, Squeeze::OneLine),
                (10, Squeeze::Stacked),
                (14, Squeeze::NoPrice),
                (26, Squeeze::IconOverVenue),
            ] {
                let chips: Vec<Chip> = (0..count).map(chip).collect();
                let mut cache = FaceCache::default();
                let rooms = (room, tabs_room, 4.0);
                let (worn, faces) = squeezed_faces(&chips, Some(4), rooms, 1.5, &mut cache, width);
                assert_eq!(worn, stage, "{count} tabs on {room}");
                let taken: f32 = (faces.iter())
                    .map(|(face, target)| face.width + target.reserve() + 4.0)
                    .sum();
                assert!(
                    taken <= tabs_room,
                    "{count} tabs take {taken} of {tabs_room}"
                );
            }
        }
    }

    /// The face starts where the workspace's corner mark ends.
    #[test]
    fn the_face_starts_past_the_corner_mark() {
        assert_eq!(
            quantick_session::tab_chip::TEXT_LEFT_PT,
            quantick_appearance::tab_strip::WORKSPACE_CORNER_PT
        );
    }

    /// The corner's words name the colour as the workspace menu writes it.
    #[test]
    fn the_corner_description_names_the_colour_in_hex() {
        assert_eq!(
            corner_description([0x3A, 0x7B, 0xD5]),
            "Workspace corner #3A7BD5"
        );
        let words = tab_words(Some([0x3A, 0x7B, 0xD5]), "price 1".into());
        assert_eq!(words, "Workspace corner #3A7BD5; price 1");
        assert_eq!(tab_words(None, "price 1".into()), "price 1");
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
