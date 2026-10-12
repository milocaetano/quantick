// The ledger's state between frames: the view the control plane sets, the
// cut that follows it, the keys, and what a reload prunes.

use std::collections::{BTreeMap, BTreeSet};

use quantick_civil::{CivilDate, DateRange, TzOffset};
use quantick_engine::Side;
use quantick_sim::history::SessionSource;
use quantick_sim::{ClosedTrade, ExitReason};
use rust_decimal::Decimal;

use super::*;
use crate::report::sessions::SessionInfo;
use crate::report::{HistoryRow, LoadedHistory};

const UTC: TzOffset = TzOffset::new(0);
/// 2026-03-16 13:00:00 UTC.
const DAY_START: i64 = 1_773_666_000_000;

fn key(file: &str) -> SessionKey {
    SessionKey::parse(&format!("WINJ26/{file}")).expect("a session key")
}

fn trade(closed_ms: i64) -> ClosedTrade {
    ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(101),
        opened_ms: closed_ms - 1000,
        closed_ms,
        pnl_points: Decimal::ONE,
        exit_reason: ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}

fn at(trade: &ClosedTrade) -> TradeRef {
    TradeRef {
        opened_ms: trade.opened_ms,
        closed_ms: trade.closed_ms,
        nth: 0,
    }
}

/// A history holding one trade in each of `files`, the first one tagged.
fn history(files: &[&str]) -> LoadedHistory {
    let mut rows = Vec::new();
    let mut sessions = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        let session = key(file);
        let closed = trade(DAY_START + i64::try_from(index).unwrap() * 60_000);
        let tags = if index == 0 {
            vec!["msc".to_owned()]
        } else {
            Vec::new()
        };
        let mut info = SessionInfo {
            key: session.clone(),
            symbol: "WINJ26".to_owned(),
            source: Some(SessionSource::Replay),
            name: None,
            live: false,
            trades: 1,
            net: Decimal::ONE,
            first_closed_ms: Some(closed.closed_ms),
            tags: BTreeMap::new(),
        };
        if !tags.is_empty() {
            info.tags.insert(at(&closed), tags.clone());
        }
        sessions.insert(session.clone(), info);
        rows.push(HistoryRow {
            symbol: "WINJ26".to_owned(),
            source: Some(SessionSource::Replay),
            session: Some(session),
            trade_ref: at(&closed),
            tags,
            trade: closed,
        });
    }
    LoadedHistory {
        rows,
        files: files.len(),
        unreadable_files: 0,
        problem_rows: 0,
        sessions,
    }
}

#[test]
fn a_session_gone_from_the_folder_leaves_every_piece_of_ledger_state() {
    let kept = key("a.csv");
    let gone = key("b.csv");
    let both = history(&["a.csv", "b.csv"]);
    let mut state = LedgerState::default();
    state.reloaded(&both);
    let moved = state.apply(
        SessionPress::StartDelete(gone.clone(), "Replay".to_owned(), 1),
        Some(&both),
    );
    assert_eq!(moved, None, "a confirm row, not yet an edit");
    state.view.set_folded(gone.clone(), true);
    state.view.set_folded(kept.clone(), true);
    state.selected_saved = Some((gone.clone(), at(&both.rows[1].trade)));
    state.tag_text = Some((gone.clone(), at(&both.rows[1].trade), "#x".to_owned()));
    assert_eq!(state.focused.as_ref(), Some(&gone));

    // The delete happened (here, elsewhere, by hand); the folder is re-read.
    state.reloaded(&history(&["a.csv"]));
    assert_eq!(state.focused, None, "Del must not act on it again");
    assert_eq!(state.edit, None, "no confirm for a session already gone");
    assert_eq!(state.selected_saved, None);
    assert_eq!(state.tag_text, None);
    assert_eq!(state.view.folded, BTreeSet::from([kept]), "only its fold");
}

#[test]
fn a_view_change_sets_what_it_names_and_says_whether_anything_moved() {
    let mut view = LedgerView::default();
    let day = CivilDate::from_ms(DAY_START, UTC);
    let change = LedgerViewChange {
        source: Some(SourceFilter::Replay),
        range: Some(Some(DateRange::new(day, day))),
        search: Some("#msc".to_owned()),
        fold: vec![(key("a.csv"), true)],
        ..LedgerViewChange::default()
    };
    assert!(view.apply(&change));
    assert_eq!(view.filter.source, SourceFilter::Replay);
    assert_eq!(view.filter.search, "#msc");
    assert!(view.folded.contains(&key("a.csv")));
    assert!(!view.apply(&change), "the same change again moves nothing");
    assert!(
        !view.apply(&LedgerViewChange::default()),
        "an empty change only reads"
    );

    let reset = LedgerViewChange {
        clear: true,
        fold: vec![(key("a.csv"), false)],
        ..LedgerViewChange::default()
    };
    assert!(view.apply(&reset));
    assert!(view.filter.is_default());
    assert!(view.folded.is_empty());
}

#[test]
fn the_cut_follows_a_filter_set_from_outside_the_filter_row() {
    let loaded = history(&["a.csv", "b.csv"]);
    let mut state = LedgerState::default();
    state.reloaded(&loaded);
    state.ensure_cut(Some(&loaded), UTC);
    assert_eq!(state.cut.as_ref().unwrap().kept.len(), 2);
    // The control plane sets the view; no frame of the filter row ran.
    state.view.apply(&LedgerViewChange {
        search: Some("#msc".to_owned()),
        ..LedgerViewChange::default()
    });
    state.ensure_cut(Some(&loaded), UTC);
    assert_eq!(state.cut.as_ref().unwrap().kept, vec![0]);
    let label = &state.cut.as_ref().unwrap().labels[&key("a.csv")];
    assert_eq!(
        label.folded,
        label.text.to_lowercase(),
        "folded once per cut"
    );
}

#[test]
fn f2_names_and_del_deletes_the_focused_session_but_never_a_live_one() {
    let loaded = history(&["a.csv", "b.csv"]);
    let mut state = LedgerState::default();
    state.reloaded(&loaded);
    state.ensure_cut(Some(&loaded), UTC);
    let none = BTreeSet::new();
    assert_eq!(state.key_press(false, &none, Some(&loaded)), None);
    assert_eq!(state.key_press(true, &none, Some(&loaded)), None);
    state.focused = Some(key("a.csv"));
    assert_eq!(
        state.key_press(true, &none, Some(&loaded)),
        Some(SessionPress::StartRename(key("a.csv")))
    );
    assert!(matches!(
        state.key_press(false, &none, Some(&loaded)),
        Some(SessionPress::StartDelete(session, _, 1)) if session == key("a.csv")
    ));
    let live = BTreeSet::from([key("a.csv")]);
    assert_eq!(state.key_press(false, &live, Some(&loaded)), None);
}

#[test]
fn a_live_trade_no_file_holds_has_nothing_to_tag() {
    let loaded = history(&["a.csv"]);
    let state = LedgerState {
        selected_saved: Some((key("a.csv"), at(&loaded.rows[0].trade))),
        ..LedgerState::default()
    };
    assert_eq!(
        state.tag_target(Some(None), Some(&loaded)),
        None,
        "a live selection answers, even with nothing to give"
    );
    let (session, trade, tags) = state
        .tag_target(None, Some(&loaded))
        .expect("the saved selection");
    assert_eq!(session, key("a.csv"));
    assert_eq!(trade, at(&loaded.rows[0].trade));
    assert_eq!(tags, vec!["msc".to_owned()]);
}

/// This session's own trades pass the filter the saved rows were cut with,
/// with the tags their file gives them; what the filter hides is counted.
#[test]
fn live_trades_are_filtered_by_their_files_tags_and_counted_when_hidden() {
    let loaded = history(&["a.csv", "b.csv"]);
    let trades: Vec<ClosedTrade> = loaded.rows.iter().map(|row| row.trade.clone()).collect();
    let mut at: Vec<Option<JournaledTrade>> = loaded
        .rows
        .iter()
        .map(|row| {
            Some(JournaledTrade {
                session: row.session.clone().unwrap(),
                trade: row.trade_ref,
            })
        })
        .collect();
    at.push(None);
    let mut trades = trades;
    trades.push(trade(DAY_START + 600_000));
    let sources = vec![SessionSource::Live; trades.len()];
    let mut state = LedgerState::default();
    state.view.filter.search = "#msc".to_owned();
    state.ensure_cut(Some(&loaded), UTC);
    let (kept, hidden) = state.live_trades(&trades, &at, &sources, Some(&loaded), UTC);
    assert_eq!(hidden, 2, "the untagged trade and the one no file holds");
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].index, 0);
    assert_eq!(kept[0].tags, ["msc".to_owned()]);

    state.view.filter = LedgerFilter::default();
    let (kept, hidden) = state.live_trades(&trades, &at, &sources, Some(&loaded), UTC);
    assert_eq!(hidden, 0);
    let order: Vec<usize> = kept.iter().map(|live| live.index).collect();
    assert_eq!(order, vec![2, 1, 0], "newest first");
}

/// A session row picked, then a trade of another session selected: F2 and
/// Del follow the selection, never the row picked before it.
#[test]
fn selecting_a_trade_never_makes_its_session_what_the_keys_act_on() {
    let loaded = history(&["a.csv", "b.csv"]);
    let mut state = LedgerState::default();
    state.reloaded(&loaded);
    state.ensure_cut(Some(&loaded), UTC);
    let none = BTreeSet::new();
    assert_eq!(
        state.apply(SessionPress::Fold(key("a.csv"), true), None),
        None
    );
    // Rename and delete belong to the session row: a selected trade gives
    // F2 and Del nothing to act on.
    state.trade_selected();
    assert_eq!(state.key_press(true, &none, Some(&loaded)), None);
    assert_eq!(state.key_press(false, &none, Some(&loaded)), None);
    // Picking a session row makes it the target once more.
    state.apply(SessionPress::Fold(key("a.csv"), false), None);
    assert_eq!(
        state.key_press(true, &none, Some(&loaded)),
        Some(SessionPress::StartRename(key("a.csv")))
    );
}

/// An edit takes the keyboard once, when it starts; an edit whose inline row
/// the cut placed nowhere is cancelled rather than left waiting.
#[test]
fn an_edit_focuses_once_and_ends_when_its_row_is_unreachable() {
    let loaded = history(&["a.csv"]);
    let mut state = LedgerState::default();
    state.reloaded(&loaded);
    state.apply(SessionPress::StartRename(key("a.csv")), Some(&loaded));
    assert!(state.focus_edit, "the new edit asks for the keyboard");
    state.edit_placed(true);
    assert!(state.edit.is_some(), "a placed row keeps its edit");
    state.edit_placed(false);
    assert_eq!(state.edit, None, "a row nobody can reach is no edit");
    assert!(!state.focus_edit);
}

/// The strip completes the token being typed: suggestions narrow by its
/// prefix and leave out what is typed already, and a pick replaces the
/// partial token instead of appending a second one.
#[test]
fn a_tag_suggestion_completes_the_token_being_typed() {
    let known = vec![
        ("msc".to_owned(), 4),
        ("open".to_owned(), 3),
        ("mean".to_owned(), 2),
    ];
    let tags = |text: &str| -> Vec<&str> {
        tag_suggestions(&known, text, 6)
            .into_iter()
            .map(|(tag, _)| tag)
            .collect()
    };
    assert_eq!(tags(""), ["msc", "open", "mean"]);
    assert_eq!(tags("#m"), ["msc", "mean"]);
    assert_eq!(tags("#M"), ["msc", "mean"], "tags are lowercase");
    assert_eq!(
        tags("#msc "),
        ["open", "mean"],
        "a typed tag is not offered"
    );
    assert_eq!(tags("#msc #me"), ["mean"]);
    assert_eq!(tags("#x"), Vec::<&str>::new());
    assert_eq!(complete_tag("#m", "msc"), "#msc ");
    assert_eq!(complete_tag("", "msc"), "#msc ");
    assert_eq!(complete_tag("#open #m", "msc"), "#open #msc ");
    assert_eq!(complete_tag("#open ", "msc"), "#open #msc ");
}
