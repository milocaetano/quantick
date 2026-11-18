use super::*;

/// A window that lends its strip when `lends` says so; tab 5 is on screen.
struct Window {
    lends: bool,
    surface: StripSurface,
    label: String,
}

impl Window {
    fn new(lends: bool) -> Self {
        Self {
            lends,
            surface: StripSurface::default(),
            label: "BTCUSDT".to_owned(),
        }
    }
}

impl HomeTabHost for Window {
    fn home_tab(&mut self) -> Option<HomeTab<'_>> {
        let shown = shown(5, &self.label);
        self.lends.then(|| HomeTab::new(&mut self.surface, shown))
    }
}

fn shown(tab_id: u64, label: &str) -> MarketShown<'_> {
    MarketShown {
        tab_id,
        label,
        replaying: false,
    }
}

#[test]
fn home_holds_the_market_it_came_over_and_leaving_puts_it_back() {
    let mut window = Window::new(true);
    show_home(&mut window).unwrap();
    assert!(window.surface.is_home());
    window.surface.observe(&shown(5, "BTCUSDT"));
    assert!(window.surface.is_home(), "the market Home hides is held");
    leave_home(&mut window).unwrap();
    assert!(!window.surface.is_home());
    show_home(&mut window).unwrap();
    window.surface.observe(&shown(6, "ETHUSDT"));
    assert!(!window.surface.is_home(), "another market leaves Home");
}

#[test]
fn a_window_that_lends_no_strip_refuses_both_calls() {
    let mut window = Window::new(false);
    let home = show_home(&mut window).unwrap_err();
    assert_eq!(home.message, "the Home tab is unavailable");
    let leave = leave_home(&mut window).unwrap_err();
    assert_eq!(leave.message, "the Home tab is unavailable");
}
