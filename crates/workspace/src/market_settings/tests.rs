use super::*;
use crate::layout_document::{SavedBand, SavedPoint, parse};

const LEGACY: &str = include_str!("../../fixtures/market-settings/legacy-layouts.toml");
const MERGED: &str = include_str!("../../fixtures/market-settings/merged.toml");
const STRIPPED: &str = include_str!("../../fixtures/market-settings/stripped-layouts.toml");

/// What the shipped config answers for these symbols, without the config.
fn asset(_venue: &str, symbol: &str) -> String {
    if symbol.starts_with("WIN") {
        "WIN*".to_owned()
    } else {
        symbol.to_owned()
    }
}

fn win(venue: &str) -> MarketKey {
    MarketKey {
        venue: venue.to_owned(),
        asset: "WIN*".to_owned(),
    }
}

fn key(venue: &str, role: PaneRole) -> DrawingKey {
    DrawingKey {
        market: win(venue),
        role,
    }
}

fn context(bars: &str) -> PaneRole {
    PaneRole::Context {
        bars: bars.to_owned(),
    }
}

fn level(id: Option<u64>, price: f64) -> SavedDrawing {
    SavedDrawing {
        id,
        tool: "horizontal-line".to_owned(),
        name: None,
        author: None,
        points: vec![SavedPoint {
            time_ms: Some(1_000),
            price,
            bar: 3.0,
        }],
        band: SavedBand::Price,
        color: [255, 200, 0, 255],
        width_px: 1.5,
        fill_alpha: 0,
        locked: false,
        hidden: false,
        magnet: false,
        shared: false,
        payload: None,
        text: None,
        origin: None,
    }
}

fn known(tool: &str) -> bool {
    tool != "a-tool-from-the-future"
}

fn absorbed() -> MarketSettings {
    let mut settings = MarketSettings::default();
    settings.absorb(&mut parse(LEGACY).unwrap(), asset);
    settings
}

#[test]
fn a_market_spread_over_layouts_and_contracts_merges_into_the_golden_file() {
    let mut book = parse(LEGACY).unwrap();
    let mut settings = MarketSettings::default();
    let report = settings.absorb(&mut book, asset);
    assert_eq!(to_text(&settings).unwrap(), MERGED);
    assert_eq!(report.moved, 8);
    assert_eq!(report.already_held, 0);
}

#[test]
fn the_merge_leaves_the_layouts_and_takes_only_their_drawings() {
    let mut book = parse(LEGACY).unwrap();
    assert!(book.has_legacy_drawings());
    MarketSettings::default().absorb(&mut book, asset);
    assert!(!book.has_legacy_drawings());
    assert_eq!(toml::to_string_pretty(&book).unwrap(), STRIPPED);
}

/// Conservation, not a sample: every legacy entry is in the store, under the
/// layout, contract and pane it came from.
#[test]
fn every_drawing_keeps_its_source_and_none_is_dropped() {
    let settings = absorbed();
    assert_eq!(settings.drawing_count(), 8);
    let slot = settings.drawings(&key("metatrader-b3", PaneRole::ContextSlot(2)));
    let origins: Vec<_> = slot
        .iter()
        .map(|item| {
            let origin = item
                .origin
                .as_ref()
                .expect("every migrated entry is labelled");
            (
                origin.layout.as_str(),
                origin.symbol.as_str(),
                origin.pane,
                origin.id,
            )
        })
        .collect();
    assert_eq!(
        origins,
        [
            ("Fluxo", "WINV26", 2, Some(7)),
            ("Force Bar Only", "WINZ26", 2, Some(7)),
            ("Force Bar Only", "WINZ26", 2, None),
        ]
    );
}

#[test]
fn colliding_ids_are_renumbered_and_the_original_stays_on_the_label() {
    let settings = absorbed();
    let flow = settings.drawings(&key("metatrader-b3", PaneRole::Flow));
    let ids: Vec<_> = flow.iter().map(|item| item.id).collect();
    assert_eq!(ids, [Some(1), Some(2), Some(3)], "unique inside one set");
    assert_eq!(flow[2].origin.as_ref().unwrap().id, Some(1));
}

/// The launch that dies between the two writes finds the old drawings still
/// in the layouts file; importing one old export twice does too.
#[test]
fn absorbing_the_same_layouts_again_adds_nothing() {
    let mut settings = absorbed();
    let once = settings.clone();
    let report = settings.absorb(&mut parse(LEGACY).unwrap(), asset);
    assert_eq!(settings, once);
    assert_eq!((report.moved, report.already_held), (0, 8));
}

/// A pane write re-saves every entry: bars re-anchor, colours pass through the
/// live form. The source is what says it is the same entry.
#[test]
fn an_entry_edited_since_is_not_taken_again() {
    let mut settings = absorbed();
    let key = key("metatrader-b3", PaneRole::Flow);
    let mut items = settings.drawings(&key).to_vec();
    items[0].points[0].bar = 99.5;
    items[0].points[0].price = 1.0;
    items[0].color = [9, 9, 9, 9];
    settings.replace(&key, items, known);
    let edited = settings.clone();
    let report = settings.absorb(&mut parse(LEGACY).unwrap(), asset);
    assert_eq!(settings, edited);
    assert_eq!((report.moved, report.already_held), (0, 8));
}

/// Two equal entries from before ids travelled are two marks, on the first
/// migration and not again on the second.
#[test]
fn two_equal_entries_without_ids_are_both_kept_once() {
    let text = LEGACY.replace(
        "[[layouts.drawings.items]]\nid = 4\n",
        "[[layouts.drawings.items]]\n",
    );
    let twice = text.replace(
        "[[layouts.drawings]]\nfeed = \"metatrader-b3\"\nsymbol = \"WIN$N\"",
        "[[layouts.drawings.items]]\ntool = \"horizontal-line\"\nband = { axis = \"price\" }\ncolor = [255, 200, 0, 255]\nwidth_px = 1.5\nfill_alpha = 0\npoints = [{ time_ms = 1000, price = 67000.5, bar = 3.0 }]\n\n[[layouts.drawings]]\nfeed = \"metatrader-b3\"\nsymbol = \"WIN$N\"",
    );
    let btc = DrawingKey {
        market: MarketKey {
            venue: "binance".to_owned(),
            asset: "BTCUSDT".to_owned(),
        },
        role: PaneRole::ContextSlot(1),
    };
    let mut settings = MarketSettings::default();
    let first = settings.absorb(&mut parse(&twice).unwrap(), asset);
    assert_eq!(first.moved, 9);
    assert_eq!(settings.drawings(&btc).len(), 2);
    let second = settings.absorb(&mut parse(&twice).unwrap(), asset);
    assert_eq!((second.moved, second.already_held), (0, 9));
    assert_eq!(settings.drawings(&btc).len(), 2);
}

/// Ids are local counters: another export can reuse one for a different mark.
#[test]
fn a_different_mark_under_a_reused_id_is_kept() {
    let mut settings = absorbed();
    let other = LEGACY.replace("price = 67000.5", "price = 70000.5");
    let report = settings.absorb(&mut parse(&other).unwrap(), asset);
    assert_eq!((report.moved, report.already_held), (1, 7));
    assert_eq!(settings.drawing_count(), 9);
}

#[test]
fn a_roll_inside_one_asset_moves_nothing() {
    let mut settings = absorbed();
    let before = settings.clone();
    let market = win("metatrader-b3");
    assert_eq!(settings.carry_market(&market, &market), 0);
    assert_eq!(settings, before);
}

#[test]
fn a_roll_onto_another_asset_copies_each_role_and_never_duplicates() {
    let market = |asset: &str| MarketKey {
        venue: "binance".to_owned(),
        asset: asset.to_owned(),
    };
    let on = |asset: &str, role: PaneRole| DrawingKey {
        market: market(asset),
        role,
    };
    let (flow, top) = (PaneRole::Flow, context("time:1m"));
    let mut settings = MarketSettings::default();
    settings.replace(&on("OLD", flow.clone()), vec![level(Some(1), 90.0)], known);
    settings.replace(&on("OLD", top.clone()), vec![level(Some(1), 100.0)], known);
    settings.replace(&on("NEW", top.clone()), vec![level(Some(1), 120.0)], known);

    assert_eq!(settings.carry_market(&market("OLD"), &market("NEW")), 2);
    assert_eq!(settings.drawings(&on("NEW", flow.clone())).len(), 1);
    assert_eq!(
        settings.drawings(&on("NEW", top.clone()))[0].points[0].price,
        120.0,
        "what the new contract held stays"
    );
    assert_eq!(settings.drawings(&on("NEW", top.clone())).len(), 2);
    assert_eq!(
        settings.drawings(&on("OLD", flow.clone())).len(),
        1,
        "the old contract keeps its copy"
    );
    assert_eq!(
        settings.carry_market(&market("OLD"), &market("NEW")),
        0,
        "rolling again adds nothing"
    );

    // A visit re-anchors the copy and a pane may renumber it: still one mark.
    let mut visited = level(Some(40), 90.0);
    visited.points[0].bar += 17.0;
    settings.replace(&on("NEW", flow), vec![visited], known);
    assert_eq!(
        settings.carry_market(&market("OLD"), &market("NEW")),
        0,
        "a mark the new contract already shows is not added twice"
    );
}

#[test]
fn a_venue_is_part_of_the_market() {
    let settings = absorbed();
    assert_eq!(settings.drawings(&key("replay", PaneRole::Flow)).len(), 1);
    assert_eq!(
        settings
            .drawings(&key("metatrader-b3", PaneRole::Flow))
            .len(),
        3
    );
    assert!(
        settings
            .drawings(&key("elsewhere", PaneRole::Flow))
            .is_empty()
    );
}

#[test]
fn a_pane_claims_its_slot_once_under_its_interval() {
    let mut settings = absorbed();
    let market = win("metatrader-b3");
    let role = context("time:1m");
    settings.replace(
        &key("metatrader-b3", role.clone()),
        vec![level(Some(7), 1.0)],
        known,
    );

    assert!(settings.claim_slot(&market, 2, "time:1m"));
    assert!(!settings.claim_slot(&market, 2, "time:1m"), "claimed once");
    assert!(!settings.claim_slot(&market, 1, "time:1m"), "no such slot");

    assert!(
        settings
            .drawings(&key("metatrader-b3", PaneRole::ContextSlot(2)))
            .is_empty()
    );
    let claimed = settings.drawings(&key("metatrader-b3", role));
    let ids: Vec<_> = claimed.iter().map(|item| item.id).collect();
    assert_eq!(ids, [Some(7), Some(8), Some(9), None]);
    assert_eq!(claimed[1].origin.as_ref().unwrap().layout, "Fluxo");
}

#[test]
fn a_slot_nobody_opens_stays_in_the_file() {
    let settings = absorbed();
    let text = to_text(&settings).unwrap();
    assert!(text.contains("role = \"context-slot:2\""));
    assert_eq!(parse_settings(&text).unwrap(), settings);
}

#[test]
fn replacing_a_set_keeps_unknown_tools_and_an_empty_set_leaves_no_entry() {
    let mut settings = MarketSettings::default();
    let key = key("binance", PaneRole::Flow);
    let mut future = level(Some(2), 50.0);
    future.tool = "a-tool-from-the-future".to_owned();
    settings.replace(&key, vec![level(Some(1), 100.0), future.clone()], known);
    settings.replace(&key, vec![level(Some(3), 101.0)], known);
    assert_eq!(settings.drawings(&key).len(), 2);
    assert!(settings.drawings(&key).contains(&future));
    settings.replace(&key, Vec::new(), known);
    assert_eq!(
        settings.drawings(&key),
        [future],
        "what the pane never held"
    );
    settings.replace(&key, Vec::new(), |_| true);
    assert!(settings.drawings(&key).is_empty());
    assert_eq!(to_text(&settings).unwrap(), "version = 1\n");
}

#[test]
fn sets_are_kept_in_key_order_whatever_order_they_were_drawn_in() {
    let mut settings = MarketSettings::default();
    settings.replace(
        &key("b", context("time:5m")),
        vec![level(Some(1), 1.0)],
        known,
    );
    settings.replace(
        &key("a", context("time:1m")),
        vec![level(Some(1), 2.0)],
        known,
    );
    settings.replace(&key("b", PaneRole::Flow), vec![level(Some(1), 3.0)], known);
    let keys: Vec<_> = settings.keys().collect();
    assert_eq!(
        keys,
        [
            key("a", context("time:1m")),
            key("b", PaneRole::Flow),
            key("b", context("time:5m")),
        ]
    );
}

#[test]
fn a_role_this_build_does_not_know_survives_a_rewrite() {
    let text = "version = 1\n\n[[drawings]]\nvenue = \"v\"\nasset = \"a\"\nrole = \"split:left\"\n";
    let settings = parse_settings(text).unwrap();
    assert_eq!(
        settings.keys().next().unwrap().role,
        PaneRole::Other("split:left".to_owned())
    );
    assert!(
        to_text(&settings)
            .unwrap()
            .contains("role = \"split:left\"")
    );
}

#[test]
fn every_role_reads_back_as_written() {
    for role in [
        PaneRole::Flow,
        context("time:1m"),
        context("renko:20"),
        PaneRole::ContextSlot(3),
    ] {
        assert_eq!(PaneRole::from(String::from(role.clone())), role);
    }
    assert_eq!(String::from(context("time:1m")), "context:time:1m");
    assert_eq!(String::from(PaneRole::ContextSlot(3)), "context-slot:3");
}

#[test]
fn a_file_from_a_newer_format_is_refused_for_its_version() {
    assert!(matches!(
        parse_settings("version = 99\n"),
        Err(SettingsError::Version(99))
    ));
    assert!(matches!(
        parse_settings("not toml ["),
        Err(SettingsError::Malformed(_))
    ));
}

#[test]
fn a_recording_is_its_own_market() {
    assert_eq!(shown_market(("mt5", "WINZ26"), None), ("mt5", "WINZ26"));
    assert_eq!(
        shown_market(("mt5", "WINZ26"), Some("WINV26")),
        (REPLAY_FEED_KEY, "WINV26")
    );
}

#[test]
fn bar_progress_on_the_price_tag_is_on_until_a_market_turns_it_off() {
    let mut settings = MarketSettings::default();
    let b3 = win("MetaTrader 5 — B3");
    let other = win("Replay");
    assert!(settings.bar_progress_on(&b3), "on by default");
    assert!(settings.set_bar_progress(&b3, false), "a change");
    assert!(
        !settings.set_bar_progress(&b3, false),
        "the same answer is no change"
    );
    assert!(!settings.bar_progress_on(&b3));
    assert!(
        settings.bar_progress_on(&other),
        "one market's switch leaves another's"
    );

    let text = toml::to_string_pretty(&settings).unwrap();
    let read: MarketSettings = toml::from_str(&text).unwrap();
    assert!(
        !read.bar_progress_on(&b3),
        "the switch round-trips:\n{text}"
    );
    assert!(read.bar_progress_on(&other));

    assert!(settings.set_bar_progress(&b3, true));
    assert_eq!(
        toml::to_string_pretty(&settings).unwrap(),
        toml::to_string_pretty(&MarketSettings::default()).unwrap(),
        "back on leaves nothing in the file"
    );
}

/// A hand-edited file may list the switched-off markets out of order, or one
/// twice: every listed market reads off, and switching one back on clears it.
#[test]
fn bar_progress_off_reads_any_order_and_duplicates() {
    let text = r#"
version = 1

[[bar_progress_off]]
venue = "mt5"
asset = "WIN"

[[bar_progress_off]]
venue = "binance"
asset = "BTCUSDT"

[[bar_progress_off]]
venue = "mt5"
asset = "WIN"

[[bar_progress_off]]
venue = "hyperliquid"
asset = "BTC"
"#;
    let mut settings: MarketSettings = toml::from_str(text).unwrap();
    let key = |venue: &str, asset: &str| MarketKey {
        venue: venue.to_owned(),
        asset: asset.to_owned(),
    };
    for (venue, asset) in [
        ("mt5", "WIN"),
        ("binance", "BTCUSDT"),
        ("hyperliquid", "BTC"),
    ] {
        assert!(
            !settings.bar_progress_on(&key(venue, asset)),
            "{venue} {asset}"
        );
    }
    assert!(settings.bar_progress_on(&key("mt5", "WDO")));
    assert!(settings.set_bar_progress(&key("mt5", "WIN"), true));
    assert!(
        settings.bar_progress_on(&key("mt5", "WIN")),
        "the duplicate goes with it"
    );
    assert!(!settings.set_bar_progress(&key("binance", "BTCUSDT"), false));
}
