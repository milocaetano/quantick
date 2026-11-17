//! The real socket, on loopback: a stand-in bridge dials the listener, says
//! hello, receives a request and answers it.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use quantick_broker_mt5::{LinkState, Mt5Venue, TcpBridgeLink};
use quantick_engine::Side;
use quantick_trading::{OrderIntent, TradingVenue, VenueEvent, VenueNotice};
use support::{Snapshot, config, dec, reply};

const PATIENCE: Duration = Duration::from_secs(10);

/// Poll until `done` holds, collecting events; fail after `PATIENCE`.
fn poll_until(
    venue: &mut Mt5Venue,
    mut done: impl FnMut(&Mt5Venue, &[VenueEvent]) -> bool,
) -> Vec<VenueEvent> {
    let started = Instant::now();
    let mut events = Vec::new();
    while started.elapsed() < PATIENCE {
        events.extend(venue.poll());
        if done(venue, &events) {
            return events;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("gave up waiting; saw {events:?}");
}

fn send(stream: &mut TcpStream, line: &serde_json::Value) {
    stream
        .write_all(format!("{line}\n").as_bytes())
        .expect("bridge writes");
}

#[test]
fn a_bridge_on_loopback_gets_the_request_and_its_answer_lands() {
    let link = TcpBridgeLink::listen(0).expect("bind loopback");
    let addr = link.local_addr();
    assert!(addr.ip().is_loopback(), "never anything but loopback");
    let mut venue = Mt5Venue::new(config(), Box::new(link));

    let mut bridge = TcpStream::connect(addr).expect("dial");
    bridge.set_read_timeout(Some(PATIENCE)).expect("timeout");
    let mut reader = BufReader::new(bridge.try_clone().expect("clone"));
    send(
        &mut bridge,
        &serde_json::json!({"type": "exec_hello", "schema": 1, "instance": "loop"}),
    );
    poll_until(&mut venue, |venue, _| venue.link_state() == LinkState::Up);
    let mut line = String::new();
    reader.read_line(&mut line).expect("the watch request");
    assert!(line.contains("\"watch\""), "{line}");
    send(&mut bridge, &Snapshot::new().line());
    poll_until(&mut venue, |venue, _| venue.entry_block().is_none());

    venue.submit(OrderIntent::market(Side::Buy, dec(1)));
    line.clear();
    reader.read_line(&mut line).expect("the submit");
    let request: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(request["type"], "submit");

    send(
        &mut bridge,
        &reply(
            request["id"].as_str().unwrap(),
            10009,
            "Request executed",
            501,
        ),
    );
    let events = poll_until(&mut venue, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, VenueEvent::Notice(VenueNotice::Acknowledged { .. })))
    });
    assert!(!events.is_empty());

    drop(reader);
    drop(bridge);
    poll_until(&mut venue, |venue, _| venue.link_state() == LinkState::Lost);
}
