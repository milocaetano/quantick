//! Which of a market's marks a tab is lent from the store: the "All
//! windows" ones of its workspace's layouts that no chart of the tab holds.
use super::*;
use crate::{ChartPoint, DRAWING_TOOLS, persist_pane};
use quantick_workspace::layout_document::LayoutId;
use quantick_workspace::market_settings::MarketSettings;

const T0: i64 = 1_700_000_000_000;

fn scope(layout: u64) -> DrawingScope {
    DrawingScope {
        workspace: 7,
        layout: LayoutId(layout),
    }
}

fn key(scope: DrawingScope) -> DrawingKey {
    DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), None).in_scope(scope)
}

/// A store holding layout 1's set: a level on all timeframes with "All
/// windows" on, at 90, and one on all timeframes of its own layout, at 80.
fn store() -> MarketSettingsStore {
    let line = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("registered test tool");
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LayoutId(1)));
    for (price, all_windows) in [(90.0, true), (80.0, false)] {
        assert!(drawings.place(line, ChartPoint::at_time(2.0, price, Some(T0))));
        let mark = drawings.items_mut().last_mut().expect("placed");
        mark.set_all_timeframes(true);
        mark.all_windows = all_windows;
    }
    let profile = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .expect("registered test tool");
    assert!(!drawings.place(profile, ChartPoint::at_time(1.0, 70.0, Some(T0))));
    assert!(drawings.place(profile, ChartPoint::at_time(2.0, 75.0, Some(T0 + 1))));
    let mark = drawings.items_mut().last_mut().expect("placed");
    mark.set_all_timeframes(true);
    mark.all_windows = true;
    let mut settings = MarketSettings::default();
    let owner = key(scope(1));
    persist_pane(
        &mut drawings,
        &mut Vec::new(),
        &mut settings,
        &owner,
        "tick:100",
    );
    MarketSettingsStore::new(settings, std::path::PathBuf::new(), true)
}

fn lent_prices(lent: &LentMarks) -> Vec<f64> {
    let marks = lent.source().drawings.lent_to(None);
    marks.map(|(_, mark)| mark.points[0].price).collect()
}

#[test]
fn a_tab_is_lent_the_all_windows_marks_of_layouts_none_of_its_charts_holds() {
    let store = store();
    let elsewhere = DrawingScope {
        workspace: 8,
        ..scope(2)
    };
    let settling = DrawingKey {
        scope: None,
        ..key(scope(2))
    };
    for (held, shown, what) in [
        (
            vec![key(scope(2))],
            vec![90.0],
            "its All windows level only, no profile",
        ),
        (
            vec![key(scope(2)), key(scope(1))],
            vec![],
            "a chart holds it live",
        ),
        (
            vec![key(elsewhere)],
            vec![],
            "another workspace lends nothing",
        ),
        (
            vec![settling],
            vec![],
            "a chart still settling asks nothing",
        ),
    ] {
        let mut lent = LentMarks::default();
        lent.follow(&store, &held, &Drawings::default());
        assert_eq!(lent_prices(&lent), shown, "{what}");
    }
}

/// A switch made on the owning layout's chart in another tab reaches here
/// through the store: turned off, the mark leaves on the next follow.
#[test]
fn a_lent_mark_leaves_once_the_store_turns_it_off() {
    let mut store = store();
    let held = [key(scope(2))];
    let mut lent = LentMarks::default();
    lent.follow(&store, &held, &Drawings::default());
    assert_eq!(lent_prices(&lent), [90.0]);

    let set = key(scope(1)).all_timeframes_key();
    let settings = store.settings_mut();
    let mut items = settings.drawings(&set).to_vec();
    items.iter_mut().for_each(|entry| entry.all_windows = false);
    settings.replace(&set, items, |_| true);
    lent.follow(&store, &held, &Drawings::default());
    assert!(lent_prices(&lent).is_empty());
}

/// The charts each chart of a tab holding `keys`, in tab order, mirrors.
fn owners(keys: &[DrawingKey]) -> Vec<Vec<usize>> {
    let key = |chart: usize| keys.get(chart);
    let count = keys.len();
    (0..count)
        .map(|viewer| mirror_owners(count, viewer, key).collect())
        .collect()
}

/// A chart mirrors each set it does not hold once, from the first chart
/// holding it, and never the set it holds itself.
#[test]
fn a_chart_mirrors_each_set_it_does_not_hold_once() {
    let (one, two) = (key(scope(1)), key(scope(2)));
    let none: Vec<usize> = Vec::new();
    assert_eq!(
        owners(&[one.clone(), one.clone()]),
        [none.clone(), none.clone()],
        "each chart holds the one set"
    );
    assert_eq!(
        owners(&[one, two.clone(), two]),
        [vec![1], vec![0], vec![0]],
        "the first of two charts holding a set lends it"
    );
}

/// A chart still settling its layout (its key has no scope yet) holds a set
/// about to be replaced: it neither lends its marks nor borrows another's,
/// or a mark both hold would paint twice.
#[test]
fn a_chart_still_settling_its_layout_neither_lends_nor_borrows() {
    let settling = DrawingKey {
        scope: None,
        ..key(scope(2))
    };
    let (one, three) = (key(scope(1)), key(scope(3)));
    assert_eq!(
        owners(&[one, settling, three]),
        [vec![2], vec![], vec![0]],
        "the settling chart lends nothing and borrows nothing"
    );
}
