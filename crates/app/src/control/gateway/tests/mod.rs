// The `gateway.rs` unit tests, kept as two child modules of `crate::control::gateway`.
//
// They stay inside the crate rather than moving to `crates/app/tests/`: an
// integration test is a separate crate and sees only `quantick-app`'s public
// API, while these reach `ControlAccess`'s private items and the private
// `GatewayOptions`. A child module sees its ancestor's private items, so the
// split costs no widened visibility in production code.
//
// The one `use super::*` below binds `gateway.rs`'s scope here, and each nested
// module's own `use super::*` reaches it in one further hop: a child sees an
// ancestor's private bindings, glob-imported ones included. So the scope inside
// both modules is the scope they had while they lived in `gateway.rs`.
//
// Both carry a `_tests` suffix. Siblings glob each other in through the same
// `use super::*`, and a module named `tests` would be a name every future
// sibling has to step around; the suffix is the convention `app/tests/` settled
// on for the same reason.

use std::sync::atomic::AtomicUsize;

use super::*;
// Three server-thread helpers the tests exercise directly. The host does not
// call all three, so they are named here rather than re-bound in `gateway.rs`
// only to be seen from a test.
use super::server::{activity_status_high_watermark, drain_bounded_since, try_reserve_in_flight};

mod gateway_tests {
    use super::*;

    /// A directory that does not exist yet — the only caller asserts exactly
    /// that — inside this thread's own scratch folder, which is removed when
    /// the thread ends whether or not anything ever created it.
    fn unique_test_directory(name: &str) -> PathBuf {
        static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(1);
        crate::scratch::thread_dir("control-gateway").join(format!(
            "{name}-{}",
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// The panel may not promise what the contract will refuse.
    ///
    /// Every annotate capability requires the `annotate` floor *and* its own
    /// scope. Granting a scope alone used to raise the ceiling to `annotator`
    /// and put "answering on the chart" on the status line over a connection
    /// that was then denied every call it made.
    #[test]
    fn granting_an_annotate_scope_grants_the_floor_every_capability_also_needs() {
        let mut access = ControlAccess::new();
        access
            .configure_scopes("all-reads,annotate.chart")
            .expect("the test grants registered scopes");

        assert!(
            access.grants_annotate(),
            "the panel says the window can be answered on"
        );
        assert!(
            access
                .configured_scopes
                .iter()
                .any(|permission| permission.as_str() == ANNOTATE_PERMISSION_ID),
            "so the floor every annotate capability requires has to be there too"
        );

        // The contrast: the floor on its own opens nothing, so it must not
        // make the status line claim the window can be answered on.
        let mut floor_only = ControlAccess::new();
        floor_only
            .configure_scopes("all-reads,annotate")
            .expect("the test grants registered scopes");
        assert!(
            !floor_only.grants_annotate(),
            "a floor with no scope under it answers on nothing"
        );
    }

    #[test]
    fn empty_discovery_returns_a_next_step_without_creating_or_starting_anything() {
        let directory = unique_test_directory("empty-discovery");
        assert!(!directory.exists());

        let discovery = quantick_control_local::client::discover_in(
            &directory,
            &quantick_control_local::client::ConnectOptions::observer(
                "gateway unit test",
                "0.0.0",
                BTreeSet::new(),
            ),
        )
        .unwrap();
        assert!(discovery.clients.is_empty());
        assert!(discovery.issues.is_empty());
        assert!(!discovery.next_steps.is_empty());
        assert!(!directory.exists());

        let error = discovery.select(None).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
        assert!(!error.context.next_steps.is_empty());
        assert!(!directory.exists());
    }

    #[test]
    fn one_frame_never_drains_more_than_the_reviewed_request_ceiling() {
        let (sender, receiver) = bounded(CONTROL_UI_MAX_REQUESTS_PER_FRAME + 2);
        for value in 0..(CONTROL_UI_MAX_REQUESTS_PER_FRAME + 2) {
            sender.send(value).unwrap();
        }
        let mut handled = Vec::new();
        // A start in the future reads as zero elapsed: the budget cannot
        // interfere, so this proves the count ceiling alone, whatever the
        // scheduler does to the test thread.
        let observation = drain_bounded_since(
            &receiver,
            Instant::now() + Duration::from_secs(60),
            0,
            |value| handled.push(value),
        );
        assert_eq!(handled.len(), CONTROL_UI_MAX_REQUESTS_PER_FRAME);
        assert_eq!(receiver.len(), 2);
        assert!(observation.queue_has_more);
        assert!(
            !observation.budget_exceeded,
            "stopping on the count ceiling is not a budget stop"
        );
    }

    #[test]
    fn a_capture_that_exhausts_the_budget_ends_the_frame_drain() {
        // The count ceiling is the deterministic guard; the elapsed-time
        // budget is the authoritative one (plan §10.2). Prove the latter on
        // its own: captures that each cost the whole budget must not run two
        // to a frame, although the count ceiling alone would allow four. A
        // scheduler that preempts the thread for longer than the budget
        // before the first check makes the drain run nothing, which proves
        // nothing either way — try again, a few times.
        let budget = Duration::from_micros(CONTROL_UI_BUDGET_US);
        let mut last = None;
        for _ in 0..5 {
            let (sender, receiver) = bounded(4);
            for value in 0..4 {
                sender.send(value).unwrap();
            }
            let mut handled = 0usize;
            let observation = drain_bounded_since(&receiver, Instant::now(), 0, |_| {
                handled += 1;
                let until = Instant::now() + budget;
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
            });
            last = Some((handled, observation, receiver.len()));
            if handled == 1 {
                break;
            }
        }
        let (handled, observation, remaining) = last.expect("at least one attempt ran");
        assert_eq!(
            handled, 1,
            "the second capture must wait for the next frame"
        );
        assert_eq!(observation.processed, 1);
        assert!(observation.budget_exceeded);
        assert!(observation.queue_has_more);
        assert_eq!(remaining, 3);
    }

    #[test]
    fn exit_with_access_disabled_costs_nothing() {
        // The default state: no gateway thread exists, none will report, and
        // the application's exit must not wait for one.
        let mut access = ControlAccess::new();
        let started = Instant::now();
        access.shutdown_for_exit();
        assert!(
            started.elapsed() < Duration::from_millis(EXIT_SHUTDOWN_TIMEOUT_MS / 4),
            "a disabled gateway returned at once, not after the exit timeout"
        );
    }

    #[test]
    fn client_rate_limiter_has_a_bounded_burst_and_refills() {
        let started = Instant::now();
        let mut limiter = ClientRateLimiter {
            available_token_nanos: u128::from(CONTROL_CLIENT_BURST)
                * ClientRateLimiter::ONE_TOKEN_NANOS,
            last_refill: started,
        };
        for _ in 0..CONTROL_CLIENT_BURST {
            assert!(limiter.allow(started));
        }
        assert!(!limiter.allow(started));
        assert!(limiter.allow(started + Duration::from_secs(1)));
    }

    #[test]
    fn global_response_slots_enforce_the_reviewed_buffer_bound() {
        assert_eq!(
            CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
                * quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES,
            quantick_control::limits::CONTROL_MAX_BUFFERED_RESPONSE_BYTES
        );
        let in_flight = AtomicUsize::new(0);
        for _ in 0..CONTROL_MAX_BUFFERED_RESPONSE_SLOTS {
            assert!(try_reserve_in_flight(
                &in_flight,
                CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
            ));
        }
        assert!(!try_reserve_in_flight(
            &in_flight,
            CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
        ));
    }

    #[test]
    fn gateway_options_cannot_exceed_reviewed_hard_limits() {
        assert!(GatewayOptions::default().validate().is_ok());

        let mut options = GatewayOptions {
            request_queue_capacity: CONTROL_REQUEST_QUEUE_CAPACITY + 1,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            request_queue_capacity: 0,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_connections: CONTROL_MAX_CONNECTIONS + 1,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_connections: 0,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_in_flight_per_connection: CONTROL_MAX_IN_FLIGHT_PER_CONNECTION + 1,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_in_flight_per_connection: 0,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            handshake_timeout: Duration::from_millis(CONTROL_HANDSHAKE_TIMEOUT_MS + 1),
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            handshake_timeout: Duration::ZERO,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            request_timeout: Duration::from_millis(CONTROL_REQUEST_TIMEOUT_MS + 1),
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            request_timeout: Duration::ZERO,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());
    }

    #[test]
    fn a_revoked_connection_stays_revoked_when_its_socket_closes() {
        // Revoking closes the socket, so the disconnect it causes lands on the
        // same frame as any request the revoked client had already queued.
        // `poll_statuses` runs before `drain_bounded_since`, so lifting the
        // revocation here would hand that request a clean bill of health and
        // let it act. Connection IDs are random per connection, so keeping the
        // id costs nothing: no later client can inherit it.
        let mut access = ControlAccess::new();
        let (_requests_tx, requests) = bounded(1);
        let (statuses_tx, statuses) = bounded(1);
        let (commands, _commands_rx) = bounded(1);
        let connection_id = ConnectionId::from_bytes([7; 16]);
        access.state = AccessState::Enabled(GatewayRuntime {
            grant_generation: 0,
            requests,
            statuses,
            commands,
            cancellation: Arc::new(AtomicBool::new(false)),
            public: GatewayPublicInfo {
                instance_id: InstanceId::from_bytes([1; 16]),
                port: 0,
                descriptor_path: PathBuf::new(),
                published_at_unix_ms: 0,
            },
        });
        access.revoked_connections.insert(connection_id.clone());
        statuses_tx
            .try_send(ConnectionStatus::Disconnected(connection_id.clone()))
            .unwrap();
        access.poll_statuses(Instant::now());
        assert!(
            access.revoked_connections.contains(&connection_id),
            "the revocation outlives the disconnect it caused"
        );
    }

    #[test]
    fn activity_updates_leave_capacity_for_connection_lifecycle_events() {
        let high_watermark = activity_status_high_watermark(CONTROL_MAX_CONNECTIONS);
        assert_eq!(
            GATEWAY_STATUS_CAPACITY - high_watermark,
            CONTROL_MAX_CONNECTIONS * GATEWAY_CRITICAL_STATUS_SLOTS_PER_CONNECTION
        );
    }
}

mod cockpit_tier_tests {
    use super::*;

    /// A profile no code path constructs is a tier nothing can reach.
    ///
    /// The seven `layout.*` capabilities shipped registered, catalogued and
    /// refused at the gate, because `configured_profile` knew only the
    /// observer and the annotator. Nothing failed: the registry was valid, the
    /// schema published, the catalog complete — and every call was denied. A
    /// capability set is not delivered until a connection can hold the ceiling
    /// it needs, so that is what this asserts.
    #[test]
    fn granting_the_cockpit_scopes_reaches_the_cockpit_profile() {
        let mut access = ControlAccess::new();
        access
            .configure_scopes("cockpit,cockpit.layout")
            .expect("the cockpit scopes are registered permissions");
        assert!(
            access.grants_cockpit(),
            "the floor and a scope together open the tier"
        );
        assert_eq!(
            access.configured_profile().as_str(),
            COCKPIT_PROFILE_ID,
            "the layout capabilities are refused under any lower ceiling"
        );
    }

    /// The floor alone opens nothing, and a scope without the floor opens
    /// nothing either — every cockpit capability requires both, so claiming
    /// the tier on half of it would put a grant on the panel that is refused
    /// on every call.
    #[test]
    fn half_the_cockpit_grant_is_not_the_cockpit_tier() {
        for scopes in ["cockpit", "cockpit.layout"] {
            let mut access = ControlAccess::new();
            access
                .configure_scopes(scopes)
                .expect("registered permission");
            assert!(
                !access.grants_cockpit(),
                "{scopes} alone must not open the tier"
            );
            assert_ne!(access.configured_profile().as_str(), COCKPIT_PROFILE_ID);
        }
    }

    /// The annotate tier is untouched by the new one: a trader who granted
    /// only the chart scopes gets exactly what that consent text describes.
    #[test]
    fn the_annotate_tier_does_not_pick_up_the_cockpit_ceiling() {
        let mut access = ControlAccess::new();
        access
            .configure_scopes("annotate,annotate.chart")
            .expect("registered permissions");
        assert!(access.grants_annotate());
        assert!(
            !access.grants_cockpit(),
            "annotate must not carry a grant to rearrange the window"
        );
        assert_eq!(access.configured_profile().as_str(), ANNOTATOR_PROFILE_ID);
    }

    /// The tiers have to **nest**, not merely coexist.
    ///
    /// `handshake::authorize` intersects the requested and the granted ceiling
    /// and then demands that one of the two *be* that intersection — "profiles
    /// are built nested" is the comment it says so under. Two sibling write
    /// tiers overlap without nesting, and an incomparable pair is refused
    /// outright: a client asking for `--profile annotator` against a cockpit
    /// grant could not connect at all.
    #[test]
    fn the_cockpit_ceiling_contains_the_annotator_ceiling() {
        use quantick_control::handshake::ProfileAuthority;

        let access = ControlAccess::new();
        let registry = access.contract.registry();
        let ceiling = |id: &str| {
            registry
                .permission_ceiling(&ProfileId::new(id).expect("static profile ID is valid"))
                .expect("the profile is registered")
        };
        assert!(
            ceiling(ANNOTATOR_PROFILE_ID).is_subset(&ceiling(COCKPIT_PROFILE_ID)),
            "the two write tiers are siblings, so the handshake refuses any \
                 connection that names one against a grant holding the other"
        );
    }

    /// A trader who grants both tiers keeps both.
    ///
    /// The connection takes one ceiling, and every scope the human ticked has
    /// to survive it — `authorize` drops a granted scope the ceiling does not
    /// hold. Under a cockpit ceiling that was a sibling of the annotator, that
    /// meant ticking "rearrange my charts" silently switched off "answer on
    /// the chart".
    #[test]
    fn granting_both_tiers_keeps_every_scope_the_trader_ticked() {
        use quantick_control::handshake::ProfileAuthority;

        let mut access = ControlAccess::new();
        access
            .configure_scopes("annotate,annotate.chart,cockpit,cockpit.layout")
            .expect("registered permissions");
        assert!(access.grants_annotate());
        assert!(access.grants_cockpit());

        let profile = access.configured_profile();
        let ceiling = access
            .contract
            .registry()
            .permission_ceiling(&profile)
            .expect("the configured profile is registered");
        for scope in &access.configured_scopes {
            assert!(
                ceiling.contains(scope),
                "the {profile} ceiling drops the granted scope {scope}"
            );
        }
    }
}
