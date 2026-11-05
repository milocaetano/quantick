//! Switch mode: the palette the toolbar's market button opens.
use super::*;

fn venue(feed_id: &str, name: &str, chip: char) -> Venue {
    Venue {
        feed_id: feed_id.to_owned(),
        name: name.to_owned(),
        chip,
        kind: VenueKind::Live,
        bar: Some("tick:50".to_owned()),
        bridged: false,
    }
}

fn market(feed_id: &str, symbol: &str) -> CatalogMarket {
    CatalogMarket {
        feed_id: feed_id.to_owned(),
        symbol: symbol.to_owned(),
        asset: symbol.to_owned(),
        aliases: Vec::new(),
        user_added: false,
    }
}

fn tab(tab_id: u64, feed_id: &str, symbol: &str, replay: bool) -> OpenMarket {
    OpenMarket {
        tab_id,
        position: tab_id as usize,
        feed_id: feed_id.to_owned(),
        symbol: symbol.to_owned(),
        asset: symbol.to_owned(),
        workspace: "Scalp".to_owned(),
        bar: "tick:2000".to_owned(),
        replay,
        streaming: true,
        price: None,
        trades_per_s: None,
    }
}

/// B3 with WINZ26 (tab 1, rolling to WINF27) and WDOZ26, Hyperliquid with
/// BTC, and a recording of WINV26 in tab 2.
fn snapshot() -> MarketSnapshot {
    MarketSnapshot {
        venues: vec![
            venue("b3", "MetaTrader 5 \u{b7} B3", '5'),
            venue("hyperliquid", "Hyperliquid", 'H'),
        ],
        markets: vec![
            market("b3", "WINZ26"),
            market("b3", "WINF27"),
            market("b3", "WDOZ26"),
            market("hyperliquid", "BTC"),
        ],
        open: vec![
            tab(1, "b3", "WINZ26", false),
            tab(2, "replay", "WINV26", true),
        ],
        workspaces: vec!["Default".to_owned(), "Scalp".to_owned()],
        ..MarketSnapshot::default()
    }
}

fn roll(refusal: Option<&str>) -> Option<RollOffer> {
    Some(RollOffer {
        target: "WINF27".to_owned(),
        refusal: refusal.map(str::to_owned),
    })
}

fn palette(roll: Option<RollOffer>) -> (MarketSnapshot, MarketPalette) {
    let snapshot = snapshot();
    let target = SwitchTarget::of(&snapshot, 1, roll).expect("tab 1 is live");
    (snapshot, MarketPalette::switching(target))
}

fn listing(snapshot: &MarketSnapshot, palette: &mut MarketPalette, query: &str) -> MarketListing {
    palette.set_query(query);
    palette.listing(&snapshot.world())
}

/// One row of the table: what each row says, whether Enter can choose it,
/// and what the primary label then reads.
fn table(listing: &MarketListing) -> Vec<(String, String, Option<String>)> {
    listing
        .rows()
        .map(|row| {
            let label = switch_action(row).map(|action| action.label());
            (row.symbol.clone(), row.status.label(), label)
        })
        .collect()
}

fn row(symbol: &str, status: &str, label: Option<&str>) -> (String, String, Option<String>) {
    (
        symbol.to_owned(),
        status.to_owned(),
        label.map(str::to_owned),
    )
}

#[test]
fn switch_mode_labels_skips_the_tab_and_shows_the_roll_first_without_selecting_it() {
    let (snapshot, mut palette) = palette(roll(None));
    let shown = listing(&snapshot, &mut palette, "");
    assert_eq!(shown.groups[0].heading, ROLL_GROUP, "the roll leads");
    assert_eq!(
        table(&shown),
        [
            row("WINF27", "Roll from WINZ26", Some("Roll to WINF27")),
            row("WINZ26", "this tab", None),
            row(
                "WDOZ26",
                "live",
                Some("Switch to WDOZ26 \u{b7} MetaTrader 5 \u{b7} B3")
            ),
            row("BTC", "live", Some("Switch to BTC \u{b7} Hyperliquid")),
        ],
        "no recording, and no plain row for the contract the roll offers"
    );
    assert_eq!(
        palette.primary_label(&shown),
        "Switch to WDOZ26 \u{b7} MetaTrader 5 \u{b7} B3",
        "the default is the first venue row: a reflex Enter never rolls"
    );
    palette.move_selection(1, &shown);
    assert_eq!(
        palette.primary_label(&shown),
        "Switch to BTC \u{b7} Hyperliquid"
    );
    palette.move_selection(-2, &shown);
    assert_eq!(
        palette.primary_label(&shown),
        "Roll to WINF27",
        "the roll is one deliberate move away, and selection skips the tab"
    );
}

#[test]
fn with_only_the_roll_left_nothing_is_selected_until_the_trader_moves() {
    let (snapshot, mut palette) = palette(roll(None));
    let shown = listing(&snapshot, &mut palette, "winz");
    assert_eq!(
        table(&shown),
        [
            row("WINF27", "Roll from WINZ26", Some("Roll to WINF27")),
            row("WINZ26", "this tab", None),
        ]
    );
    assert_eq!(palette.selected_row(&shown), None);
    assert_eq!(palette.submit(&shown, false), None, "Enter does not roll");
    palette.move_selection(1, &shown);
    assert_eq!(
        palette.submit(&shown, false),
        Some(PaletteIntent::Roll {
            tab_id: 1,
            from_symbol: "WINZ26".to_owned(),
        })
    );
}

#[test]
fn a_refused_roll_is_shown_greyed_and_cannot_be_selected() {
    let (snapshot, mut palette) = palette(roll(Some("Close the paper position first")));
    let shown = listing(&snapshot, &mut palette, "");
    let rolled = &shown.groups[0].rows[0];
    assert!(rolled.is_roll() && !rolled.selectable());
    assert_eq!(switch_action(rolled), None);
    for step in [1, 1, 1, -1, -1, -1, -1] {
        palette.move_selection(step, &shown);
        assert!(
            palette
                .selected_row(&shown)
                .is_some_and(|row| !row.is_roll()),
            "no move lands on a refused roll"
        );
    }
    palette.select(&shown, rolled);
    assert!(!palette.is_selected(&shown, rolled), "nor does a hover");
}

#[test]
fn the_roll_row_is_found_as_any_row_is() {
    let mut snapshot = snapshot();
    for market in &mut snapshot.markets {
        if market.symbol.starts_with("WIN") {
            market.asset = "WIN".to_owned();
            market.aliases = vec!["Mini indice".to_owned()];
        }
    }
    let target = SwitchTarget::of(&snapshot, 1, roll(None)).expect("tab 1 is live");
    let mut palette = MarketPalette::switching(target);
    for query in ["winf", "winz", "metatrader", "WIN", "mini"] {
        assert!(
            listing(&snapshot, &mut palette, query)
                .rows()
                .any(MarketRow::is_roll),
            "{query} finds the roll as it finds the plain rows"
        );
    }
    assert!(
        !listing(&snapshot, &mut palette, "hyper")
            .rows()
            .any(MarketRow::is_roll)
    );
}

#[test]
fn no_roll_target_means_no_roll_row() {
    let (snapshot, mut palette) = palette(None);
    let shown = listing(&snapshot, &mut palette, "");
    assert!(
        shown
            .rows()
            .all(|row| !matches!(row.status, MarketStatus::Roll { .. }))
    );
    assert!(shown.groups.iter().all(|group| group.heading != ROLL_GROUP));
}

#[test]
fn the_roll_row_follows_the_query_and_carries_its_refusal() {
    let (snapshot, mut palette) = palette(roll(Some("Close the paper position first")));
    let win = listing(&snapshot, &mut palette, "winf");
    assert_eq!(win.groups[0].heading, ROLL_GROUP);
    let rolled = &win.groups[0].rows[0];
    assert_eq!(
        rolled.hover.as_deref(),
        Some("Close the paper position first")
    );
    assert_eq!(
        win.rows().filter(|row| row.symbol == "WINF27").count(),
        1,
        "the roll target is listed once"
    );
    assert!(
        listing(&snapshot, &mut palette, "btc")
            .rows()
            .all(|row| !matches!(row.status, MarketStatus::Roll { .. })),
        "a query naming neither contract hides the roll"
    );
    assert!(
        listing(&snapshot, &mut palette, "> run")
            .rows()
            .next()
            .is_none()
    );
    let (snapshot, mut quiet) = self::palette(roll(None));
    let shown = listing(&snapshot, &mut quiet, "");
    assert_eq!(shown.groups[0].rows[0].hover.as_deref(), Some(ROLL_KEEPS));
}

#[test]
fn the_caption_names_the_tab_s_market_and_the_footer_starts_on_its_values() {
    let (snapshot, mut palette) = palette(None);
    let target = palette.switch_target().expect("switching");
    assert_eq!(
        target.caption(),
        "Switching WINZ26 \u{b7} MetaTrader 5 \u{b7} B3 on this tab \u{b7} drawings stay with each market"
    );
    let shown = listing(&snapshot, &mut palette, "");
    assert_eq!(
        palette.bar(&shown),
        "tick:2000",
        "the tab's bar, not the row's"
    );
    assert_eq!(palette.workspace(), "Scalp");
    assert!(!palette.bar_changed() && !palette.workspace_changed());
    palette.set_bar_kind("tick", "2000");
    assert!(
        !palette.bar_changed(),
        "picking the tab's own bar changes nothing"
    );
}

#[test]
fn enter_switches_with_only_what_the_trader_changed_and_ctrl_enter_is_inert() {
    let (snapshot, mut palette) = palette(None);
    let shown = listing(&snapshot, &mut palette, "btc");
    assert_eq!(palette.submit(&shown, true), None, "no background switch");
    assert_eq!(
        palette.submit(&shown, false),
        Some(PaletteIntent::Switch(SwitchChoice {
            tab_id: 1,
            feed_id: "hyperliquid".to_owned(),
            symbol: "BTC".to_owned(),
            bar: None,
            workspace: None,
            add: false,
        }))
    );
    palette.set_bar_size(&shown, "500");
    palette.set_workspace("Default");
    let Some(PaletteIntent::Switch(choice)) = palette.submit(&shown, false) else {
        panic!("a switch");
    };
    assert_eq!(
        (choice.bar.as_deref(), choice.workspace.as_deref()),
        (Some("tick:500"), Some("Default"))
    );
}

#[test]
fn enter_on_the_roll_row_rolls_from_the_tab_s_contract() {
    let (snapshot, mut palette) = palette(roll(None));
    let shown = listing(&snapshot, &mut palette, "");
    palette.move_selection(-1, &shown);
    assert_eq!(
        palette.submit(&shown, false),
        Some(PaletteIntent::Roll {
            tab_id: 1,
            from_symbol: "WINZ26".to_owned(),
        })
    );
    assert_eq!(
        palette.remove_selected(&shown),
        None,
        "Delete leaves the roll alone"
    );
}

#[test]
fn a_no_match_row_adds_then_switches() {
    let (snapshot, mut palette) = palette(None);
    let shown = listing(&snapshot, &mut palette, "SOLUSDT");
    assert_eq!(palette.primary_label(&shown), "Add SOLUSDT and switch");
    let Some(PaletteIntent::Switch(choice)) = palette.submit(&shown, false) else {
        panic!("a switch");
    };
    assert!(choice.add);
}

#[test]
fn only_the_tab_s_own_row_is_left_and_nothing_submits() {
    let mut snapshot = snapshot();
    snapshot.markets.retain(|market| market.symbol == "WINZ26");
    let target = SwitchTarget::of(&snapshot, 1, None).expect("tab 1 is live");
    let mut palette = MarketPalette::switching(target);
    let shown = palette.listing(&snapshot.world());
    assert_eq!(palette.primary_label(&shown), "Switch");
    assert_eq!(palette.submit(&shown, false), None);
}

#[test]
fn a_recording_or_a_missing_tab_has_no_switch_target() {
    let snapshot = snapshot();
    assert!(
        SwitchTarget::of(&snapshot, 2, None).is_none(),
        "a recording"
    );
    assert!(
        SwitchTarget::of(&snapshot, 9, None).is_none(),
        "no such tab"
    );
}

#[test]
fn the_open_mode_is_untouched() {
    let snapshot = snapshot();
    let mut palette = MarketPalette::default();
    let shown = listing(&snapshot, &mut palette, "");
    assert!(shown.rows().all(|row| row.status != MarketStatus::ThisTab));
    assert!(shown.groups.iter().all(|group| group.heading != ROLL_GROUP));
    assert_eq!(palette.primary_label(&shown), "Open second tab");
}
