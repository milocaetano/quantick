use super::*;
use crate::scratch;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_stores::alerts::AlertRuntime;
use quantick_stores::script_alarms::{FiredAlert, ScriptAlarm as Raised};
use serde_json::json;

struct Host(AlertRuntime);
impl AlertDefinitionsPort for Host {
    fn alert_runtime(&mut self) -> &mut AlertRuntime {
        &mut self.0
    }
}

fn host(name: &str) -> Host {
    let path = scratch::thread_dir(name).join("alerts.json");
    Host(AlertRuntime::open(path, 0))
}

fn raised(title: &str) -> (String, Raised) {
    let alarm = Raised {
        indicator: "Renko POC flip".to_owned(),
        bar_close_ms: 42,
        alert: FiredAlert {
            title: title.to_owned(),
            message: "POC above".to_owned(),
        },
    };
    ("WINZ25".to_owned(), alarm)
}

fn read(host: &mut Host, input: Value) -> Result<Value, ControlError> {
    request(host, &mut (), &FakeWindow::assistant(), &input)
}

#[test]
fn history_reads_every_kept_alarm_or_only_newer_ones() {
    let mut host = host("script-alarm-history");
    assert_eq!(
        read(&mut host, json!({})).unwrap(),
        json!({"alarms": [], "last_sequence": "0"})
    );
    let alarms = [raised("Bull flip"), raised("Bear flip")];
    host.0.script_alarms.record(alarms, 1_000);
    let all = read(&mut host, json!({})).unwrap();
    assert_eq!(all["last_sequence"], "2");
    assert_eq!(
        all["alarms"][0],
        json!({
            "sequence": "1",
            "utc_ms": 1_000,
            "symbol": "WINZ25",
            "indicator": "Renko POC flip",
            "title": "Bull flip",
            "message": "POC above",
            "bar_close_ms": 42,
        })
    );
    let newer = read(&mut host, json!({"after": "1"})).unwrap();
    assert_eq!(newer["alarms"].as_array().unwrap().len(), 1);
    assert_eq!(newer["alarms"][0]["title"], "Bear flip");
}

#[test]
fn history_refuses_an_unknown_field() {
    let mut host = host("script-alarm-refusal");
    assert!(read(&mut host, json!({"since": 1})).is_err());
}

#[test]
fn each_delivered_alarm_is_journaled_once_and_toasted() {
    let mut log = ScriptAlarmLog::default();
    let delivered = log.record([raised("Bull flip")], 1_000);
    let mut access = FakeAccess::default();
    let actor = FakeWindow::trader();
    let notes = journal(Some((&mut access, &actor)), &delivered);
    assert_eq!(notes, ["WINZ25 Renko POC flip: Bull flip — POC above"]);
    assert_eq!(access.events.len(), 1);
    assert_eq!(access.events[0].kind.as_str(), FIRED_EVENT_KIND);

    let notes = journal::<FakeAccess>(None, &delivered);
    assert_eq!(notes.len(), 1, "a window with no journal still toasts");
}
