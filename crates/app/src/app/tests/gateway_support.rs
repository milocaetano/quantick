use super::*;

pub(super) fn observer_instance() -> quantick_control::id::InstanceId {
    quantick_control::id::InstanceId::from_bytes([0x42; 16])
}

pub(super) fn observer_scope(name: &str) -> quantick_control::id::SnapshotScopeId {
    quantick_control::id::SnapshotScopeId::new(name).expect("test scope ID is valid")
}

pub(super) fn gateway_test_scopes() -> std::collections::BTreeSet<quantick_control::id::PermissionId>
{
    [
        "observe",
        "observe.system",
        "observe.workspace",
        "observe.market",
        "observe.chart",
        "observe.indicators",
        "observe.drawings",
        "observe.orderflow",
        "observe.replay",
        "observe.health",
        "observe.attention",
        "observe.events",
    ]
    .into_iter()
    .map(|id| quantick_control::id::PermissionId::new(id).unwrap())
    .collect()
}

pub(super) fn gateway_test_options() -> quantick_control_local::client::ConnectOptions {
    quantick_control_local::client::ConnectOptions::observer(
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        gateway_test_scopes(),
    )
}

/// A client that asks for the annotate tier as well, for the tests that
/// prove what the trader's grant does and does not open.
pub(super) fn annotator_test_options() -> quantick_control_local::client::ConnectOptions {
    let mut scopes = gateway_test_scopes();
    for id in [
        "annotate",
        "annotate.attention",
        "annotate.chart",
        "annotate.notification",
        "annotate.script",
    ] {
        scopes.insert(quantick_control::id::PermissionId::new(id).unwrap());
    }
    quantick_control_local::client::ConnectOptions::for_profile(
        "annotator",
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        scopes,
    )
}

/// Grant the annotate tier for the next connection through the panel's
/// own named call — the door the checkboxes and the hook both use.
pub(super) fn grant_annotate_for_test(app: &mut QuantickApp, scopes: &str) {
    app.control
        .control_access
        .as_mut()
        .expect("control access is installed")
        .configure_scopes(scopes)
        .expect("the test grants registered scopes");
}

/// One remote call, served by the frames it takes: send, let the
/// application drain its queue, read the reply.
pub(super) fn remote_call(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    capability: &str,
    payload: serde_json::Value,
) -> quantick_control::wire::ResponseEnvelope {
    let request_id = client
        .send(capability, payload)
        .expect("the request is sent");
    for _ in 0..400 {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, request_id);
    response
}

/// A client that asks for the cockpit tier, for the tests that prove what a
/// capability declaring `IdempotencyPolicy::Optional` does with a key.
pub(super) fn cockpit_test_options() -> quantick_control_local::client::ConnectOptions {
    let mut scopes = gateway_test_scopes();
    for id in ["cockpit", "cockpit.layout"] {
        scopes.insert(quantick_control::id::PermissionId::new(id).unwrap());
    }
    quantick_control_local::client::ConnectOptions::for_profile(
        "cockpit",
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        scopes,
    )
}

/// `remote_call`, under an idempotency key and a caller-chosen request ID —
/// so a retry can be told apart from the call it retries, which is the whole
/// thing being asserted.
pub(super) fn remote_call_with_key(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    request_id: &str,
    capability: &str,
    payload: serde_json::Value,
    key: &str,
) -> quantick_control::wire::ResponseEnvelope {
    let sent = client
        .send_with_idempotency_key(
            quantick_control::id::RequestId::new(request_id).expect("test request ID is valid"),
            capability,
            1,
            payload,
            quantick_control::id::IdempotencyKey::new(key.to_owned())
                .expect("test idempotency key is valid"),
        )
        .expect("the request is sent");
    for _ in 0..400 {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, sent);
    response
}

/// A gateway directory of this test's own, **not created**: the gateway makes
/// its own instances directory, and handing it one that already exists is a
/// different starting state than every one of these tests was written
/// against. It sits inside this thread's scratch folder, which is removed
/// when the test's thread ends.
pub(super) fn gateway_test_directory(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
    crate::scratch::thread_dir("gateway-test").join(format!(
        "{name}-{}",
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(super) fn enable_test_gateway(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    directory: &std::path::Path,
    queue_capacity: usize,
) -> std::path::PathBuf {
    app.control
        .control_access
        .as_mut()
        .expect("control access is installed")
        .enable_for_test(ctx, directory.to_path_buf(), queue_capacity);
    wait_for_test_gateway_descriptor(app, ctx)
}

pub(super) fn enable_test_gateway_with_limits(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    directory: &std::path::Path,
    queue_capacity: usize,
    request_timeout: std::time::Duration,
    max_connections: usize,
) -> std::path::PathBuf {
    app.control
        .control_access
        .as_mut()
        .expect("control access is installed")
        .enable_for_test_with_limits(
            ctx,
            directory.to_path_buf(),
            queue_capacity,
            request_timeout,
            max_connections,
        );
    wait_for_test_gateway_descriptor(app, ctx)
}

/// How long a test waits for another thread to reach a state. A fixed number
/// of short sleeps is not a clock: on a loaded machine the iterations run out
/// long before the work they are waiting for had its chance, and the wait then
/// reports a defect where there is only contention.
pub(super) const GATEWAY_TEST_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Wait one interval of the connection's request rate limit before a retry.
/// A retry loop that runs faster than the limit is answered by the limiter's
/// `control.backpressure` once its burst is spent, and the refusal a test was
/// waiting out is then mistaken for another. Derived from the limit, so a
/// lower limit slows the loops rather than breaking them.
pub(super) fn pause_one_request_interval() {
    let per_second = quantick_control::limits::CONTROL_CLIENT_RATE_PER_SECOND;
    std::thread::sleep(std::time::Duration::from_secs(1) / per_second);
}

pub(super) fn wait_for_test_gateway_descriptor(
    app: &mut QuantickApp,
    ctx: &egui::Context,
) -> std::path::PathBuf {
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        run_frame(app, ctx);
        if let Some(path) = app
            .control
            .control_access
            .as_ref()
            .and_then(crate::control::ControlAccess::descriptor_path_for_test)
        {
            return path;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "test gateway did not publish discovery within {GATEWAY_TEST_WAIT:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

pub(super) fn wait_for_queued_gateway_requests(app: &QuantickApp, expected: usize) {
    wait_for_queue_depth(app, &format!("reach {expected}"), |queued| {
        queued == expected
    });
}

/// [`wait_for_queued_gateway_requests`] for a caller that only needs the
/// requests to have arrived, not to be the only ones there. Exact equality is
/// a trap once anything else can be in flight: one extra request from another
/// connection, or one left over from an earlier step, and a wait that insists
/// on its own number never sees it.
///
/// What it does not buy: tolerance of the queue shrinking under the caller.
/// The response worker answers a request that missed its deadline without any
/// frame running, and a set that loses one of its own members never reaches
/// its depth under either comparison. A caller that waits for a whole backlog
/// is relying on that deadline being far away, not on this spelling.
pub(super) fn wait_for_at_least_queued_gateway_requests(app: &QuantickApp, expected: usize) {
    wait_for_queue_depth(app, &format!("reach {expected} or more"), |queued| {
        queued >= expected
    });
}

pub(super) fn wait_for_queue_depth(
    app: &QuantickApp,
    wanted: &str,
    reached: impl Fn(usize) -> bool,
) {
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        let queued = app
            .control
            .control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test();
        if reached(queued) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "gateway request queue did not {wanted} within {GATEWAY_TEST_WAIT:?} (queued {queued})"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Run application frames until the gateway request queue is empty. A frame
/// admits up to `CONTROL_UI_MAX_REQUESTS_PER_FRAME` requests, but only inside
/// `CONTROL_UI_BUDGET_US`; on a loaded machine the frame's other work can spend
/// that budget before the drain even starts, and the queue then empties over
/// several frames. What a caller asserts by calling this is that application
/// frames drain the queue — the socket thread never does — so how many frames
/// the budget needed is reported only when the wait gives up.
pub(super) fn drain_gateway_requests(app: &mut QuantickApp, ctx: &egui::Context) {
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    let mut frames = 0;
    loop {
        run_frame(app, ctx);
        frames += 1;
        if app
            .control
            .control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test()
            == 0
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "application frames did not drain the queued gateway requests within \
             {GATEWAY_TEST_WAIT:?} ({frames} frames)"
        );
    }
}

pub(super) fn disable_test_gateway(app: &mut QuantickApp, ctx: &egui::Context) {
    app.control
        .control_access
        .as_mut()
        .expect("control access is installed")
        .disable_for_test();
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        run_frame(app, ctx);
        if app
            .control
            .control_access
            .as_ref()
            .expect("control access is installed")
            .is_disabled_for_test()
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "test gateway did not stop cleanly within {GATEWAY_TEST_WAIT:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

pub(super) fn response_error(
    response: &quantick_control::wire::ResponseEnvelope,
) -> &quantick_control::error::ControlError {
    match &response.outcome {
        quantick_control::wire::ResponseOutcome::Failure { error } => error,
        quantick_control::wire::ResponseOutcome::Success { .. } => {
            panic!("expected a structured gateway failure")
        }
    }
}

pub(super) fn success_result(
    response: &quantick_control::wire::ResponseEnvelope,
) -> serde_json::Value {
    match &response.outcome {
        quantick_control::wire::ResponseOutcome::Success { result } => result.clone(),
        quantick_control::wire::ResponseOutcome::Failure { error } => {
            panic!("expected a success: {error:?}")
        }
    }
}

/// The read scopes an evidence client asks for: everything the safe
/// default grants, plus the two the evidence tier adds — both sensitive,
/// both off until the trader ticks them.
pub(super) fn evidence_test_options() -> quantick_control_local::client::ConnectOptions {
    let mut scopes = gateway_test_scopes();
    for id in ["observe.evidence", "observe.screenshot"] {
        scopes.insert(quantick_control::id::PermissionId::new(id).unwrap());
    }
    // The analyst ceiling, because that is where `observe.evidence` and
    // `observe.screenshot` sit: an observer connection asking for them is
    // capped back to the floor and refused, which is the tier working.
    quantick_control_local::client::ConnectOptions::for_profile(
        quantick_control_local::client::ANALYST_PROFILE_ID,
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        scopes,
    )
}
