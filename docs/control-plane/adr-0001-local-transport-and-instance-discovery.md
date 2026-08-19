# ADR 0001: Local transport and running-instance discovery

**Status:** Accepted

**Date:** 2026-08-19

**Decision owners:** Quantick maintainers

## Context

An agent must inspect the Quantick instance the user already has open. The
adapter cannot create a second application process and pretend it represents
the visible session. The gateway must also remain independent of MCP so a CLI
and later adapters can use the same contract.

The first implementation must work on Windows, Linux, and macOS without
placing network or serialization work on the render, trade-ingest, or depth
paths. It must support more than one running instance and fail closed when an
instance descriptor is stale or ambiguous.

## Decision

### 1. Process topology

The running Quantick application hosts a local control gateway. A separate
`quantick-mcp` process is launched by Codex, Claude Code, or another MCP client
over STDIO. The adapter discovers the application gateway, authenticates, and
translates MCP calls into the vendor-neutral Quantick control protocol.

```text
MCP client
    |
    | MCP over STDIO
    v
quantick-mcp
    |
    | authenticated Quantick control protocol
    v
gateway in the already-running Quantick app
```

Neither `quantick-mcp` nor the gateway may start Quantick. With no running
instance, discovery returns an empty list and a next step telling the user to
start the application.

### 2. Gateway transport

The first gateway transport is TCP bound to the literal IPv4 loopback address
`127.0.0.1` on an operating-system-assigned port. It never binds to `0.0.0.0`,
a LAN interface, a hostname, or IPv6 wildcard. Remote access is out of scope.

The wire format is length-prefixed UTF-8 JSON:

```text
4-byte unsigned big-endian payload length
exactly that many UTF-8 JSON bytes
```

The receiver validates the length against
`CONTROL_PROTOCOL_MAX_FRAME_BYTES` before allocating or parsing the payload.
Requests and responses carry IDs, so one connection may multiplex bounded
in-flight work. The first frame must be a handshake. Any other first frame
closes the connection without executing a capability.

The handshake contains:

```text
protocol_min
protocol_max
instance_id
bearer_token
client_name
client_version
requested_profile
requested_scopes
```

An accepted response contains:

```text
protocol_version
instance_id
process_nonce
connection_id
application_version
application_commit
effective_profile
effective_scopes
effective_limits
```

The response chooses the highest mutually supported protocol version. The app
intersects the requested profile and scopes with the configured connection
ceiling and the user's current grant. It never echoes the bearer token. Token
comparison is constant-time, and an authentication failure returns only a safe
code before closing the connection. Authentication and profile selection finish
before the client can list or invoke capabilities.

### 3. Instance identity and token

Each application process creates at startup:

- a cryptographically random 128-bit `instance_id`;
- a cryptographically random 128-bit `process_nonce`;
- a cryptographically random 256-bit bearer token.

The token is encoded with unpadded base64url. It is valid only for that
process, is rotated when local access is re-enabled, and is destroyed when
access is disabled or the application exits. It is never accepted from a URL,
command-line argument, log field, or MCP configuration file.

### 4. Descriptor directory

When the user enables local observer access, the app publishes one descriptor
named `<instance_id>.json` in the current user's private Quantick runtime
directory:

| Platform | Directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Quantick\control\instances` |
| Linux | `$XDG_RUNTIME_DIR/quantick/control/instances` |
| macOS | `~/Library/Application Support/Quantick/control/instances` |

On Linux, gateway startup fails closed when `XDG_RUNTIME_DIR` is unavailable or
not owned by the current user. A persistent or world-shared temporary directory
is not a fallback for a bearer token.

The directory and descriptor must be accessible only to the current user:

- Unix creates the directory with mode `0700` and the file with mode `0600`.
- Windows creates them under the user's Local AppData and verifies that the
  effective ACL does not grant read access to broad principals such as
  `Everyone` or `Users`.

The descriptor is written to a new file, flushed, and atomically renamed after
the socket is listening. Symlinks and reparse-point redirection are rejected.
Its maximum encoded size is `CONTROL_DESCRIPTOR_MAX_BYTES`.

The descriptor contains only:

```text
descriptor_version
instance_id
process_nonce
process_id
process_started_at_unix_ms
application_version
application_commit
protocol_min
protocol_max
transport
host
port
bearer_token
published_at_unix_ms
```

For descriptor version 1, `transport` is exactly `tcp`, `host` is exactly
`127.0.0.1`, and `port` is an integer from 1 through 65,535. The endpoint is not
a URL or free-form command string.

No workspace content, feed credential, account identifier, symbol, position,
filesystem path outside the descriptor directory, or client permission is
published there.

### 5. Discovery and stale descriptors

The adapter reads only regular `.json` files owned by the current user and
smaller than `CONTROL_DESCRIPTOR_MAX_BYTES`. It validates every field before
using an endpoint.

For each candidate it:

1. connects to the advertised loopback endpoint within the handshake timeout;
2. authenticates with the descriptor token;
3. verifies that the returned `instance_id` and `process_nonce` match;
4. records the live instance and closes candidates that fail validation.

The app removes its descriptor on clean disable and shutdown. A client may
remove a stale descriptor only when it owns the file and can prove the process
identity no longer exists or no longer matches its recorded start time. A
connection failure alone is not permission to delete the file of a slow or
suspended process.

Live instances are returned in deterministic
`(published_at_unix_ms, instance_id)` order. One live instance may be selected
automatically. More than one returns
`control.instance_ambiguous` until the user or client chooses an `instance_id`.
The adapter never silently selects the newest window.

### 6. Request execution

Socket I/O, authentication, JSON parsing, serialization, compression, parked
waiters, and client rate limiting run away from the UI thread. The gateway
places only structurally validated, statically authorized, bounded envelopes in
the UI request queue. The UI-thread dispatcher rechecks dynamic availability,
current permission generation, preconditions, and expected revisions
immediately before invoking a handler; queue time never turns an old check into
authority.

The UI thread drains work up to `CONTROL_UI_BUDGET_US` for one frame. It
produces owned DTOs and never waits for a client. Full queues return
`control.backpressure`. `wait_for_change` registers its cursor outside the UI
queue and enters the queue only when it has a bounded event page to read.

### 7. MCP transport

STDIO is the required first transport for `quantick-mcp`. The client owns the
adapter process lifecycle; the adapter owns no application lifecycle. MCP
server instructions put the connection rule, read-before-act rule, instance
selection rule, and authority boundary in their first 512 characters.

Streamable HTTP for MCP may be added later. It does not replace or expose the
in-app gateway, and it requires its own authentication review.

## Why loopback TCP first

Loopback TCP gives one implementation on all three target operating systems,
supports concurrent local clients, and lets an independent CLI prove the port.
The bearer token and private descriptor directory provide the authorization
boundary that a port number does not.

Named pipes and Unix domain sockets offer useful operating-system identity and
ACL properties, but require two transports and platform-specific lifecycle
code before the contract is proven. They remain a compatible future gateway
transport because the contract and framing do not depend on TCP.

## Alternatives considered

### Embed MCP in the application

Rejected. It couples the app to one client protocol, puts MCP lifecycle inside
the UI process, and prevents a CLI from proving the control port.

### Start a new Quantick process from the adapter

Rejected. The user wants the already-open session. A new process has different
state and creates a ghost platform the user is not looking at.

### Use STDIO directly between the client and the app

Rejected. STDIO belongs to a child process and cannot attach to an arbitrary
running desktop process. It remains correct between the MCP client and
`quantick-mcp`.

### Expose HTTP directly from the app

Rejected for the first release. It increases parsing and routing surface,
encourages remote use before authentication exists, and conflates the control
contract with a public API. A later HTTP adapter can use the same gateway.

### Use named pipes on Windows and Unix domain sockets elsewhere

Deferred. This may replace or supplement loopback TCP after the contract is
proven and the threat model has platform-specific tests.

### Inject mouse and keyboard input

Rejected. Coordinates are not stable capabilities, do not provide structured
results, cannot explain availability, and make evidence depend on pixels.

## Consequences

- The app gains a small cold-path gateway host but no MCP dependency.
- `quantick-mcp` remains a leaf adapter and may restart independently.
- A private descriptor contains a bearer secret, so permissions and stale-file
  handling are security-critical and require platform tests.
- Loopback traffic is plaintext. This is acceptable only under the local-user
  threat boundary and must not be extended to non-loopback interfaces.
- More than one open instance requires an explicit choice.
- A future CLI can connect with the same descriptor, handshake, and framing.

## Required tests

PR 3 must cover:

- bind only to `127.0.0.1`;
- descriptor owner, permissions, maximum size, atomic publication, and clean
  removal on each supported platform;
- wrong token, wrong instance ID, wrong nonce, and non-overlapping versions;
- stale descriptor, PID reuse, suspended process, and abrupt app exit;
- zero, one, and multiple live instances;
- oversized and truncated frames before JSON parsing;
- full queues, request timeout, rate limit, and connection limit;
- concurrent parked wait and ordinary read;
- shutdown with connected and half-open clients;
- proof that the adapter cannot start the application.

## References

- [Quantick control contract](control-contract.md)
- [Observer threat model](observer-threat-model.md)
- [OpenAI MCP documentation](https://developers.openai.com/codex/mcp)
- [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp)
