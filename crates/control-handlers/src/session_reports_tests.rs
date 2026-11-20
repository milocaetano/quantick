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
    let dir = scratch.path().join("paper");
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    let stamp = day.start_ms(TzOffset::new(-180)) + 60000;
    for key in ["broker/WINZ26/a.csv", "broker/WDOZ26/b.csv", "WINZ26/a.csv"] {
        let path = SessionKey::parse(key).unwrap().path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = format!(
            "# quantick-trades 2\n# symbol=WINZ26\n# source=live\n{}\n{},{stamp},long,1,100,105,5,manual,,,,\n",
            quantick_sim::history::HEADER,
            stamp - 1000
        );
        let text = if key == "broker/WINZ26/a.csv" {
            text + "torn exit row\n"
        } else {
            text
        };
        std::fs::write(path, text).unwrap();
    }
    let mut host = Host {
        desk: Some(PaperDesk::new(
            quantick_paper::PaperAccount::with_trades_dir(dir.clone()),
        )),
        dir,
    };
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
