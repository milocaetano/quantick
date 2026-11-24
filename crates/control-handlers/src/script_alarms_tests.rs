use super::*;
use crate::scratch;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_alerts::SourceContext;
use quantick_stores::alerts::AlertRuntime;
use quantick_stores::script_alarms::{FiredAlert, Raised, ScriptAlarm};
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

fn raised(title: &str) -> Raised {
    let alarm = ScriptAlarm {
        tag: 1,
        indicator: "Renko POC flip".to_owned(),
        bar_close_ms: 42,
        alert: FiredAlert {
            title: title.to_owned(),
            message: "POC above".to_owned(),
        },
    };
    let source = SourceContext::Live {
        feed: "mt5".to_owned(),
    };
    Raised {
        source,
        symbol: "WINZ25".to_owned(),
        chart: "Renko 6".to_owned(),
        alarm,
    }
}

fn read(host: &mut Host, input: Value) -> Result<Value, ControlError> {
    request(host, &mut (), &FakeWindow::assistant(), &input)
}

#[test]
fn history_reads_every_kept_alarm_or_only_newer_ones() {
    let mut host = host("script-alarm-history");
    assert_eq!(
        read(&mut host, json!({})).unwrap(),
        json!({"alarms": [], "last_sequence": "0", "oldest_sequence": "0"})
    );
    let alarms = [raised("Bull flip"), raised("Bear flip")];
    host.0.script_alarms.record(alarms, 1_000);
    let all = read(&mut host, json!({})).unwrap();
    assert_eq!(all["last_sequence"], "2");
    assert_eq!(all["oldest_sequence"], "1");
    assert_eq!(
        all["alarms"][0],
        json!({
            "sequence": "1",
            "utc_ms": 1_000,
            "source": {"Live": {"feed": "mt5"}},
            "symbol": "WINZ25",
            "chart": "Renko 6",
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
fn each_delivered_alarm_is_journaled_once_even_while_the_ring_waits() {
    let mut log = ScriptAlarmLog::default();
    let delivered = log.record([raised("Bull flip")], 1_000);
    let mut access = FakeAccess::default();
    let actor = FakeWindow::trader();
    assert_eq!(journal(Some((&mut access, &actor)), &delivered).count(), 0);
    assert_eq!(access.events.len(), 1);
    assert_eq!(access.events[0].kind.as_str(), FIRED_EVENT_KIND);
    assert_eq!(access.events[0].payload["chart"], "Renko 6");
    assert!(log.ring(0).is_some(), "the first alarm rings");

    let burst = log.record([raised("Bull flip"), raised("Bear flip")], 2_000);
    assert_eq!(journal(Some((&mut access, &actor)), &burst).count(), 0);
    assert_eq!(log.ring(1), None, "the burst waits for the gap");
    assert_eq!(access.events.len(), 3, "each alarm is journaled at once");
}
