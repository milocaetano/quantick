# Quantick control contract

**Status:** Accepted for implementation

**Date:** 2026-08-19

This document fixes the contract decisions required by PR 0. It applies to
`quantick-control`, the in-app gateway, MCP, and every later adapter.

## 1. Scope

The control contract describes a running Quantick instance without exposing
`QuantickApp`, egui types, or domain internals. It defines:

- stable identifiers and schemas;
- request, response, error, revision, and actor semantics;
- effect tiers and profile mapping;
- the fixed MCP surface and the dynamic capability registry;
- protocol, queue, pagination, timeout, and retention limits;
- replay determinism for agent actions;
- ownership of durable trade annotations;
- local setup flows for Codex and Claude Code.

MCP is one adapter. No rule in this document may require domain code to know
that a request came from MCP.

## 2. Identifiers and versions

### 2.1 Identifier shape

Capability IDs, module IDs, snapshot scope IDs, event kinds, permission IDs,
effect IDs, profile IDs, risk-flag IDs, and error codes are validated strings
stored in newtypes. They are not closed Rust enums.

Every ID must match:

```text
^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$
```

Capability IDs, snapshot scope IDs, event kinds, and error codes are
namespaced and therefore must contain at least one dot. Module, effect,
profile, permission, and risk-flag IDs may be one segment or namespaced. This
allows the initial `observe` effect and a future extension such as
`filesystem.write` without converting extensible IDs into closed enums.
Every ID also fits `CONTROL_ID_MAX_BYTES` and
`CONTROL_ID_MAX_SEGMENTS`; validation occurs before registry lookup.

Runtime identity values are a different type from registry IDs. Server-created
instance, process nonce, connection, principal, evidence, and resource IDs use
128 random bits encoded as unpadded base64url. A client-created `request_id` is
1 through `CONTROL_REQUEST_ID_MAX_BYTES` printable ASCII bytes and is opaque to
the server. None of these values is accepted where a registry ID is required.

The first segment is the owning module. Later segments move from the resource
to the operation or event:

```text
chart.snapshot
chart.window.read
chart.viewport.pan
attention.cursor
attention.mark.create
indicator.script.attach
paper.order.place
control.auth_failed
```

An ID is permanent after release. A display label may change; an ID may not.
Aliases are explicit registry entries with a deprecation marker and removal
version, never silent rewrites.

### 2.2 Versions

Each capability has a positive `u32` version. The protocol has a supported
inclusive version range. A handshake selects the highest mutually supported
protocol version; failure to overlap returns `control.version_unsupported`.

Capability and protocol versions are independent. Adding a capability does not
bump the protocol. A breaking change to one capability bumps only that
capability unless the envelope itself changed.

## 3. Envelopes and wire values

Every external request envelope carries:

```text
protocol_version
request_id
instance_id
capability_id
capability_version
expected_revisions?
idempotency_key?
dry_run?
reason?
payload
```

After authentication and authorization, the gateway creates an internal
authorized request by attaching the trusted actor context from section 6. That
internal type, rather than the external DTO, is accepted by the registry
executor.

Every capability response carries:

```text
protocol_version
request_id
instance_id
capture_revision?
module_revisions
result | error
warnings
```

`capture_revision` is required for a coherent captured read and absent when no
capture occurred. An action response includes the module revisions after the
attempt. Handshake frames use their own schema and do not invent application
revisions.

Wire DTOs follow these rules:

- Decimal values are canonical decimal strings matching
  `^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$`; negative zero, leading plus signs,
  exponents, leading integer zeros, and trailing fractional zeros are invalid.
  A meaningful display scale travels in a separate field.
- JSON numbers are integers. Non-integral values use canonical decimal strings.
- Revisions, event sequences, and other full-range `u64` ordering tokens are
  unsigned decimal strings on the wire, even though their Rust representation
  is `u64`.
- Absolute wall-clock fields include epoch and unit, normally `_at_unix_ms` or
  `_time_unix_ms`; monotonic durations include `_elapsed_ms` or `_elapsed_us`.
- Byte counts include `_bytes`; capacities include `_items` or `_entries`.
- Optional, unavailable, inferred, and incomplete values remain distinct.
- Collections whose order can reach a client have deterministic ordering.
- Unknown additive fields are ignored by tolerant readers.
- Private rendering types and filesystem paths never cross the wire by
  accident.

Canonical input, query, result, audit, and manifest digests use Quantick
Canonical JSON version 1. It recursively sorts object keys by UTF-8 byte order,
preserves array order, emits integers in minimal base-10 form, emits only
required JSON string escapes, preserves exact UTF-8 without Unicode
normalization, and forbids floating-point JSON numbers. The digest form is
`sha256:<64 lowercase hexadecimal characters>`. Binary resource chunks are
hashed as raw bytes; their ordered hashes are included in the canonical
manifest. Golden cross-language vectors are part of the committed schemas, and
changing these rules is a protocol break.

## 4. Schema compatibility

Every public input, result, event, error detail, and resource manifest has a
JSON Schema committed under `schemas/control/` when its implementation lands.
Tests generate the schema and compare it with the committed file.

The compatibility rule is:

- Adding an optional field, capability, scope, or event kind is additive.
- Removing or renaming a field, making an optional field required, changing a
  unit, narrowing a valid range, or changing the meaning of a value is
  breaking.
- A breaking capability change increments that capability's version and keeps
  the previous version available for at least one minor release.
- A protocol-envelope breaking change increments the protocol version.

Examples included in capability descriptors are validated against the same
schemas. Duplicate IDs, invalid descriptors, invalid examples, and undeclared
permissions fail tests.

## 5. Revisions and concurrency

Each running instance owns a fresh random `instance_id`. Revisions are
monotonic `u64` counters scoped to that instance:

- each observable module has a revision;
- a snapshot has one `capture_revision` assigned during its UI-thread capture;
- the snapshot includes every module revision observed in that capture;
- an event records the revision after the change it describes.

Revisions are ordering tokens, not time. They reset when the application
restarts and are meaningful only with their `instance_id`.

A capability that can overwrite, remove, reorder, or financially affect state
requires the relevant `expected_revisions`. A mismatch returns
`control.revision_conflict` with the current revisions and changes nothing.
Additive actions may omit an expected revision only when the descriptor states
why stale input cannot damage existing state.

Captures occur in one bounded UI-thread pass. Serialization and compression
run after that pass on owned DTOs. A response must never mix module values from
different UI-thread captures while claiming one capture revision.

### 5.1 Event cursors

An event cursor is a wire object, not an index into a current allocation:

```text
instance_id
next_sequence
```

`next_sequence` is the next monotonic `u64` journal sequence the client expects
to read. A first read omits the cursor and explicitly selects `oldest` or
`latest`; there is no implicit start position. A response returns events in
sequence order and a `next_cursor` one past the last returned event, or the
input or resolved starting cursor when the page is empty.

If retention has advanced past the requested sequence, the read begins at the
oldest retained event and returns that position in `dropped_before`. A cursor
from another instance, a sequence ahead of the journal tail, or an invalid
start/cursor combination returns `control.cursor_invalid`. Cursors contain no
pointer, token, secret, timestamp, or client-owned mutable state.

### 5.2 Retries and dry runs

`request_id` correlates one attempt and is never a retry key. A capability
descriptor declares idempotency as `forbidden`, `optional`, or `required`, and
separately declares whether it supports `dry_run`. Financial actions and any
other non-idempotent action that is safe to retry require an idempotency key.
A connection rejects duplicate request IDs while the first request is in
flight.

The gateway scopes an idempotency record by `instance_id`, trusted
`principal_id`, capability ID and version, and the opaque key. It records the
canonical digest of `expected_revisions` and `payload` before the handler can
mutate state. Request ID, key, reason, and actor metadata are excluded; the
trusted principal is already part of the store key. A retry with the same
digest returns the original terminal result. Reusing the key with a different
digest returns `control.idempotency_conflict`; retrying while the first attempt
is still executing returns retryable `control.request_in_progress`. Raw keys
are never logged.

Records remain for the configured retention period or the life of the instance,
whichever ends first. The store does not evict an unexpired record to accept a
new request whose descriptor requires idempotency; it returns backpressure at
capacity. Such capabilities return compact results that fit the record limit.
They may reference a durable resource only when its lifetime is at least the
idempotency retention; a temporary in-memory resource is invalid. A dry run
never mutates state or reserves an idempotency key, and an unsupported dry run
fails before dispatch.

### 5.3 Pagination cursors

A page cursor is server-produced structured data:

```text
instance_id
scope_id
query_digest
consistency_mode
consistency_revision
high_water_position?
next_position
```

The first request supplies the query and no cursor. The capability descriptor
declares one pagination consistency mode:

- `revision_locked`: any relevant module revision change makes the next page
  stale;
- `append_only`: the cursor fixes a high-water position and content-generation
  revision; appends after that position are ignored, while a correction or
  backfill at or before it makes the page stale;
- `retained_resource`: every page reads one immutable retained resource and
  returns `control.resource_gone` when that resource expires.

Every later page must use the same scope, canonical query digest, and
consistency values. A violation returns `control.page_stale` rather than
combining incompatible states. Live chart history uses `append_only`; its
in-progress bar is returned by the coherent snapshot, not mixed into a
multi-page closed-bar series. `next_position` and `high_water_position` are
bounded scope-specific values such as a slot, timestamp, or byte offset; they
are never pointers, paths, commands, or credentials. A cursor from another
instance or a cursor/query mismatch returns `control.cursor_invalid`. Every
page reports `has_more`, its item count, and an optional next cursor.

## 6. Actors and audit records

Every action, including a human action routed through the registry, carries:

```text
actor_kind: human_ui | automation | agent
principal_id
client_name
connection_id
request_id
reason
requested_at_unix_ms
```

`principal_id`, `connection_id`, and `request_id` are opaque IDs. The gateway
constructs the actor context after authentication and rejects reserved actor
fields in the external envelope. Other additive fields follow the compatibility
rule in section 3. A human UI action obtains the same context from an in-process
dispatcher. The client cannot select `actor_kind`, `principal_id`, or
`connection_id`.

The gateway derives a local principal from the authenticated instance-token
generation and assigns a connection ID. `client_name` comes from the handshake
and is self-declared metadata, displayed as such and never treated as a
cryptographically verified identity. The dispatcher stamps
`requested_at_unix_ms`; a client-supplied clock is never used for audit order.

An audit record includes a monotonic instance-scoped `audit_sequence`, the
actor, capability ID and version, canonical input digest, revisions, result
code, and `completed_at_unix_ms`. The sequence is authoritative for ordering;
wall-clock time is informational. Authorship also lives on the resulting
drawing, annotation, preset, script attachment, strategy, or order whenever
that object is durable. A log entry alone is insufficient.

Control audit records and request metrics use storage separate from the
semantic `EventJournal`. Authentication failures, evidence captures, rate
limits, and ordinary read telemetry cannot evict a user mark or domain event.
Only a state transition that a semantic client may need, such as a connection
becoming unavailable, enters the journal, and repetitive operational changes
are coalesced.

## 7. Effects, permissions, and profiles

The effect attached to a capability describes what a remote invocation can do,
not whether the UI happened to mutate internal bookkeeping while reporting a
human action.

| Effect ID | Remote effect | Minimum permission |
| --- | --- | --- |
| `observe` | Reads or derives data without changing user-visible state | `observe` |
| `annotate` | Adds reversible state or emits a bounded notification; cannot remove existing work or affect a position | `annotate` |
| `cockpit` | Can replace, hide, move, or discard work in the session | `cockpit` |
| `paper` | Can change simulated orders, positions, or strategy state | `paper` |
| `live` | Can affect a broker, venue, account, or real-money risk | `live` |

Effects are registry strings so future modules can add a more specific effect
without changing `quantick-control`. The host owns an `EffectPolicyRegistry`.
An effect policy declares its permission floor, profile ceilings, confirmation
class, MCP hint floor, and required risk flags. A module must register that
policy before registering a capability that uses the effect. Duplicate or
missing policies fail startup, so an unknown effect still fails closed without
making `quantick-control` the extension gatekeeper. The app bootstrap registers
the five initial policies above.

Risk flags are separate from effects. Initial flags include
`sensitive_data`, `filesystem_read`, `filesystem_write`, `state_loss`,
`user_interrupt`, `audible_output`, `simulated_financial`, and
`live_financial`.

Registry validation rejects contradictory metadata. `observe` must be
read-only and non-destructive. `annotate` must be non-destructive; a durable
annotate result must be reversible, while an irreversible transient effect must
declare `user_interrupt` and its stricter rate policy. Audible output also
declares `audible_output`. Cockpit state loss, simulated finance, and live
finance require their matching risk flags. These checks apply to extension
capabilities as well as the initial registry.

Profiles grant permissions:

| Profile | Permissions |
| --- | --- |
| `observer` | `observe` plus the user-granted `observe.*` scopes; no write permission |
| `annotator` | Observer permissions plus `annotate` |
| `developer` | Annotator permissions plus `cockpit` |
| `paper` | Developer permissions plus `paper` |

There is no live-trading profile in the planned implementation.

Profiles are app-registered policy presets, not a closed enum in the contract
crate. Each descriptor expands to an explicit permission ceiling; inheritance
is flattened and cycle-checked at startup. Unknown profiles fail the handshake.
Adding a live profile still requires the separate threat model and policy even
though the ID type itself is extensible.

The profile is an authority ceiling. A capability also requires its declared
scope, such as `observe.chart` or `observe.user_text`. Selecting `observer`
does not silently grant every sensitive read scope.

The host also owns a `PermissionRegistry`. Each permission descriptor supplies
an ID, label, explanation of data or authority granted, sensitivity, default
grant policy, and the profile ceilings under which it may appear. Snapshot and
action modules register their permissions beside their scopes or capabilities.
A permission ceiling contributes that permission to the named profile and its
descendants during finalization; it does not bypass the separate current user
grant applied by the handshake. This two-phase registration lets a module add a
scope without editing a central profile descriptor.
A capability that names an undeclared permission fails registration; a client
that requests an undeclared permission fails closed. The in-app consent UI is
generated from these descriptors rather than a hand-maintained MCP-only list.

Write profiles are also ceilings, not blanket consent. The annotate tier opens
with independent `annotate.attention`, `annotate.chart`,
`annotate.notification`, `annotate.sound`, and `annotate.script` scopes.
`annotate.sound` is off by default and is required in addition to
`annotate.notification` for audible output. Later threat-model extensions must
define cockpit and paper subscopes before those profiles ship.

A `risk_reducing` descriptor flag may select a lighter confirmation policy, but
does not lower the capability's effect. A paper or live safety action requires
an explicit `paper.safety` or future `live.safety` grant. Observer, annotator,
and developer profiles never receive financial authority through this flag.

### 7.1 Marks and the observer boundary

Cursor position, selection, and marks created by the human in the Quantick UI
are observable state. An observer may read them and wait for their events.

Creating a mark through a remote call changes session state. The capability is
`attention.mark.create`, has the `annotate` effect, records its actor, and is
unavailable to the `observer` profile. The UI hotkey, a deterministic test, and
an authorized agent call the same handler. This distinction prevents a write
from being mislabeled as observation.

## 8. Capability registry and MCP tools

The application registry is dynamic. MCP tools are deliberately small and
stable.

The first named read tools are:

```text
quantick_describe
quantick_get_snapshot
quantick_get_chart_window
quantick_get_scene
quantick_read_events
quantick_wait_for_change
quantick_get_diagnostics
quantick_capture_evidence
quantick_search_capabilities
```

The long tail uses:

```text
quantick_invoke
```

`quantick_invoke` accepts a capability ID, version, and declared input. It does
not bypass availability, permission, confirmation, revision, idempotency, or
audit checks. A capability unavailable to the current profile remains
unavailable through `invoke`.

High-frequency workflows may earn a named tool after usage evidence. The first
planned write tools are `quantick_annotate`, `quantick_notify`, and
`quantick_attach_script`; each still resolves to the same registry entry that
`invoke` would use.

MCP tool annotations are a client hint, not an authorization boundary. Named
observer tools set `readOnlyHint: true`, `destructiveHint: false`, and
`openWorldHint: false`. The configured profile is a ceiling known when the MCP
server starts, so `quantick_invoke` uses conservative hints for that ceiling:

| Profile ceiling | `readOnlyHint` | `destructiveHint` | `idempotentHint` | `openWorldHint` |
| --- | --- | --- | --- | --- |
| `observer` | `true` | `false` | `false` | `false` |
| `annotator` | `false` | `false` | `false` | `false` |
| `developer` or `paper` | `false` | `true` | `false` | `true` |

The effective profile and scopes never exceed this ceiling. Increasing a grant
requires a new authenticated connection before any tool hint can loosen;
reducing or revoking a grant takes effect immediately. A named write tool
derives its hints from the registry descriptor and fails startup if the fixed
tool annotation contradicts that descriptor. The registry executor enforces
the real effect, permission, confirmation, revision, and idempotency policy
after lookup.

Clients may cache the MCP tool list. They must not cache dynamic availability.
`quantick_describe` and `quantick_search_capabilities` report availability and
the blocking reason from the current application state.

Every instance-bound MCP tool accepts an optional routing `instance_id` that
the adapter removes before validating the capability payload. With exactly one
live instance, omission selects it. With zero instances, the call returns
`control.instance_gone`; with more than one, omission returns
`control.instance_ambiguous` and the deterministically ordered choices. There
is no hidden mutable "current instance." The adapter may also be launched with
`--instance <id>` to pin every call and fail if that instance disappears.
`quantick_describe` is the exception: without an ID it lists live instances;
with an ID it describes that instance and its current grant and capabilities.

For STDIO, `quantick-mcp` reserves standard output exclusively for MCP protocol
frames. Redacted diagnostics go to standard error. A panic, tracing subscriber,
or dependency must never print non-protocol text to standard output.

## 9. Availability and errors

Every descriptor reports one of:

```text
available
unavailable
confirmation_required
permission_denied
```

An unavailable capability includes a stable reason code and a human-readable
next step. Calling it returns the same reason instead of a generic failure.

Errors use the shape defined in the development plan and stable dotted codes:

```text
code
message
retryable
current_revisions?
violated_precondition?
details?
next_steps?
diagnostic_id?
```

`code`, `retryable`, revisions, precondition IDs, and typed details are
machine-readable. The optional fields are bounded, redacted, and absent when
they do not apply. Message and next-step text may improve without a protocol
version change; clients must branch on `code`.
The initial cross-module codes are:

```text
control.auth_failed
control.backpressure
control.capability_unknown
control.capability_unavailable
control.cursor_invalid
control.idempotency_conflict
control.instance_ambiguous
control.instance_gone
control.invalid_request
control.page_stale
control.payload_too_large
control.permission_denied
control.revision_conflict
control.request_in_progress
control.resource_gone
control.scope_denied
control.timeout
control.version_unsupported
```

Internal error strings, panic payloads, tokens, paths, and credentials are not
returned. An opaque diagnostic ID correlates a safe client error with local
structured logs.

## 10. Limits and retention

These names become constants in `quantick-control::limits` and defaults in the
app configuration. Values are initial safety and memory bounds, not measured
performance claims. PR 2 and PR 3 may lower defaults after measurement. Raising
a hard limit requires a reviewed contract change and threat-model check.

| Name | Initial value | Kind | Purpose |
| --- | ---: | --- | --- |
| `CONTROL_TOKEN_BYTES` | 32 | hard | 256-bit instance bearer token |
| `CONTROL_RUNTIME_ID_BYTES` | 16 | hard | 128-bit generated runtime identifiers |
| `CONTROL_REQUEST_ID_MAX_BYTES` | 128 | hard | Bound client correlation identifiers |
| `CONTROL_DESCRIPTOR_MAX_BYTES` | 16 KiB | hard | Bound descriptor reads before JSON parsing |
| `CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES` | 16 KiB | hard | Bound one registry descriptor, including schemas and examples |
| `CONTROL_PROTOCOL_MAX_FRAME_BYTES` | 16 MiB | hard | Reject a frame before allocation or JSON parsing |
| `CONTROL_MAX_REQUEST_BYTES` | 1 MiB | hard | Bound schemas, scripts, and action inputs |
| `CONTROL_MAX_RESPONSE_BYTES` | 8 MiB | hard | Force pagination or resource chunking |
| `CONTROL_MAX_BUFFERED_RESPONSE_BYTES` | 64 MiB | hard | Bound responses waiting across all connections |
| `CONTROL_MAX_JSON_DEPTH` | 64 | hard | Reject adversarial nesting |
| `CONTROL_MAX_STRING_BYTES` | 256 KiB | hard | Bound any one wire string, including script source |
| `CONTROL_ID_MAX_BYTES` | 128 | hard | Bound any registry or protocol identifier |
| `CONTROL_ID_MAX_SEGMENTS` | 8 | hard | Bound dotted-identifier parsing and display |
| `CONTROL_CLIENT_NAME_MAX_BYTES` | 128 | hard | Bound self-declared handshake metadata |
| `CONTROL_REASON_MAX_BYTES` | 1 KiB | hard | Bound optional action rationale and audit data |
| `CONTROL_IDEMPOTENCY_KEY_MAX_BYTES` | 128 | hard | Bound an opaque client retry key |
| `CONTROL_IDEMPOTENCY_MAX_ENTRIES` | 1,024 | default | Bound retained retry records per instance |
| `CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES` | 64 KiB | hard | Keep cached action results compact |
| `CONTROL_IDEMPOTENCY_RETENTION_MS` | 86,400,000 | default | Retain retry records for 24 hours or the instance lifetime |
| `CONTROL_DEFAULT_PAGE_ITEMS` | 256 | default | Keep ordinary reads compact |
| `CONTROL_MAX_PAGE_ITEMS` | 2,048 | hard | Bound chart and event pages |
| `CONTROL_REQUEST_QUEUE_CAPACITY` | 64 | default | Bound pending UI-thread work |
| `CONTROL_MAX_CONNECTIONS` | 8 | default | Bound local clients per instance |
| `CONTROL_MAX_IN_FLIGHT_PER_CONNECTION` | 8 | hard | Prevent one client from owning the queue |
| `CONTROL_MAX_PARKED_WAITERS` | 16 | hard | Bound `wait_for_change` registrations |
| `CONTROL_HANDSHAKE_TIMEOUT_MS` | 2,000 | default | Drop unauthenticated sockets quickly |
| `CONTROL_REQUEST_TIMEOUT_MS` | 5,000 | default | Bound ordinary calls |
| `CONTROL_WAIT_TIMEOUT_MAX_MS` | 30,000 | hard | Bound one long poll |
| `CONTROL_CLIENT_RATE_PER_SECOND` | 20 | default | Sustained per-client rate |
| `CONTROL_CLIENT_BURST` | 40 | default | Short per-client burst |
| `CONTROL_NOTIFICATION_RATE_PER_MINUTE` | 6 | default | Bound visible or audible interruptions per client |
| `CONTROL_NOTIFICATION_BURST` | 2 | hard | Prevent notification floods within one moment |
| `CONTROL_UI_BUDGET_US` | 250 | default | Maximum control work in one frame; calibrated in PR 2 against a 28 us core-capture p99 |
| `CONTROL_EVENT_JOURNAL_CAPACITY` | 8,192 | default | Bound semantic event memory |
| `CONTROL_EVENT_MAX_BYTES` | 64 KiB | hard | Force large event data into a resource |
| `CONTROL_EVENT_JOURNAL_MAX_BYTES` | 32 MiB | hard | Bound the journal even when events vary in size |
| `CONTROL_AUDIT_MAX_ENTRIES` | 4,096 | default | Bound the separate in-memory control audit view |
| `CONTROL_AUDIT_RECORD_MAX_BYTES` | 16 KiB | hard | Keep one structured audit record compact |
| `CONTROL_AUDIT_MAX_BYTES` | 16 MiB | hard | Bound the audit view independently of entry count |
| `CONTROL_AUDIT_RETENTION_MS` | 86,400,000 | default | Retain control audit records for at most 24 hours |
| `CONTROL_EVIDENCE_MAX_BUNDLES` | 8 | default | Bound retained captures |
| `CONTROL_EVIDENCE_MAX_TOTAL_BYTES` | 64 MiB | hard | Bound total in-memory evidence |
| `CONTROL_EVIDENCE_RETENTION_MS` | 900,000 | default | Retain evidence for 15 minutes |

Evidence payloads are resources read in chunks no larger than
`CONTROL_MAX_RESPONSE_BYTES`; `quantick_capture_evidence` returns a manifest
and resource ID, not an unbounded inline bundle. Old evidence is evicted by the
earlier of count, total bytes, or retention time.

The event journal evicts by the earlier of entry capacity or total encoded
bytes. A semantic event larger than `CONTROL_EVENT_MAX_BYTES` contains a
bounded summary and resource ID instead of inline data. The gateway applies
backpressure before queued and buffered responses exceed their global byte
budget; a slow client cannot reserve one maximum response per allowed request.
The separate audit view evicts by the earliest of entry count, encoded bytes,
or retention time.

`wait_for_change` parks outside the UI request queue and does not count as an
in-flight UI request while it waits. When its cursor advances, only the bounded
read that completes the call enters the queue.

The executor checks `CONTROL_UI_BUDGET_US` between requests and independently
bounded capture scopes. It never suspends one coherent capture across frames.
Page and scope limits must keep each indivisible capture below the budget; an
unexpected overrun is telemetry and a performance-test failure, not permission
to return a mixed-revision response.

PR 2 calibrated the default against the shared-host method and core-scope
capture benchmark in [PR 2 observer performance evidence](pr2-performance.md).

## 11. Replay determinism decision

**Decision:** Any accepted non-observe registry action during replay that can
affect session output is a replay input, regardless of whether its actor is a
human, automation, or agent. It must be recorded in a durable, versioned
control trace. A replay with an unrecorded action is not eligible as a
deterministic fixture.

The market recording remains immutable. The control trace is a sidecar ordered
by replay time and sequence number. Each entry stores:

```text
trace_version
replay_elapsed_us
sequence
actor
capability_id
capability_version
canonical_input
expected_revisions
result_code
result_digest
```

Replaying the pair injects each accepted control action at the same logical
replay position. Wall-clock time is never used. The dispatcher durably appends
an intent before state changes and records its terminal result afterwards. An
incomplete trace is not fixture-eligible. If a determinism-affecting action
cannot append its intent, the action fails before state changes. Observe calls
and reads of human attention do not enter the trace.

The trace is required for annotate, cockpit, and paper actions that affect
plots, layout, replay position, strategy state, or simulated trading. The PR
that introduces the first such action must implement the trace port or keep the
action unavailable during replay.

## 12. Trade annotation ownership

**Decision:** Durable notes about closed trades belong to an app-owned
`trade_annotations` module, not to `quantick-sim` and not to MCP.

The simulator remains a pure domain crate. The app stores annotations beside
the selected paper journal so the history and its notes move together. A
versioned sidecar records:

- a stable `TradeReference` derived from the journal identity, canonical trade
  fields, and duplicate occurrence index;
- the note ID and exact UTF-8 text;
- actor and timestamps;
- optional source mark and evidence IDs;
- edit revision and deletion state.

The reference supports version-1 history rows whose aggregate IDs are unknown.
No note field is added to `ClosedTrade`. Import and export either carry the
annotation sidecar or state explicitly that annotations were not included.

## 13. Codex and Claude Code setup flows

The `quantick-mcp` binary is a local STDIO adapter. It discovers already
running Quantick instances and never launches the application. Machine-specific
binary paths and instance tokens are not committed to the repository.

### 13.1 Codex

1. Build or install `quantick-mcp`.
2. Start Quantick manually and enable local observer access for this run.
3. Register the adapter:

   ```powershell
   codex mcp add quantick -- "<absolute-path-to-quantick-mcp>" --profile observer
   ```

4. Verify configuration with `codex mcp list`.
5. Start or restart the Codex client and use `/mcp` to verify the connection.
6. Call `quantick_describe`. If more than one instance is running, select one
   explicitly before reading it.

Codex CLI, the Codex IDE extension, and the ChatGPT desktop app share the local
Codex MCP configuration. A future setup helper may emit an equivalent
`[mcp_servers.quantick]` table, but the CLI is the primary documented path.

### 13.2 Claude Code

1. Build or install `quantick-mcp`.
2. Start Quantick manually and enable local observer access for this run.
3. Register the adapter in local scope:

   ```powershell
   claude mcp add --transport stdio --scope local quantick -- "<absolute-path-to-quantick-mcp>" --profile observer
   ```

4. Verify it with `claude mcp get quantick` or `claude mcp list`.
5. In Claude Code, use `/mcp` to inspect status and approve a project-scoped
   configuration if that scope is used later.
6. Call `quantick_describe` and select an instance explicitly when ambiguous.

Open-source examples may include a `.mcp.json` template only after the binary
has a portable installation command. It must contain no token, username, home
path, or automatic application launch.

## 14. Consequences for PR 1

PR 1 must prove these decisions with tests:

- string newtypes accept valid extension IDs and reject invalid IDs;
- a fake second module registers without editing the contract crate;
- that fake module can register an effect policy, while a capability whose
  effect has no policy fails closed;
- version negotiation succeeds only on an overlapping range;
- an accepted handshake downscopes to the app grant, reports the selected
  limits, and never echoes its bearer token;
- descriptors, examples, and schemas validate;
- canonical JSON and SHA-256 golden vectors produce identical digests for
  reordered objects and distinct digests for distinct arrays or UTF-8 strings;
- breaking schema fixtures require a version bump;
- permissions fail closed for unknown effects;
- contradictory effect, read-only, reversibility, destructive, and risk fields
  fail descriptor validation;
- oversized frames are rejected before payload allocation;
- revision conflicts change no state in the fake host;
- a second page cannot change its query; revision-locked pages reject a change,
  while append-only pages accept a later append but reject a correction below
  their high-water position;
- retrying an idempotent fake action returns its first result, while reusing
  the key with a different payload changes no state and returns a conflict;
- the fake host and client use the same envelopes as later adapters.

PR 1 does not implement sockets, MCP, app snapshots, persistence, or trading.

## 15. Deferred decisions

The following decisions need implementation evidence and are intentionally not
fixed by PR 0:

- the Rust MCP SDK, if any;
- optional Streamable HTTP support;
- named pipes or Unix domain sockets as an additional gateway transport;
- compression format for evidence resources;
- remote authentication and every live-trading policy.

## References

- [Development plan](../mcp-control-plane-development-plan.md)
- [Capability inventory](capability-inventory.md)
- [ADR 0001](adr-0001-local-transport-and-instance-discovery.md)
- [Observer threat model](observer-threat-model.md)
- [OpenAI MCP documentation](https://developers.openai.com/codex/mcp)
- [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp)
