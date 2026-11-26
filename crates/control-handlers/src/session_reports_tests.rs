use super::*;
use crate::test_support::{FakeAccess, FakeWindow, PaperDesk};
use crate::trade::{PaperOwner, TradeDesk};
use quantick_civil::TzOffset;
use quantick_paper::report::session_edit::{JournalEdit, JournalEditOutcome, JournalScope};

#[test]
fn sources_call_normalizes_unions_and_rejects_invalid_input_without_success_events() {
    let scratch = crate::scratch::ScratchDir::new("report-sources");
    let mut host = Host {
        desk: Some(PaperDesk::new(
            quantick_paper::PaperAccount::with_trades_dir(scratch.path().to_owned()),
        )),
        dir: scratch.path().to_owned(),
    };
    let mut access = FakeAccess::with_budget(0);
    let inspected = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"sources"}),
    )
    .unwrap();
    assert_eq!(inspected["open"], false);
    let dates = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"inspect"}),
    )
    .unwrap();
    assert_eq!(dates["open"], false);
    assert_eq!(dates["timezone"], "UTC-03:00");
    assert_eq!(dates["applied_days"], json!([]));
    assert_eq!(dates["applied_day_count"], "0");
    let input_schema = quantick_control_schema::session_reports::descriptor().input_schema;
    let first = quantick_civil::CivilDate::from_ymd(2026, 11, 1);
    let too_many: Vec<String> = (0..=MAX_LISTED_DAYS as i64)
        .map(|offset| first.offset_days(offset).iso())
        .collect();
    for (input, code) in [
        (
            json!({"action":"days", "days":["2026-11-24", "2026-11-300"]}),
            "invalid_dates",
        ),
        (
            json!({"action":"days", "days":["2026-11-31"]}),
            "invalid_dates",
        ),
        (json!({"action":"days", "days":too_many}), "invalid_dates"),
        (
            json!({"action":"days", "days":[format!("2026-11-24{}", "9".repeat(4096))]}),
            "invalid_dates",
        ),
        (
            json!({"action":"period", "period":"2years"}),
            "invalid_period",
        ),
    ] {
        // The schema admits every day list, so the gateway hands each one to
        // the handler and the refusal carries its code.
        quantick_control::schema::validate_instance(&input_schema, &input).unwrap();
        let error = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap_err();
        let wire = serde_json::to_string(&error).unwrap();
        assert!(!wire.contains("9999"), "no echo of the long entry: {wire}");
        assert_eq!(error.context.details.unwrap()["code"], code);
    }
    assert!(access.events.is_empty());
    for (sources, expected) in [
        (json!(["broker", "real"]), json!(["broker", "real"])),
        (
            json!(["simulator", "real", "broker", "real"]),
            json!(["broker", "real", "replay"]),
        ),
        (json!(["both"]), json!(["broker", "real", "replay"])),
        (json!([]), json!([])),
    ] {
        let input = json!({"action":"sources", "sources":sources});
        let output = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap();
        assert_eq!(output["sources"], expected);
        assert_eq!(output["action"], "sources");
        assert_eq!(access.events.last().unwrap().payload["report"], output);
        assert_eq!(
            open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap(),
            output
        );
    }
    let before = access.events.len();
    for input in [
        json!({"action":"sources", "sources":["unknown"]}),
        json!({"action":"sources", "sources":vec!["real"; 6]}),
        json!({"action":"sources", "session":"X/a.csv"}),
    ] {
        assert!(open(&mut host, &mut access, &FakeWindow::assistant(), &input).is_err());
    }
    assert_eq!(access.events.len(), before);
    host.desk = None;
    let error = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"sources"}),
    )
    .unwrap_err();
    assert_eq!(error.context.details.unwrap()["code"], "no_ledger");
}

struct Host {
    desk: Option<PaperDesk>,
    dir: std::path::PathBuf,
}
impl PaperOwner for Host {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        self.desk.as_ref().map(|desk| desk as &dyn TradeDesk)
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        self.desk.as_mut().map(|desk| desk as &mut dyn TradeDesk)
    }
    fn persist(&mut self, _: quantick_session::paper::SharedChange) {}
    fn journal_scope(&self) -> JournalScope {
        JournalScope {
            dir: self.dir.clone(),
            live: vec![],
        }
    }
    fn journal_timezone(&self) -> TzOffset {
        TzOffset::new(-180)
    }
    fn edit_journal(&mut self, _: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        panic!("opening a report never edits a journal")
    }
}

#[test]
fn session_and_day_calls_show_the_report_return_scope_and_journal_readback() {
    let scratch = crate::scratch::ScratchDir::new("report-calls");
    let mut host = day_of_trades(&scratch);
    let mut access = FakeAccess::with_budget(0);
    for (input, count) in [
        (
            json!({"action":"session", "session":"broker/WINZ26/a.csv"}),
            1,
        ),
        (
            json!({"action":"day", "session":"broker/WINZ26/a.csv", "day":"2026-11-18"}),
            2,
        ),
    ] {
        let result = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap();
        assert_eq!(result["origin"], "broker");
        assert_eq!(
            result["skipped_rows"], "1",
            "a torn clicked file is disclosed"
        );
        assert_eq!(result["unreadable_files"], "0");
        assert_eq!(result["trade_count"], count.to_string());
        assert_eq!(
            host.desk.as_ref().unwrap().reports.last().unwrap().trades,
            count as usize
        );
        assert_eq!(access.events.last().unwrap().kind.as_str(), EVENT_KIND);
        assert_eq!(access.events.last().unwrap().payload["report"], result);
        let repeated = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap();
        assert_eq!(
            repeated, result,
            "a retry selects the same scope without a journal write"
        );
    }
    let ordinary = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"ordinary"}),
    )
    .unwrap();
    assert_eq!(ordinary["action"], "ordinary");
    assert_eq!(access.events.last().unwrap().payload["report"], ordinary);
    assert!(host.desk.as_ref().unwrap().reports.is_empty());
    let before = host.desk.as_ref().unwrap().reports.len();
    for input in [
        json!({"action":"ordinary","day":"2026-11-18"}),
        json!({"action":"session", "session":"../a.csv"}),
        json!({"action":"session", "session":"WINZ26/missing.csv"}),
        json!({"action":"day", "session":"broker/WINZ26/a.csv", "day":"2026-11-19"}),
        json!({"action":"day", "session":"broker/WINZ26/a.csv", "day":"2026-02-30"}),
    ] {
        assert!(open(&mut host, &mut access, &FakeWindow::assistant(), &input).is_err());
    }
    assert_eq!(host.desk.as_ref().unwrap().reports.len(), before);
    host.desk = None;
    let error = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"session", "session":"WINZ26/a.csv"}),
    )
    .unwrap_err();
    assert_eq!(error.context.details.unwrap()["code"], "no_ledger");
    assert_eq!(
        access.events.len(),
        5,
        "refusals do not emit successful opens"
    );
}

/// One trade a minute into 2026-11-18 in each of two broker sessions and a
/// simulator one; the first broker file ends in a torn row.
fn day_of_trades(scratch: &crate::scratch::ScratchDir) -> Host {
    let dir = scratch.path().join("paper");
    for key in ["broker/WINZ26/a.csv", "broker/WDOZ26/b.csv", "WINZ26/a.csv"] {
        let path = SessionKey::parse(key).unwrap().path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = format!(
            "# quantick-trades 3\n# symbol=WINZ26\n# source=live\n{}\n{},{STAMP},long,1,100,105,5,manual,,,,\n",
            quantick_sim::history::HEADER,
            STAMP - 1000
        );
        let text = if key == "broker/WINZ26/a.csv" {
            text + "torn exit row\n"
        } else {
            text
        };
        std::fs::write(path, text).unwrap();
    }
    Host {
        desk: Some(PaperDesk::new(
            quantick_paper::PaperAccount::with_trades_dir(dir.clone()),
        )),
        dir,
    }
}

/// 2026-11-18 00:01 at UTC-3, the journal timezone of [`Host`].
const STAMP: i64 = 1_794_970_860_000;

#[test]
fn whole_day_and_tag_calls_open_one_origin_and_refuse_empty_or_invalid_scopes() {
    let scratch = crate::scratch::ScratchDir::new("report-day-tag");
    let mut host = day_of_trades(&scratch);
    let trade = quantick_sim::history::TradeRef {
        opened_ms: STAMP - 1000,
        closed_ms: STAMP,
        nth: 0,
    };
    for key in ["WINZ26/a.csv", "broker/WDOZ26/b.csv"] {
        let key = SessionKey::parse(key).unwrap();
        quantick_paper::report::session_edit::tag_trade(&host.dir, &key, trade, &["#Setup"])
            .unwrap();
    }
    let mut access = FakeAccess::with_budget(0);
    for (input, action, origin, count) in [
        (
            json!({"action":"whole_day", "origin":"broker", "day":"2026-11-18"}),
            "whole_day",
            "broker",
            2,
        ),
        (
            json!({"action":"whole_day", "origin":"simulator", "day":"2026-11-18"}),
            "whole_day",
            "simulator",
            1,
        ),
        (
            json!({"action":"tag", "origin":"simulator", "tag":"#SETUP"}),
            "tag",
            "simulator",
            1,
        ),
        (
            json!({"action":"tag", "origin":"broker", "tag":"setup"}),
            "tag",
            "broker",
            1,
        ),
    ] {
        let result = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap();
        assert_eq!(result["action"], action);
        assert_eq!(result["origin"], origin);
        assert_eq!(
            result["session"],
            Value::Null,
            "no one session is the scope"
        );
        assert_eq!(result["trade_count"], count.to_string());
        if action == "tag" {
            assert_eq!(result["tag"], "setup");
            assert_eq!(result["day"], Value::Null);
        } else {
            assert_eq!(result["day"], "2026-11-18");
        }
        assert_eq!(
            host.desk.as_ref().unwrap().reports.last().unwrap().trades,
            count as usize
        );
        assert_eq!(access.events.last().unwrap().payload["report"], result);
        let repeated = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap();
        assert_eq!(repeated, result, "a retry selects the same scope");
    }
    let reports = host.desk.as_ref().unwrap().reports.len();
    let events = access.events.len();
    for (input, code) in [
        (
            json!({"action":"whole_day", "origin":"broker", "day":"2026-11-19"}),
            Some("no_trades_in_scope"),
        ),
        (
            json!({"action":"tag", "origin":"broker", "tag":"fade"}),
            Some("no_trades_in_scope"),
        ),
        (
            json!({"action":"tag", "origin":"simulator", "tag":"two words"}),
            Some("invalid_tag"),
        ),
        (
            json!({"action":"whole_day", "origin":"both", "day":"2026-11-18"}),
            None,
        ),
        (json!({"action":"tag", "tag":"setup"}), None),
    ] {
        let error = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap_err();
        assert_eq!(
            error.context.details.map(|details| details["code"].clone()),
            code.map(Value::from),
            "{input}"
        );
    }
    assert_eq!(host.desk.as_ref().unwrap().reports.len(), reports);
    assert_eq!(access.events.len(), events, "refusals emit no opens");
}

#[test]
fn days_call_sets_the_picked_days_idempotently_and_clear_dates_empties_them() {
    let scratch = crate::scratch::ScratchDir::new("report-days");
    let mut host = Host {
        desk: Some(PaperDesk::new(
            quantick_paper::PaperAccount::with_trades_dir(scratch.path().to_owned()),
        )),
        dir: scratch.path().to_owned(),
    };
    let mut access = FakeAccess::with_budget(0);
    let input =
        json!({"action":"days", "days":["2026-11-25", "2026-11-12", "2026-11-24", "2026-11-25"]});
    let set = open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap();
    assert_eq!(set["action"], "days");
    assert_eq!(
        set["applied_days"],
        json!(["2026-11-12", "2026-11-24", "2026-11-25"]),
        "oldest first, duplicates collapsed"
    );
    assert_eq!(set["applied_day_count"], "3");
    assert_eq!(set["timezone"], "UTC-03:00");
    assert_eq!(access.events.last().unwrap().payload["report"], set);
    assert_eq!(
        open(&mut host, &mut access, &FakeWindow::assistant(), &input).unwrap(),
        set,
        "a retry sets the same days, never toggles them"
    );
    let cleared = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"clear_dates"}),
    )
    .unwrap();
    assert_eq!(cleared["applied_days"], json!([]));
    assert_eq!(cleared["applied_day_count"], "0");
    let emptied = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"days", "days":[]}),
    )
    .unwrap();
    assert_eq!(emptied["applied_days"], json!([]));
    let copies = json!({"action":"days", "days":vec!["2026-11-24"; MAX_LISTED_DAYS + 1]});
    let input_schema = quantick_control_schema::session_reports::descriptor().input_schema;
    quantick_control::schema::validate_instance(&input_schema, &copies).unwrap();
    let one = open(&mut host, &mut access, &FakeWindow::assistant(), &copies).unwrap();
    assert_eq!(
        one["applied_days"],
        json!(["2026-11-24"]),
        "copies collapse"
    );
}

/// A pick longer than the readback lists its newest days; the count
/// carries the rest.
#[test]
fn a_long_pick_reads_back_its_newest_days() {
    let scratch = crate::scratch::ScratchDir::new("report-days-newest");
    let mut desk = PaperDesk::new(quantick_paper::PaperAccount::with_trades_dir(
        scratch.path().to_owned(),
    ));
    let first = quantick_civil::CivilDate::from_ymd(2026, 8, 1);
    let last = first.offset_days(99);
    desk.report_days = DaySet::span(first, last);
    let mut host = Host {
        desk: Some(desk),
        dir: scratch.path().to_owned(),
    };
    let mut access = FakeAccess::with_budget(0);
    let read = open(
        &mut host,
        &mut access,
        &FakeWindow::assistant(),
        &json!({"action":"inspect"}),
    )
    .unwrap();
    let listed = read["applied_days"].as_array().unwrap();
    assert_eq!(listed.len(), MAX_LISTED_DAYS);
    assert_eq!(listed[0], first.offset_days(8).iso());
    assert_eq!(listed[MAX_LISTED_DAYS - 1], last.iso());
    assert_eq!(read["applied_day_count"], "100");
}
