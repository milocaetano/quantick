// Sessions, the ledger filter, the day-then-session grouping and the three
// edits, over real files in a scratch trades folder.

use std::path::Path;

use quantick_civil::{CivilDate, DateRange, TzOffset};
use quantick_engine::Side;
use quantick_sim::history::{self, SessionSource};
use quantick_sim::{ClosedTrade, ExitReason};
use rust_decimal::Decimal;

use super::*;
use crate::report::session_edit::{
    DELETED_DIR, SessionEditError, delete_session, rename_session, tag_trade,
};
use crate::report::{SourceFilter, load_history};
use crate::scratch::ScratchDir;

const UTC: TzOffset = TzOffset::new(0);
/// 2026-03-16 13:00:00 UTC.
const DAY_START: i64 = 1_773_666_000_000;

fn trade_at(closed_ms: i64, pnl: i64) -> ClosedTrade {
    ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - 1000,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}

/// The first (here the only) trade of its file with `trade`'s times.
fn first_ref(trade: &ClosedTrade) -> TradeRef {
    TradeRef {
        opened_ms: trade.opened_ms,
        closed_ms: trade.closed_ms,
        nth: 0,
    }
}

/// Write one session file the way the account does.
fn write_session(
    dir: &Path,
    file: &str,
    source: SessionSource,
    trades: &[ClosedTrade],
) -> SessionKey {
    let folder = dir.join("WINJ26");
    std::fs::create_dir_all(&folder).unwrap();
    let mut text = history::write_header("WINJ26", source);
    for trade in trades {
        text.push_str(&history::write_trade(trade));
    }
    std::fs::write(folder.join(file), text).unwrap();
    SessionKey::parse(&format!("WINJ26/{file}")).unwrap()
}

#[test]
fn a_key_names_exactly_one_session_file_and_nothing_else() {
    let key = SessionKey::parse("WINJ26/20260316-130000.rerun-2.csv").expect("a session");
    assert_eq!(key.parts(), ("WINJ26", "20260316-130000.rerun-2.csv"));
    assert_eq!(key.rerun(), Some(2));
    assert_eq!(
        SessionKey::parse("WINJ26/20260316-130000.csv")
            .unwrap()
            .rerun(),
        None
    );
    for refused in [
        "../x.csv",
        "WINJ26/../x.csv",
        "/abs/x.csv",
        ".deleted/x.csv",
        "WINJ26/x.txt",
        "WINJ26/a/b.csv",
        "x.csv",
        "C:\\x/y.csv",
        "WINJ26/.csv",
    ] {
        assert_eq!(SessionKey::parse(refused), None, "{refused}");
    }
    let dir = Path::new("trades");
    assert_eq!(SessionKey::of(dir, &key.path(dir)), Some(key));
}

#[test]
fn three_replays_of_one_day_are_three_sessions_not_one() {
    let dir = ScratchDir::new("sessions-three-replays");
    // The same tape traded three times: identical venue times, three files.
    let run = [
        trade_at(DAY_START + 60_000, 4),
        trade_at(DAY_START + 120_000, -1),
    ];
    let first = write_session(
        dir.path(),
        "20260316-130100.csv",
        SessionSource::Replay,
        &run,
    );
    let second = write_session(
        dir.path(),
        "20260316-130100.rerun-1.csv",
        SessionSource::Replay,
        &run,
    );
    let third = write_session(
        dir.path(),
        "20260316-130100.rerun-2.csv",
        SessionSource::Replay,
        &run[..1],
    );
    let loaded = load_history(dir.path(), Some("WINJ26"), &[]);
    assert_eq!(loaded.rows.len(), 5);
    assert_eq!(loaded.sessions.len(), 3);

    let newest_first: Vec<(Option<&SessionKey>, &ClosedTrade)> = loaded
        .rows
        .iter()
        .rev()
        .map(|row| (row.session.as_ref(), &row.trade))
        .collect();
    let days = group_by_day_and_session(newest_first, UTC);
    assert_eq!(days.len(), 1, "one civil day");
    assert_eq!(days[0].count, 5);
    assert_eq!(days[0].net, Decimal::from(4 + 4 + 4 - 1 - 1));
    let mut keys: Vec<_> = days[0]
        .sessions
        .iter()
        .map(|s| s.key.clone().unwrap())
        .collect();
    keys.sort();
    assert_eq!(keys, vec![first, second.clone(), third.clone()]);
    let counts: Vec<usize> = days[0].sessions.iter().map(|s| s.count).collect();
    assert_eq!(counts.iter().sum::<usize>(), 5);
    assert!(days[0].sessions.iter().all(|s| s.items.len() == s.count));

    let label = loaded.sessions[&third].default_label(UTC);
    assert_eq!(label, "Replay · 13:01 · rerun 2");
    assert_eq!(
        loaded.sessions[&second].label(UTC),
        "Replay · 13:01 · rerun 1"
    );
}

#[test]
fn days_split_before_sessions_do() {
    let a = SessionKey::parse("X/a.csv").unwrap();
    let b = SessionKey::parse("X/b.csv").unwrap();
    let day = 86_400_000;
    let rows = [
        (Some(&a), trade_at(DAY_START + day, 1)),
        (Some(&b), trade_at(DAY_START + day - 10, 2)),
        (Some(&a), trade_at(DAY_START + day - 20, 3)),
        (Some(&a), trade_at(DAY_START, 5)),
    ];
    let days = group_by_day_and_session(rows.iter().map(|(key, trade)| (*key, trade)), UTC);
    assert_eq!(days.len(), 2);
    assert_eq!(days[0].sessions.len(), 2);
    assert_eq!(
        days[0].sessions[0].items,
        vec![0, 2],
        "a collects across the interleave"
    );
    assert_eq!(days[0].sessions[1].items, vec![1]);
    assert_eq!(days[1].sessions.len(), 1);
    assert_eq!(days[1].day, CivilDate::from_ms(DAY_START, UTC));
}

#[test]
fn the_filter_ands_source_dates_tags_and_session_words() {
    let tagged = ["msc".to_owned(), "fade".to_owned()];
    let trade = trade_at(DAY_START, 3);
    let day = CivilDate::from_ms(DAY_START, UTC);
    let filter = |source, range, search: &str| LedgerFilter {
        source,
        range,
        search: search.to_owned(),
    };
    let admits = |filter: LedgerFilter, source, tags: &[String], label: &str| {
        let label = SessionLabel::new(label.to_owned());
        filter
            .matcher(UTC)
            .admits(source, &trade, tags, Some(&label))
    };
    let live = Some(SessionSource::Live);
    let replay = Some(SessionSource::Replay);
    assert!(LedgerFilter::default().is_default());
    assert!(admits(
        LedgerFilter::default(),
        replay,
        &[],
        "Replay · 13:00"
    ));
    assert!(!admits(
        filter(SourceFilter::Real, None, ""),
        replay,
        &[],
        ""
    ));
    assert!(
        admits(filter(SourceFilter::Real, None, ""), None, &[], ""),
        "unrecorded is real"
    );
    assert!(admits(
        filter(SourceFilter::Replay, None, ""),
        replay,
        &[],
        ""
    ));
    let same_day = Some(DateRange::new(day, day));
    let next_day = Some(DateRange::new(day.offset_days(1), day.offset_days(2)));
    assert!(admits(
        filter(SourceFilter::All, same_day, ""),
        live,
        &[],
        ""
    ));
    assert!(!admits(
        filter(SourceFilter::All, next_day, ""),
        live,
        &[],
        ""
    ));
    assert!(admits(
        filter(SourceFilter::All, None, "#msc"),
        live,
        &tagged,
        ""
    ));
    assert!(admits(
        filter(SourceFilter::All, None, "#MSC #fade"),
        live,
        &tagged,
        ""
    ));
    assert!(
        !admits(
            filter(SourceFilter::All, None, "#msc #other"),
            live,
            &tagged,
            ""
        ),
        "tags AND"
    );
    assert!(!admits(
        filter(SourceFilter::All, None, "#msc"),
        live,
        &[],
        ""
    ));
    assert!(admits(
        filter(SourceFilter::All, None, "morning"),
        live,
        &[],
        "Morning fade"
    ));
    assert!(!admits(
        filter(SourceFilter::All, None, "morning evening"),
        live,
        &[],
        "Morning fade"
    ));
    assert!(admits(
        filter(SourceFilter::All, None, "fade #msc"),
        live,
        &tagged,
        "Morning FADE"
    ));
    assert!(
        admits(filter(SourceFilter::All, None, "#"), live, &[], ""),
        "a lone # is still being typed"
    );
    assert!(filter(SourceFilter::All, None, " # ").is_default());
}

#[test]
fn a_listing_keeps_sessions_with_a_kept_trade_newest_first() {
    let dir = ScratchDir::new("sessions-listing");
    let old = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        &[trade_at(DAY_START, 1)],
    );
    let new = write_session(
        dir.path(),
        "20260316-150000.csv",
        SessionSource::Replay,
        &[
            trade_at(DAY_START + 7_200_000, 2),
            trade_at(DAY_START + 7_300_000, 3),
        ],
    );
    let trade = trade_at(DAY_START + 7_300_000, 3);
    tag_trade(dir.path(), &new, first_ref(&trade), &["msc"]).unwrap();
    let loaded = load_history(dir.path(), None, &[]);
    let live = live_keys(dir.path(), &[old.path(dir.path())]);

    let (all, hidden) = summarize_sessions(&loaded, &LedgerFilter::default(), UTC, &live);
    assert_eq!(hidden, 0);
    let keys: Vec<&SessionKey> = all.iter().map(|s| &s.info.key).collect();
    assert_eq!(keys, vec![&new, &old], "newest first");
    assert!(all[1].live && !all[0].live);
    assert_eq!((all[0].trades, all[0].net), (2, Decimal::from(5)));

    let msc = LedgerFilter {
        search: "#msc".to_owned(),
        ..LedgerFilter::default()
    };
    let (tagged, hidden) = summarize_sessions(&loaded, &msc, UTC, &live);
    assert_eq!(hidden, 2, "only the tagged trade stays");
    assert_eq!(tagged.len(), 1);
    assert_eq!((tagged[0].trades, tagged[0].net), (1, Decimal::from(3)));

    // The ledger's own cut agrees with the listing, row for row.
    let labels = session_labels(&loaded, UTC);
    let kept = kept_rows(&loaded, &labels, &msc, UTC);
    assert_eq!(kept.len(), 1);
    assert_eq!(loaded.rows[kept[0]].tags, vec!["msc"]);
    assert_eq!(
        kept_rows(&loaded, &labels, &LedgerFilter::default(), UTC).len(),
        3
    );
    assert_eq!(tag_counts(&loaded), vec![("msc".to_owned(), 1)]);
}

#[test]
fn a_name_and_tags_survive_a_reload_and_the_ledger_reads_them() {
    let dir = ScratchDir::new("sessions-name-tags");
    let trade = trade_at(DAY_START, 3);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        std::slice::from_ref(&trade),
    );
    let times = first_ref(&trade);
    assert_eq!(
        rename_session(dir.path(), &key, "  Opening drive  ").unwrap(),
        Some("Opening drive".to_owned())
    );
    assert_eq!(
        tag_trade(dir.path(), &key, times, &["#MSC", "fade"]).unwrap(),
        vec!["msc", "fade"]
    );

    // A restart is a fresh read of the folder.
    let loaded = load_history(dir.path(), None, &[]);
    let info = &loaded.sessions[&key];
    assert_eq!(info.name.as_deref(), Some("Opening drive"));
    assert_eq!(info.label(UTC), "Opening drive");
    assert_eq!(loaded.rows[0].tags, vec!["msc", "fade"]);
    assert_eq!(loaded.rows[0].session.as_ref(), Some(&key));
    assert_eq!(loaded.rows.len(), 1, "metadata lines are not trades");
    assert_eq!(loaded.problem_rows, 0);

    rename_session(dir.path(), &key, "").unwrap();
    tag_trade(dir.path(), &key, times, &[]).unwrap();
    let loaded = load_history(dir.path(), None, &[]);
    assert_eq!(loaded.sessions[&key].name, None, "cleared");
    assert!(loaded.rows[0].tags.is_empty());
}

#[test]
fn a_legacy_file_without_metadata_loads_with_a_default_label() {
    let dir = ScratchDir::new("sessions-legacy");
    let folder = dir.path().join("WINJ26");
    std::fs::create_dir_all(&folder).unwrap();
    let mut text = format!(
        "# quantick-trades 2\n# symbol=WINJ26\n{}\n",
        history::HEADER
    );
    text.push_str(&history::write_trade(&trade_at(DAY_START, 2)));
    std::fs::write(folder.join("20260316-130000.csv"), text).unwrap();
    let loaded = load_history(dir.path(), None, &[]);
    let info = loaded.sessions.values().next().expect("one session");
    assert_eq!(info.name, None);
    assert_eq!(info.label(UTC), "Unrecorded · 13:00");
    assert!(loaded.rows[0].tags.is_empty());
}

#[test]
fn edits_refuse_what_they_cannot_name() {
    let dir = ScratchDir::new("sessions-refusals");
    let trade = trade_at(DAY_START, 3);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        std::slice::from_ref(&trade),
    );
    let missing = SessionKey::parse("WINJ26/nope.csv").unwrap();
    assert_eq!(
        rename_session(dir.path(), &missing, "x")
            .unwrap_err()
            .code(),
        "session_not_found"
    );
    let error = tag_trade(dir.path(), &key, first_ref(&trade_at(2, 0)), &["a"]).unwrap_err();
    assert_eq!(error.code(), "trade_not_found");
    let error = tag_trade(dir.path(), &key, first_ref(&trade), &["two words"]).unwrap_err();
    assert_eq!(error.code(), "invalid_tag");
    let text = std::fs::read_to_string(key.path(dir.path())).unwrap();
    assert!(!text.contains("# tags="), "a refused edit writes nothing");
}

#[test]
fn an_edit_after_a_torn_row_starts_on_its_own_line() {
    let dir = ScratchDir::new("sessions-torn");
    let trade = trade_at(DAY_START, 3);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        &[trade],
    );
    let path = key.path(dir.path());
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("1773666000000,17736");
    std::fs::write(&path, text).unwrap();
    rename_session(dir.path(), &key, "after the tear").unwrap();
    let loaded = load_history(dir.path(), None, &[]);
    assert_eq!(
        loaded.sessions[&key].name.as_deref(),
        Some("after the tear")
    );
    assert_eq!(loaded.problem_rows, 1, "only the torn row is lost");
}

#[test]
fn delete_moves_the_file_aside_and_the_history_no_longer_lists_it() {
    let dir = ScratchDir::new("sessions-delete");
    let keep = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        &[trade_at(DAY_START, 1)],
    );
    let gone = write_session(
        dir.path(),
        "20260316-140000.csv",
        SessionSource::Replay,
        &[trade_at(DAY_START + 3_600_000, 2)],
    );
    let moved = delete_session(dir.path(), &gone, &[]).expect("deleted");
    assert!(moved.starts_with(dir.path().join(DELETED_DIR).join("WINJ26")));
    assert!(moved.is_file(), "moved, not erased");
    assert!(!gone.path(dir.path()).exists());

    for scope in [None, Some("WINJ26")] {
        let loaded = load_history(dir.path(), scope, &[]);
        assert_eq!(loaded.rows.len(), 1, "{scope:?}");
        assert_eq!(loaded.files, 1);
        assert!(loaded.sessions.contains_key(&keep));
        assert!(!loaded.sessions.contains_key(&gone));
    }
    assert_eq!(
        delete_session(dir.path(), &gone, &[]).unwrap_err().code(),
        "session_not_found",
        "a second delete finds nothing"
    );

    // A session of the same name deleted again lands beside the first.
    write_session(
        dir.path(),
        "20260316-140000.csv",
        SessionSource::Replay,
        &[trade_at(DAY_START, 2)],
    );
    let again = delete_session(dir.path(), &gone, &[]).expect("deleted again");
    assert_ne!(again, moved);
    assert!(again.is_file() && moved.is_file());
}

#[test]
fn the_live_session_is_never_deleted_and_is_read_only_for_its_metadata() {
    let dir = ScratchDir::new("sessions-live");
    let trade = trade_at(DAY_START, 1);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        std::slice::from_ref(&trade),
    );
    let live = vec![key.path(dir.path())];
    let error = delete_session(dir.path(), &key, &live).unwrap_err();
    assert_eq!(error, SessionEditError::LiveSession(key.clone()));
    assert_eq!(error.code(), "live_session");
    assert!(key.path(dir.path()).is_file(), "untouched");

    // Naming and tagging the live session is allowed; its rows stay out of
    // the saved history, its name and tags do not.
    rename_session(dir.path(), &key, "Now").unwrap();
    tag_trade(dir.path(), &key, first_ref(&trade), &["live"]).unwrap();
    let loaded = load_history(dir.path(), None, &live);
    assert!(loaded.rows.is_empty(), "its trades are in the simulator");
    assert_eq!(loaded.files, 0);
    let info = &loaded.sessions[&key];
    assert!(info.live);
    assert_eq!(info.name.as_deref(), Some("Now"));
    assert_eq!(info.tags_of(&first_ref(&trade)), ["live".to_owned()]);
}

/// Windows opens `WINJ26./x.csv` and `winj26/x.csv` as the live
/// `WINJ26/x.csv`, and the account itself may journal under either
/// spelling. None of them may move it.
#[test]
fn no_spelling_of_the_live_session_gets_past_the_delete_refusal() {
    for refused in [
        "WINJ26 /20260316-130000.csv",
        "WINJ26/20260316-130000.csv ",
        "WINJ26/20260316-130000.csv.",
    ] {
        assert_eq!(SessionKey::parse(refused), None, "{refused:?}");
    }
    let dir = ScratchDir::new("sessions-live-spellings");
    let trade = trade_at(DAY_START, 1);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        std::slice::from_ref(&trade),
    );
    let shouted = SessionKey::parse("winj26/20260316-130000.csv").expect("another spelling");
    let dotted = SessionKey::parse("WINJ26./20260316-130000.csv").expect("a suffix folder");
    // Elsewhere `WINJ26.` is a folder of its own, holding no file here.
    let mut lives = vec![key.path(dir.path())];
    if cfg!(windows) {
        lives.push(dotted.path(dir.path()));
    }
    let mut error = None;
    for live in lives {
        for spelling in [&key, &shouted, &dotted] {
            let refused = delete_session(dir.path(), spelling, std::slice::from_ref(&live))
                .expect_err("the live file is never deleted");
            // A case-insensitive volume finds the live file under the other
            // spelling and refuses it; a case-sensitive one finds none.
            assert!(
                matches!(
                    refused,
                    SessionEditError::LiveSession(_) | SessionEditError::NotFound(_)
                ),
                "{spelling} against {live:?}: {refused:?}"
            );
            assert!(key.path(dir.path()).is_file(), "the live file never moved");
            if spelling == &shouted {
                error = Some(refused);
            }
        }
    }
    let error = error.expect("the shouted spelling was tried");
    if cfg!(windows) {
        assert_eq!(error, SessionEditError::LiveSession(key.clone()));
        // An edit acts on, and answers with, the file's own spelling.
        assert_eq!(
            rename_session(dir.path(), &shouted, "Now"),
            Ok(Some("Now".to_owned()))
        );
        assert_eq!(
            load_history(dir.path(), None, &[]).sessions[&key]
                .name
                .as_deref(),
            Some("Now")
        );
    }
}

/// A ledger scoped to another instrument still knows the live session's
/// tags, so its tag strip opens on them and Save cannot wipe them blind.
#[test]
fn the_live_sessions_names_and_tags_are_read_whatever_the_scope() {
    let dir = ScratchDir::new("sessions-live-out-of-scope");
    let trade = trade_at(DAY_START, 1);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        std::slice::from_ref(&trade),
    );
    rename_session(dir.path(), &key, "Now").unwrap();
    tag_trade(dir.path(), &key, first_ref(&trade), &["keep"]).unwrap();
    let live = vec![key.path(dir.path())];
    let loaded = load_history(dir.path(), Some("OTHER"), &live);
    assert!(loaded.rows.is_empty());
    assert_eq!(loaded.files, 0, "the out-of-scope file is not counted");
    let info = &loaded.sessions[&key];
    assert!(info.live);
    assert_eq!(info.name.as_deref(), Some("Now"));
    assert_eq!(info.tags_of(&first_ref(&trade)), ["keep".to_owned()]);
}

/// Two partial closes of one position share both times. Each is its own
/// trade to tag, read back on its own row — never the first row's tags on
/// both, never only on the first.
#[test]
fn partial_closes_with_the_same_times_carry_their_own_tags() {
    let dir = ScratchDir::new("sessions-partial-closes");
    let trade = trade_at(DAY_START, 1);
    let key = write_session(
        dir.path(),
        "20260316-130000.csv",
        SessionSource::Live,
        &[trade.clone(), trade.clone()],
    );
    let second = TradeRef {
        nth: 1,
        ..first_ref(&trade)
    };
    tag_trade(dir.path(), &key, second, &["scale"]).unwrap();
    let loaded = load_history(dir.path(), None, &[]);
    let tags: Vec<&[String]> = loaded.rows.iter().map(|row| row.tags.as_slice()).collect();
    assert_eq!(tags, vec![&[][..], &["scale".to_owned()][..]]);
    assert_eq!(loaded.rows[1].trade_ref, second);
    let third = TradeRef {
        nth: 2,
        ..first_ref(&trade)
    };
    assert_eq!(
        tag_trade(dir.path(), &key, third, &["x"])
            .unwrap_err()
            .code(),
        "trade_not_found"
    );
}

/// This session's re-runs of a tape that crossed midnight come back to a
/// day already listed: one header per day, and one inline editor for a
/// session however many days it spans.
#[test]
fn a_day_is_listed_once_and_an_edit_stands_in_one_place() {
    let a = SessionKey::parse("X/a.csv").unwrap();
    let b = SessionKey::parse("X/b.csv").unwrap();
    let day = 86_400_000;
    // Newest first: run b across midnight, then run a across the same one.
    let rows = [
        (Some(&b), trade_at(DAY_START + day, 1)),
        (Some(&b), trade_at(DAY_START, 2)),
        (Some(&a), trade_at(DAY_START + day, 3)),
        (Some(&a), trade_at(DAY_START, 4)),
    ];
    let mut days = group_by_day_and_session(rows.iter().map(|(key, trade)| (*key, trade)), UTC);
    assert_eq!(days.len(), 2, "two civil days, each listed once");
    assert!(days.iter().all(|day| day.sessions.len() == 2));

    let mut placed = false;
    place_session_rows(&mut days, Some(&a), &BTreeSet::new(), &mut placed);
    let rows_of = |days: &[DayGroup], key: &SessionKey| -> Vec<SessionRow> {
        days.iter()
            .flat_map(|day| &day.sessions)
            .filter(|session| session.key.as_ref() == Some(key))
            .map(|session| session.row)
            .collect()
    };
    assert_eq!(
        rows_of(&days, &a),
        vec![SessionRow::Inline, SessionRow::Head]
    );
    assert_eq!(rows_of(&days, &b), vec![SessionRow::Head, SessionRow::Head]);
    // The saved list after the live one: the editor is already placed.
    let mut saved = group_by_day_and_session([(Some(&a), &rows[3].1)], UTC);
    place_session_rows(&mut saved, Some(&a), &BTreeSet::new(), &mut placed);
    assert_eq!(rows_of(&saved, &a), vec![SessionRow::Head]);

    // A folded newest day hands the editor to the next open one.
    let mut placed = false;
    let folded = BTreeSet::from([days[0].day.day_number()]);
    place_session_rows(&mut days, Some(&a), &folded, &mut placed);
    assert_eq!(rows_of(&days[1..], &a), vec![SessionRow::Inline]);
}

/// A day of one session still says which session it was — Live or Replay —
/// so its trades never stand without a head; only a session no file holds
/// stays bare, and a folded day places nothing.
#[test]
fn a_one_session_day_is_headed_and_a_keyless_session_stays_bare() {
    let a = SessionKey::parse("X/a.csv").unwrap();
    let b = SessionKey::parse("X/b.csv").unwrap();
    let day = 86_400_000;
    let rows = [
        (None, trade_at(DAY_START + 2 * day, 5)),
        (Some(&b), trade_at(DAY_START + day, 1)),
        (Some(&a), trade_at(DAY_START, 2)),
    ];
    let mut days = group_by_day_and_session(rows.iter().map(|(key, trade)| (*key, trade)), UTC);
    assert_eq!(days.len(), 3);
    assert!(days.iter().all(|day| day.sessions.len() == 1));

    let mut placed = false;
    place_session_rows(&mut days, None, &BTreeSet::new(), &mut placed);
    let placed_rows: Vec<SessionRow> = days.iter().map(|day| day.sessions[0].row).collect();
    assert_eq!(
        placed_rows,
        vec![SessionRow::Bare, SessionRow::Head, SessionRow::Head],
        "keyless bare; every keyed one-session day headed"
    );
    assert!(!placed, "no edit, no inline row");

    // Editing a one-session day's session puts the inline row there, once.
    let mut placed = false;
    place_session_rows(&mut days, Some(&a), &BTreeSet::new(), &mut placed);
    assert_eq!(days[2].sessions[0].row, SessionRow::Inline);
    assert_eq!(days[1].sessions[0].row, SessionRow::Head);

    // A folded day places nothing.
    let mut fresh = group_by_day_and_session(rows.iter().map(|(key, trade)| (*key, trade)), UTC);
    let folded = BTreeSet::from([fresh[1].day.day_number()]);
    let mut placed = false;
    place_session_rows(&mut fresh, None, &folded, &mut placed);
    assert_eq!(
        fresh[1].sessions[0].row,
        SessionRow::Bare,
        "folded: untouched"
    );
    assert_eq!(fresh[2].sessions[0].row, SessionRow::Head);
}
