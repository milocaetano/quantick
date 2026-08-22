# PR 3 local gateway evidence

**Branch:** `feat/control-gateway`

**Rate class:** Startup and infrequent requests.

## Result

The running application now hosts an explicitly enabled, observer-only control
gateway on literal `127.0.0.1`. Socket I/O, authentication, bounded framing,
JSON parsing, schema validation, authorization, response serialization, rate
limiting, and timeout handling run outside the application thread. The
application thread receives only prepared requests through a bounded channel
and returns owned projection DTOs through a bounded response channel.

The gateway is transport-neutral above the framing boundary. It has no MCP
dependency, never starts Quantick, and exposes no action capability. A later
adapter can discover and invoke the same versioned contract without moving
application behavior into MCP.

## Runtime topology

```text
local client
  -> private instance descriptor
  -> authenticated 127.0.0.1 TCP connection worker
  -> validated and authorized prepared request
  -> bounded UI request queue
  -> owned semantic DTO captured within the frame budget
  -> response worker serialization and schema validation
  -> correlated response envelope
```

The listener is non-blocking. Each accepted connection is explicitly changed
to blocking mode before it enters its timeout-bound worker; this is required on
Windows, where an accepted socket may otherwise preserve the listener's
non-blocking behavior. The UI path performs only non-blocking channel
operations and does not acquire the socket writer mutex.

## Discovery and authority

- Local access is off on every process start.
- Process initialization creates a 128-bit instance ID and a 128-bit process
  nonce. Each explicit enable creates a new 256-bit ephemeral bearer token;
  all three values come from the operating-system random source.
- The descriptor fixes `transport` to `tcp` and `host` to `127.0.0.1`, carries a
  strict descriptor version, and has a 16 KiB read-before-parse limit.
- Publication writes, flushes, synchronizes, and atomically renames a private
  file. Clean disable and application exit remove it.
- Unix publication verifies current ownership and `0700`/`0600` modes. Windows
  publication verifies that the security descriptor owner matches the current
  process token, rejects reparse points, installs a protected DACL, requires
  current-user read access, and rejects allow ACLs for principals other than
  that user, `SYSTEM`, and builtin administrators. Discovery only verifies the
  policy and does not rewrite it.
- Authentication compares the token in constant time. The handshake also
  binds the instance ID, process nonce, protocol range, profile ceiling, and
  effective scopes.
- Every enable rotates the token and authority generation. Disable and client
  revocation invalidate queued work before application-thread execution.
- Sensitive paper, evidence, user-text, diagnostic-log, and screenshot scopes
  are not part of the default grant.

## Shared local transport crate

The descriptor directory (publication and discovery, with the platform
ownership and ACL checks) and the blocking loopback client live in
`quantick-control-local`, a crate that depends only on `quantick-control`.
The application uses its publication half; the MCP adapter — which never
depends on the application — and a later CLI use its discovery and client
halves, so the security-critical file checks exist once. The application's
gateway tests drive the same `LocalClient` the adapter will use, and the crate
proves that client against a fake loopback gateway of its own: accepted and
rejected handshakes, a reply for another process, a closed port, selection
among zero, one and many instances, and an empty directory that is neither
created nor published into.

## Reachable without a click

`QUANTICK_CONTROL_PANEL=1` opens the Local agent access window through the
Tools menu entry's own function, and `QUANTICK_CONTROL_ACCESS=1` enables
observer access on the first frame through the panel button's own function.
There is one path to an enabled gateway for the human, the hook and any later
operator; both hooks are registered in the `ui-harness` table.

## Bounds and stable failures

The reviewed production defaults remain centralized in
`quantick_control::limits`: 64 discovery entries, 64 KiB and 32 scopes before
authentication, 64 queued UI requests, 8 connections, 8 in-flight requests per
connection, 8 global maximum-response slots, a 40-request burst refilled at 20
requests per second, a 2 second handshake timeout, and a 5 second request
timeout. Application-thread chart pages have their own measured ceiling of 32
bars; the larger general protocol page ceiling remains available only to
capture shapes that do not exceed the UI budget.

The gateway returns structured stable errors for:

| Condition | Error code |
| --- | --- |
| Wrong token, instance ID, or process nonce | `control.auth_failed` |
| Non-overlapping protocol version | `control.version_unsupported` |
| Missing or closed instance | `control.instance_gone` |
| Multiple live instances without a selection | `control.instance_ambiguous` |
| Full queue, connection limit, in-flight limit, or rate limit | `control.backpressure` |
| Request not completed before its deadline | `control.timeout` |
| Scope outside the authenticated grant | `control.scope_denied` |

Discovery of an empty directory returns no candidates and a next step. It does
not create the directory, publish a descriptor, or launch the application.

## Frame budget

Control work is admitted under both a 250 microsecond elapsed-time budget and a
deterministic ceiling of four requests per frame. The elapsed-time guard is
authoritative; one coherent capture is never preempted midway. Remaining work
requests another repaint.

The real-socket acceptance test queues eight snapshot requests over two clients.
After one frame at least four remain queued, proving that work is deferred rather
than drained without bound. Subsequent frames complete every correlated
response. An initial maximum chart page of 2,048 bars measured a 6,857
microsecond p99 and was rejected by the performance test. The capability-specific
ceiling of 32 bars measured a 99 microsecond p99 and a 100 microsecond worst
capture over 100 captures. The core observer snapshot measured a 22 microsecond
p99 and a 24 microsecond worst capture over 500 captures.

## Versioned contract artifacts

The committed schema catalog now includes the strict instance descriptor,
handshake reply, observer capability inputs, describe result, snapshot-scope
descriptor, and semantic outputs. `observer-capability-catalog-v1.json` is
generated from the same `ObserverContract` used by the running gateway; it is a
review snapshot, not a second registry. One persistent input and output
validator is compiled when each capability is registered and reused for every
request and response.

## Focused verification

```text
cargo check -p quantick-app --all-targets
  PASS

cargo test -p quantick-app gateway -- --nocapture --test-threads=1
  PASS: 15 passed, 0 failed

cargo test -p quantick-app observer_ -- --nocapture --test-threads=1
  PASS: 11 passed, 0 failed
  CONTROL_CORE_CAPTURE {"capture_p99_us":22,"capture_worst_us":24,"captures":500}
  CONTROL_MAX_CHART_WINDOW_CAPTURE {"capture_p99_us":99,"capture_worst_us":100,"captures":100,"bars":32}
```

The full workspace format, lint, build, and test gates are recorded after the
architecture review, immediately before the PR 3 commit.

## Deferred surface

PR 3 deliberately does not expose MCP tools, semantic events,
`wait_for_change`, evidence capture, annotations, state-changing actions, or
trading. Those surfaces retain the delivery order and authority tiers in the
development plan. A parked wait and its concurrency acceptance therefore land
with the event capability rather than as a hidden gateway-only operation.
