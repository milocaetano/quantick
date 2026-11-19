use super::*;
use crate::test_support::{FakeAccess, FakeWindow, PaperDesk};
use crate::trade::{PaperOwner, TradeDesk};
use quantick_civil::TzOffset;
use quantick_paper::report::session_edit::{JournalEdit, JournalEditOutcome, JournalScope};

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
    let before = host.desk.as_ref().unwrap().reports.len();
    for input in [
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
        4,
        "refusals do not emit successful opens"
    );
}
