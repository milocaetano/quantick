//! Width-bounded presentation of the existing market arrangement.
use std::ops::Range;

/// The room the strip gives its market tabs, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripRoom {
    /// Everything left of the strip's controls, Home included.
    pub room: f32,
    /// Home with its label, and without: the Q alone.
    pub home: (f32, f32),
    /// The space after each tab.
    pub gap: f32,
    /// The narrowest a lone tab wider than the whole strip is drawn.
    pub floor: f32,
}

/// What the strip shows of its market tabs this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct TabFit {
    /// The tabs drawn, in order; the overflow menu reaches the rest.
    pub visible: Range<usize>,
    /// Each shown tab's width: its natural one, never squeezed, save a lone
    /// tab wider than the whole strip, which takes the room the bare Home
    /// leaves it.
    pub widths: Vec<f32>,
    /// Home keeps its label: every tab fits beside it whole. A lone market
    /// wider than the room costs it the label too; with no market at all
    /// nothing competes for the room.
    pub home_labelled: bool,
}

/// Fit the market tabs, `natural` points wide each and a gap after each,
/// beside Home. A shown tab is never squeezed below its natural width: the
/// longest run that fits whole from the first tab is shown, or, when `active`
/// is past it, the longest that fits ending at `active`; the rest go to the
/// overflow menu. One tab always shows.
///
/// Asked of Home's labelled width whatever it drew last frame, so dropping the
/// label can never free the room that would bring it back on the next one;
/// and the label and the overflow come from one count, so they never show
/// together.
#[must_use]
pub fn fit_tabs(natural: &[f32], strip: StripRoom, active: usize) -> TabFit {
    let room = strip.room - strip.home.0;
    let span = |run: Range<usize>| natural[run].iter().map(|w| w + strip.gap).sum::<f32>();
    let fits = |run: Range<usize>| span(run) <= room;
    let count = natural.len();
    let whole = (1..=count).take_while(|end| fits(0..*end)).last();
    let whole = whole.unwrap_or(0);
    let active = active.min(count.saturating_sub(1));
    let visible = if count == 0 || active < whole.max(1) {
        0..whole.max(1).min(count)
    } else {
        let before = (0..active).rev().take_while(|from| fits(*from..active + 1));
        before.last().unwrap_or(active)..active + 1
    };
    let lone = visible.len() == 1 && !fits(visible.clone());
    let widest = (strip.room - strip.home.1 - strip.gap).max(strip.floor);
    let width = |natural: &f32| if lone { natural.min(widest) } else { *natural };
    TabFit {
        widths: natural[visible.clone()].iter().map(width).collect(),
        home_labelled: whole == count,
        visible,
    }
}

/// A market tab's hover: `Scalp · WINZ26 · MetaTrader 5 — B3`, plus
/// ` (replay)` on a recording.
#[must_use]
pub fn tab_hover(workspace: &str, symbol: &str, venue: &str, feed: TabFeed) -> String {
    let replay = if feed == TabFeed::Replay {
        " (replay)"
    } else {
        ""
    };
    format!("{workspace} \u{b7} {symbol} \u{b7} {venue}{replay}")
}

/// What a market tab's feed is doing, as the mark beside its symbol says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabFeed {
    Live,
    /// Connected, but the venue reports a failure behind the open socket, such
    /// as a terminal closed: the canvas shows the notice, so the tab is not live.
    Attention,
    /// Playing a recording; the tab also says so in words.
    Replay,
    /// A live session was lost and has not come back.
    Disconnected,
    /// No live session yet.
    Stopped,
}

impl TabFeed {
    /// A recording wins. A connected feed that needs attention is neither live
    /// nor lost: its socket is open while its venue reports a failure. Only a
    /// feed that is not connected and needs attention reads as lost.
    #[must_use]
    pub const fn of(replaying: bool, connected: bool, attention: bool) -> Self {
        match (replaying, connected, attention) {
            (true, _, _) => Self::Replay,
            (false, true, false) => Self::Live,
            (false, true, true) => Self::Attention,
            (false, false, true) => Self::Disconnected,
            (false, false, false) => Self::Stopped,
        }
    }

    /// Whether the tab wears a mark at all: a healthy live feed shows nothing,
    /// so only a recording, a lost or stopped feed, or a venue notice does.
    #[must_use]
    pub const fn marked(self) -> bool {
        !matches!(self, Self::Live)
    }

    /// The word printed after the symbol: a recording is never passed off as live.
    #[must_use]
    pub const fn word(self) -> Option<&'static str> {
        match self {
            Self::Replay => Some("Replay"),
            _ => None,
        }
    }
}

/// A chip label's market and its muted suffix: `SYMBOL · venue`.
#[must_use]
pub fn chip_parts(label: &str) -> (&str, &str) {
    label.split_once(" · ").unwrap_or((label, ""))
}

/// The tab shortcuts pressed this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabKeys {
    pub home: bool,
    pub new_tab: bool,
    pub close_tab: bool,
    pub next: bool,
    pub previous: bool,
}

/// What the active market tab holds this frame. Borrowed, so comparing it on
/// every frame Home is up costs no allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketShown<'a> {
    pub tab_id: u64,
    pub label: &'a str,
    pub replaying: bool,
}

/// The market Home was brought over, kept to compare later frames against.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MarketHeld {
    tab_id: u64,
    label: String,
    replaying: bool,
}

impl MarketHeld {
    fn holds(&self, market: &MarketShown<'_>) -> bool {
        self.tab_id == market.tab_id
            && self.label == market.label
            && self.replaying == market.replaying
    }
}

/// Which surface the central canvas shows: Home, or the active market.
///
/// Presentation only. The arrangement keeps its active market underneath, so no
/// market tab is removed, reordered or stopped while Home is up, and nothing
/// here is saved: every launch opens on a market.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StripSurface {
    /// The market Home was brought over; `None` while a market is on screen.
    home_over: Option<MarketHeld>,
}

impl StripSurface {
    #[must_use]
    pub const fn is_home(&self) -> bool {
        self.home_over.is_some()
    }

    /// Bring Home forward over `market`. Showing it twice is the same state.
    pub fn show_home(&mut self, market: MarketShown<'_>) {
        self.home_over = Some(MarketHeld {
            tab_id: market.tab_id,
            label: market.label.to_owned(),
            replaying: market.replaying,
        });
    }

    /// Put the active market back on screen.
    pub fn show_market(&mut self) {
        self.home_over = None;
    }

    /// Leave Home when the active tab now holds anything else, whichever route
    /// got it there: a new tab, a restored workspace, a replay, a source change.
    pub fn observe(&mut self, market: &MarketShown<'_>) {
        if self
            .home_over
            .as_ref()
            .is_some_and(|over| !over.holds(market))
        {
            self.home_over = None;
        }
    }

    /// What the tab shortcuts still mean on this surface. On Home nothing is
    /// open to close, and cycling lands back on the market it left.
    #[must_use]
    pub fn settle_keys(&mut self, keys: TabKeys) -> TabKeys {
        if !self.is_home() {
            return keys;
        }
        if keys.next || keys.previous {
            self.show_market();
        }
        TabKeys {
            new_tab: keys.new_tab,
            ..TabKeys::default()
        }
    }

    /// The market that looks active, by whichever handle the caller holds on
    /// it, `None` while Home does.
    #[must_use]
    pub fn active_chip<T>(&self, active: T) -> Option<T> {
        if self.is_home() { None } else { Some(active) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market(tab_id: u64, label: &str) -> MarketShown<'_> {
        MarketShown {
            tab_id,
            label,
            replaying: false,
        }
    }

    /// `markets` tabs `chip` wide with no gap, beside a Home `labelled` wide.
    fn uniform(markets: usize, (room, labelled): (f32, f32), chip: f32, active: usize) -> TabFit {
        let strip = StripRoom {
            room,
            home: (labelled, 30.0),
            gap: 0.0,
            floor: 40.0,
        };
        fit_tabs(&vec![chip; markets], strip, active)
    }

    #[test]
    fn selected_overflow_market_is_visible_without_changing_identity() {
        assert_eq!(uniform(10, (200.0, 0.0), 100.0, 9).visible, 8..10);
        assert_eq!(uniform(10, (200.0, 0.0), 100.0, 0).visible, 0..2);
        assert_eq!(uniform(1, (500.0, 0.0), 100.0, 0).visible, 0..1);
        assert_eq!(uniform(0, (0.0, 0.0), 100.0, 0).visible, 0..0);
        assert_eq!(uniform(10, (0.0, 0.0), 100.0, 9).visible, 9..10);
    }

    /// A shown tab keeps its natural width, the gaps counted: what does not
    /// fit whole goes to the overflow menu, never clipped.
    #[test]
    fn a_shown_tab_keeps_its_natural_width_and_the_rest_overflow() {
        let strip = StripRoom {
            room: 500.0,
            home: (0.0, 0.0),
            gap: 4.0,
            floor: 40.0,
        };
        let fit = fit_tabs(&[100.0; 5], strip, 0);
        assert_eq!((fit.visible, fit.widths), (0..4, vec![100.0; 4]));
        assert!(!fit.home_labelled);
        let mixed = [100.0, 100.0, 320.0, 100.0];
        let fit = fit_tabs(&mixed, strip, 0);
        assert_eq!((fit.visible, fit.widths), (0..2, vec![100.0, 100.0]));
        let fit = fit_tabs(&mixed, strip, 2);
        assert_eq!((fit.visible, fit.widths), (1..3, vec![100.0, 320.0]));
        let fit = fit_tabs(&[100.0, 60.0], strip, 1);
        assert_eq!((fit.visible, fit.widths), (0..2, vec![100.0, 60.0]));
        assert!(fit.home_labelled);
        // A lone tab wider than the whole strip takes the strip.
        let fit = fit_tabs(&[600.0], strip, 0);
        assert_eq!((fit.visible, fit.widths), (0..1, vec![496.0]));
        assert!(!fit.home_labelled);
    }

    #[test]
    fn the_window_opens_on_a_market_and_home_is_idempotent() {
        let mut surface = StripSurface::default();
        assert!(!surface.is_home());
        assert_eq!(surface.active_chip(3), Some(3));
        surface.show_home(market(1, "BTC"));
        let once = surface.clone();
        surface.show_home(market(1, "BTC"));
        assert_eq!(surface, once);
        assert!(surface.is_home());
        assert_eq!(surface.active_chip(3), None);
        assert_eq!(surface.active_chip(7_u64), None, "or a tab id");
        surface.show_market();
        assert_eq!(surface, StripSurface::default());
    }

    #[test]
    fn home_stays_until_the_active_market_changes_by_any_route() {
        let mut surface = StripSurface::default();
        surface.show_home(market(1, "BTC"));
        surface.observe(&market(1, "BTC"));
        assert!(surface.is_home(), "the same market keeps Home up");

        surface.observe(&market(2, "BTC"));
        assert!(!surface.is_home(), "another tab came forward");

        surface.show_home(market(1, "BTC"));
        surface.observe(&market(1, "ETH"));
        assert!(!surface.is_home(), "the tab changed market");

        surface.show_home(market(1, "BTC"));
        surface.observe(&MarketShown {
            replaying: true,
            ..market(1, "BTC")
        });
        assert!(!surface.is_home(), "the tab began a replay");

        surface.observe(&market(9, "SOL"));
        assert!(!surface.is_home(), "a market on screen stays on screen");
    }

    #[test]
    fn home_refuses_close_and_turns_cycling_into_the_way_back() {
        let keys = TabKeys {
            close_tab: true,
            next: true,
            ..TabKeys::default()
        };
        let mut surface = StripSurface::default();
        assert_eq!(surface.settle_keys(keys), keys, "a market keeps its keys");

        surface.show_home(market(1, "BTC"));
        let close_only = TabKeys {
            close_tab: true,
            new_tab: true,
            ..TabKeys::default()
        };
        let settled = surface.settle_keys(close_only);
        assert!(!settled.close_tab && settled.new_tab);
        assert!(surface.is_home(), "closing nothing stays on Home");

        assert_eq!(surface.settle_keys(keys), TabKeys::default());
        assert!(!surface.is_home(), "and the cycle does not also move a tab");
    }

    /// The strip's own call: the fit it also hands to the overflow.
    fn fits(markets: usize, room: f32, labelled: f32, chip: f32) -> bool {
        uniform(markets, (room, labelled), chip, 0).home_labelled
    }

    /// With every market closed (`allow_close_last`), nothing competes for the
    /// room: the label stays wherever it fits at all.
    #[test]
    fn no_market_keeps_the_label_whenever_it_fits() {
        for room in [70.0, 71.0, 269.0, 1130.0] {
            assert!(fits(0, room, 70.0, 200.0), "{room} px");
        }
    }

    #[test]
    fn home_keeps_its_label_until_the_market_chips_overflow() {
        // 1130 px of room, a 70 px labelled Home, 200 px chips: five fit.
        assert!(fits(1, 1130.0, 70.0, 200.0));
        assert!(fits(5, 1130.0, 70.0, 200.0));
        assert!(!fits(6, 1130.0, 70.0, 200.0));
        assert!(!fits(9, 1130.0, 70.0, 200.0));
    }

    /// The width that decides is the labelled one, never the width drawn, so a
    /// room that just misses stays missed however the tab is drawn.
    #[test]
    fn the_rule_cannot_flip_flop_at_the_boundary() {
        let (room, labelled) = (70.0 + 2.0 * 200.0 - 1.0, 70.0);
        for _ in 0..4 {
            assert!(!fits(2, room, labelled, 200.0));
        }
        assert!(fits(2, room + 1.0, labelled, 200.0));
    }

    /// The logo stands alone exactly when a chip is hidden, or when the one
    /// market left would no longer fit beside the label.
    #[test]
    fn home_is_logo_only_exactly_when_a_chip_is_hidden_or_crushed() {
        assert!(!fits(1, 10.0, 70.0, 200.0), "no room at all");
        assert!(!fits(1, 269.0, 70.0, 200.0));
        assert!(fits(1, 270.0, 70.0, 200.0));
        assert!(!fits(2, 10.0, 70.0, 200.0));
        for room in [0.0, 10.0, 269.0, 270.0, 469.0, 470.0, 1130.0] {
            for markets in 1..=9 {
                for active in 0..markets {
                    let fit = uniform(markets, (room, 70.0), 200.0, active);
                    assert_eq!(
                        fit.home_labelled,
                        fit.visible.len() == markets && room >= 270.0,
                        "{markets} markets in {room} px"
                    );
                }
            }
        }
    }

    /// Where `n * chip` and the running sum round apart in f32, the label
    /// still follows what is shown, so it and the overflow never show together.
    #[test]
    fn the_label_follows_the_shown_tabs_where_f32_rounding_could_split_them() {
        for (chip, markets, room) in [
            (84.451_7_f32, 7, 591.161_87_f32),
            (141.264_6, 3, 423.793_8),
            (136.460_54, 5, 682.302_7),
        ] {
            let fit = uniform(markets, (room, 0.0), chip, 0);
            assert_eq!(
                fit.home_labelled,
                fit.visible.len() == markets,
                "{chip} x {markets}"
            );
        }
    }

    #[test]
    fn a_recording_wins_and_a_venue_notice_is_neither_live_nor_lost() {
        assert_eq!(TabFeed::of(true, false, true), TabFeed::Replay);
        assert_eq!(TabFeed::of(true, true, false), TabFeed::Replay);
        assert_eq!(TabFeed::of(false, true, false), TabFeed::Live);
        // A venue notice on a connected feed: neither live nor "lost".
        assert_eq!(TabFeed::of(false, true, true), TabFeed::Attention);
        assert_eq!(TabFeed::of(false, false, true), TabFeed::Disconnected);
        assert_eq!(TabFeed::of(false, false, false), TabFeed::Stopped);
    }

    #[test]
    fn only_a_recording_is_named_in_words() {
        let all = [
            TabFeed::Live,
            TabFeed::Attention,
            TabFeed::Replay,
            TabFeed::Disconnected,
            TabFeed::Stopped,
        ];
        let named: Vec<_> = all.iter().filter_map(|feed| feed.word()).collect();
        assert_eq!(named, ["Replay"]);
    }

    #[test]
    fn only_a_healthy_live_feed_wears_no_mark() {
        assert!(!TabFeed::Live.marked());
        for feed in [
            TabFeed::Attention,
            TabFeed::Replay,
            TabFeed::Disconnected,
            TabFeed::Stopped,
        ] {
            assert!(feed.marked(), "{feed:?}");
        }
    }

    #[test]
    fn the_hover_names_workspace_symbol_and_venue() {
        assert_eq!(
            tab_hover("Scalp", "WINZ26", "MetaTrader 5 — B3", TabFeed::Live),
            "Scalp · WINZ26 · MetaTrader 5 — B3"
        );
        assert_eq!(
            tab_hover("Default", "BTCUSDT", "Binance", TabFeed::Replay),
            "Default · BTCUSDT · Binance (replay)"
        );
    }

    #[test]
    fn a_chip_label_splits_into_market_and_suffix() {
        assert_eq!(
            chip_parts("WINV26 · MetaTrader 5 — B3"),
            ("WINV26", "MetaTrader 5 — B3")
        );
        assert_eq!(chip_parts("WINV26"), ("WINV26", ""));
    }
}
