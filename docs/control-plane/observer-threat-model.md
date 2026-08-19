# Observer profile threat model

**Status:** Accepted for implementation

**Date:** 2026-08-19

**Scope:** The local `observer` profile, the instance descriptor, the in-app
gateway, `quantick-mcp`, semantic snapshots, events, attention state, and
in-memory evidence resources.

This threat model does not authorize cockpit, paper, or live actions. Each
later authority tier must extend this document before it ships.

## 1. Security objectives

The observer implementation must:

1. expose only an instance the user explicitly enabled;
2. remain reachable only from the local machine and current OS user;
3. authenticate every connection before disclosing instance state;
4. prevent an observer from changing user-visible or financial state;
5. minimize and label sensitive data leaving the application;
6. bound memory, CPU, queue, connection, and retention costs;
7. preserve snapshot consistency and report missing or stale data honestly;
8. make connected clients and revocation visible in the application;
9. fail closed on ambiguity, malformed input, unsupported versions, or
   unknown permissions;
10. leave no token, evidence bundle, or descriptor after its intended
    lifetime.

## 2. Assets

| Asset | Sensitivity | Examples |
| --- | --- | --- |
| Instance authority | Critical | bearer token, effective profile, connection identity |
| Trading state | High | paper positions, orders, P&L, strategy state |
| User-created content | High | drawing text, typed marks, script source, trade notes |
| Workspace state | Medium | tabs, symbols, layouts, active feed, viewport |
| Market data | Medium | visible bars, order flow, depth, replay data |
| Diagnostic data | Medium to high | logs, paths, endpoint errors, build and OS details |
| Attention state | Medium | cursor target, selected object, recent human marks |
| Evidence resources | Aggregate high | correlated state, events, logs, metrics, optional image |
| Availability | High during trading | frame time, feed handling, UI responsiveness |

Market data may have venue licensing restrictions even when it is public to the
application. The control plane does not imply a right to redistribute it.

## 3. Actors

| Actor | Trust level | Notes |
| --- | --- | --- |
| Quantick user | Trusted authority | Enables access, chooses scopes, revokes clients |
| Quantick app | Trusted computing base | Owns state, authorization, capture, and action dispatch |
| `quantick-mcp` | Trusted local adapter | Translates MCP; has no domain authority of its own |
| Configured MCP client | Conditionally trusted | Receives only granted scopes; may transmit data to a model provider |
| Other local OS user | Untrusted | Must not read descriptor or connect successfully |
| Untrusted local process under another user | Untrusted | May scan ports and race descriptor publication |
| Same-user malicious process | Outside isolation guarantee | Can often read the user's files or process memory already |
| Remote network peer | Untrusted | Must have no route to the gateway |
| Market, feed, log, or imported text | Untrusted data | Must never become instructions or executable input |

The same-user malware limitation is explicit: a bearer token in a file cannot
protect a user from a process already running with that user's full filesystem
rights. The design still prevents accidental discovery, cross-user access,
network access, stale reuse, and unauthorized MCP clients.

## 4. Trust boundaries

```text
remote model/provider
        ^
        | data chosen by the configured MCP client
--------+---------------- client trust boundary ----------------
Codex / Claude Code / other local MCP client
        |
        | MCP over STDIO
        v
quantick-mcp
        |
        | authenticated loopback protocol
--------+---------------- app process boundary -----------------
gateway I/O threads -> bounded UI request queue -> Quantick state
        |
        +-> bounded event journal and in-memory evidence store
```

The model provider is not part of the local trusted computing base. Once the
configured client sends granted data to a provider, Quantick cannot enforce
provider retention. The app must make this consequence clear when observer
access is enabled.

## 5. Consent and profile boundary

Local control access is disabled at every application start. It is not restored
from workspace state. Enabling it creates a new token and descriptor for that
run. Disabling it:

- stops accepting connections;
- closes current connections;
- rotates and destroys the token;
- removes the descriptor;
- clears retained evidence and parked waiters;
- leaves a local audit event.

The in-app access panel shows client name, connection time, requested and
effective profile, granted read scopes, last request time, and a revoke action.
The maximum effective profile and scope set are fixed at authentication.
Increasing authority closes the old connection and requires a new handshake;
removing a scope or revoking access applies immediately to in-flight and future
requests.

`observer` is a ceiling, not permission to return every field. The first
implementation exposes these read scopes independently:

```text
observe.system
observe.workspace
observe.market
observe.chart
observe.indicators
observe.drawings
observe.orderflow
observe.replay
observe.paper
observe.health
observe.attention
observe.events
observe.evidence
```

The following sensitive or aggregate additions are off by default and require
a visible scope grant:

```text
observe.paper
observe.evidence
observe.user_text
observe.diagnostic_logs
observe.screenshot
```

Raw filesystem paths, credentials, environment variables, arbitrary files,
shell access, and process launch are not observer scopes.

## 6. Allowed and denied behavior

### Allowed

- List live instances without disclosing their state.
- Read granted semantic snapshots, chart pages, scene state, and diagnostics.
- Read the resolved cursor, current selection, and marks created by the human.
- Read bounded semantic events and wait for a cursor change.
- Create a bounded, redacted evidence bundle in memory.
- Read evidence resource chunks while the resource remains retained.

### Denied

- Create a mark, annotation, drawing, notification, script, preset, or note.
- Change a tab, focus, viewport, layer, replay transport, or configuration.
- Place, cancel, modify, close, flatten, arm, or disarm anything.
- Export evidence or any other data to an arbitrary path.
- Read an arbitrary file, directory, environment variable, clipboard, or shell
  result.
- Start Quantick, a terminal, a broker, or any other process.
- Select one of several instances silently.

A human-created mark is observable. `attention.mark.create` is an annotate
capability and is not exposed to the observer profile.

## 7. Data minimization and redaction

- Snapshot callers name scopes; omitted scopes are not captured and are listed
  as omitted.
- Chart reads default to the visible window and require pagination beyond it.
- User-authored text is replaced with a presence marker unless
  `observe.user_text` is granted.
- Attention and scene references do not bypass the target module's scope. For
  example, a cursor over a paper order requires both `observe.attention` and
  `observe.paper` to reveal order fields; otherwise it reports only a redacted
  target kind and stable presence marker. Redacted reference IDs are scoped
  opaque aliases and never embed a symbol, account, order, path, or note.
- Diagnostics use an allowlist of structured fields. Raw log lines are not
  returned unless `observe.diagnostic_logs` is granted, and are redacted even
  then.
- Tokens, credential-shaped values, authorization headers, account secrets,
  environment values, and private keys are always removed.
- Paths are reduced to logical store IDs or basenames unless a specific safe
  path is required to explain a failure. Home directories and usernames are
  removed.
- Evidence manifests state every omitted, redacted, inferred, stale, or
  unavailable field.
- Evidence capture requires `observe.evidence` plus every source scope included
  in the bundle; it cannot use aggregation to launder an ungranted scope.
- Evidence resource IDs are random and unguessable but are not treated as
  authorization. Every chunk read rechecks the current grant against the
  manifest's source scopes.
- Screenshots require `observe.screenshot`, an explicit capture request, and a
  visible in-app indicator. They carry the same retention limits as the bundle.

External strings such as symbol names, imported text, venue errors, script
diagnostics, and note contents are data. They never contribute to MCP server
instructions, capability descriptions, schema text, or executable commands.

## 8. Threats and required controls

| ID | Threat | Required controls | Verification |
| --- | --- | --- | --- |
| O-01 | Gateway exposed to LAN or internet | Bind literal `127.0.0.1`; reject configured host overrides; no port forwarding feature | Socket integration test checks bind address and remote-interface refusal |
| O-02 | Another OS user reads the token | Private runtime directory, owner and ACL checks, `0700`/`0600`, no temp fallback | Platform permission tests |
| O-03 | Descriptor symlink or replacement race | Reject symlinks/reparse points; exclusive create; atomic rename; owner validation | Adversarial filesystem tests |
| O-04 | Stale descriptor reaches a new process after PID reuse | Match `instance_id`, random `process_nonce`, process start time, and authenticated handshake | PID-reuse and stale-file tests |
| O-05 | Token appears in logs, URLs, crash text, or client config | Token only in private descriptor and handshake field; redacting logger; never command-line or URL | Secret canary scan over logs and errors |
| O-06 | Client requests or later assumes a stronger profile | App computes an immutable connection grant below the configured ceiling; elevation requires reconnect; token carries no authority; unknown permission fails closed | Handshake and mid-connection escalation tests |
| O-07 | `quantick_invoke` bypasses tool annotations | Authorization is enforced in the registry executor after lookup, not in the MCP wrapper | Invoke-vs-named-tool parity tests |
| O-08 | Observer creates a mark or other state | Remote effect classification; observer grants only `observe`; mark creation is `annotate` | State digest unchanged after every observer tool test |
| O-09 | Prompt injection arrives through feed, logs, notes, or symbols | Treat external strings as typed data; fixed server instructions; no command interpolation | Fixtures containing instruction-like strings remain payload data |
| O-10 | Oversized or malformed frames exhaust memory | Length prefix checked before allocation; hard byte, depth, item, and string bounds; truncated-frame timeout | Fuzz and boundary tests |
| O-11 | Long polls or slow clients starve the UI | Park waiters off the UI queue; bounded waiter count; write timeouts; no UI-thread socket work | Concurrent wait/read and slow-reader tests |
| O-12 | Request flood consumes frames | Per-client rate and burst limits; bounded connections and queue; 1 ms opening frame budget | Flood test plus frame-budget instrumentation |
| O-13 | Snapshot combines incompatible moments | One UI-thread capture revision; owned DTO; module revisions; explicit gaps | Mutation-between-modules test |
| O-14 | Client acts on the wrong instance | Deterministic list; explicit selection on ambiguity; instance ID on every envelope | Two-instance test |
| O-15 | Adapter launches a hidden app | No launch code or process capability; empty discovery returns a next step | Process-spawn guard and zero-instance integration test |
| O-16 | Arbitrary file or shell access is smuggled through a generic tool | No generic path, command, eval, or shell capability; schema rejects extra fields | Capability inventory and schema tests |
| O-17 | Evidence accumulates or launders sensitive data | Separate evidence and source scopes; count, byte, and time eviction; grant recheck on every chunk; clear on disable/exit; no default disk export | Cross-scope access, retention, revocation, and shutdown tests |
| O-18 | Screenshot captures unrelated windows or secrets | Capture only Quantick-owned surface; explicit scope and request; visible indicator; revision correlation | Window ownership and consent tests |
| O-19 | Diagnostics reveal secrets or private paths | Structured allowlist, mandatory redaction, opaque diagnostic IDs | Redaction fixtures with canary secrets and home paths |
| O-20 | Protocol downgrade changes authorization meaning | Highest overlapping version; no overlap fails; effect and permission IDs validated | Version negotiation tests |
| O-21 | Client keeps reading after revocation | Connection generation bound to token; close sockets; cancel waiters; clear resources | Mid-request and parked-wait revocation tests |
| O-22 | Market-data volume or licensing is exceeded | Visible-window default, pagination, rate limits, no automatic full-history dump | Page-limit and rate tests |
| O-23 | Client spoofs a human or another agent in audit records | Gateway constructs actor context after authentication; client controls only self-declared name and optional reason | Actor-field injection and cross-connection attribution tests |
| O-24 | Observer telemetry or audit traffic evicts user marks and domain events | Separate bounded control audit storage; only semantic transitions enter the event journal; repetitive operational events are coalesced | Flood audit and reconnect telemetry, then prove a retained mark remains readable |
| O-25 | Attention, selection, scene, or evidence references bypass a denied module scope | Intersect the reference scope with every target scope; return only a redacted presence marker when any required scope is absent | Cross-scope cursor, selection, scene, mark, and evidence fixtures |

## 9. Security invariants

The following properties are release blockers:

1. The user and domain state digest is unchanged after any sequence of observer
   calls. Observer calls may change only isolated control-plane operational
   state: bounded caches, capture counters, connection telemetry, request
   metrics, control audit records, client read cursors, and evidence resources.
2. Those operational artifacts cannot alter rendering, market data, replay,
   paper trading, persistence, later action availability, or the retention
   budget of the semantic event journal.
3. A token from one instance or token generation cannot authenticate to
   another.
4. Disabling access makes every existing and future request fail without an
   application restart.
5. Unknown scopes, effects, capability versions, and protocol versions fail
   closed.
6. No observer response contains a seeded canary secret or full seeded home
   path in redaction tests.
7. The gateway adds no per-trade or per-depth allocation, lock, or branch when
   no semantic event is emitted.
8. Every remotely initiated audit record uses gateway-assigned principal and
   connection IDs; untrusted actor fields never reach the registry executor.

## 10. Abuse and incident response

The app records local structured events for enable, disable, connection,
authentication failure, profile denial, rate limiting, revocation, evidence
capture, and unexpected gateway shutdown. These events contain no token or raw
payload.

The access panel offers one action that disables the gateway and revokes every
client immediately. Closing Quantick also revokes access. A suspected token
leak is handled by disable and re-enable, which creates a new token and
descriptor.

## 11. Deferred threat-model extensions

Before the corresponding tier ships, this document must be extended for:

- annotate actions and replay control traces;
- cockpit state loss, filesystem import/export, and confirmation;
- paper orders, strategies, idempotency, and risk-reducing exceptions;
- any Streamable HTTP or remote transport;
- broker credentials, live orders, account data, and emergency controls.

## References

- [Control contract](control-contract.md)
- [ADR 0001](adr-0001-local-transport-and-instance-discovery.md)
- [Development plan](../mcp-control-plane-development-plan.md)
