//! Width-bounded presentation of the existing market arrangement.
use std::ops::Range;

/// Keep the selected market visible, with the overflow menu reaching the rest.
#[must_use]
pub fn visible_tabs(count: usize, active: usize, slots: usize) -> Range<usize> {
    let slots = slots.min(count);
    let end = active.saturating_add(1).max(slots).min(count);
    end.saturating_sub(slots)..end
}

/// How many market chips the strip holds, never fewer than the one on screen.
///
/// Asked of the labelled width whatever the tab drew last frame, so dropping the
/// label can never free the room that would bring it back on the next one.
#[must_use]
pub fn market_slots(room: f32, labelled_width: f32, chip_width: f32) -> usize {
    (((room - labelled_width).max(0.0) / chip_width).floor() as usize).max(1)
}

/// Whether Home keeps its label beside `markets` chips: until one is hidden.
#[must_use]
pub fn home_label_fits(markets: usize, room: f32, labelled_width: f32, chip_width: f32) -> bool {
    markets <= market_slots(room, labelled_width, chip_width)
}

/// Which bundled logo raster, by its pixel size, to draw `points` wide. A size
/// that lands on a raster is drawn pixel for pixel; a fractional scale samples
/// the next raster that does not have to be stretched up.
#[must_use]
pub fn logo_raster_px(points: f32, pixels_per_point: f32) -> u32 {
    let physical = points * pixels_per_point;
    let exact = [16, 32, 48, 96]
        .into_iter()
        .find(|size| (physical - *size as f32).abs() < 0.5);
    match exact {
        Some(size) => size,
        None if physical <= 48.0 => 48,
        None => 96,
    }
}

/// How opaque the Home logo is while another surface is on screen.
const HOME_LOGO_INACTIVE_OPACITY: f32 = 0.65;

/// How opaque the Home logo is. Hover is deliberately not an input: pointing at
/// the tab changes its text and nothing else.
#[must_use]
pub const fn home_logo_opacity(active: bool) -> f32 {
    if active {
        1.0
    } else {
        HOME_LOGO_INACTIVE_OPACITY
    }
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

    #[test]
    fn selected_overflow_market_is_visible_without_changing_identity() {
        assert_eq!(visible_tabs(10, 9, 2), 8..10);
        assert_eq!(visible_tabs(10, 0, 2), 0..2);
        assert_eq!(visible_tabs(1, 0, 5), 0..1);
        assert_eq!(visible_tabs(0, 0, 0), 0..0);
        assert_eq!(visible_tabs(10, 9, 0), 10..10);
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

    #[test]
    fn home_keeps_its_label_until_the_market_chips_overflow() {
        // 1130 px of room, a 70 px labelled Home, 200 px chips: five fit.
        assert!(home_label_fits(1, 1130.0, 70.0, 200.0));
        assert!(home_label_fits(5, 1130.0, 70.0, 200.0));
        assert!(!home_label_fits(6, 1130.0, 70.0, 200.0));
        assert!(!home_label_fits(9, 1130.0, 70.0, 200.0));
    }

    /// The width that decides is the labelled one, never the width drawn, so a
    /// room that just misses stays missed however the tab is drawn.
    #[test]
    fn the_rule_cannot_flip_flop_at_the_boundary() {
        let (room, labelled) = (70.0 + 2.0 * 200.0 - 1.0, 70.0);
        for _ in 0..4 {
            assert!(!home_label_fits(2, room, labelled, 200.0));
        }
        assert!(home_label_fits(2, room + 1.0, labelled, 200.0));
    }

    /// One market is always on screen, so it never costs Home its label however
    /// narrow the window: the logo stands alone exactly when a chip is hidden.
    #[test]
    fn home_is_logo_only_exactly_when_a_chip_is_hidden() {
        assert!(home_label_fits(1, 10.0, 70.0, 200.0), "no room at all");
        assert!(!home_label_fits(2, 10.0, 70.0, 200.0));
        for room in [0.0, 10.0, 269.0, 270.0, 469.0, 470.0, 1130.0] {
            let slots = market_slots(room, 70.0, 200.0);
            for markets in 1..=9 {
                for active in 0..markets {
                    let shown = visible_tabs(markets, active, slots).len();
                    assert_eq!(
                        home_label_fits(markets, room, 70.0, 200.0),
                        shown == markets,
                        "{markets} markets in {room} px"
                    );
                }
            }
        }
    }

    #[test]
    fn the_logo_raster_matches_the_scale_or_is_never_stretched_up() {
        for (points, scale, raster) in [
            (16.0, 1.0, 16),
            (16.0, 2.0, 32),
            (16.0, 1.25, 48),
            (16.0, 1.5, 48),
            (16.0, 3.0, 48),
            (48.0, 1.0, 48),
            (48.0, 2.0, 96),
            (48.0, 1.5, 96),
        ] {
            assert_eq!(logo_raster_px(points, scale), raster, "{points} at {scale}");
        }
    }

    #[test]
    fn hover_cannot_reach_the_logo_opacity() {
        assert!((home_logo_opacity(true) - 1.0).abs() < f32::EPSILON);
        assert!((home_logo_opacity(false) - 0.65).abs() < f32::EPSILON);
    }
}
