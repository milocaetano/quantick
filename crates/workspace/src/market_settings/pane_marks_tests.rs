use super::*;
use crate::layout_document::LayoutId;

const MERGED: &str = include_str!("../../fixtures/market-settings/merged.toml");

fn market(asset: &str) -> MarketKey {
    MarketKey {
        venue: "binance".to_owned(),
        asset: asset.to_owned(),
    }
}

fn flow(asset: &str) -> DrawingKey {
    DrawingKey::of_pane("binance", asset.to_owned(), None)
}

fn context(asset: &str, bars: &str) -> DrawingKey {
    DrawingKey::of_pane("binance", asset.to_owned(), Some(bars.to_owned()))
}

/// A dated level: every anchor names a market instant.
fn level(id: u64, price: f64, shared: bool) -> SavedDrawing {
    SavedDrawing {
        id: Some(id),
        shared,
        ..SavedDrawing::level(price)
    }
}

fn pinned(id: u64, price: f64, bars: &str) -> SavedDrawing {
    SavedDrawing {
        bars: Some(bars.to_owned()),
        ..level(id, price, false)
    }
}

fn known(tool: &str) -> bool {
    tool != "a-tool-from-the-future"
}

fn prices(items: &[SavedDrawing]) -> Vec<f64> {
    items.iter().map(|item| item.points[0].price).collect()
}

#[test]
fn scoped_all_timeframes_never_widen_workspace_layout_market_or_venue() {
    let scope = DrawingScope {
        workspace: 7,
        layout: LayoutId(3),
    };
    let key = flow("BTCUSDT").in_scope(scope);
    let mut settings = MarketSettings::default();
    let mut mark = level(1, 100.0, true);
    mark.all_windows = true;
    let write = settings.store_pane(&key, "time:1m", vec![mark], &[], known);
    let same = context("BTCUSDT", "time:5m").in_scope(scope);
    assert_eq!(prices(&settings.pane_drawings(&same, "time:5m")), [100.0]);
    assert!(write.reaches(&key, &same));
    let foreign = [
        flow("BTCUSDT").in_scope(DrawingScope {
            workspace: 8,
            ..scope
        }),
        flow("BTCUSDT").in_scope(DrawingScope {
            layout: LayoutId(4),
            ..scope
        }),
        flow("ETHUSDT").in_scope(scope),
        DrawingKey::of_pane("other", "BTCUSDT".to_owned(), None).in_scope(scope),
        flow("BTCUSDT"),
    ];
    for other in foreign {
        assert!(settings.pane_drawings(&other, "time:1m").is_empty());
        assert!(!write.reaches(&key, &other));
    }
    settings.store_pane(
        &key,
        "time:1m",
        vec![pinned(2, 90.0, "time:1m")],
        &settings.pane_drawings(&key, "time:1m"),
        known,
    );
    assert!(settings.pane_drawings(&key, "time:5m").is_empty());
    assert_eq!(prices(&settings.pane_drawings(&key, "time:1m")), [90.0]);
}

#[test]
fn legacy_scope_migration_preserves_unresolved_marks_and_existing_alert_identity() {
    let layouts = LayoutBook::default();
    let scope = DrawingScope {
        workspace: 7,
        layout: layouts.active_id(),
    };
    let key = flow("BTCUSDT");
    let scoped = key.clone().in_scope(scope);
    let mut mark = level(1, 100.0, false);
    mark.layout = Some(layouts.active_id());
    let identity = crate::drawing_identity::issue("document", &key, 1, "unique").unwrap();
    mark.durable_identity = Some(identity.clone());
    let mut unresolved = level(2, 90.0, true);
    unresolved.tool = "future-tool".to_owned();
    unresolved.layout = Some(LayoutId(999));
    let mut missing_origin = level(3, 80.0, true);
    missing_origin.origin = Some(crate::layout_document::SavedOrigin {
        layout: "removed layout".to_owned(),
        feed: "binance".to_owned(),
        symbol: "BTCUSDT".to_owned(),
        pane: 0,
        id: Some(3),
        fingerprint: 0,
    });
    let mut settings = MarketSettings::default();
    settings.replace(
        &key,
        vec![mark, unresolved.clone(), missing_origin.clone()],
        |_| true,
    );
    let other = key.clone().in_scope(DrawingScope {
        workspace: 8,
        ..scope
    });
    assert_eq!(
        settings.migrate_scopes(&[scoped.clone(), other], &layouts),
        0
    );
    assert_eq!(
        settings.migrate_scopes(std::slice::from_ref(&scoped), &layouts),
        1
    );
    assert_eq!(settings.drawings(&key), [unresolved, missing_origin]);
    assert_eq!(
        settings.drawings(&scoped)[0].durable_identity,
        Some(identity.clone())
    );
    assert!(crate::drawing_identity::belongs_to(&identity, &scoped));
    assert_eq!(settings.migrate_scopes(&[scoped], &layouts), 0);
    assert_eq!(
        parse_settings(&to_text(&settings).unwrap())
            .unwrap()
            .drawing_count(),
        3
    );
}

#[test]
fn legacy_without_owner_metadata_claims_only_a_sole_complete_candidate() {
    let layouts = LayoutBook::default();
    let key = flow("BTCUSDT");
    let scope = DrawingScope {
        workspace: 1,
        layout: layouts.active_id(),
    };
    let owner = key.clone().in_scope(scope);
    let other = key.clone().in_scope(DrawingScope {
        workspace: 2,
        ..scope
    });
    let mut settings = parse_settings(
        "version = 1\n[[drawings]]\nvenue = 'future'\nasset = 'unknown'\nrole = 'future-role'\n",
    )
    .unwrap();
    settings.replace(&key, vec![level(1, 100.0, false)], known);
    assert_eq!(
        settings.migrate_scopes(&[owner.clone(), other], &layouts),
        0
    );
    assert_eq!(settings.drawings(&key).len(), 1);
    settings.set_drawing_autosave(&owner, false);
    assert_eq!(
        settings.migrate_scopes(std::slice::from_ref(&owner), &layouts),
        0
    );
    settings.set_drawing_autosave(&owner, true);
    assert_eq!(
        settings.migrate_scopes(std::slice::from_ref(&owner), &layouts),
        1
    );
    assert_eq!(settings.drawings(&owner).len(), 1);
    assert!(
        settings
            .keys()
            .any(|key| key.role == PaneRole::Other("future-role".to_owned()))
    );
}

#[test]
fn deleting_a_layout_keeps_scoped_drawings_preferences_and_alert_identity_at_the_original_owner() {
    let mut layouts = LayoutBook::default();
    let original = layouts.active_id();
    let label = layouts.get(original).unwrap().name.clone();
    let neighbor = layouts.create(Some("Neighbor")).unwrap();
    let scope = DrawingScope {
        workspace: 7,
        layout: original,
    };
    let key = flow("BTCUSDT").in_scope(scope);
    let identity = crate::drawing_identity::issue("document", &key, 1, "unique").unwrap();
    let mut mark = level(1, 100.0, true);
    mark.layout = Some(original);
    mark.durable_identity = Some(identity.clone());
    let mut settings = MarketSettings::default();
    settings.replace(&key, vec![mark], known);
    settings.set_drawing_autosave(&key, false);
    let before = settings.clone();

    layouts.delete(original).unwrap();
    assert!(!settings.retarget_layout(original, neighbor));
    assert_eq!(settings, before);
    assert!(crate::drawing_identity::belongs_to(&identity, &key));
    let neighbor_key = key.clone().in_scope(DrawingScope {
        layout: neighbor,
        ..scope
    });
    assert!(settings.pane_drawings(&neighbor_key, "time:5m").is_empty());
    assert!(settings.drawing_autosave(&neighbor_key));

    let mut reopened = crate::layout_document::parse(&toml::to_string(&layouts).unwrap()).unwrap();
    let recreated = reopened.create(Some(&label)).unwrap();
    assert_ne!(recreated, original);
    let recreated_key = key.in_scope(DrawingScope {
        layout: recreated,
        ..scope
    });
    assert!(settings.pane_drawings(&recreated_key, "time:5m").is_empty());
    assert!(settings.drawing_autosave(&recreated_key));
}

#[test]
fn a_mark_for_all_timeframes_reaches_every_pane_of_its_market_at_any_bars() {
    let mut settings = MarketSettings::default();
    let drawn_on = context("BTCUSDT", "time:15m");
    let write = settings.store_pane(
        &drawn_on,
        "time:15m",
        vec![level(1, 100.0, true)],
        &[],
        known,
    );
    assert!(write.all_timeframes, "the market's set changed");

    for (key, bars) in [
        (context("BTCUSDT", "time:15m"), "time:15m"),
        (context("BTCUSDT", "time:5m"), "time:5m"),
        (context("BTCUSDT", "renko:20"), "renko:20"),
        (flow("BTCUSDT"), "tick:100"),
        (flow("BTCUSDT"), "time:5m"),
    ] {
        assert_eq!(
            prices(&settings.pane_drawings(&key, bars)),
            [100.0],
            "{key:?} at {bars}"
        );
    }
    assert!(
        settings
            .pane_drawings(&flow("ETHUSDT"), "tick:100")
            .is_empty(),
        "never across markets"
    );
    assert!(
        settings.drawings(&drawn_on).is_empty(),
        "kept once for the market, not under the chart it was drawn on"
    );
}

#[test]
fn a_mark_for_this_chart_shows_only_at_the_bars_it_was_pinned_to() {
    let mut settings = MarketSettings::default();
    let key = flow("BTCUSDT");
    settings.store_pane(
        &key,
        "time:15m",
        vec![pinned(1, 100.0, "time:15m")],
        &[],
        known,
    );

    assert_eq!(prices(&settings.pane_drawings(&key, "time:15m")), [100.0]);
    assert!(
        settings.pane_drawings(&key, "time:5m").is_empty(),
        "switching away hides it"
    );
    // The pane at 5m writes back what it shows; the 15m mark is not its to drop.
    settings.store_pane(&key, "time:5m", Vec::new(), &[], known);
    assert_eq!(
        prices(&settings.pane_drawings(&key, "time:15m")),
        [100.0],
        "switching back restores it"
    );
    assert!(
        settings
            .pane_drawings(&context("BTCUSDT", "time:15m"), "time:15m")
            .is_empty(),
        "another role at the same bars does not show it"
    );
}

#[test]
fn an_entry_from_before_pinning_shows_at_every_bar_rule_of_its_role() {
    let mut settings = MarketSettings::default();
    let key = flow("BTCUSDT");
    settings.replace(&key, vec![level(1, 100.0, false)], known);
    for bars in ["tick:100", "time:1m", "renko:20"] {
        assert_eq!(prices(&settings.pane_drawings(&key, bars)), [100.0]);
    }
    assert!(
        settings
            .pane_drawings(&context("BTCUSDT", "time:1m"), "time:1m")
            .is_empty()
    );
}

#[test]
fn a_shared_entry_from_before_reaches_every_pane_and_moves_only_on_an_edit() {
    let mut settings = MarketSettings::default();
    let old = context("BTCUSDT", "time:1m");
    settings.replace(
        &old,
        vec![level(1, 100.0, true), level(2, 90.0, false)],
        known,
    );
    let before = settings.clone();

    assert_eq!(
        prices(&settings.pane_drawings(&flow("BTCUSDT"), "tick:100")),
        [100.0]
    );
    assert_eq!(
        prices(&settings.pane_drawings(&old, "time:1m")),
        [100.0, 90.0]
    );

    // Written back as shown: nothing changed, nothing is rewritten.
    let shown = settings.pane_drawings(&flow("BTCUSDT"), "tick:100");
    let write = settings.store_pane(&flow("BTCUSDT"), "tick:100", shown.clone(), &shown, known);
    assert_eq!(write, PaneWrite::default());
    assert_eq!(
        settings, before,
        "an old file stays as it was until an edit"
    );

    // An edit moves it to the market's set, once.
    let mut moved = level(1, 101.0, true);
    moved.id = Some(5);
    let write = settings.store_pane(&flow("BTCUSDT"), "tick:100", vec![moved], &shown, known);
    assert!(write.all_timeframes);
    assert_eq!(prices(settings.drawings(&old)), [90.0]);
    assert_eq!(
        prices(&settings.pane_drawings(&old, "time:1m")),
        [101.0, 90.0]
    );
}

#[test]
fn a_write_keeps_what_the_pane_never_held() {
    let mut settings = MarketSettings::default();
    let key = flow("BTCUSDT");
    let mut future = level(9, 50.0, true);
    future.tool = "a-tool-from-the-future".to_owned();
    settings.replace(
        &DrawingKey::all_timeframes(market("BTCUSDT")),
        vec![future.clone()],
        |_| true,
    );
    settings.replace(&key, vec![pinned(3, 70.0, "time:1h")], known);

    settings.store_pane(
        &key,
        "time:5m",
        vec![level(1, 100.0, true), pinned(2, 80.0, "time:5m")],
        &[],
        known,
    );
    assert!(
        settings
            .drawings(&DrawingKey::all_timeframes(market("BTCUSDT")))
            .contains(&future)
    );
    // The future tool's mark is listed for the pane to skip, after the
    // pane's own: a write puts what it held first.
    assert_eq!(
        prices(&settings.pane_drawings(&key, "time:1h")),
        [100.0, 50.0, 70.0]
    );
    assert_eq!(
        prices(&settings.pane_drawings(&key, "time:5m")),
        [100.0, 50.0, 80.0]
    );
}

#[test]
fn an_undated_mark_stays_with_its_chart_whatever_its_scope() {
    let mut settings = MarketSettings::default();
    let mut undated = pinned(1, 100.0, "time:5m");
    undated.shared = true;
    undated.points[0].time_ms = None;
    let key = context("BTCUSDT", "time:5m");
    settings.store_pane(&key, "time:5m", vec![undated], &[], known);
    assert_eq!(prices(settings.drawings(&key)), [100.0]);
    assert!(
        settings
            .pane_drawings(&flow("BTCUSDT"), "time:5m")
            .is_empty(),
        "no instant to place it by on another cut"
    );
}

#[test]
fn a_write_names_the_panes_it_reaches() {
    let mut settings = MarketSettings::default();
    let writer = flow("BTCUSDT");
    let own = settings.store_pane(
        &writer,
        "time:5m",
        vec![pinned(1, 1.0, "time:5m")],
        &[],
        known,
    );
    assert_eq!(
        own,
        PaneWrite {
            all_timeframes: false,
            own: true,
            merged: false,
        }
    );
    assert!(own.reaches(&writer, &flow("BTCUSDT")));
    assert!(!own.reaches(&writer, &context("BTCUSDT", "time:5m")));

    let wide = settings.store_pane(&writer, "time:5m", vec![level(2, 2.0, true)], &[], known);
    assert!(wide.all_timeframes && wide.own);
    assert!(wide.reaches(&writer, &context("BTCUSDT", "renko:20")));
    assert!(!wide.reaches(&writer, &flow("ETHUSDT")));

    let none = settings.store_pane(&writer, "time:5m", vec![level(2, 2.0, true)], &[], known);
    assert!(!none.changed());
}

#[test]
fn a_store_with_both_kinds_reads_back_as_written() {
    let mut settings = MarketSettings::default();
    settings.store_pane(
        &flow("BTCUSDT"),
        "time:15m",
        vec![level(1, 100.0, true), pinned(2, 90.0, "time:15m")],
        &[],
        known,
    );
    settings.name_unnamed_drawings();
    let text = to_text(&settings).unwrap();
    assert!(text.contains("role = \"all-timeframes\""));
    assert!(text.contains("bars = \"time:15m\""));
    assert_eq!(parse_settings(&text).unwrap(), settings);
    assert_eq!(
        PaneRole::from(String::from(PaneRole::AllTimeframes)),
        PaneRole::AllTimeframes
    );
}

#[test]
fn an_old_file_loads_and_writes_back_unchanged_but_for_the_names_it_lacked() {
    let settings = parse_settings(MERGED).unwrap();
    let text = to_text(&settings).unwrap();
    assert_eq!(super::tests::without_names(&text), MERGED);
    assert!(
        !MERGED.contains("bars = "),
        "the fixture predates pinning, or this proves nothing"
    );
}

#[test]
fn held_changes_count_the_market_set_once_per_market() {
    let mut store = crate::market_settings_store::MarketSettingsStore::new(
        MarketSettings::default(),
        std::path::PathBuf::from(MARKET_SETTINGS_FILE),
        false,
    );
    store.set_held(true);
    let keys = [flow("BTCUSDT"), context("BTCUSDT", "time:1m")];
    store
        .settings_mut()
        .store_pane(&keys[0], "time:5m", vec![level(1, 100.0, true)], &[], known);
    assert_eq!(store.held_changes(&keys), 1);

    assert!(store.discard(&keys, &[]));
    assert!(
        store
            .settings()
            .drawings(&DrawingKey::all_timeframes(market("BTCUSDT")))
            .is_empty(),
        "closing every pane of the market drops its unsaved set"
    );
}

#[test]
fn a_recut_moves_nothing_unless_a_mark_is_pinned_to_either_cut() {
    let mut settings = MarketSettings::default();
    let key = flow("BTCUSDT");
    settings.replace(&key, vec![level(1, 100.0, false)], known);
    assert!(
        settings.same_marks_at(&key, "tick:100", "tick:200"),
        "an unpinned mark shows at both"
    );
    settings.store_pane(
        &key,
        "tick:100",
        vec![level(1, 100.0, false), pinned(2, 90.0, "tick:100")],
        &[],
        known,
    );
    assert!(!settings.same_marks_at(&key, "tick:100", "tick:200"));
    assert!(!settings.same_marks_at(&key, "tick:200", "tick:100"));
    assert!(settings.same_marks_at(&key, "tick:200", "tick:300"));
}

#[test]
fn the_highest_id_counts_every_set_of_the_market_and_no_other() {
    let mut settings = MarketSettings::default();
    settings.replace(&flow("BTCUSDT"), vec![level(4, 1.0, false)], known);
    settings.replace(
        &context("BTCUSDT", "time:1m"),
        vec![pinned(9, 1.0, "time:1m")],
        known,
    );
    settings.replace(&flow("ETHUSDT"), vec![level(50, 1.0, false)], known);
    assert_eq!(settings.max_drawing_id(&market("BTCUSDT")), 9);
    assert_eq!(settings.max_drawing_id(&market("SOLUSDT")), 0);
}

#[test]
fn a_write_from_a_stale_base_keeps_the_edit_it_did_not_see() {
    let mut settings = MarketSettings::default();
    let key = flow("BTCUSDT");
    let both = vec![level(1, 100.0, true), level(2, 200.0, true)];
    settings.store_pane(&key, "tick:100", both.clone(), &[], known);
    // Another chart moved mark 1; this one, still on `both`, moves mark 2.
    let other = vec![level(1, 101.0, true), level(2, 200.0, true)];
    settings.store_pane(&key, "tick:100", other, &both, known);
    let mine = vec![level(1, 100.0, true), level(2, 201.0, true)];
    let write = settings.store_pane(&key, "tick:100", mine, &both, known);
    assert!(write.merged && write.all_timeframes);
    assert_eq!(
        prices(&settings.pane_drawings(&key, "tick:100")),
        [101.0, 201.0]
    );
}

/// Autosave off: moving an old shared mark out of a chart nobody has open is
/// part of the save and of the discard, or the file holds it twice or loses it.
#[test]
fn a_held_save_and_discard_carry_the_move_out_of_an_old_role() {
    let held = || {
        let mut settings = MarketSettings::default();
        settings.replace(
            &context("BTCUSDT", "time:1m"),
            vec![level(1, 100.0, true), level(2, 90.0, false)],
            known,
        );
        let mut store = crate::market_settings_store::MarketSettingsStore::new(
            settings,
            std::path::PathBuf::from(MARKET_SETTINGS_FILE),
            false,
        );
        store.set_held(true);
        let shown = store.settings().pane_drawings(&flow("BTCUSDT"), "tick:100");
        let mut moved = shown[0].clone();
        moved.points[0].price = 101.0;
        store
            .settings_mut()
            .store_pane(&flow("BTCUSDT"), "tick:100", vec![moved], &shown, known);
        store
    };
    let open = [flow("BTCUSDT")];

    let store = held();
    let saved = store.commit_document(&open).expect("something to save");
    let on_disk = |key: &DrawingKey| prices(&saved.pane_drawings(key, "time:1m"));
    assert_eq!(on_disk(&flow("BTCUSDT")), [101.0], "moved once");
    assert_eq!(on_disk(&context("BTCUSDT", "time:1m")), [101.0, 90.0]);

    let mut store = held();
    assert!(store.discard(&open, &[]));
    assert_eq!(store.settings(), &store_before_edit(), "nothing lost");
}

fn store_before_edit() -> MarketSettings {
    let mut settings = MarketSettings::default();
    settings.replace(
        &context("BTCUSDT", "time:1m"),
        vec![level(1, 100.0, true), level(2, 90.0, false)],
        known,
    );
    settings
}
