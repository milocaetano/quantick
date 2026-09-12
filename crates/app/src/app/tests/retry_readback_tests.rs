//! The retry matrix's rows, proven through the real local gateway.
//!
//! `docs/control-plane/retry-matrix.md` makes one claim per mutable
//! capability: retry it under the same key and get the first answer without a
//! second effect, or read it back and learn whether it applied. Every test
//! here makes a real connection to a real gateway — the socket, the handshake,
//! `ObserverContract::prepare`, the idempotency store, `execute_on_ui` — and
//! reconciles through the read the matrix names for that capability, looked
//! up from `control::retry_matrix` rather than restated, so a row that names
//! the wrong read fails here as well as in the drift guard.
//!
//! Two ways a call goes unanswered are exercised, because they are the two a
//! client meets:
//!
//! - **A dropped answer on a live connection.** The client sent a keyed call,
//!   the application acted, and the reply never reached whatever was waiting
//!   for it. The client retries under the same key on the same connection.
//! - **A lost connection.** The client's socket went away with the call in
//!   flight and no answer. Deduplication ends with the connection (#362), so
//!   this client reconciles by reading back over a new one. The tests drive
//!   both outcomes the client cannot tell apart: the call was already queued
//!   and the application ran it (*applied*), or the trader withdrew the
//!   connection's access before the application reached it and it was
//!   refused before dispatch (*not applied*). Only the readback separates
//!   them, which is the claim.
//!
//! The application thread is served through the `#[cfg(test)]` seams in
//! `control/gateway/retry_seams.rs` where a test has to see what reached it:
//! [`crate::control::ServedRequest::began`] is the flag the idempotency store's
//! unknown-outcome branch reads, so "the retry did not act" is asserted on
//! exactly the thing that decides it.

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use quantick_control::{
    error::codes,
    id::{ConnectionId, IdempotencyKey, RequestId},
    limits::CONTROL_DEFAULT_PAGE_ITEMS,
    wire::{ResponseEnvelope, ResponseOutcome},
};
use quantick_control_local::client::{ConnectOptions, LocalClient};
use serde_json::{Value, json};

use super::*;
use crate::control::{ControlAccess, ServedRequest, retry_matrix};

/// The name every test client connects under, and so the author every object
/// an interrupted call placed is attributed to.
const CLIENT_NAME: &str = "quantick integration test";

/// Scopes a client asks for: the safe reads plus `extra`.
fn options(profile: &str, extra: &[&str]) -> ConnectOptions {
    let mut scopes = gateway_test_scopes();
    for id in extra {
        scopes.insert(quantick_control::id::PermissionId::new(*id).unwrap());
    }
    ConnectOptions::for_profile(profile, CLIENT_NAME, env!("CARGO_PKG_VERSION"), scopes)
}

const ANNOTATE_SCOPES: &[&str] = &[
    "annotate",
    "annotate.attention",
    "annotate.chart",
    "annotate.notification",
    "annotate.script",
    "annotate.sound",
];

fn connect(directory: &std::path::Path, options: &ConnectOptions) -> LocalClient {
    quantick_control_local::client::discover_in(directory, options)
        .unwrap()
        .select(None)
        .unwrap()
}

/// Lend the control host to `act` with the application beside it, the way a
/// frame does.
fn with_access<R>(
    app: &mut QuantickApp,
    act: impl FnOnce(&mut ControlAccess, &mut QuantickApp) -> R,
) -> R {
    let mut access = app
        .control
        .control_access
        .take()
        .expect("control access is installed");
    let result = act(&mut access, app);
    app.control.control_access = Some(access);
    result
}

fn serve(app: &mut QuantickApp) -> Vec<ServedRequest> {
    with_access(app, |access, app| access.serve_queued_for_test(app))
}

/// One keyed call, the application thread served through the seam; the
/// answer and every request the application took off its queue meanwhile.
fn keyed_call(
    app: &mut QuantickApp,
    client: &mut LocalClient,
    request_id: &str,
    capability: &str,
    payload: Value,
    key: &str,
) -> (ResponseEnvelope, Vec<ServedRequest>) {
    let sent = client
        .send_with_idempotency_key(
            RequestId::new(request_id).unwrap(),
            capability,
            1,
            payload,
            IdempotencyKey::new(key.to_owned()).unwrap(),
        )
        .expect("the request is sent");
    let mut served = Vec::new();
    let deadline = Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        served.extend(serve(app));
        if client.reply_pending(Duration::from_millis(5)) {
            break;
        }
        assert!(Instant::now() < deadline, "no answer to {request_id}");
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, sent);
    (response, served)
}

/// [`keyed_call`] without a key.
fn unkeyed_call(
    app: &mut QuantickApp,
    client: &mut LocalClient,
    capability: &str,
    payload: Value,
) -> (ResponseEnvelope, Vec<ServedRequest>) {
    let sent = client
        .send(capability, payload)
        .expect("the request is sent");
    let mut served = Vec::new();
    let deadline = Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        served.extend(serve(app));
        if client.reply_pending(Duration::from_millis(5)) {
            break;
        }
        assert!(Instant::now() < deadline, "no answer to {capability}");
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, sent);
    (response, served)
}

fn error_code(response: &ResponseEnvelope) -> Option<&str> {
    match &response.outcome {
        ResponseOutcome::Success { .. } => None,
        ResponseOutcome::Failure { error } => Some(error.code.as_str()),
    }
}

/// Every value at a matrix field path: dotted properties, `[]` stepping into
/// an array. The same grammar `retry_matrix::schema_has_path` checks against
/// the published schema, walked here over a real answer.
fn values_at(value: &Value, path: &str) -> Vec<Value> {
    let mut nodes = vec![value.clone()];
    for segment in path.split('.') {
        let (name, into_array) = match segment.strip_suffix("[]") {
            Some(name) => (name, true),
            None => (segment, false),
        };
        nodes = nodes
            .iter()
            .filter_map(|node| node.get(name))
            .flat_map(|field| {
                if into_array {
                    field.as_array().cloned().unwrap_or_default()
                } else {
                    vec![field.clone()]
                }
            })
            .collect();
    }
    nodes
}

/// The readback the matrix names for `capability`, read over `client`: the
/// values at the row's field, in the row's scope or across every retained
/// event of the row's kind.
fn readback(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut LocalClient,
    capability: &str,
) -> Vec<Value> {
    let row = retry_matrix::readback(capability).expect("the matrix has a row");
    if let Some(scope) = row.scope {
        let response = remote_call(app, ctx, client, row.read, json!({ "scopes": [scope] }));
        let result = success_result(&response);
        return values_at(&result["scopes"][scope]["value"], row.field);
    }
    let kind = row.event.expect("a row reads a scope or an event kind");
    let mut values = Vec::new();
    let mut page = json!({ "start": "oldest", "limit": CONTROL_DEFAULT_PAGE_ITEMS });
    loop {
        let response = remote_call(app, ctx, client, row.read, page);
        let result = success_result(&response);
        for event in result["events"].as_array().expect("a page of events") {
            if event["kind"] == kind {
                values.extend(values_at(event, row.field));
            }
        }
        if result["has_more"] != true {
            return values;
        }
        page = json!({ "cursor": result["next_cursor"], "limit": CONTROL_DEFAULT_PAGE_ITEMS });
    }
}

fn connection_ids(app: &QuantickApp) -> BTreeSet<ConnectionId> {
    app.control
        .control_access
        .as_ref()
        .expect("control access is installed")
        .connection_ids_for_test()
        .into_iter()
        .collect()
}

/// Connect, and learn the id the application knows the connection by.
fn connect_listed(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    directory: &std::path::Path,
    options: &ConnectOptions,
) -> (LocalClient, ConnectionId) {
    let before = connection_ids(app);
    let client = connect(directory, options);
    let deadline = Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        run_frame(app, ctx);
        if let Some(id) = connection_ids(app).difference(&before).next() {
            return (client, id.clone());
        }
        assert!(Instant::now() < deadline, "the connection was never listed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// How a client lost its call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lost {
    /// The connection dropped after the gateway queued the call; the
    /// application ran it with nobody left to tell.
    AfterQueueing,
    /// The trader withdrew the connection's access while the call was queued;
    /// the application refused it before dispatch.
    ByRevocation,
}

/// Send one unkeyed call on a fresh connection and lose it, then let the
/// application frames run. The client learns nothing either way.
fn lose_call(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    directory: &std::path::Path,
    options: &ConnectOptions,
    capability: &str,
    payload: Value,
    lost: Lost,
) {
    let (mut client, id) = connect_listed(app, ctx, directory, options);
    client
        .send(capability, payload)
        .expect("the request is sent");
    wait_for_queued_gateway_requests(app, 1);
    if lost == Lost::ByRevocation {
        with_access(app, |access, _| access.revoke(id));
    }
    drop(client);
    drain_gateway_requests(app, ctx);
}

fn count_of(values: &[Value], wanted: &Value) -> usize {
    values.iter().filter(|value| *value == wanted).count()
}

/// Two anchors on the newest bars, for the tools that need two.
fn two_anchors(app: &QuantickApp) -> Vec<Value> {
    let pane = app.active_tab().drawing_pane();
    let newest = pane.slots().saturating_sub(1);
    [newest.saturating_sub(1), newest]
        .into_iter()
        .map(|slot| {
            let time = pane.slot_open_time(slot).expect("the bar has a time");
            let price = pane
                .closed_bar(slot)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .unwrap_or(1.0);
            json!({ "time_unix_ms": time, "price": format!("{price}") })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Optional families: a dropped answer, retried under the same key
// ---------------------------------------------------------------------------

/// Layout: the retry is given the first call's recorded answer and the
/// application does not see it; then, over a new connection — where the key
/// no longer deduplicates — the matrix's readback says the layout exists, so
/// the client has nothing left to send.
#[test]
fn a_dropped_layout_answer_is_replayed_and_the_layout_is_made_once() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-layout");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let cockpit = options("cockpit", &["cockpit", "cockpit.layout"]);
    let mut client = connect(&directory, &cockpit);
    let before = readback(&mut app, &ctx, &mut client, "layout.tab.create").len();

    let (lost, first) = keyed_call(
        &mut app,
        &mut client,
        "first",
        "layout.tab.create",
        json!({}),
        "layout-key",
    );
    // `lost` is the answer the client never saw; nothing below reads it
    // except to compare the retry against it.
    let (retry, second) = keyed_call(
        &mut app,
        &mut client,
        "second",
        "layout.tab.create",
        json!({}),
        "layout-key",
    );

    assert_eq!(first.len(), 1, "the first call reached the application");
    assert!(first[0].began, "and got past every refusal");
    assert!(second.is_empty(), "the retry never reached the application");
    assert!(matches!(lost.outcome, ResponseOutcome::Success { .. }));
    assert_eq!(retry.outcome, lost.outcome, "the retry is the first answer");
    assert_eq!(
        retry.request_id.as_str(),
        "second",
        "addressed to the retry"
    );
    assert_eq!(
        readback(&mut app, &ctx, &mut client, "layout.tab.create").len(),
        before + 1,
        "one layout, not two"
    );

    drop(client);
    let mut reconnected = connect(&directory, &cockpit);
    assert_eq!(
        readback(&mut app, &ctx, &mut reconnected, "layout.tab.create").len(),
        before + 1,
        "a new connection reconciles by readback: the layout is there"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// Feed recovery: the same guarantee, on a tab whose feed has left the feed
/// table — so the call really runs and answers `respawned: false` without
/// opening a venue socket from a test. What the key buys is asserted where it
/// is decided: the application began the call once.
#[test]
fn a_dropped_feed_recovery_answer_is_replayed_and_the_recovery_runs_once() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-feed");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = readback(&mut app, &ctx, &mut client, "feed.reconnect");
    // Retired after the frames above, which re-sync the tab's feed id, and
    // before the calls below, which the seam serves without running one.
    app.active_tab_mut().feed_id = "retired-feed".to_owned();

    let (lost, first) = keyed_call(
        &mut app,
        &mut client,
        "first",
        "feed.reconnect",
        json!({}),
        "recover-key",
    );
    let (retry, second) = keyed_call(
        &mut app,
        &mut client,
        "second",
        "feed.reconnect",
        json!({}),
        "recover-key",
    );

    assert_eq!(first.len(), 1);
    assert!(first[0].began, "the recovery ran");
    assert_eq!(success_result(&lost)["respawned"], false);
    assert!(second.is_empty(), "the retry never reached the application");
    assert_eq!(retry.outcome, lost.outcome);
    assert_eq!(retry.request_id.as_str(), "second");
    let after = readback(&mut app, &ctx, &mut client, "feed.reconnect");
    assert!(!after.is_empty(), "the feed state reads back");
    assert_eq!(after, before, "nothing was respawned, and the state agrees");
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// Trade shaping declares `Optional` and no production grant reaches it, so
/// this starts the gateway under the `trader` ceiling through a test seam to
/// prove the store holds for the family before anyone hands that ceiling out.
/// [`no_production_grant_reaches_a_trade_capability_keyed_or_not`] is the
/// other half: without the seam, nothing reaches it at all.
#[test]
fn a_dropped_trade_shaping_answer_is_replayed_and_the_ticket_changes_once() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-trade");
    grant_annotate_for_test(&mut app, "all-reads,observe.paper,trade");
    app.control
        .control_access
        .as_mut()
        .expect("control access is installed")
        .enable_for_test_under_ceiling(
            &ctx,
            directory.clone(),
            Duration::from_millis(quantick_control::limits::CONTROL_REQUEST_TIMEOUT_MS),
            crate::control::TRADER_PROFILE_ID,
        );
    wait_for_test_gateway_descriptor(&mut app, &ctx);
    let mut client = connect(&directory, &options("trader", &["observe.paper", "trade"]));
    let ruler_before = readback(&mut app, &ctx, &mut client, "trade.ruler.set");
    let tickets_before = readback(&mut app, &ctx, &mut client, "trade.instrument.set_money").len();

    let (lost, first) = keyed_call(
        &mut app,
        &mut client,
        "first",
        "trade.ruler.set",
        json!({ "ticks": 7 }),
        "ruler-key",
    );
    let (retry, second) = keyed_call(
        &mut app,
        &mut client,
        "second",
        "trade.ruler.set",
        json!({ "ticks": 7 }),
        "ruler-key",
    );

    assert!(
        matches!(lost.outcome, ResponseOutcome::Success { .. }),
        "{:?}",
        lost.outcome
    );
    assert_eq!(first.len(), 1);
    assert!(first[0].began);
    assert!(second.is_empty(), "the retry never reached the application");
    assert_eq!(retry.outcome, lost.outcome);
    assert_eq!(retry.request_id.as_str(), "second");
    assert_eq!(
        readback(&mut app, &ctx, &mut client, "trade.instrument.set_money").len(),
        tickets_before + 1,
        "one `trade.ticket.changed`: the ticket changed once"
    );
    let ruler_after = readback(&mut app, &ctx, &mut client, "trade.ruler.set");
    assert_ne!(ruler_after, ruler_before, "the ruler moved");
    assert!(
        ruler_after.contains(&json!(7)),
        "and reads back where it was set: {ruler_after:?}"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

// ---------------------------------------------------------------------------
// The unknown outcome
// ---------------------------------------------------------------------------

/// The branch the store records when nobody can say: the application began a
/// keyed action and had still not answered a full request window after the
/// call's deadline. The retry is refused, not retryable, and told to read the
/// state back — and the readback settles what the retry could not.
#[test]
fn a_keyed_action_held_past_its_deadline_is_refused_as_unknown_and_reconciled_by_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-unknown");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    // Long enough that the seam below always serves the call inside its
    // deadline on a loaded machine; the test waits two of these in total.
    let window = Duration::from_millis(750);
    enable_test_gateway_with_limits(&mut app, &ctx, &directory, 4, window, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = readback(&mut app, &ctx, &mut client, "layout.tab.create").len();

    client
        .send_with_idempotency_key(
            RequestId::new("first").unwrap(),
            "layout.tab.create",
            1,
            json!({}),
            IdempotencyKey::new("held-key".to_owned()).unwrap(),
        )
        .expect("the request is sent");
    wait_for_queued_gateway_requests(&app, 1);
    // The application acts and keeps its answer: from the response worker's
    // side, an application thread still inside the action.
    let held = with_access(&mut app, |access, app| {
        access.serve_one_withholding_answer_for_test(app)
    })
    .expect("the call was queued");
    assert!(
        held.served.began,
        "served inside its deadline, so it may have acted"
    );

    let first = client.read().expect("the deadline answers");
    assert_eq!(error_code(&first), Some(codes::TIMEOUT));
    assert!(
        response_error(&first).retryable,
        "a timeout invites a retry"
    );

    // The invited retry. While the worker is still waiting out the settle
    // window the key is in flight, and the contract's answer to that is "ask
    // again" — so this asks again, slower than the connection's rate limit.
    let mut attempts = 0;
    let (refused, served) = loop {
        attempts += 1;
        let (response, served) = keyed_call(
            &mut app,
            &mut client,
            &format!("retry-{attempts}"),
            "layout.tab.create",
            json!({}),
            "held-key",
        );
        if error_code(&response) != Some(codes::REQUEST_IN_PROGRESS) {
            break (response, served);
        }
        assert!(attempts < 400, "the settle window never closed");
        std::thread::sleep(Duration::from_millis(50));
    };

    assert!(served.is_empty(), "no retry reached the application");
    assert_eq!(error_code(&refused), Some(codes::TIMEOUT));
    let error = response_error(&refused);
    assert!(
        !error.retryable,
        "retrying this key cannot learn the outcome"
    );
    assert!(
        error
            .context
            .next_steps
            .iter()
            .any(|step| step.contains("Read the state back")),
        "the refusal names the readback: {:?}",
        error.context.next_steps
    );
    // Reconciliation, through the read the matrix names: the call applied.
    assert_eq!(
        readback(&mut app, &ctx, &mut client, "layout.tab.create").len(),
        before + 1,
        "the held call made its layout, and the retries made none"
    );
    // The answer arriving now has nobody to go to, and changes nothing.
    assert!(!held.deliver(), "the worker settled without it");
    let (again, served) = keyed_call(
        &mut app,
        &mut client,
        "again",
        "layout.tab.create",
        json!({}),
        "held-key",
    );
    assert!(served.is_empty());
    assert_eq!(again.outcome, refused.outcome, "the refusal is stable");
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

// ---------------------------------------------------------------------------
// Forbidden families: a lost call, resolved by readback
// ---------------------------------------------------------------------------

/// Every annotate capability: a lost create resolves by counting the caller's
/// drawings, a lost remove by looking for the id it named.
#[test]
fn an_interrupted_annotation_is_resolved_by_its_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-annotate");
    grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let annotator = options("annotator", ANNOTATE_SCOPES);
    // Listed before any call is lost, so the revocation below can never
    // mistake the reader for the connection it withdraws.
    let (mut reader, _) = connect_listed(&mut app, &ctx, &directory, &annotator);
    let anchors = two_anchors(&app);
    let mine = json!(CLIENT_NAME);

    for (capability, payload) in [
        (
            "annotate.label.create",
            json!({ "anchors": [anchors[1].clone()], "text": "lost label" }),
        ),
        (
            "annotate.arrow.create",
            json!({ "anchors": anchors.clone() }),
        ),
        (
            "annotate.zone.create",
            json!({ "anchors": anchors.clone() }),
        ),
    ] {
        for (lost, applied) in [(Lost::ByRevocation, false), (Lost::AfterQueueing, true)] {
            let before = count_of(&readback(&mut app, &ctx, &mut reader, capability), &mine);
            lose_call(
                &mut app,
                &ctx,
                &directory,
                &annotator,
                capability,
                payload.clone(),
                lost,
            );
            let after = count_of(&readback(&mut app, &ctx, &mut reader, capability), &mine);
            assert_eq!(
                after,
                before + usize::from(applied),
                "{capability}, lost {lost:?}: the readback says applied = {applied}"
            );
        }
    }

    let placed = remote_call(
        &mut app,
        &ctx,
        &mut reader,
        "annotate.label.create",
        json!({ "anchors": [anchors[1].clone()], "text": "to remove" }),
    );
    let id = success_result(&placed)["annotation_id"].clone();
    for (lost, applied) in [(Lost::ByRevocation, false), (Lost::AfterQueueing, true)] {
        lose_call(
            &mut app,
            &ctx,
            &directory,
            &annotator,
            "annotate.remove",
            json!({ "annotation_id": id }),
            lost,
        );
        let listed = readback(&mut app, &ctx, &mut reader, "annotate.remove").contains(&id);
        assert_eq!(
            listed, !applied,
            "annotate.remove, lost {lost:?}: the readback says applied = {applied}"
        );
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A mark is an append to the journal and nothing else, so the journal is its
/// readback: the note the caller sent is either there or it is not.
#[test]
fn an_interrupted_attention_mark_is_resolved_by_its_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-mark");
    grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let annotator = options("annotator", ANNOTATE_SCOPES);
    // Listed before any call is lost, so the revocation below can never
    // mistake the reader for the connection it withdraws.
    let (mut reader, _) = connect_listed(&mut app, &ctx, &directory, &annotator);

    for (lost, applied, note) in [
        (Lost::ByRevocation, false, "mark lost before dispatch"),
        (Lost::AfterQueueing, true, "mark lost after queueing"),
    ] {
        lose_call(
            &mut app,
            &ctx,
            &directory,
            &annotator,
            "attention.mark.create",
            json!({ "note": note }),
            lost,
        );
        let notes = readback(&mut app, &ctx, &mut reader, "attention.mark.create");
        assert_eq!(
            count_of(&notes, &json!(note)),
            usize::from(applied),
            "attention.mark.create, lost {lost:?}: the readback says applied = {applied}"
        );
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A lost attach resolves by a script slot the pre-call reading lacked; a lost
/// detach by the slot it named.
#[test]
fn an_interrupted_script_attach_or_detach_is_resolved_by_its_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-script");
    grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let annotator = options("annotator", ANNOTATE_SCOPES);
    // Listed before any call is lost, so the revocation below can never
    // mistake the reader for the connection it withdraws.
    let (mut reader, _) = connect_listed(&mut app, &ctx, &directory, &annotator);
    let script = json!({
        "name": "agent ema",
        "source": "//@version=5\nindicator(\"agent ema\")\nplot(close)\n",
    });

    for (lost, applied) in [(Lost::ByRevocation, false), (Lost::AfterQueueing, true)] {
        let before = readback(&mut app, &ctx, &mut reader, "indicator.script.attach");
        lose_call(
            &mut app,
            &ctx,
            &directory,
            &annotator,
            "indicator.script.attach",
            script.clone(),
            lost,
        );
        let after = readback(&mut app, &ctx, &mut reader, "indicator.script.attach");
        assert_eq!(
            after.len(),
            before.len() + usize::from(applied),
            "indicator.script.attach, lost {lost:?}: the readback says applied = {applied}"
        );
    }

    let attached = remote_call(
        &mut app,
        &ctx,
        &mut reader,
        "indicator.script.attach",
        script,
    );
    let slot = success_result(&attached)["slot_id"].clone();
    for (lost, applied) in [(Lost::ByRevocation, false), (Lost::AfterQueueing, true)] {
        lose_call(
            &mut app,
            &ctx,
            &directory,
            &annotator,
            "indicator.script.detach",
            json!({ "slot_id": slot }),
            lost,
        );
        let detached = readback(&mut app, &ctx, &mut reader, "indicator.script.detach");
        assert_eq!(
            count_of(&detached, &slot),
            usize::from(applied),
            "indicator.script.detach, lost {lost:?}: the readback says applied = {applied}"
        );
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A notification leaves nothing on the chart to read, so the journal entry
/// it appends is its readback — for every channel, the sound included.
#[test]
fn an_interrupted_notification_is_resolved_by_its_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-notify");
    grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let annotator = options("annotator", ANNOTATE_SCOPES);
    // Listed before any call is lost, so the revocation below can never
    // mistake the reader for the connection it withdraws.
    let (mut reader, _) = connect_listed(&mut app, &ctx, &directory, &annotator);

    for capability in ["notify.popup", "notify.toast", "notify.sound"] {
        for (lost, applied) in [(Lost::ByRevocation, false), (Lost::AfterQueueing, true)] {
            let message = format!("{capability} lost {lost:?}");
            lose_call(
                &mut app,
                &ctx,
                &directory,
                &annotator,
                capability,
                json!({ "message": message }),
                lost,
            );
            let messages = readback(&mut app, &ctx, &mut reader, capability);
            assert_eq!(
                count_of(&messages, &json!(message)),
                usize::from(applied),
                "{capability}, lost {lost:?}: the readback says applied = {applied}"
            );
        }
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

// ---------------------------------------------------------------------------
// Unreachable: trade
// ---------------------------------------------------------------------------

/// The `trade.*` rows say no grant reaches them. With every scope the panel
/// can tick — `trade` included — and the highest ceiling a grant can hand
/// out, every trade capability is refused before the application sees it,
/// with a key and without one, and nothing is recorded in the ticket's trail.
#[test]
fn no_production_grant_reaches_a_trade_capability_keyed_or_not() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("retry-trade-unreachable");
    grant_annotate_for_test(
        &mut app,
        "all-reads,observe.paper,annotate-tier,cockpit,cockpit.layout,cockpit.recover,trade",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut extra = vec![
        "observe.paper",
        "cockpit",
        "cockpit.layout",
        "cockpit.recover",
        "trade",
    ];
    extra.extend_from_slice(ANNOTATE_SCOPES);
    let mut client = connect(&directory, &options("cockpit", &extra));
    assert!(
        !client
            .handshake()
            .effective_scopes
            .iter()
            .any(|scope| scope.as_str() == "trade"),
        "the handshake drops the scope no grantable ceiling holds"
    );
    let tickets_before = readback(&mut app, &ctx, &mut client, "trade.instrument.set_money").len();

    let trade_rows = retry_matrix::READBACKS
        .iter()
        .filter(|row| row.capability.starts_with("trade."));
    for (index, row) in trade_rows.enumerate() {
        for keyed in [false, true] {
            let (response, served) = if keyed {
                keyed_call(
                    &mut app,
                    &mut client,
                    &format!("trade-{index}"),
                    row.capability,
                    json!({}),
                    &format!("trade-key-{index}"),
                )
            } else {
                unkeyed_call(&mut app, &mut client, row.capability, json!({}))
            };
            assert!(
                served.is_empty(),
                "{} reached the application",
                row.capability
            );
            let code = error_code(&response).expect("refused");
            assert!(
                code == codes::SCOPE_DENIED || code == codes::PERMISSION_DENIED,
                "{} (keyed: {keyed}) is refused on authority, not on its input: {code}",
                row.capability
            );
        }
    }
    assert_eq!(
        readback(&mut app, &ctx, &mut client, "trade.instrument.set_money").len(),
        tickets_before,
        "nothing reached the ticket"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
