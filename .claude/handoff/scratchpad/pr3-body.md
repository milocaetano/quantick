## Summary

PR 3 of `docs/mcp-control-plane-development-plan.md`: the running application hosts an explicitly enabled, observer-only control gateway on literal `127.0.0.1`, and the pieces a client needs to find and talk to it live in a crate an adapter can link.

- `crates/app/src/control/gateway.rs` — the gateway: a non-blocking loopback listener, one worker thread per connection (authentication, bounded framing, JSON parsing, schema validation, authorization, rate limiting, response serialization and schema validation all off the application thread), a bounded UI request queue drained inside a per-frame budget, backpressure, timeouts, revocation and clean shutdown. The **Local agent access** panel (Tools menu) enables and disables access, picks the read scopes of the next connection, lists connected clients with per-client revoke, and shows the last drain against the budget.
- `crates/app/src/control/contract.rs` — the immutable observer contract: four read capabilities (`control.describe`, `snapshot.read`, `chart.window.read`, `health.diagnostics.read`), the observer permission and scope descriptors, compiled input/output validators, and the prepared-dispatch split between worker and UI thread.
- `crates/control-local` (**new**, `quantick-control-local`) — the private instance-descriptor directory (publication by the app, discovery by a client, one implementation of the ownership/ACL checks on each platform) and the blocking loopback client (`LocalClient`, `discover`, `LiveInstances::select`). Depends only on `quantick-control`; never starts the app, never binds a listener.
- `crates/control` — `InstanceDescriptor` (strict, loopback-only), `HandshakeReply`, typed handshake reads on the codec, the pre-authentication codec ceiling, and the reviewed limits the gateway enforces; schema catalog and snapshots extended.
- Two hooks (`QUANTICK_CONTROL_PANEL`, `QUANTICK_CONTROL_ACCESS`) reach the Local agent access surface from a launch through the menu entry's and the panel button's own functions; registered in `ui-harness`.

Authored in this worktree by Codex (first commit, committed as found); rebased, reshaped and gated by Claude (the following commits). Evidence: `docs/control-plane/pr3-gateway-evidence.md`.

**Stacked on #213 (PR 2). Base branch: `feat/control-observer`** — merge #213 first; GitHub retargets this PR to `main` when its base lands.

## Rate class and tier

Startup and infrequent requests. Observe tier only — no capability in this PR changes user-visible state; enabling access is the human's action in the panel and is not exposed remotely.

## Performance

Idle frame (no request pending, the production default): the plan's method — the ignored `control_idle_dense_replay_benchmark` built from identical source into the base (PR 2 head) and this branch, `CARGO_INCREMENTAL=0`, 600 measured frames × 64 live trades, eight alternating pairs (five base-first, three candidate-first) on one host. `frame_cpu_ms` median: base 0.941 ms, candidate 0.878 ms; frame p99 median: base 1.34 ms, candidate 1.19 ms. The candidate is never slower in any pair; the small consistent advantage is layout noise and is not claimed. With the gateway disabled `needs_frame_service()` is false and the frame loop never enters the control module.

Enabled: control work is admitted under a 250 µs elapsed-time budget per frame (authoritative; `a_capture_that_exhausts_the_budget_ends_the_frame_drain`) and a deterministic ceiling of four requests per frame (`one_frame_never_drains_more_than_the_reviewed_request_ceiling`); a coherent capture is never preempted; remaining work requests another repaint (`gateway_ui_budget_defers_work_beyond_one_frame`). The maximum chart page (32 bars) captures at a measured p99 of ~100 µs; the core observer snapshot at ~22 µs.

## Security and data honesty

- Disabled on every process start; enabling creates a fresh 256-bit bearer token, publishes a private descriptor, and disabling or exit removes it.
- Token comparison is constant time; the handshake binds instance ID, process nonce, protocol range, profile ceiling and effective scopes; sensitive scopes (`observe.paper`, `observe.evidence`, `observe.user_text`, `observe.diagnostic_logs`, `observe.screenshot`) are not in the default grant.
- Unix publication verifies owner and `0700`/`0600`; Windows publication verifies the owner SID, installs a protected DACL, rejects reparse points and foreign allow ACEs. Discovery verifies, never rewrites.
- Every failure is a stable `control.*` code; no token, path or panic text crosses the wire.
- Discovery of an empty or missing directory returns no candidates and a next step; nothing in the gateway or the client can start the application.

## Audit against the plan, ADR 0001 and the threat model

Every PR 3 deliverable and acceptance criterion, every ADR 0001 required test, every normative statement in ADR §2–6 and every observer threat-model control was checked against the code before review. Closed here: a post-authentication read timeout (a half-written frame no longer holds a connection thread; the codec tells an idle timeout from a stalled frame), duplicate `request_id` rejection while in flight (contract §5.2), the advertised `request_timeout_ms` now being the applied one, bounded discovery over a polluted directory, a unit test proving the elapsed-time budget on its own, and seven gateway tests (request before handshake, literal loopback endpoint, exit shutdown removes discovery, per-client revoke, a non-reading client does not stall another, a half-written frame, observer reads leave every module revision unchanged).

Deferred, with the reason (details in the evidence doc): stale-descriptor cleanup by a client and PID-reuse proof (ADR permits rather than requires; lands with the adapter), actor context/audit records for reads (contract §6: none for observe; PR 5a), the per-frame request count kept as a second guard beside the authoritative time budget, the global in-flight ceiling reusing the buffered-response slot count (conservative), and the panel listing the registry's declared-but-not-yet-consumed scopes (generated from the registry on purpose).

## Docking

- Another owner module docks into the gateway by registering a capability in `ObserverContract` (`a_second_registered_handler_docks_without_changing_gateway_dispatch`) and a projection in `ProjectionRegistry`; nothing in `gateway.rs` names a capability.
- The adapter docks through `quantick-control-local` (discovery + `LocalClient`) and the contract crate only; it never links the application. The crate's own tests prove the client against a fake loopback gateway.
- Blast radius: added `crates/control-local/*` (3 files), `crates/app/src/control/{gateway,contract}.rs`, `crates/control/src/descriptor.rs`, 8 schema documents, the evidence doc; edited `app.rs` (one field, the Tools menu entry, the on-exit hook, the first-frame service call and the two env hooks — plus tests), `control/mod.rs`, `registry.rs` (permissions per scope, scope-count bound), `chart.rs` (page ceiling), the codec/handshake/limits/schema modules of `control`, `workspace_deps`, `CLAUDE.md`, the plan §4.1, ADR 0001 consequences, `ui-harness`.

## Verification

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` — all exit 0 on the branch head.
- `cargo test -p quantick-app gateway -- --test-threads=1`: 22 passed. `cargo test -p quantick-control-local`: 10 passed.
- Visual QA of the Local agent access panel: **not run** — the session was not authorized to launch the desktop app; the surface is reachable for a later pass through `QUANTICK_CONTROL_PANEL=1` / `QUANTICK_CONTROL_ACCESS=1`.

## Trader UX review (Local agent access panel)

Flow: *Tools → Local agent access… → pick read scopes → Enable observer access*; later *Revoke* one client or *Disable and revoke all clients*; the status line shows **Agent access: on** while enabled.

- Rafa (scalper): never touches it mid-tape; it is a menu-invited, non-modal egui window that steals no focus and paints nothing on the chart; when enabled, control work is bounded at 250 µs per frame and the idle benchmark shows no frame cost. Can trade through it.
- Marina (multi-tab): one dialect (an egui window like the style and footprint dialogs); the status button is glanceable from any tab. Access is off on every start by design (threat model), so she re-enables per session — the scope *selection* is not remembered across restarts either: **Consider**, deferred (a remembered selection would be a stored grant, which wants its own threat-model line).
- Duda (newcomer): the header states in plain words that granted data may be sent to the model provider; the sensitive scopes are marked and off by default; enable/disable/revoke are one click each and none is destructive to her work. **Should-fix, resolved in this PR:** the scope checkboxes were labelled by raw ID (`observe.attention`) with the meaning a hover away — they now read the description first with the ID beside it.

No Blocker. Rafa trades through it, Marina keeps her workspace, Duda can find and understand it without the manual.

## Architecture review

step 0: code-review at high over `feat/control-observer...HEAD` (the PR 3 diff alone; the bundled skill's eight finder angles never reported — the pass is the reviewer's own over `gateway.rs`, `codec.rs`, `discovery.rs`, `client.rs` and the `app.rs` wiring), 6 findings, 0 confirmed, 6 plausible — all low; nothing open.

Deferred, with reason:
- One response thread per UI-bound request (bounded by 8 in-flight × 8 connections) and the accept loop's 5 ms poll while enabled: both live on gateway threads, never the render thread; measured idle-frame cost is nil (above) and the enabled cost is bounded by the reviewed caps. A per-connection timer and a blocking accept with a wake socket are refinements for when a measurement asks for them.
- Stale descriptors of a crashed instance are listed until a connect fails (reported as `control.instance_gone` with a next step), and PID reuse is not detected: ADR 0001 §5 permits cleanup rather than requiring it; the proof-of-identity removal lands with the adapter's own housekeeping.
- Requests queued when the user disables access wait out the request timeout on their response threads instead of being refused at once: the client receives a structured `control.timeout`/`control.instance_gone`; immediate refusal is a refinement.
- A worker-side `describe` between revoke and socket close is answered: the window is the accept loop's next poll (5 ms); the UI path already rechecks revocation before dispatch.
- A client that connects and sends nothing until the handshake timeout is rejected as `control.invalid_request` rather than `control.timeout`: the connection is closed either way; the code will say `timeout` in the next change that touches the handshake path.

Second pass (`code-review 221 high` after the budget-boundary fix; the reviewer fork ended waiting for its finders, so the three bug-hunting finders — pitfalls, tracer, line-scan — reported to the coordinator directly, and each candidate was verified against the code). Of their findings, seven resolved on the branch, four deferred with reason:

1. `shutdown_for_exit` waited the full `EXIT_SHUTDOWN_TIMEOUT_MS` (2 s) on every normal application exit when access had never been enabled — the default state, where no gateway thread exists and none will report — then logged a spurious timeout. `Disabled` returns at once (`exit_with_access_disabled_costs_nothing`).
2. Any `accept()` error other than `WouldBlock` tore the whole gateway down (descriptor removed, every client dropped), although a peer that resets or aborts before accept — a port scan — is that connection's failure: `ConnectionReset`, `ConnectionAborted` and `Interrupted` are logged at debug and skipped; the listener's own failures still stop access.
3. `reject_connection_capacity` wrote the backpressure reply and closed with the client's handshake unread, which resets the socket and can discard the reply before the client reads it (Windows) — it now reads the handshake first, off the accept thread.
4. The accepted handshake was written before the admission checks (`Identified` command, `Connected` status at the high-watermark), so a client told "accepted" and then dropped for a saturated channel learned of it only as `control.instance_gone` on its first request — admission comes first and a refused client hears `control.backpressure`; an `Accepted` frame that fails to write after the application heard "connected" sends `Disconnected`.
5. `budget_exceeded` was re-derived from a later clock reading, so a drain that stopped on the count ceiling, or ran nothing because statuses spent the budget, was labelled "exceeded by one non-preemptible capture" — the reason is recorded where the loop stops, and the panel distinguishes "budget spent before any request ran"; the two drain unit tests no longer depend on the scheduler (a future start for the count-ceiling test, a bounded retry for the budget test).
6. `LocalClient::connect` advertised the instance's own protocol range as the client's, making negotiation vacuous — it advertises the range this build implements and refuses, before dialling and with `control.version_unsupported`, an instance whose advertised range does not overlap; `send_with_request_id` validates the envelope before it leaves, so a caller's malformed request is refused structured instead of closing the connection.
7. `GatewayOptions::validate` accepted a sub-millisecond `request_timeout` that advertised as `0` and failed every handshake — refused with the other invalid limits.
8. Windows ownership check: an elevated process creates objects owned by its token's default owner (`BUILTIN\Administrators`), not the user SID, so access could not be enabled from an elevated Quantick — the path is accepted when owned by the user SID **or** the token's `TokenOwner` SID (`current_windows_token_owner_sid`); not verified in an elevated session here, stated as such.

Deferred: the handshake's `effective_limits.default_page_items` / `max_page_items` are protocol-level defaults while `chart.window.read` caps its page at 32 — the per-capability ceiling is the descriptor's `expected_cost.max_items`, documented there; the global buffered-response slots (8) equal the per-connection cap, so one client that stops reading can hold them for a write timeout each — the threat model treats local authenticated clients as semi-trusted and the write timeout bounds it, a per-connection share is the follow-up if measured; stale descriptors are not garbage-collected (ADR 0001 §5 housekeeping, carried from the first pass); a post-handshake envelope that fails validation still closes the connection on the gateway side (the client now refuses it first).

Shape: docking — another capability docks in `ObserverContract` (proven by `a_second_registered_handler_docks_without_changing_gateway_dispatch`), another client in `quantick-control-local`; performance — startup and per-request, idle frame flat (eight pairs), enabled work under a 250 µs budget proven on its own; hardcoded — every limit is a named constant in `quantick_control::limits` or a module constant with its unit; tests — 22 gateway + 10 local-transport tests, each ADR required test present or deferred by name; standardisation and readability — clean; second operator — the panel's enable/disable/revoke are named functions the hooks call (`QUANTICK_CONTROL_PANEL`, `QUANTICK_CONTROL_ACCESS`), the enabled state is readable (`describe`, the status button), and the surface is registered in `ui-harness`.
