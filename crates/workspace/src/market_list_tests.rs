use super::*;

fn venue(feed_id: &str, name: &str, chip: char, kind: VenueKind, bar: &str) -> Venue {
    Venue {
        feed_id: feed_id.to_owned(),
        name: name.to_owned(),
        chip,
        kind,
        bar: bar.to_owned(),
        bridged: feed_id == "b3",
    }
}

fn market(feed_id: &str, symbol: &str, asset: &str) -> CatalogMarket {
    CatalogMarket {
        feed_id: feed_id.to_owned(),
        symbol: symbol.to_owned(),
        asset: asset.to_owned(),
        aliases: Vec::new(),
        user_added: false,
    }
}

fn venues() -> Vec<Venue> {
    vec![
        venue("binance", "Binance", 'B', VenueKind::Live, "tick:50"),
        venue(
            "hyperliquid",
            "Hyperliquid",
            'H',
            VenueKind::Live,
            "tick:50",
        ),
        venue("b3", "MetaTrader 5 · B3", '5', VenueKind::Live, "tick:2000"),
        venue("bybit", "Bybit", 'B', VenueKind::ComingSoon, "tick:50"),
    ]
}

fn markets() -> Vec<CatalogMarket> {
    let mut win = market("b3", "WIN$N", "WIN*");
    win.aliases = vec!["ibovespa".to_owned()];
    let mut winz = market("b3", "WINZ26", "WIN*");
    winz.aliases = vec!["ibovespa".to_owned()];
    let mut pepe = market("binance", "PEPEUSDT", "PEPEUSDT");
    pepe.user_added = true;
    vec![
        market("binance", "BTCUSDT", "BTCUSDT"),
        market("binance", "ETHUSDT", "ETHUSDT"),
        pepe,
        market("hyperliquid", "BTC", "BTC"),
        market("hyperliquid", "ETH", "ETH"),
        win,
        winz,
        market("b3", "WDOZ26", "WDO*"),
        market("bybit", "BTCUSDT", "BTCUSDT"),
    ]
}

fn open(tab_id: u64, position: usize, feed_id: &str, symbol: &str) -> OpenMarket {
    OpenMarket {
        tab_id,
        position,
        feed_id: feed_id.to_owned(),
        symbol: symbol.to_owned(),
        asset: symbol.to_owned(),
        workspace: "Default".to_owned(),
        bar: "tick:50".to_owned(),
        replay: false,
        streaming: true,
        price: Some("75,984.3".to_owned()),
        trades_per_s: Some(38),
    }
}

struct Fixture {
    venues: Vec<Venue>,
    markets: Vec<CatalogMarket>,
    open: Vec<OpenMarket>,
    recent: RecentMarkets,
}

impl Fixture {
    fn new() -> Self {
        Self {
            venues: venues(),
            markets: markets(),
            open: Vec::new(),
            recent: RecentMarkets::default(),
        }
    }

    fn world(&self) -> MarketWorld<'_> {
        MarketWorld {
            venues: &self.venues,
            markets: &self.markets,
            open: &self.open,
            recent: &self.recent,
            starter_feed: "binance",
        }
    }

    fn list(&self, query: &str) -> MarketListing {
        list_markets(&self.world(), query)
    }
}

fn headings(listing: &MarketListing) -> Vec<&str> {
    listing
        .groups
        .iter()
        .map(|group| group.heading.as_str())
        .collect()
}

fn symbols(group: &MarketGroup) -> Vec<(&str, &str)> {
    group
        .rows
        .iter()
        .map(|row| (row.feed_id.as_str(), row.symbol.as_str()))
        .collect()
}

#[test]
fn an_empty_query_with_nothing_recent_starts_here_then_one_group_per_venue() {
    let listing = Fixture::new().list("");
    assert_eq!(
        headings(&listing),
        [
            START_HERE_GROUP,
            "Hyperliquid",
            "MetaTrader 5 · B3",
            "Bybit"
        ],
        "the starter venue is shown once, under Start here"
    );
    assert_eq!(
        symbols(&listing.groups[0]),
        [
            ("binance", "BTCUSDT"),
            ("binance", "ETHUSDT"),
            ("binance", "PEPEUSDT")
        ]
    );
    assert_eq!(
        symbols(&listing.groups[2]),
        [("b3", "WIN$N"), ("b3", "WINZ26"), ("b3", "WDOZ26")]
    );
}

#[test]
fn recent_comes_first_open_tabs_then_recently_closed_and_start_here_goes() {
    let mut fixture = Fixture::new();
    fixture.open = vec![open(7, 1, "b3", "WINZ26"), open(9, 2, "binance", "BTCUSDT")];
    fixture.recent.closed(ClosedMarket {
        feed_id: "binance".to_owned(),
        symbol: "ETHUSDT".to_owned(),
        at: "10:02".to_owned(),
        price: Some("3,412.8".to_owned()),
    });
    fixture.recent.closed(ClosedMarket {
        feed_id: "b3".to_owned(),
        symbol: "WDOZ26".to_owned(),
        at: "18:29".to_owned(),
        price: Some("5,412.0".to_owned()),
    });
    let listing = fixture.list("");
    assert_eq!(
        headings(&listing),
        [
            RECENT_GROUP,
            "Binance",
            "Hyperliquid",
            "MetaTrader 5 · B3",
            "Bybit"
        ]
    );
    assert_eq!(
        symbols(&listing.groups[0]),
        [
            ("b3", "WINZ26"),
            ("binance", "BTCUSDT"),
            ("b3", "WDOZ26"),
            ("binance", "ETHUSDT")
        ],
        "open tabs in strip order, then the newest close first"
    );
    assert_eq!(symbols(&listing.groups[1]), [("binance", "PEPEUSDT")]);
    assert_eq!(symbols(&listing.groups[3]), [("b3", "WIN$N")]);
}

#[test]
fn a_typed_query_is_one_markets_group_kept_together_by_the_asset_key() {
    let listing = Fixture::new().list("btc");
    assert_eq!(headings(&listing), [MARKETS_GROUP]);
    assert_eq!(
        symbols(&listing.groups[0]),
        [
            ("binance", "BTCUSDT"),
            ("bybit", "BTCUSDT"),
            ("hyperliquid", "BTC")
        ],
        "rows sharing an asset key sit together, in the order the asset first appears"
    );
}

#[test]
fn the_filter_is_a_case_insensitive_substring_over_symbol_venue_asset_and_aliases() {
    let fixture = Fixture::new();
    let only = |query: &str| symbols(&fixture.list(query).groups[0]).len();
    assert_eq!(only("ETH"), only("eth"));
    assert_eq!(
        symbols(&fixture.list("HYPER").groups[0]),
        [("hyperliquid", "BTC"), ("hyperliquid", "ETH")],
        "the venue text matches"
    );
    assert_eq!(
        symbols(&fixture.list("wdo*").groups[0]),
        [("b3", "WDOZ26")],
        "the asset key matches"
    );
    assert_eq!(
        symbols(&fixture.list("Ibovespa").groups[0]),
        [("b3", "WIN$N"), ("b3", "WINZ26")],
        "an alias brings in its asset and nothing else"
    );
}

#[test]
fn a_replay_tab_is_a_replay_row_under_its_own_asset() {
    let mut fixture = Fixture::new();
    let mut replay = open(4, 1, "b3", "WINV26");
    replay.asset = "WIN*".to_owned();
    replay.replay = true;
    fixture.open = vec![replay];
    let listing = fixture.list("win");
    let rows = &listing.groups[0].rows;
    assert_eq!(
        symbols(&listing.groups[0]),
        [("b3", "WIN$N"), ("b3", "WINZ26"), ("b3", "WINV26")]
    );
    let replay = &rows[2];
    assert_eq!(
        (replay.chip, replay.venue.as_str(), replay.kind),
        ('R', REPLAY_VENUE, VenueKind::Replay)
    );
    assert_eq!(replay.status, MarketStatus::Replay);
    assert_eq!(
        rows[1].status,
        MarketStatus::Live,
        "a replay of WINV26 is not a live tab on WINZ26"
    );
}

#[test]
fn no_match_heads_the_query_and_offers_one_add_row_per_venue_that_could_list_it() {
    let listing = Fixture::new().list("  solusdt ");
    assert_eq!(
        headings(&listing),
        ["No market matches \u{201c}solusdt\u{201d}"]
    );
    let rows = &listing.groups[0].rows;
    assert_eq!(
        symbols(&listing.groups[0]),
        [
            ("binance", "SOLUSDT"),
            ("hyperliquid", "SOLUSDT"),
            ("b3", "SOLUSDT")
        ],
        "a coming-soon venue cannot list anything yet"
    );
    assert!(rows.iter().all(|row| row.status == MarketStatus::Add));
    assert_eq!(
        rows[2].bar, "tick:2000",
        "an add row opens on its venue's bar"
    );
}

#[test]
fn the_command_prefix_lists_no_markets() {
    let listing = Fixture::new().list("> work");
    assert_eq!(headings(&listing), [COMMANDS_GROUP]);
    assert!(listing.groups[0].rows.is_empty());
}

#[test]
fn every_status_string() {
    let cases = [
        (MarketStatus::Live, "live", StatusTone::Muted),
        (
            MarketStatus::OpenTab(2),
            "open \u{b7} tab 2",
            StatusTone::Open,
        ),
        (
            MarketStatus::WaitingForBridge,
            "waiting for bridge",
            StatusTone::Warn,
        ),
        (
            MarketStatus::Closed("18:29".to_owned()),
            "closed 18:29",
            StatusTone::Muted,
        ),
        (MarketStatus::Replay, "replay", StatusTone::Replay),
        (MarketStatus::ComingSoon, "coming soon", StatusTone::Muted),
        (
            MarketStatus::Add,
            "add to My markets and open",
            StatusTone::Add,
        ),
    ];
    for (status, label, tone) in cases {
        assert_eq!(status.label(), label);
        assert_eq!(status.tone(), tone, "{label}");
    }
}

#[test]
fn a_row_reads_its_status_price_and_rate_from_what_the_window_knows() {
    let mut fixture = Fixture::new();
    let mut waiting = open(3, 1, "b3", "WIN$N");
    (waiting.streaming, waiting.price, waiting.trades_per_s) = (false, None, None);
    fixture.open = vec![waiting, open(5, 2, "binance", "BTCUSDT")];
    fixture.recent.closed(ClosedMarket {
        feed_id: "b3".to_owned(),
        symbol: "WDOZ26".to_owned(),
        at: "18:29".to_owned(),
        price: Some("5,412.0".to_owned()),
    });
    let listing = fixture.list("");
    let row = |feed: &str, symbol: &str| {
        listing
            .rows()
            .find(|row| row.feed_id == feed && row.symbol == symbol)
            .expect("listed")
    };
    let win = row("b3", "WIN$N");
    assert_eq!(win.status, MarketStatus::WaitingForBridge);
    assert_eq!(
        win.hover.as_deref(),
        Some("Waiting for bridge \u{b7} attach the Quantick EA to WIN$N in MetaTrader")
    );
    let btc = row("binance", "BTCUSDT");
    assert_eq!(btc.status, MarketStatus::OpenTab(2));
    assert_eq!(
        (btc.price.as_deref(), btc.rate_label()),
        (Some("75,984.3"), Some("38/s".to_owned()))
    );
    let wdo = row("b3", "WDOZ26");
    assert_eq!(wdo.status, MarketStatus::Closed("18:29".to_owned()));
    assert_eq!(
        (wdo.price.as_deref(), wdo.price_dim),
        (Some("5,412.0"), true)
    );
    let eth = row("binance", "ETHUSDT");
    assert_eq!(eth.status, MarketStatus::Live);
    assert_eq!(
        (eth.price.as_deref(), eth.rate_label()),
        (None, None),
        "a market that has not streamed shows no price"
    );
    let bybit = row("bybit", "BTCUSDT");
    assert_eq!(bybit.status, MarketStatus::ComingSoon);
    assert!(!bybit.selectable());
}

#[test]
fn the_primary_label_says_what_enter_does() {
    let mut fixture = Fixture::new();
    let mut scalp = open(9, 2, "binance", "ETHUSDT");
    scalp.workspace = "Scalp".to_owned();
    let mut replay = open(11, 3, "b3", "WINV26");
    replay.replay = true;
    fixture.open = vec![open(5, 1, "binance", "BTCUSDT"), scalp, replay];
    let listing = fixture.list("");
    let row = |feed: &str, symbol: &str| {
        listing
            .rows()
            .find(|row| row.feed_id == feed && row.symbol == symbol)
            .expect("listed")
            .clone()
    };
    let mut unopened_replay = row("b3", "WINV26");
    unopened_replay.tabs.clear();
    let add = fixture.list("solusdt").groups[0].rows[0].clone();
    let table: [(MarketRow, &str, &str, Option<&str>); 9] = [
        (
            row("hyperliquid", "BTC"),
            "tick:50",
            "Default",
            Some("Open"),
        ),
        (
            row("binance", "BTCUSDT"),
            "tick:50",
            "Default",
            Some("Focus tab 1"),
        ),
        (
            row("binance", "BTCUSDT"),
            "tick:50",
            "Swing",
            Some("Open second tab"),
        ),
        (
            row("binance", "BTCUSDT"),
            "tick:200",
            "Default",
            Some("Open second tab"),
        ),
        (
            row("binance", "ETHUSDT"),
            "tick:50",
            "Scalp",
            Some("Focus tab 2"),
        ),
        (row("b3", "WINV26"), "tick:50", "Swing", Some("Focus tab 3")),
        (unopened_replay, "tick:50", "Default", Some("Open replay")),
        (add, "tick:50", "Default", Some("Add SOLUSDT and open")),
        (row("bybit", "BTCUSDT"), "tick:50", "Default", None),
    ];
    for (row, bar, workspace, label) in table {
        assert_eq!(
            primary_action(&row, bar, workspace)
                .map(|action| action.label())
                .as_deref(),
            label,
            "{} on {} at {bar} / {workspace}",
            row.symbol,
            row.feed_id
        );
    }
    assert_eq!(PrimaryAction::Run.label(), "Run");
}

#[test]
fn the_window_and_the_palette_focus_the_same_tab() {
    let mut second = open(6, 2, "binance", "BTCUSDT");
    second.workspace = "Swing".to_owned();
    let tabs = [open(5, 1, "binance", "BTCUSDT"), second];
    let found = |workspace: &str, bar: &str| {
        focus_target(&tabs, "binance", "BTCUSDT", bar, workspace).map(|tab| tab.tab_id)
    };
    assert_eq!(found("Swing", "tick:50"), Some(6));
    assert_eq!(found("Default", "tick:50"), Some(5));
    assert_eq!(found("Scalp", "tick:50"), None);
    assert_eq!(found("Default", "volume:5"), None);
    assert_eq!(
        focus_target(&tabs, "hyperliquid", "BTCUSDT", "tick:50", "Default"),
        None
    );
}

#[test]
fn a_market_opens_on_its_tab_bar_else_its_venue_bar() {
    let mut fixture = Fixture::new();
    let mut win = open(3, 1, "b3", "WINZ26");
    win.bar = "tick:3000".to_owned();
    fixture.open = vec![win];
    let world = fixture.world();
    assert_eq!(
        market_bar(&world, "b3", "WINZ26").as_deref(),
        Some("tick:3000")
    );
    assert_eq!(
        market_bar(&world, "b3", "WIN$N").as_deref(),
        Some("tick:2000")
    );
    assert_eq!(market_bar(&world, "b3", "NOPE"), None);
    let snapshot = MarketSnapshot {
        venues: fixture.venues.clone(),
        markets: fixture.markets.clone(),
        starter_feed: "binance".to_owned(),
        ..MarketSnapshot::default()
    };
    assert_eq!(
        list_markets(&snapshot.world(), ""),
        fixture_without_tabs().list("")
    );
}

fn fixture_without_tabs() -> Fixture {
    Fixture::new()
}

#[test]
fn recent_markets_keep_the_newest_close_once_and_stop_at_the_limit() {
    let mut recent = RecentMarkets::default();
    let closed = |symbol: &str, at: &str| ClosedMarket {
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        at: at.to_owned(),
        price: None,
    };
    for index in 0..RECENT_CLOSED_LIMIT + 3 {
        recent.closed(closed(&format!("S{index}"), "09:00"));
    }
    recent.closed(closed("S5", "10:15"));
    let entries = recent.entries();
    assert_eq!(entries.len(), RECENT_CLOSED_LIMIT);
    assert_eq!(
        (entries[0].symbol.as_str(), entries[0].at.as_str()),
        ("S5", "10:15")
    );
    assert_eq!(
        entries.iter().filter(|entry| entry.symbol == "S5").count(),
        1,
        "closing a market again moves it to the front"
    );
}

#[test]
fn removal_takes_only_a_market_the_trader_added_and_no_tab_shows() {
    let mut fixture = Fixture::new();
    assert_eq!(
        removal_refusal(&fixture.world(), "binance", "PEPEUSDT"),
        None
    );
    assert_eq!(
        removal_refusal(&fixture.world(), "binance", "BTCUSDT").as_deref(),
        Some("BTCUSDT comes from the config file; only a market you added can be removed")
    );
    assert_eq!(
        removal_refusal(&fixture.world(), "binance", "NOPE").as_deref(),
        Some("NOPE is not in this venue's list")
    );
    fixture.open = vec![open(5, 1, "binance", "PEPEUSDT")];
    assert_eq!(
        removal_refusal(&fixture.world(), "binance", "PEPEUSDT").as_deref(),
        Some("A tab is showing PEPEUSDT \u{2014} close it first")
    );
}

mod palette_tests {
    use super::*;

    #[test]
    fn the_toast_names_what_opened_or_which_tab_came_forward() {
        let mut choice = OpenChoice {
            feed_id: "binance".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            bar: "tick:50".to_owned(),
            workspace: "Swing".to_owned(),
            background: false,
        };
        assert_eq!(
            open_toast(&choice, None),
            "Opened BTCUSDT \u{b7} tick 50 \u{b7} Swing"
        );
        choice.background = true;
        assert_eq!(
            open_toast(&choice, None),
            "Opened in the background \u{b7} BTCUSDT \u{b7} tick 50 \u{b7} Swing"
        );
        assert_eq!(open_toast(&choice, Some(1)), "Focused BTCUSDT \u{b7} tab 1");
    }

    #[test]
    fn it_opens_empty_on_default_with_the_field_focused() {
        let palette = MarketPalette::default();
        assert_eq!(palette.query(), "");
        assert_eq!(palette.workspace(), DEFAULT_WORKSPACE);
        assert!(!palette.workspace_changed());
        assert_eq!(palette.focus(), PaletteFocus::Field);
        assert_eq!(palette.refusal(), None);
    }

    #[test]
    fn selection_skips_rows_that_cannot_be_chosen_and_wraps() {
        let fixture = Fixture::new();
        let listing = fixture.list("btc");
        let mut palette = MarketPalette::default();
        palette.set_query("btc");
        let picked = |palette: &MarketPalette| {
            let row = palette.selected_row(&listing).expect("a row");
            (row.feed_id.clone(), row.symbol.clone())
        };
        assert_eq!(
            picked(&palette),
            ("binance".to_owned(), "BTCUSDT".to_owned())
        );
        palette.move_selection(1, &listing);
        assert_eq!(
            picked(&palette),
            ("hyperliquid".to_owned(), "BTC".to_owned()),
            "the coming-soon Bybit row is passed over"
        );
        palette.move_selection(1, &listing);
        assert_eq!(
            picked(&palette),
            ("binance".to_owned(), "BTCUSDT".to_owned())
        );
        palette.move_selection(-1, &listing);
        assert_eq!(
            picked(&palette),
            ("hyperliquid".to_owned(), "BTC".to_owned())
        );
    }

    #[test]
    fn bars_follow_the_selected_row_until_the_trader_changes_them() {
        let mut fixture = Fixture::new();
        let mut win = open(3, 1, "b3", "WINZ26");
        win.bar = "tick:2000".to_owned();
        fixture.open = vec![win, open(5, 2, "binance", "BTCUSDT")];
        let listing = fixture.list("");
        let mut palette = MarketPalette::default();
        assert_eq!(palette.bar(&listing), "tick:2000");
        palette.move_selection(1, &listing);
        assert_eq!(palette.bar(&listing), "tick:50");
        assert!(!palette.bar_changed());
        palette.set_bar_kind("volume", "5");
        assert_eq!(palette.bar(&listing), "volume:5");
        palette.set_bar_size(&listing, "12");
        assert_eq!(palette.bar(&listing), "volume:12");
        palette.move_selection(-1, &listing);
        assert_eq!(
            palette.bar(&listing),
            "volume:12",
            "a change outlives the row"
        );
        assert!(palette.bar_changed());
        let mut sized = MarketPalette::default();
        sized.set_bar_size(&listing, "3000");
        assert_eq!(
            sized.bar(&listing),
            "tick:3000",
            "a size keeps the row's kind"
        );
    }

    #[test]
    fn tab_cycles_field_bars_workspace_and_back() {
        let mut palette = MarketPalette::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            palette.cycle_focus();
            seen.push(palette.focus());
        }
        assert_eq!(
            seen,
            [
                PaletteFocus::Bars,
                PaletteFocus::Workspace,
                PaletteFocus::Field,
                PaletteFocus::Bars
            ]
        );
    }

    #[test]
    fn enter_opens_with_the_footer_choices_and_ctrl_enter_in_the_background() {
        let fixture = Fixture::new();
        let listing = fixture.list("eth");
        let mut palette = MarketPalette::default();
        palette.set_query("eth");
        palette.set_workspace("Swing");
        assert!(palette.workspace_changed());
        let choice = OpenChoice {
            feed_id: "binance".to_owned(),
            symbol: "ETHUSDT".to_owned(),
            bar: "tick:50".to_owned(),
            workspace: "Swing".to_owned(),
            background: true,
        };
        assert_eq!(
            palette.submit(&listing, true),
            Some(PaletteIntent::Open(choice))
        );
    }

    #[test]
    fn an_add_row_asks_to_add_and_a_refusal_stays_until_the_next_submit() {
        let fixture = Fixture::new();
        let listing = fixture.list("solusdt");
        let mut palette = MarketPalette::default();
        palette.set_query("solusdt");
        let Some(PaletteIntent::Add(choice)) = palette.submit(&listing, false) else {
            panic!("an add row adds");
        };
        assert_eq!(
            (choice.feed_id.as_str(), choice.symbol.as_str()),
            ("binance", "SOLUSDT")
        );
        palette.refuse("Binance does not list SOLUSDT".to_owned());
        palette.set_query("solusd");
        assert_eq!(
            palette.refusal(),
            Some("Binance does not list SOLUSDT"),
            "typing a fix keeps the reason on screen"
        );
        let _ = palette.submit(&fixture.list("solusd"), false);
        assert_eq!(palette.refusal(), None);
    }

    #[test]
    fn delete_asks_to_remove_the_selected_market_and_nothing_on_an_add_row() {
        let fixture = Fixture::new();
        let listing = fixture.list("pepe");
        let mut palette = MarketPalette::default();
        palette.set_query("pepe");
        assert_eq!(
            palette.remove_selected(&listing),
            Some(PaletteIntent::Remove {
                feed_id: "binance".to_owned(),
                symbol: "PEPEUSDT".to_owned(),
            })
        );
        palette.set_query("solusdt");
        assert_eq!(palette.remove_selected(&fixture.list("solusdt")), None);
    }

    #[test]
    fn up_and_down_move_whatever_has_focus() {
        let fixture = Fixture::new();
        let listing = fixture.list("eth");
        let workspaces = ["Default".to_owned(), "Scalp".to_owned()];
        let size = |kind: &str| {
            if kind == "volume" {
                "5".to_owned()
            } else {
                "1".to_owned()
            }
        };
        let mut palette = MarketPalette::default();
        palette.set_query("eth");
        assert_eq!(palette.primary_label(&listing), "Open");
        palette.press(PaletteKey::Down, &listing, &workspaces, size);
        assert_eq!(palette.selected_row(&listing).unwrap().symbol, "ETH");
        palette.press(PaletteKey::Tab, &listing, &workspaces, size);
        palette.press(PaletteKey::Down, &listing, &workspaces, size);
        assert_eq!(
            palette.bar_parts(&listing),
            ("volume".to_owned(), "5".to_owned())
        );
        palette.press(PaletteKey::Tab, &listing, &workspaces, size);
        palette.press(PaletteKey::Up, &listing, &workspaces, size);
        assert_eq!(palette.workspace(), "Scalp");
        let enter = palette.press(
            PaletteKey::Enter { background: false },
            &listing,
            &workspaces,
            size,
        );
        assert!(matches!(enter, Some(PaletteIntent::Open(choice)) if choice.bar == "volume:5"));
        palette.set_query("> ");
        assert!(palette.commands_mode());
        assert_eq!(palette.primary_label(&fixture.list("> ")), "Run");
    }

    #[test]
    fn delete_leaves_a_market_the_config_declares() {
        let fixture = Fixture::new();
        let listing = fixture.list("btcusdt");
        let mut palette = MarketPalette::default();
        palette.set_query("btcusdt");
        assert_eq!(palette.remove_selected(&listing), None);
    }

    #[test]
    fn a_choice_fills_in_the_market_bar_and_default_and_refuses_strangers() {
        let fixture = Fixture::new();
        let snapshot = MarketSnapshot {
            venues: fixture.venues.clone(),
            markets: fixture.markets.clone(),
            workspaces: vec!["Default".to_owned()],
            ..MarketSnapshot::default()
        };
        let choice = snapshot.choice(("b3", "WINZ26"), None, None, true).unwrap();
        assert_eq!(
            (
                choice.bar.as_str(),
                choice.workspace.as_str(),
                choice.background
            ),
            ("tick:2000", "Default", true)
        );
        assert!(snapshot.choice(("b3", "NOPE"), None, None, false).is_err());
        let unknown = snapshot.choice(("b3", "WINZ26"), None, Some("Swing".to_owned()), false);
        assert_eq!(unknown.unwrap_err(), "No workspace is named Swing");
    }

    #[test]
    fn nothing_selectable_submits_nothing() {
        let fixture = Fixture::new();
        let mut palette = MarketPalette::default();
        palette.set_query(">");
        assert_eq!(palette.submit(&fixture.list(">"), false), None);
    }
}
