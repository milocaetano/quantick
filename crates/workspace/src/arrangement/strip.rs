//! Width-bounded presentation of the existing market arrangement.
use std::ops::Range;

/// Keep the selected market visible, with the overflow menu reaching the rest.
#[must_use]
pub fn visible_tabs(count: usize, active: usize, slots: usize) -> Range<usize> {
    let slots = slots.min(count);
    let end = active.saturating_add(1).max(slots).min(count);
    end.saturating_sub(slots)..end
}

/// Whether Home keeps its label beside `markets` chips.
///
/// Asked of the labelled width whatever the tab drew last frame, so dropping the
/// label can never free the room that would bring it back on the next one.
#[must_use]
pub fn home_label_fits(markets: usize, room: f32, labelled_width: f32, chip_width: f32) -> bool {
    let slots = ((room - labelled_width).max(0.0) / chip_width).floor() as usize;
    markets <= slots
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

/// What the active market tab held when it was last compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketShown {
    pub tab_id: u64,
    pub label: String,
    pub replaying: bool,
}

/// Which surface the central canvas shows: Home, or the active market.
///
/// Presentation only. The arrangement keeps its active market underneath, so no
/// market tab is removed, reordered or stopped while Home is up, and nothing
/// here is saved: every launch opens on a market.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StripSurface {
    /// The market Home was brought over; `None` while a market is on screen.
    home_over: Option<MarketShown>,
}

impl StripSurface {
    #[must_use]
    pub const fn is_home(&self) -> bool {
        self.home_over.is_some()
    }

    /// Bring Home forward over `market`. Showing it twice is the same state.
    pub fn show_home(&mut self, market: MarketShown) {
        self.home_over = Some(market);
    }

    /// Put the active market back on screen.
    pub fn show_market(&mut self) {
        self.home_over = None;
    }

    /// Leave Home when the active tab now holds anything else, whichever route
    /// got it there: a new tab, a restored workspace, a replay, a source change.
    pub fn observe(&mut self, market: &MarketShown) {
        if self.home_over.as_ref().is_some_and(|over| over != market) {
            self.home_over = None;
        }
    }

    /// The strip position that looks active, `None` while Home does.
    #[must_use]
    pub const fn active_chip(&self, active: usize) -> Option<usize> {
        if self.is_home() { None } else { Some(active) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market(tab_id: u64, label: &str) -> MarketShown {
        MarketShown {
            tab_id,
            label: label.to_owned(),
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
        assert!(!home_label_fits(1, 10.0, labelled, 200.0), "no room at all");
    }

    #[test]
    fn hover_cannot_reach_the_logo_opacity() {
        assert!((home_logo_opacity(true) - 1.0).abs() < f32::EPSILON);
        assert!((home_logo_opacity(false) - 0.65).abs() < f32::EPSILON);
    }
}
