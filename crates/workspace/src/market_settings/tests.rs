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
