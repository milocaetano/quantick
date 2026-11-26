//! The real socket, on loopback: a stand-in bridge dials the listener, proves
//! this run's secret, says hello, receives a request and answers it; a
//! connection that does not prove it is closed having been told nothing.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use quantick_broker_mt5::auth::{AUTH_DEADLINE, BRIDGE_SECRET_BYTES};
use quantick_broker_mt5::transport::MAX_LINE_BYTES;
use quantick_broker_mt5::{
    BridgeLink, BridgeSecret, LinkEvent, LinkState, Mt5Venue, TcpBridgeLink,
};
use quantick_engine::Side;
use quantick_trading::{OrderIntent, TradingVenue, VenueEvent, VenueNotice};
use support::{Snapshot, config, dec, reply};

const PATIENCE: Duration = Duration::from_secs(10);

fn secret() -> BridgeSecret {
    BridgeSecret::from_bytes([5; BRIDGE_SECRET_BYTES])
}

/// Dial `link` as the right bridge would: the secret, then the hello.
fn dial_authenticated(link: &TcpBridgeLink) -> TcpStream {
    let mut bridge = TcpStream::connect(link.local_addr()).expect("dial");
    bridge.set_read_timeout(Some(PATIENCE)).expect("timeout");
    bridge
        .write_all(format!("{}\n", secret().to_hex()).as_bytes())
        .expect("bridge writes");
    bridge
}

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
    let link = TcpBridgeLink::listen(0, secret()).expect("bind loopback");
    assert!(
        link.local_addr().ip().is_loopback(),
        "never anything but loopback"
    );
    let mut bridge = dial_authenticated(&link);
    let mut venue = Mt5Venue::new(config(), Box::new(link));

    let mut reader = BufReader::new(bridge.try_clone().expect("clone"));
    send(
        &mut bridge,
        &serde_json::json!({"type": "exec_hello", "schema": 2, "instance": "loop"}),
    );
    poll_until(&mut venue, |venue, _| venue.link_state() == LinkState::Up);
    let mut line = String::new();
    reader.read_line(&mut line).expect("the watch request");
    assert!(line.contains("\"watch\""), "{line}");
    send(&mut bridge, &Snapshot::new().line());
    venue.tick(0);
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

/// Dial `link`, let `speak` write whatever it writes on a thread of its own,
/// and keep asking the link to send an order line until the listener closes
/// the connection. Refused means: closed, not one byte written to it, and
/// not one event surfaced, a disconnect included. Returns how long it took.
fn assert_refused(
    link: &mut TcpBridgeLink,
    speak: impl FnOnce(TcpStream) + Send + 'static,
) -> Duration {
    let started = Instant::now();
    let client = TcpStream::connect(link.local_addr()).expect("dial");
    client
        .set_read_timeout(Some(Duration::from_millis(20)))
        .expect("timeout");
    let speaker = {
        let writer = client.try_clone().expect("clone");
        std::thread::spawn(move || speak(writer))
    };
    let mut received = Vec::new();
    let mut chunk = [0u8; 256];
    let mut reader = &client;
    loop {
        assert!(started.elapsed() < PATIENCE, "the listener never closed it");
        link.send(r#"{"type":"submit","id":"x"}"#);
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => received.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let took = started.elapsed();
    let _ = speaker.join();
    assert!(
        received.is_empty(),
        "an unproven connection was written to: {received:?}"
    );
    let mut events = Vec::new();
    link.poll(&mut events);
    assert!(
        events.is_empty(),
        "an unproven connection surfaced {events:?}"
    );
    took
}

/// After a refusal the listener is free: the right bridge's lines land.
fn assert_the_bridge_still_connects(link: &mut TcpBridgeLink) {
    let mut bridge = dial_authenticated(link);
    send(
        &mut bridge,
        &serde_json::json!({"type": "exec_hello", "schema": 2, "instance": "after"}),
    );
    let started = Instant::now();
    let mut events = Vec::new();
    while !events
        .iter()
        .any(|event| matches!(event, LinkEvent::Line(line) if line.contains("after")))
    {
        assert!(
            started.elapsed() < PATIENCE,
            "the bridge never got through; saw {events:?}"
        );
        link.poll(&mut events);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_wrong_secret_is_refused_before_any_line() {
    let mut link = TcpBridgeLink::listen(0, secret()).expect("bind loopback");
    let wrong = BridgeSecret::from_bytes([6; BRIDGE_SECRET_BYTES]).to_hex();
    assert_refused(&mut link, move |mut client| {
        let _ = client.write_all(format!("{wrong}\n").as_bytes());
        let _ = client.write_all(b"{\"type\":\"state\",\"balance\":\"1\"}\n");
    });
    assert_the_bridge_still_connects(&mut link);
}

#[test]
fn a_bridge_that_sends_no_secret_is_refused() {
    let mut link = TcpBridgeLink::listen(0, secret()).expect("bind loopback");
    assert_refused(&mut link, |mut client| {
        let _ = client.write_all(b"{\"type\":\"exec_hello\",\"schema\":2,\"instance\":\"x\"}\n");
    });
    assert_refused(&mut link, drop);
    assert_the_bridge_still_connects(&mut link);
}

#[test]
fn a_first_line_past_its_bound_is_refused() {
    let mut link = TcpBridgeLink::listen(0, secret()).expect("bind loopback");
    assert_refused(&mut link, |mut client| {
        let _ = client.write_all(&[b'a'; 4096]);
    });
    assert_the_bridge_still_connects(&mut link);
}

#[test]
fn a_first_line_dribbled_past_the_deadline_is_refused() {
    let mut link = TcpBridgeLink::listen(0, secret()).expect("bind loopback");
    // The right secret, but one byte at a time: never whole in time.
    let hex = secret().to_hex();
    let took = assert_refused(&mut link, move |mut client| {
        for byte in format!("{hex}\n").bytes() {
            if client.write_all(&[byte]).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    assert!(
        took >= AUTH_DEADLINE,
        "closed after {took:?}, before the deadline"
    );
    assert_the_bridge_still_connects(&mut link);
}

/// The bound on a proven bridge's lines: one of exactly `MAX_LINE_BYTES`
/// arrives whole, and one byte more ends the connection without that line
/// or anything sent after it being delivered.
#[test]
fn a_line_past_its_bound_ends_the_connection_and_is_never_delivered() {
    let mut link = TcpBridgeLink::listen(0, secret()).expect("bind loopback");
    let mut bridge = dial_authenticated(&link);
    let fits = "x".repeat(MAX_LINE_BYTES);
    let over = "y".repeat(MAX_LINE_BYTES + 1);
    let writer = std::thread::spawn(move || {
        // The listener may close mid-write; that is the behaviour under test.
        let _ = bridge.write_all(format!("{fits}\n{over}\nafter\n").as_bytes());
    });
    let started = Instant::now();
    let mut events = Vec::new();
    while !events
        .iter()
        .any(|event| matches!(event, LinkEvent::Disconnected))
    {
        assert!(
            started.elapsed() < PATIENCE,
            "the over-long line never ended the connection"
        );
        link.poll(&mut events);
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = writer.join();
    let lines: Vec<usize> = events
        .iter()
        .filter_map(|event| match event {
            LinkEvent::Line(line) => Some(line.len()),
            LinkEvent::Disconnected => None,
        })
        .collect();
    assert_eq!(
        lines,
        vec![MAX_LINE_BYTES],
        "only the line within the bound"
    );
}

/// Whatever an unproven peer sends first — under the bound or past it, with
/// or without a newline — it is refused, and the right bridge still gets
/// through afterwards. One listener serves every case, as one serves a run.
#[test]
fn any_first_bytes_but_the_secret_are_refused() {
    use proptest::prelude::*;
    use proptest::test_runner::TestRunner;

    let link = std::cell::RefCell::new(TcpBridgeLink::listen(0, secret()).expect("bind loopback"));
    let mut runner = TestRunner::new(ProptestConfig::with_cases(32));
    runner
        .run(&prop::collection::vec(any::<u8>(), 0..512), |first| {
            assert_refused(&mut link.borrow_mut(), move |mut client| {
                let _ = client.write_all(&first);
                // End the stream so a line with no newline is refused at
                // once instead of at the deadline.
                let _ = client.shutdown(std::net::Shutdown::Write);
            });
            Ok(())
        })
        .unwrap();
    assert_the_bridge_still_connects(&mut link.borrow_mut());
}
