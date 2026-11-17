use super::*;
use crate::market_palette::MarketPaletteHost;
use quantick_workspace::arrangement::strip::{MarketShown, StripSurface};

/// A window that lends what `lends` says: the strip, recording widget panes
/// and recording market tabs.
struct Window {
    lends: bool,
    surface: StripSurface,
    label: String,
    asked: Vec<String>,
    snapshot: MarketSnapshot,
    moves: Vec<String>,
    palette: Option<(String, bool)>,
}

impl Window {
    fn new(lends: bool) -> Self {
        Self {
            lends,
            surface: StripSurface::default(),
            label: "BTCUSDT".to_owned(),
            asked: Vec::new(),
            snapshot: MarketSnapshot {
                starter_feed: "binance".to_owned(),
                ..MarketSnapshot::default()
            },
            moves: Vec::new(),
            palette: None,
        }
    }
}

struct Panes<'a>(&'a mut Vec<String>);

impl WidgetHost for Panes<'_> {
    fn widget(&mut self, id: &str, open: Option<bool>) -> Result<WidgetRead, String> {
        self.0.push(format!("{id} {open:?}"));
        Err("recorded".to_owned())
    }
}

struct Tabs<'a>(&'a mut Vec<String>);

impl MarketTabs for Tabs<'_> {
    fn focus_tab(&mut self, index: usize) {
        self.0.push(format!("focus {index}"));
    }
    fn open_tab(&mut self, choice: &OpenChoice, bar: Option<BarConfiguration>) -> TabOpening {
        let bar = bar.map(|bar| bar.to_config_string());
        self.0.push(format!("open {} {bar:?}", choice.symbol));
        Ok(9)
    }
    fn set_flow_bar(&mut self, tab: u64, bar: BarConfiguration) -> Result<(), String> {
        self.0.push(format!("bar {tab} {}", bar.to_config_string()));
        Ok(())
    }
}

impl ChromeOwners for Window {
    fn home_tab(&mut self) -> Option<HomeTab<'_>> {
        let shown = MarketShown {
            tab_id: 5,
            label: &self.label,
            replaying: false,
        };
        self.lends.then(|| HomeTab::new(&mut self.surface, shown))
    }
    fn widgets(&mut self) -> Option<Box<dyn WidgetHost + '_>> {
        let panes: Box<dyn WidgetHost + '_> = Box::new(Panes(&mut self.asked));
        self.lends.then_some(panes)
    }
    fn listed_markets(&self) -> MarketSnapshot {
        self.snapshot.clone()
    }
    fn market_tabs(&mut self) -> Option<Box<dyn MarketTabs + '_>> {
        let tabs: Box<dyn MarketTabs + '_> = Box::new(Tabs(&mut self.moves));
        self.lends.then_some(tabs)
    }
    fn show_market_palette(&mut self, query: &str, target: Option<SwitchTarget>) -> bool {
        self.palette = Some((query.to_owned(), target.is_some()));
        self.lends
    }
}

fn choice(symbol: &str) -> OpenChoice {
    OpenChoice {
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        bar: String::new(),
        context_bars: Vec::new(),
        inherit_bar: true,
        workspace: "Default".to_owned(),
        background: false,
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
    crate::home_tab::show_home(&mut window).unwrap();
    assert!(window.surface.is_home());
    window.surface.observe(&shown(5, "BTCUSDT"));
    assert!(window.surface.is_home(), "the market Home hides is held");
    crate::home_tab::leave_home(&mut window).unwrap();
    assert!(!window.surface.is_home());
    crate::home_tab::show_home(&mut window).unwrap();
    window.surface.observe(&shown(6, "ETHUSDT"));
    assert!(!window.surface.is_home(), "another market leaves Home");
}

#[test]
fn a_window_that_lends_nothing_refuses_each_call_by_family() {
    let mut window = Window::new(false);
    let home = crate::home_tab::show_home(&mut window).unwrap_err();
    assert_eq!(home.message, "the Home tab is unavailable");
    assert_eq!(
        crate::home_tab::leave_home(&mut window)
            .unwrap_err()
            .message,
        "the Home tab is unavailable"
    );
    assert_eq!(
        crate::widgets::widget(&mut window, "tape", None).unwrap_err(),
        "widget panes are unavailable here"
    );
    assert_eq!(
        window.open_tab(&choice("X"), None).unwrap_err(),
        "markets cannot be opened here"
    );
    assert_eq!(
        window.edit_catalog("binance", "X", true).unwrap_err(),
        "markets cannot be opened here"
    );
    let bar = quantick_engine::bar_registry::BUILTIN_BARS
        .parse("tick:50")
        .unwrap();
    assert_eq!(
        window.set_flow_bar(5, bar).unwrap_err(),
        "markets cannot be opened here"
    );
    let palette = window.open_market_palette("", None).unwrap_err();
    assert_eq!(palette.message, "markets cannot be opened here");
    window.focus_tab(0);
    assert!(window.moves.is_empty() && window.asked.is_empty());
}

#[test]
fn widget_and_market_calls_reach_the_lent_owner() {
    let mut window = Window::new(true);
    assert_eq!(
        crate::widgets::widget(&mut window, "tape", Some(true)).unwrap_err(),
        "recorded"
    );
    assert_eq!(window.asked, ["tape Some(true)"]);
    assert_eq!(window.market_snapshot().starter_feed, "binance");
    window.focus_tab(2);
    assert_eq!(window.open_tab(&choice("ETHUSDT"), None), Ok(9));
    let bar = quantick_engine::bar_registry::BUILTIN_BARS
        .parse("tick:50")
        .unwrap();
    window.set_flow_bar(5, bar).unwrap();
    assert_eq!(
        window.moves,
        ["focus 2", "open ETHUSDT None", "bar 5 tick:50"]
    );
    window.open_market_palette("> ", None).unwrap();
    assert_eq!(window.palette, Some(("> ".to_owned(), false)));
}
