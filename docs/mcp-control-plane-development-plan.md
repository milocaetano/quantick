# Quantick control plane and MCP development plan

**Status:** Accepted for phased implementation; PR 0 through PR 5b merged to
`main`. Progress and remaining work are tracked in
[control-plane/roadmap.md](control-plane/roadmap.md).

**Date:** 2026-08-19

**History:** Codex produced the first draft. Claude Code reviewed and expanded
it on 2026-08-19 at the repository owner's request. The PR 0 contract review
then reconciled the plan with the measured source inventory, authority model,
transport ADR, and observer threat model.

**Why it was revised:** The vendor-neutral control-plane spine was retained, but
the first draft did not yet satisfy the workflow it was written for. The owner
wants to point at something on a running chart and be understood, and to have
the assistant answer on the chart rather than only in prose. A read-only
snapshot API cannot do both. Section 0 summarizes the changes and section 3.1
records the source measurements behind them.

**Primary goal:** Let Codex, Claude, and other MCP-compatible clients observe a
running Quantick instance, understand what the user is pointing at inside it,
answer on the chart as well as in prose, and collect reproducible evidence
without relying on screenshots.

**Repository language:** English. Code, schemas, documentation, examples,
public messages, and contributor-facing artifacts must be written in English.

## 0. What this revision changed

The retained architecture is a vendor-neutral control contract, MCP as one
adapter, a single registry shared with the interface, and authority that starts
small. The review expanded its scope, delivery order, and explicit contracts.

Three gaps against the stated objective:

- **Nothing published where the user is looking.** The draft gave pull
  snapshots and a scene tree of controls, but no cursor, selection, or way to
  mark a target, so an agent could read the whole chart and still not know which
  bar the user meant. Section 6.5 adds a resolved cursor, a published selection,
  and human-created marks inside the observe tier. Creating a mark remotely is
  an annotate action.
- **Every write deferred, including the harmless ones.** The draft had one write
  cliff. Section 2.6 replaces it with tiers by effect, and PR 5b ships the tier
  that cannot lose the user's work inside the MVP, so the agent can answer on
  the chart instead of only reading it.
- **Live indicator authoring unnamed.** Compiling a script from prose, reading
  structured diagnostics, and attaching it is among the highest-value
  capabilities on the objective list, and the draft mentioned indicators only as
  settings actions. PR 5b makes it explicit, using compile diagnostics that
  already exist.

Contract corrections are stated where they belong: extensible validated IDs
(5.5); snapshot-tested schema compatibility (5.6); gateway-assigned actor
identity (5.2); explicit cursor, retry, and `dry_run` semantics (PR 0); MCP tool
annotations that fail conservatively for generic invocation (7.1); byte and
time budgets instead of request-count guesses (10.2); and a durable replay trace
for determinism-affecting actions (PR 0). `wait_for_change` parks off the UI
thread (6.4), screenshots correlate with semantic captures (section 8), and the
adapter never starts the application (9.1).

Section 3.1 records the measurements these changes rest on, so the next reviewer
can check them rather than trust them.

## 1. Intended outcome

Quantick will expose its own versioned, vendor-neutral Control API. MCP will be
the first adapter for that API, not the place where platform behavior lives.

At the end of the MVP, the workflow will be:

1. The user starts Quantick normally.
2. The user explicitly enables local access with the `observer` profile.
3. Codex or Claude connects through the Quantick MCP server.
4. The user asks a question such as, "What is happening on this chart?", or
   points at something on the chart and marks it.
5. The agent receives a coherent snapshot of the open session, including the
   feed, tabs, panes, visible bars, indicators, drawings, order flow, replay,
   paper trading, health, and recent errors, plus what the cursor is over, what
   is selected, and what the user just marked. It answers from structured data
   instead of reading pixels, subject to the scopes the user granted. Paper
   state, evidence, user text, diagnostic log access, and screenshots are not
   granted silently.
6. If the user grants the `annotator` profile, the agent can also answer on the
   chart. It can attach a label, an arrow, a popup, a sound, or a compiled
   indicator, and then read the result back through the same snapshot the user
   sees.
7. Observer access remains read-only when the additional profile is not
   granted.
8. When needed, the agent creates an evidence bundle with a stable ID that can
   be attached to an investigation, issue, or pull request.

The MVP stops there. It carries no cockpit write that can lose state the user
built by hand, and no action with financial impact. Both belong to later
phases, on the tiers defined in section 2.6.

Two properties of this workflow are requirements, not conveniences. The agent
must be able to tell *which* bar, cell, or object the user means, because a
chart has hundreds of each and prose cannot disambiguate them. And the agent
must be able to reply on the surface the user is looking at, because a
correction that arrives only as text puts the burden of translation back on the
user. A control plane that reads without those two is a query interface, not a
second operator.

## 2. Core principles

### 2.1 One platform, one execution path

The graphical interface and an agent must not have separate implementations of
the same operation. A button, keyboard shortcut, test, startup hook, or agent
calls the same registered action. Only the `ActorContext` changes.

This prevents MCP from becoming a second application with private ways to
change state that users and the regular interface never exercise.

### 2.2 Act, Read, Discover

Every user-facing capability must satisfy the repository's second-operator
contract:

- **Act:** It exists as a named call, not only inside a click handler.
- **Read:** Its result can be inspected as data, not only as pixels.
- **Discover:** Its parameters, availability, and constraints are published by
  one registry shared with the user interface.

### 2.3 MCP is an adapter

Domain code must not know about MCP, JSON-RPC, Codex, or Claude. Future CLI,
REST, WebSocket, Python, or C adapters should reuse the same contract and
executor.

### 2.4 Authority starts small

The first profile is local, opt-in, and observation-first. Registering a
capability does not enable it automatically. Cockpit changes, paper trading,
and live trading use separate authority profiles.

### 2.5 No hot-path tax

When the control plane is disabled or idle, it must introduce:

- no serialization per trade;
- no snapshot reconstruction per frame;
- no lock or wait on the render thread;
- no unbounded queue;
- no duplicate bar aggregation;
- no MCP message for every depth update or trade print.

### 2.6 Effect tiers, not one write cliff

"Read" and "write" is too coarse a split to schedule by. Sorting a capability
by the damage it can do gives four tiers plus one asymmetry, and the tiers,
not the calendar, decide what may ship together.

| Tier | Examples | Property that sets the tier |
| --- | --- | --- |
| Observe | snapshot, chart window, scene, events, cursor, human-created marks | A remote call changes no user-visible state |
| Annotate and notify | agent-created mark, label, arrow, popup, sound, attach a script | No state loss or money; durable additions are reversible |
| Cockpit | tab, focus, viewport, bar spec, layers | Can discard work done by hand |
| Financial | paper orders, strategies, then live trading | Moves money or its record |

The observe tier needs authentication, explicit data scopes, redaction, and
rate limits, but no mutation guard. Durable changes in the annotate tier need
attribution and a one-action undo. Transient notifications need attribution,
rate limits, and an explicit user grant because a sound cannot literally be
undone. The cockpit tier needs `expected_revisions` because the user can lose
work. The financial tier needs everything in section 9.

The asymmetry: an operation that only ever *reduces* authority, such as locking
entries, flattening, disarming a strategy, or using a kill switch, cannot create
exposure. Refusing it has a worse failure mode than allowing it. Such operations
may use a lighter confirmation policy, but they keep their financial effect and
require an explicit safety permission. They never inherit annotate authority.
Section 9.4 states the rule that keeps this narrow.

The annotate tier is what makes the loop bidirectional. Without it the control
plane is a one-way mirror: the agent watches the user and has no way to show
the user anything back.

## 3. Existing foundations

The repository already provides much of the required foundation:

- `ChartState` is independent of egui and already exposes bars, the partial
  bar, trades, footprints, progress, and revisions.
- `FeedEvent`, `FeedCommand`, `FeedNotice`, and `FeedCapabilities` already
  separate data, commands, notices, and availability.
- Drawings use an extensible registry with stable IDs.
- Indicators describe their inputs and limits as data.
- The toolbar, tab strip, and other surfaces already return intents or actions
  on several paths.
- `APP_HEALTH_SUMMARY` already collects useful structured metrics.
- Logs already use stable `schema_version` and `event_code` values.
- Many `QUANTICK_*` hooks already prepare deterministic validation states.

### 3.1 Measured, not assumed

The claims above were checked against the code so the plan does not rest on
impressions:

- `ChartState` names egui only in comments, and already carries
  `timeline_revision`. A per-module revision is not new work.
- The application contains 88 distinct `QUANTICK_*` string literals: 86
  production surfaces and two test-only store variables. Many production
  surfaces already resemble a control plane, but they are startup-only,
  mostly write-only, and unobservable. The PR 0 inventory records each real
  surface and its migration target.
- `QuantickApp` has 107 fields and `crates/app/src/app.rs` is roughly 24k
  lines. This is the concrete reason section 12 forbids letting the control
  plane become a general refactor.
- `crates/app/src/app.rs` calls `request_repaint_after(16 ms)` unconditionally
  at the end of every update. The application therefore always runs a frame:
  request latency is bounded at about one frame without any wake mechanism, and
  there is no idle state in which control-plane cost could hide. Section 10
  states a millisecond budget for that reason.
- `pine::compile` already returns `Vec<PineError>`, each with a stable `code`,
  a `span`, a message, and notes. Structured compile diagnostics are ready to
  cross the wire without new work in the language crate.

### 3.2 What is genuinely missing

Two pieces are absent from the repository and must be built rather than
projected from something that already exists.

- **There is no pointer model.** Nothing in the application names what the user
  is pointing at. Drawings expose a selected index and egui controls know their
  own hover state within a frame, but there is no `hover_target`, no
  `cursor_slot`, and no resolved chart target anywhere. Section 6.5 adds one.
- **A note attached to a trade has no owner.** `sim` owns closed trades and the
  CSV history; an annotation on a trade belongs to no module. Section 6.6
  assigns it, so that a trade journal and a review tool do not invent two
  incompatible stores.

The missing component is a live, uniform port into the running instance.
Startup hooks will not be removed in one pass. They will gradually call the
same registered actions. Hooks that only make sense before the first frame may
remain.

## 4. Target architecture

```text
Codex / Claude / future clients
              |
       MCP over STDIO or HTTP
              |
        quantick-mcp
      domain-free adapter
              |
 authenticated, versioned local protocol
              |
  Control Gateway inside the running app
              |
 bounded request and response queues
              |
 +-----------------------------------+
 | Action Registry                   |
 | Snapshot Registry                 |
 | Event Journal                     |
 | Evidence Builder                  |
 +-----------------------------------+
              |
    the same application services
       used by the egui interface
              |
 engine / indicators / orderbook / replay / sim / strategy
```

### 4.1 Crate dependency graph

```text
app ----------> control
app ----------> control-local
mcp ----------> control
mcp ----------> control-local

control-local -> control
control ------> no workspace crate
mcp ----------> never depends on app
```

#### `quantick-control`

A new crate for control contracts and transport-neutral infrastructure:

- request and response envelopes;
- versioned external DTOs;
- capability IDs and metadata;
- JSON Schemas;
- structured errors;
- revisions, cursors, pagination, and limits;
- actor context, effect, and risk declarations;
- local protocol handshake and framing;
- helpers shared by hosts and clients.

It must not depend on `app`, egui, or private rendering types. Wire types are
explicit DTOs. Domain models should not gain `Serialize` merely to expose their
internals.

#### `quantick-control-local`

Added by PR 3 when the gateway landed, because two processes need the same
implementation of two things: the private instance-descriptor directory (ADR
0001 §4 — the running instance publishes there, a client discovers there, and
the ownership and permission checks on a file that holds a bearer token must
not exist twice), and the blocking loopback client that authenticates against
one gateway and exchanges framed envelopes. `quantick-control` stays free of
filesystem and socket I/O; `quantick-control-local` depends only on it, never
starts the application and never binds a listener. The app uses its
publication half; `quantick-mcp` and a later CLI use its discovery and client
halves.

#### `app::control`

The implementation hosted by the running application:

- `ControlGateway` receives and authenticates requests.
- `ActionRegistry` registers executable actions.
- `SnapshotRegistry` registers read projections.
- `EventJournal` keeps recent semantic events in a bounded buffer.
- `EvidenceBuilder` assembles consistent captures.
- An executor drains requests on the UI thread.

The gateway never gives clients references to `QuantickApp`. It receives
envelopes, produces owned snapshots, and invokes named actions.

#### `quantick-mcp`

A leaf crate and adapter binary:

- MCP over STDIO for local use;
- optional Streamable HTTP transport;
- discovery of one or more running instances;
- explicit instance selection when more than one is available;
- mapping registered capabilities to MCP tools and resources;
- `observer`, `annotator`, `developer`, and `paper` profiles;
- guided local configuration for Codex and Claude.

## 5. Core contracts

### 5.1 Capability descriptor

Every capability declares at least:

```text
id
version
title
description
module
input_schema
output_schema
examples
effect
risk
read_only
idempotency: forbidden | optional | required
dry_run_supported
reversible
destructive
required_permissions
preconditions
confirmation_policy
availability
unavailable_reason
expected_cost
```

This registry is the source for MCP, the user interface, documentation, tests,
and future SDKs. There must not be a hand-maintained agent-only list beside the
list used by the application.

### 5.2 Actor context

Every future write action receives a context similar to:

```text
actor_kind: human_ui | automation | agent
principal_id
client_name
connection_id
request_id
reason
requested_at_unix_ms
```

For remote calls, the gateway constructs the trusted actor fields after
authentication. A client may supply its self-declared name and an optional
reason, but it cannot select `actor_kind`, `principal_id`, or `connection_id`.
Human UI actions receive the same context from the in-process dispatcher.

Drawings, presets, strategies, and orders created by an agent must retain that
authorship in application state, the interface, and the journal. Authorship
cannot exist only in an MCP log.

### 5.3 Revisions and concurrency

Snapshots carry a monotonic revision for each module and one revision for the
capture. Future writes accept `expected_revisions`. If state changes after an
agent observes it, the operation fails with a structured error instead of
acting on a stale assumption.

### 5.4 Agent-readable errors

Errors use a predictable shape:

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

Error codes are stable and covered by tests, following the conventions already
used by Pine and the application logs.

### 5.5 Identifiers are strings, never enumerations

Capability IDs, snapshot scope IDs, module IDs, and event kinds are strings
resolved through a registry. None of them is a Rust enumeration in
`quantick-control`.

This is not a style preference. An enumeration makes the contract crate the
gatekeeper of every extension: a future `tape` crate, a new bar type, or a new
chart layer could not expose itself without editing `quantick-control` and
re-releasing the protocol. The registry is the docking port, and a closed enum
in the wire type would defeat it while leaving the registry in place as
decoration.

The lists in this document are therefore descriptions of what the registry is
expected to contain, not the definition of what it may contain.

### 5.6 Schema compatibility

One rule, so that clients can be tolerant readers and the platform can still
grow:

- Adding an optional field, a scope, an event kind, or a capability is additive
  and does not bump a version.
- Adding a required field, removing or renaming a field, changing its unit, or
  changing what a value means is breaking. A capability payload change bumps
  the capability version; an envelope change bumps the protocol version.

Both halves are enforced mechanically. Declared schemas are written to files
and snapshot-tested, so a breaking change appears as a reviewable diff instead
of arriving at a client as a runtime surprise.

## 6. Observation model

### 6.1 Semantic snapshot

`quantick_get_snapshot` captures a coherent view of one frame. Callers select
the scopes they need by ID. The registry is expected to open with:

- system and build;
- workspace and tabs;
- feed and market data;
- pane, viewport, and chart;
- indicators;
- drawings;
- order flow and L2;
- replay;
- paper trading;
- health and diagnostics;
- cursor, selection, and recent marks (section 6.5).

Per section 5.5 those IDs are strings from the registry, so a module added
later contributes a scope without touching `quantick-control`.

Decimal values travel as exact strings. Timestamp field names include their
unit, such as `_ms`. Data includes provenance whenever it is not obvious:
venue, backfill, live, replay, inferred, incomplete, or unavailable.

### 6.2 Visible chart window

The agent does not receive all history by default.
`quantick_get_chart_window` exposes:

- the visible slot range;
- the price range and axis inversion;
- zoom, pan, follow-live, and auto-fit state;
- OHLCV, delta, and trade count;
- the in-progress bar;
- plot values for the same range;
- visible anchors and objects;
- projected order-flow cells and events;
- gaps and coverage limits.

Larger ranges require explicit pagination.

### 6.3 Semantic scene

`quantick_get_scene` describes what is on screen without rasterizing it:

- visible controls;
- label and stable ID;
- enabled and selected state;
- the reason a control is unavailable;
- bounds when they are useful for a layout investigation;
- the owning panel, dialog, tab, and pane;
- the related registered capability.

This model answers, "What is the user seeing?" while still representing layout
defects that domain data alone cannot explain.

### 6.4 Events and follow-up observation

`EventJournal` records semantic changes, not every trade print:

- connection and reconnection;
- feed or symbol changes;
- tab, focus, or layout changes;
- bar specification or viewport changes;
- indicator recompilation, errors, or stale state;
- drawing creation, edits, and removal;
- replay start, pause, seek, and completion;
- order or position changes;
- health alerts;
- actions initiated by a human or an agent;
- selection changes and user marks (section 6.5).

Consumers use a cursor:

```text
events(cursor, limit) -> events, next_cursor, dropped_before
wait_for_change(cursor, timeout_ms) -> events or a clean timeout
```

The buffer is bounded and reports when older events have been discarded.

`wait_for_change` waits on the gateway thread, over the ring buffer, and never
on the UI thread. It must also not hold a slot in the bounded request queue
while it waits: a long poll that occupies an executor slot turns the one tool
built for patience into the one that starves every other request. A parked
waiter is a registered interest in a cursor position, not an in-flight request.

### 6.5 Deixis: what the user is pointing at

The workflow this plan exists to serve is a user showing something to an agent,
the way a product owner turns a screen toward an engineer. That requires the
platform to publish where the user's attention is. A chart holds hundreds of
bars and thousands of order-flow cells; prose cannot select one of them, and an
agent that has to infer the referent will confidently answer about the wrong
bar.

Section 3.2 records that no pointer model exists today. Three pieces are
needed. Reading the resolved cursor, selection, and marks created by the human
is observation: the application reports what the human did and changes nothing
on the human's behalf. Those reads belong to the `observer` profile. Creating a
mark through a remote call belongs to `annotator`.

**A resolved cursor.** Not a coordinate pair. The application already knows how
to turn a position into meaning during a frame, and the cursor scope publishes
that meaning:

```text
pane, tab
screen position
slot index, bar open_time_unix_ms, price
the bar under the cursor: OHLCV, delta, trade count, progress
the order-flow cell under the cursor, when a flow layer is on
the drawing, anchor, or handle under the cursor
the control under the cursor, by the same stable ID the scene uses
```

**A published selection.** Selected drawings, focused pane, active tab, and the
selected row in a trade history or event table. Drawings already carry a
selected index internally; this exposes it, with its ID, through the contract.

**A human mark.** One hotkey that appends an event carrying the fully resolved
target above, plus an optional note the user types. This is the primitive that
makes the rest work. It converts "look at this" from a gesture the agent cannot
see into a durable, structured referent the agent can quote back. Marks are
timestamped, ordered, and readable through the same cursor as every other
event, so an agent calling `wait_for_change` watches the user point in real time
instead of polling and guessing.

A mark is also the natural unit for the user's own review later: a timestamped,
fully contextualized observation about a session is most of what a trade journal
entry needs.

### 6.6 Where a note about a trade lives

The trade journal the user wants sits on a gap the repository has not filled.
`sim` owns closed trades and their CSV history and must stay a pure domain crate
with no opinion about prose. A mark (section 6.5) is transient by design.
Neither is a home for "this trade was a mistake, and here is why".

PR 0 assigns that ownership explicitly rather than leaving it to be discovered
twice, once by a journal feature and once by a trade-review tool. The
requirements are modest and worth naming now:

- an annotation references a closed trade by a stable ID that survives a
  restart, so `sim` needs no new field and no knowledge of the note;
- annotations are durable and human-editable, on the same footing as the other
  TOML state the application persists;
- an annotation records its actor, so a note written by an agent is never
  mistaken for the user's own words;
- marks can be promoted into annotations, since a mark taken during the session
  is usually what the user would have written afterwards.

## 7. MVP MCP surface

| Tool | Responsibility |
| --- | --- |
| `quantick_describe` | Report versions, instances, modules, profiles, and permissions |
| `quantick_get_snapshot` | Capture the requested scopes consistently |
| `quantick_get_chart_window` | Return visible data with range and pagination controls |
| `quantick_get_scene` | Return the semantic tree of the current interface |
| `quantick_read_events` | Read events after a cursor |
| `quantick_wait_for_change` | Wait for a relevant change with a bounded timeout |
| `quantick_get_diagnostics` | Report health, queues, workers, and recent errors |
| `quantick_capture_evidence` | Build a self-contained investigation bundle |
| `quantick_search_capabilities` | Find capabilities by name, module, or intent |
| `quantick_invoke` | Execute one registered capability by ID, with its declared arguments |

Once the annotate tier of section 2.6 lands, it adds a small, stable set:

| Tool | Responsibility |
| --- | --- |
| `quantick_annotate` | Attach a label, arrow, or zone to a resolved chart target |
| `quantick_notify` | Raise a popup, toast, or sound for the user |
| `quantick_attach_script` | Compile a Quantick Pine source and attach it to a pane |

MCP resources may mirror stable snapshots for clients that support them. Every
read also has a tool equivalent because support for resources and subscriptions
varies between clients.

### 7.1 Why the tool list is not the capability registry

An MCP client fetches the tool list when it connects and generally caches it,
while capability availability changes as the application changes state. One MCP
tool per registered capability therefore fails twice: the cached list goes stale
the moment a capability becomes unavailable, and by PR 6 and PR 7 the list would
run to hundreds of entries and crowd out the client's context before the first
question is asked.

The surface is therefore deliberately two-layered:

- A small, fixed set of named tools for the paths used constantly. A named tool
  with its own schema is far easier for a model to use correctly than a generic
  call, so the traffic that matters gets one.
- `quantick_search_capabilities` plus `quantick_invoke` for the long tail. The
  registry stays the single source of truth, capabilities appear and disappear
  with application state, and the tool list never changes shape.

`quantick_describe` reports which capabilities are currently available and why
the others are not, so a client never has to infer availability from a failed
call. Read-only tools are annotated as such, so clients that distinguish safe
tools can act on it.

The named set stays small on purpose. Promoting a capability from `invoke` to
its own tool is a deliberate decision about traffic, made with evidence, not the
default for every capability that ships.

## 8. Evidence bundle

A capture contains:

- an `evidence_id` and integrity hash;
- application version, commit, and protocol version;
- relevant operating system and graphics backend details;
- instance and session IDs;
- the consistent capture revision;
- a workspace snapshot;
- the visible chart window and semantic scene;
- the exact data used by projections;
- recent events and actions;
- relevant structured logs;
- frame, feed, book, and worker metrics;
- effective configuration with redaction;
- gaps, inferred data, and unavailable fields;
- an explicit list of information that was not captured.

By default, the bundle stays in memory for a limited retention period and is
returned as a resource. Exporting it to disk is a separate, explicit action.

A screenshot is not the normal observation path, but it is not merely a
fallback either. When one is captured, it is stamped with the same capture
revision as the scene, so every pixel region maps to a stable control or object
ID. A screenshot without that correlation invites the agent to guess; with it,
the image confirms what the scene tree already names, and the pair diagnoses
font, GPU, clipping, and composition defects that neither half can show alone.

## 9. Security and authority

### 9.1 MVP

- Disabled until the user opts in.
- Local endpoint only.
- One ephemeral token per application instance.
- Instance descriptor stored in a private user directory.
- `observer` is the default profile.
- Bounded payloads, rates, timeouts, and queues.
- An in-app panel lists connected clients and their permissions.
- Access can be revoked immediately.
- No shell tool or arbitrary file access.
- No coordinate-based mouse or keyboard injection.
- The MCP server never starts Quantick. Discovery reports the running
  instances; when there are none, it says so and asks the user to start one.
  Launching the application is the user's action, and an adapter that could
  spawn instances would both hide state from the user and make the instance
  list a thing the agent creates rather than observes.

Profiles map onto the tiers of section 2.6 rather than onto a read and write
split: `observer` covers reads of the cursor, selection, and human-created marks
from section 6.5. `annotator` adds reversible annotations, notifications,
remote marks, and script attachment. `developer` adds cockpit actions. `paper`
adds simulated trading. Live trading has no profile in this plan.

### 9.2 Future writes

- Use `dry_run` whenever an operation can be validated first.
- Require an `idempotency_key` for safe retries.
- Use `expected_revisions` to reject stale state.
- Record an audit event for every attempt and result.
- Match approval requirements to the declared effect.
- Apply the same validation and confirmation rules as the user interface.

### 9.3 Live trading

Live trading is not part of the MVP and must not ship together with paper
trading. It requires a dedicated threat model and separate pull requests:

- a two-phase prepare and confirm flow;
- an exact order summary before commit;
- short-lived confirmation bound to the market and account revision;
- independent limits for notional value, size, and frequency;
- a kill switch;
- no way for an agent to increase its own authority;
- potentially looser policies for locking or reducing risk than for unlocking
  or increasing risk;
- durable authorship on the order and in the journal.

### 9.4 Risk-reducing operations

Section 2.6 notes that an operation which only ever reduces authority is not
symmetric with the operation that grants it. Locking entries, flattening a
position, disarming a strategy, and hitting a kill switch cannot create
exposure. Refusing them has the worse failure mode: the user asks the platform
to stop and it argues instead.

The rule that keeps this from becoming a loophole:

- Risk reduction changes confirmation policy, not the capability's effect.
  Paper and live operations still require an explicit `paper.safety` or future
  `live.safety` grant; `observer`, `annotator`, and `developer` never inherit it.
- An operation qualifies only if every reachable outcome leaves risk equal or
  lower. If any argument value, ordering, or failure path can increase
  exposure, it is not a risk-reducing operation and the exception does not
  apply.
- The inverse never inherits the exception. Unlocking, re-arming, and raising a
  limit sit in the tier they belong to, with full confirmation.
- Each such capability declares this classification in its descriptor, and a
  test asserts the declaration, so the exception is auditable rather than
  argued case by case in review.
- The audit trail is identical to any other action: actor, reason, timestamp,
  and result.

This is what makes "block my entries" available under a narrow safety grant,
while "let me trade again" stays under the full authority and confirmation of
its financial tier.

## 10. Performance budget

### 10.1 Hot paths

The gateway does not participate in ingestion, aggregation, projection, or
painting. The UI thread performs control-plane work only when a request is
pending or a semantic change needs to be recorded.

### 10.2 Captures

- Build snapshots on demand.
- Clone only the requested window.
- Produce a compact DTO on the UI thread.
- Serialize and compress away from the UI thread.
- Aggregate market events before recording them.
- Spend at most a fixed time budget on control-plane work per frame, stated in
  milliseconds and enforced in code, not as a count of requests.
- Leave work that has not started in the bounded queue or return backpressure
  when the remaining frame budget is insufficient.

A request count is the wrong unit. One capture of a wide chart window with
footprints and plot series can cost more than twenty small reads, so a cap of
"N requests per frame" bounds the wrong quantity and still lets a single
oversized capture blow a frame. The executor therefore checks a clock against a
budget between requests and scopes. It never pauses one coherent capture and
resumes it against a different frame. Every allowed capture shape has a measured
upper bound below the budget; a request that could exceed it is paginated or
rejected before capture, as section 6.2 requires. An unexpected single-capture
overrun is recorded as a budget violation and fails the performance test rather
than returning a mixed-revision result.

PR 2 calibrated the budget to 250 microseconds per frame against the measured
core-scope p99 and shared-host baseline, with a hard stop well under a 16 ms
frame. A number is stated so that "as fast as possible" is never the
specification; later modules must remain inside it or paginate their work.

### 10.3 Performance evidence

Each pull request that touches the application measures against `origin/main`
with dense replay and the fields in `APP_HEALTH_SUMMARY`. The instrument already
exists: `frame_cpu_ms`, `frame_worst_ms`, `feed_arrival_ms`, and `trades_per_s`
are logged today, so the comparison needs a method rather than new telemetry.
Both runs must share one window of market conditions, since the same branch
measured in a quiet hour and a busy one produces numbers that differ by more
than any regression this plan could cause.

Numeric thresholds are calibrated against that baseline rather than guessed in
this document, with one exception: the per-frame budget of section 10.2 is
stated up front, because a budget invented after the code exists is written to
fit the code.

"Idle" needs a precise definition here. Per section 3.1 the application repaints
unconditionally every 16 ms, so there is no idle application to hide cost in.
Idle means only that no request is pending and no semantic event is being
recorded, and the acceptance condition in that state is exact: no new
allocation, no new lock, and no measurable change in `frame_cpu_ms` against
`origin/main`.

## 11. Pull request plan

Each item below is one goal, branch, and worktree. A later pull request must not
depend on unmerged code from an earlier one unless the plan explicitly marks
the work as parallel-safe.

### PR 0: Architecture contract and inventory

**Branch:** `docs/mcp-control-plane-plan`

**Rate class:** Startup and documentation only.

Deliverables:

- Review and approve this document.
- Create an initial capability inventory by module. Start from the 86
  production `QUANTICK_*` surfaces of section 3.1 and identify the two
  test-only variables separately, so the inventory begins as a measured list
  with migration targets rather than as an estimate.
- Add an ADR for local transport and instance discovery.
- Add a threat model for the `observer` profile, covering the cursor and
  selection scopes of section 6.5. Publishing where the user is looking is
  observation, but it is still information leaving the application, and the
  threat model must say so explicitly.
- Define conventions for IDs, revisions, actors, and effects, including the
  tier classification of section 2.6 as a descriptor field.
- Name maximum payload and retention policies in configuration or constants.
- Decide the tool surface shape of section 7.1: which capabilities get named
  MCP tools and which are reached through `invoke`.
- Assign ownership for a note attached to a trade, per section 6.6.
- Settle the determinism question below.

**Determinism.** `CLAUDE.md` makes "same trades in, same bars out" the first
non-negotiable rule, and an agent acting during a session becomes part of that
session's input. The event journal is a bounded ring buffer, so it is not a
durable record of what happened. PR 0 selects a durable, versioned control
trace ordered by logical replay time for every determinism-affecting
non-observe action, regardless of actor. A replay with an unrecorded action is
ineligible as a deterministic fixture. The full decision is in the control
contract.

Acceptance criteria:

- No security or compatibility decision required by PR 1 remains implicit.
- The first two expected adapters are identified: MCP and CLI.
- Codex and Claude connection flows are documented.
- The determinism decision above is recorded with its rationale.

### PR 1: The `quantick-control` crate

**Branch:** `feat/control-contract`

**Rate class:** Cold path.

Deliverables:

- Add the new crate.
- Define the versioned handshake and protocol.
- Add envelopes, errors, revisions, and cursors.
- Add capability descriptors and schemas.
- Add the actor, effect, and risk model.
- Add a bounded codec.
- Add a fake host and fake client.

Required contract files to review:

- root `Cargo.toml`;
- `CLAUDE.md`;
- `crates/pine/tests/workspace_deps.rs`.

Acceptance criteria:

- A second fake implementation proves the port.
- Tests reject duplicate IDs, invalid schemas, and invalid examples.
- Declared schemas are written to files and snapshot-tested, so a breaking
  change shows up as a diff in review (section 5.6).
- A test asserts that no capability ID, scope ID, or event kind is modelled as
  a closed enumeration (section 5.5).
- The crate has no dependency on `app` or domain crates.
- All four repository checks pass.

### PR 2: Snapshot registry and core application scopes

**Branch:** `feat/control-observer`

**Rate class:** On demand.

Deliverables:

- Add `crates/app/src/control/`.
- Add a projection registry.
- Add system, workspace, feed, chart, and health snapshots.
- Add the cursor and selection scopes of section 6.5. This is where the
  application gains a pointer model it does not have today, so the work is a
  new concept rather than the exposure of an existing one: resolving a position
  into a slot, a price, a bar, a flow cell, and an object under the cursor.
- Add a paginated chart window.
- Include explicit units and provenance in DTOs.
- Produce a consistent revision for each capture.
- Measure the baseline and calibrate the per-frame budget of section 10.2.
- Do not add a socket yet.

After this registry merges, add the remaining snapshot modules in owner-focused
pull requests that may run alongside PR 3:

- `feat/control-snapshots-analysis`: indicators and drawings;
- `feat/control-snapshots-orderflow`: tape, footprint, bubbles, heatmap, and L2;
- `feat/control-snapshots-session`: replay and paper trading.
- `feat/control-scene`: visible controls, stable IDs, state, bounds, ownership,
  unavailable reasons, and related capability IDs.

Each module registers through the projection port and touches the integration
root only to dock itself. The semantic scene must merge before PR 4 exposes
`quantick_get_scene`; the three domain snapshot branches must merge before the
MVP evidence acceptance in PR 5c.

Acceptance criteria:

- A headless test creates `QuantickApp`, changes state through the normal path,
  and verifies the snapshot.
- A two-pane capture preserves focus and provenance correctly.
- Every registered snapshot validates against its own declared schema, checked
  by a test per module, so hand-written DTO mapping cannot drift from the
  contract in silence.
- A resolved cursor over a known bar reports that bar, verified headlessly
  against a fixture rather than by eye.
- Chart pagination keeps its original high-water mark when a new live bar
  appends, but rejects a page after a correction or backfill below that mark.
- No request means no per-frame cost, measured as `frame_cpu_ms` against
  `origin/main` under the method in section 10.3.
- No egui type appears in the wire schema.

The same schema, consistency, no-egui, and performance criteria apply to every
module snapshot pull request. Scene tests also prove stable control IDs across
frames and explicit unavailable reasons without parsing rendered text.

### PR 3: Local gateway for the running instance

**Branch:** `feat/control-gateway`

**Rate class:** Startup and infrequent requests.

Deliverables:

- Implement the authenticated loopback TCP endpoint selected by the ADR.
- Publish an instance descriptor.
- Create an ephemeral token.
- Add bounded queues.
- Dispatch only validated, authorized, bounded state access on the UI thread;
  keep socket I/O, authentication, parsing, and serialization off-thread.
- Wake or repaint the application when required.
- Add timeouts, backpressure, and clean shutdown.
- Support multiple instances in the protocol.

Acceptance criteria:

- An integration client reads a running application.
- Wrong tokens, stale descriptors, a closed application, and a full queue
  return stable errors.
- Closing the application stops the gateway and removes or expires discovery.
- The render path never waits on the gateway or acquires one of its locks.
- Discovery with no running instance returns an empty list and a next step, and
  nothing in the gateway or the adapter can start the application (section
  9.1).
- The per-frame budget of section 10.2 is enforced in code and covered by a
  test that queues more work than one frame allows.

### PR 4: The `quantick-mcp` server

**Branch:** `feat/mcp-observer`

**Rate class:** Per tool call.

Deliverables:

- Add the crate and binary.
- Support STDIO.
- Keep Streamable HTTP deferred until its separate transport and authentication
  review; PR 4 ships STDIO only.
- Implement the MVP tools except advanced event and evidence support.
- Add instance selection.
- Add short, self-contained server instructions.
- Mark read-only tools correctly in their annotations.
- Reserve standard output for MCP frames and send redacted diagnostics only to
  standard error.
- Add a local configuration generator or setup assistant.
- Add MCP client smoke tests.

Acceptance criteria:

- Codex and Claude run `describe`, `get_snapshot`, `get_chart_window`, and
  `get_diagnostics` against the same instance.
- Under the observer ceiling, `quantick_invoke` is annotated read-only, no
  registered write capability is available, and attempted write IDs are denied.
- Disconnecting a client does not change application state.
- A smoke test proves that startup, errors, and shutdown emit no non-MCP bytes
  on standard output.
- All four repository checks pass.

The original plan combined events and evidence bundles in one pull request.
They are separated below because they serve different consumers and have
different urgency: the event cursor is infrastructure the pointing channel needs
in order to exist, while the bundle is a reporting artifact that is only useful
once there is something worth reporting.

### PR 5a: Events, cursor, and the pointing channel

**Branch:** `feat/control-events`

**Rate class:** Semantic changes only, never trade or frame frequency.

Deliverables:

- Add a cursor-based ring buffer.
- Implement `read_events` and `wait_for_change`, parked on the gateway thread
  and holding no request slot (section 6.4).
- Emit selection-change events.
- Add the mark hotkey of section 6.5: it appends an event carrying the fully
  resolved cursor target plus an optional typed note.
- Add the action-registry port and register `attention.mark.create`; the UI
  hotkey and deterministic tests use that handler. The gateway keeps it
  unavailable to remote observer clients.
- Implement the durable control-trace port selected in PR 0, because the human
  mark is the first registered non-observe action that can affect replay output.

Acceptance criteria:

- An agent watching `wait_for_change` observes the user select an object and
  take a mark, and can name the bar, price, and object that was marked.
- A mark taken over a footprint cell reports the cell, not only the bar.
- The journal never allocates per trade; market events are aggregated first.
- An expired cursor reports `dropped_before`.
- A parked waiter does not delay any other request, proved by a test that waits
  and reads concurrently.
- Remote write requests remain impossible under the observer profile.
- Replaying a recorded human mark injects it at the same logical replay time;
  an incomplete or missing trace makes the run fixture-ineligible.

### PR 5b: The annotate and notify tier

**Branch:** `feat/control-annotate`

**Rate class:** Human or agent actions, never trade or frame frequency.

This is the first tier that writes, and it is deliberately the tier that cannot
lose the user's work. It is also what makes the loop bidirectional: until it
ships, the agent can see the chart and has no way to answer on it.

Deliverables:

- Extend the action registry introduced in PR 5a with remotely authorized
  annotate handlers, then reuse the same registry in PR 6.
- Expose `attention.mark.create` to the `annotator` profile through the same
  handler the PR 5a UI hotkey already uses.
- Add label, arrow, and zone annotations against a resolved chart target,
  attributed to the agent as author and removable in one action.
- Add popup, toast, and sound notification capabilities.
- Add independent annotate scopes for attention, chart state, notifications,
  sound, and scripts. Sound is off by default and notifications have a stricter
  rate limit than ordinary calls.
- Add `attach_script`: compile a Quantick Pine source, return structured
  diagnostics on failure, attach the compiled indicator to a pane on success,
  and expose a matching detach.
- Record the actor on every annotation, visible in the interface and not only
  in a log.

`attach_script` is cheap and high value because the pieces exist. Section 3.1
records that `pine::compile` already returns `Vec<PineError>` with a stable
code, span, message, and notes, and `IndicatorHost` is already headless. What
it adds is the closed loop: the agent writes a script, reads its own compile
errors, fixes them, and reads the resulting plot values through the snapshot it
already has.

Acceptance criteria:

- The same handler serves the interface and the agent, shown in the pull
  request.
- An annotation created by an agent is visibly attributed as such and can be
  removed by the user in one action.
- A failed compile returns the `PineError` code, span, and notes as structured
  data, never as a rendered string an agent has to parse.
- A successful attach is readable in the indicators scope, and detach restores
  the prior state exactly.
- No capability in this tier can discard user-created state or affect a
  position, verified by review against the tier table in section 2.6.
- Notification flood tests prove the per-client rate and burst limits, and a
  client without the sound scope cannot produce audible output.
- Every action uses the PR 5a control trace, since this is the first pull request
  in which a remote agent changes session input.

### PR 5c: Evidence bundles

**Branch:** `feat/control-evidence`

**Rate class:** On-demand captures.

Deliverables:

- Add consistent captures and a temporary resource.
- Add redaction, an integrity hash, and a manifest.
- Consume the semantic scene through its existing projection port.
- Correlate an optional screenshot with the capture revision, so pixel regions
  map to stable IDs (section 8).
- Keep evidence in memory. Disk export is a later cockpit/filesystem action and
  does not ship under observer or annotator authority.
- Migrate the observation and evidence side of the `ui-harness` and `visual-qa`
  skills onto the control plane. They may still use existing deterministic
  hooks to establish a fixture until the corresponding cockpit handler lands in
  PR 6, but assertions read structured live state instead of relying only on
  window captures. Retirement of production hooks starts with the matching PR 6
  and PR 7 actions, not with the observer gateway.

MVP acceptance criteria:

- An agent explains a running session without a screenshot.
- Feed changes, replay changes, indicator changes, and connection errors appear
  through the cursor.
- The bundle reports omitted information and coverage gaps.
- A bundle containing a screenshot maps every named control to a region of that
  image.
- At least one existing validation skill reads and asserts through the live
  control plane; its fixture setup may still use an existing deterministic hook
  until the matching action is available.

### PR 6: The cockpit tier

**Initial branch:** `feat/control-actions`

**Rate class:** Human or agent actions, never trade or frame frequency.

The action registry port itself lands in PR 5a for the shared mark handler and
gains remote annotate consumers in PR 5b, so this pull request adds no port. It
adds the tier that can discard work the user did by hand, which is why
`expected_revisions` and either an undo path or an explicit irreversible-change
flow stop being optional here. Extend the threat model with cockpit subscopes,
filesystem boundaries, and state-loss confirmation before enabling the
`developer` profile. Once the port has carried multiple effects, independent
modules can move into separate pull requests:

1. workspace, tabs, and focus;
2. replay and navigation;
3. viewport, bar specification, and layers;
4. indicators;
5. drawings and presets;
6. order-flow settings.

Acceptance criteria for every action:

- The button or shortcut and MCP call the same handler.
- The result is structured.
- The capability has a descriptor in the registry.
- The actor is recorded.
- Availability and blocking reasons match the user interface.
- Relevant `expected_revisions` are required.
- Reversible actions expose undo or rollback. An irreversible action requires a
  successful dry run, explicit confirmation, and a recoverable backup when the
  underlying state can be backed up.
- An existing hook, if any, calls the same handler.

### PR 7: Paper trading and strategies

**Branches:** One per module after the registry is stable.

Deliverables:

- Add complete snapshots for orders, queued orders, positions, brackets, P&L,
  and the journal.
- Extend the threat model with paper subscopes, confirmation, idempotency,
  simulator failure modes, and the risk-reducing exception before enabling the
  `paper` profile.
- Add dry runs for commands.
- Add strategy arm and disarm operations.
- Route place, cancel, modify, close, and flatten through the same path as the
  user interface.
- Store durable authorship.
- Add a separate MCP `paper` profile.
- Implement the trade annotation store assigned in PR 0 (section 6.6), together
  with promoting a mark into an annotation. This is what makes an assisted trade
  journal possible without `sim` learning about prose.
- Classify lock, flatten, and disarm under section 9.4 and prove the
  classification with a test.

Acceptance criteria:

- Agents have no shorter execution path than human users.
- Stale state is rejected.
- Retrying does not duplicate an order.
- Replay and live paper trading still use the same simulator.
- The journal identifies the actor.
- A note written by an agent is distinguishable from one written by the user,
  in the interface and in the store.
- A risk-reducing capability is reachable under a lighter policy than its
  inverse, and a test proves the inverse did not inherit it.

### PR 8: Public API and future live trading

Begin only after the control plane has proved both MCP and at least one other
adapter. Possible deliverables include:

- an official CLI;
- REST/OpenAPI and a WebSocket event stream;
- a Python SDK;
- remote authentication or OAuth;
- a separate broker or venue gateway;
- managed approvals and policies;
- registered analytics modules.

The mere presence of configuration must never enable live trading.

## 12. Order, dependencies, and parallel work

Recommended path to the MVP:

```text
PR 0 -> PR 1 -> PR 2 -> PR 3 -> PR 4 -> PR 5a -> PR 5b -> PR 5c
                    +-> semantic scene -> PR 4
                    |                                      ^
                    +-> analysis snapshots ----------------+
                    +-> order-flow snapshots --------------+
                    +-> replay/paper snapshots -------------+
```

Keeping the contract and registry spine linear reduces protocol churn. After PR
2, owner-focused snapshot modules may run alongside PR 3. The semantic scene
merges before PR 4; the remaining modules merge before PR 5c closes the evidence
acceptance. Once the action registry is stable, indicator, drawing, replay, and
workspace actions can be developed in parallel when they do not share files.

The semantic scene did not merge before PR 4 as the diagram above wants: PR 4
shipped without `quantick_get_scene`, and the tool lands with the scene module
(roadmap 5.2). The rest of the order is unchanged.

The ordering inside PR 5 is the one change worth defending. 5a comes first
because the event cursor is what the pointing channel is built on, and pointing
is the capability that makes every later conversation specific instead of
approximate. 5b comes before 5c because a bidirectional loop is worth more than
a reporting format: an agent that can answer on the chart is useful during the
session, while a bundle is useful after it. Delivering the reporting machinery
first would produce a control plane that documents problems well and helps with
none of them.

The current `app.rs` is a likely conflict point. Introducing the control plane
must not become a general application refactor. Extract only the application
service required for the capability in each pull request.

## 13. Pull request execution playbook

### 13.1 Start the work

```powershell
git fetch origin
git worktree add -b <prefix>/<slug> ..\quantick-worktrees\<prefix>-<slug> origin/main
```

Work only in the dedicated worktree.

### 13.2 Before implementation

Record the following in the pull request objective:

- observable behavior;
- expected files;
- rate class;
- the tier from section 2.6, and for a write, what the user could lose;
- risk and authority;
- the second implementation that proves the port;
- tests that fail without the change;
- expected effect on `APP_HEALTH_SUMMARY`, measured under the shared-conditions
  method of section 10.3.

### 13.3 During implementation

- Write the contract test before the handler.
- Keep IDs, units, and error codes stable.
- Use bounded queues.
- Separate UI-thread capture from off-thread serialization.
- Never add an MCP shortcut into private state.
- Do not alter the engine or aggregators to serve the UI or API.
- Preserve unrelated user changes.

### 13.4 Verification

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

Application pull requests also run the dense replay scenario and compare
health metrics with `origin/main`. MCP pull requests also test conformance,
truncated framing, oversized payloads, timeouts, invalid tokens, and disconnects.

### 13.5 Before opening the pull request

- Complete an architecture review.
- Resolve every blocker and should-fix item, or justify it in the pull request.
- Generate or validate registry documentation.
- Include a real request and response example.
- Show that the user interface and agent used the same execution path.
- Confirm that CI is green.

## 14. End-to-end acceptance scenarios

### Scenario A: What is happening?

With Binance or MetaTrader open, the agent identifies the feed, symbol,
connection state, staleness, bar specification, in-progress bar, visible
window, indicators, layers, order flow, and paper-trading state.

### Scenario B: Stalled feed

The agent distinguishes a dropped connection, a silent feed, and a quiet
market, including the next diagnostic step reported by the application.

### Scenario C: Replay

The agent reports the session, speed, position, played and total events, gaps,
and whether the chart follows the current edge.

### Scenario D: Development bug

The user reproduces a defect and asks for evidence. The returned `evidence_id`
references a bundle with enough state, scene data, events, metrics, and known
limitations for another agent to investigate without screenshots.

### Scenario E: Authority

Under the observer profile, a write request is absent or rejected. Under a
future annotator or developer profile, a stale action fails, while an accepted
action records its actor and result.

### Scenario F: Multiple instances

The MCP server lists running instances and does not choose silently when the
selection is ambiguous. With no instance running it says so and does not start
one.

### Scenario G: Pointing

The user hovers a bar, takes a mark, and types "this absorption is what I mean".
Without asking a further question, the agent names the bar, its timestamp, its
delta, the flow cell under the cursor, and the drawing it sits inside. It then
answers about that bar and no other.

This is the scenario the plan exists for, and it is the one the original draft
could not satisfy: with snapshots alone, the agent would have had to guess which
of several hundred bars the user meant.

### Scenario H: An indicator built in conversation

The user describes an indicator in prose. The agent writes a Quantick Pine
source, calls `attach_script`, reads back a compile error with its code and
span, corrects the source, attaches it successfully, and then reads the plotted
values from the same snapshot the user is looking at. The user accepts or
discards it in one action.

The agent completes the loop without a screenshot and without the user relaying
error messages by hand.

## 15. Main risks

| Risk | Mitigation |
| --- | --- |
| MCP becomes a second application | The UI and MCP use the same action registry |
| The agent reads all and points at nothing | Cursor, selection, marks (section 6.5) |
| Reading ships and answering never does | The annotate tier is inside the MVP (PR 5b) |
| A cached tool list goes stale or grows unbounded | Named tools plus `invoke` (section 7.1) |
| A closed enum blocks a future module | IDs are registry strings (section 5.5) |
| A control action breaks replay reproducibility | Durable control trace ordered by logical replay time; an unrecorded action disqualifies the fixture |
| Hand-mapped DTOs drift from the contract | Each snapshot validates against its schema |
| One oversized capture blows a frame | A millisecond budget, not a request count |
| A long poll starves other requests | Waiters park off the executor (section 6.4) |
| The risk-reducing exception becomes a loophole | Every outcome must lower risk (section 9.4) |
| Snapshots consume frame time | Build on demand, limit ranges, and serialize off-thread |
| The tool list becomes too large | Use profiles, capability search, and module-based exposure |
| Modules produce an inconsistent snapshot | Capture in one UI-thread pass and attach revisions |
| A client acts on stale state | Require `expected_revisions` |
| A retry duplicates an action | Require `idempotency_key` |
| Local data leaks | Require opt-in, local endpoints, ephemeral tokens, and redaction |
| An agent gets a trading shortcut | Reuse UI confirmation and use two phases for live trading |
| DTOs freeze internal models | Keep external DTOs explicit and versioned |
| Refactoring overwhelms `app.rs` | Extract incrementally, one capability per pull request |
| MCP clients diverge | Run Codex and Claude conformance checks for every adapter release |

## 16. Recommended starting decisions

PR 0 adopts these defaults, with the details fixed by the control contract,
ADR, and threat model:

1. **Local first:** Use an authenticated loopback endpoint. Evaluate named
   pipes and Unix sockets in the ADR without blocking the MVP.
2. **Local MCP over STDIO:** Prefer broad client compatibility and simple
   configuration.
3. **Observer by default:** Expose no writes unless the user grants a stronger
   profile. Use `annotator` for reversible on-chart answers without cockpit
   authority.
4. **Model multiple instances immediately:** Keep this in the contract even if
   the first test runs only one instance.
5. **Keep evidence in memory:** Make disk export explicit.
6. **Use pull snapshots and cursor-based events:** Do not stream frames or each
   trade to a model.
7. **Do not expose generic synthetic input:** Coordinate-based mouse and
   keyboard control is not an API.
8. **Do not build a plugin VM yet:** The registry is the port. Add a plugin
   runtime only after a second concrete use case requires it.
9. **Publish attention, not only state:** A resolved cursor, a published
   selection, and human-created marks are part of the observer MVP. Remote
   mark creation is an annotate action. Pointing is what makes every later
   exchange specific.
10. **Ship the annotate tier inside the MVP:** Reading without answering leaves
    a one-way mirror. Annotate and notify cannot lose the user's work, so they
    are not the risk that justifies deferring them.
11. **Keep the tool list small and stable:** Named tools for the hot paths,
    `invoke` for the long tail. Never one tool per capability.
12. **Identifiers are strings:** No closed enumeration of capabilities, scopes,
    or event kinds in the contract crate.
13. **Budget in milliseconds:** State a per-frame time budget before writing the
    code, and calibrate the number against a measured baseline.
14. **The adapter never starts the application:** Discovery observes instances;
    it does not create them.

## 17. Immediate next action

PR 0 and PR 1 are merged; PR 2 through PR 5a are open as one stack. The
current next action, the merge order of that stack, and the docking points of
the remaining MVP work are kept in
[control-plane/roadmap.md](control-plane/roadmap.md), which this section no
longer duplicates.

The sequence PR 0 → PR 1 → application was chosen to establish a small,
testable boundary before touching the large `QuantickApp` state, and to keep
MCP SDK decisions out of domain code. That rationale stands.

## 18. MVP definition of done

The MVP is complete when:

- a running Quantick instance advertises opt-in local access;
- Codex and Claude can connect through MCP;
- read tools return documented schemas;
- an agent can explain the current session without a screenshot;
- the user can point at a bar, a cell, or an object and the agent names exactly
  what was pointed at;
- the agent can answer on the chart as well as in prose, through the annotate
  tier, with its authorship visible and its annotations removable in one action;
- an indicator can be described in prose, compiled, corrected from structured
  diagnostics, and attached without a screenshot;
- relevant changes can be followed with a cursor;
- an evidence bundle can reproduce an investigation;
- no cockpit or financial write capability is available;
- the per-frame control budget is enforced and measured;
- idle hot paths gain no new lock or allocation, where idle means no pending
  request rather than an idle application;
- health metrics show no material regression against `origin/main`, compared
  under one window of market conditions;
- tests, the four repository checks, architecture review, and CI all pass.

## 19. References

- [Control-plane roadmap](control-plane/roadmap.md): where each plan item
  stands, the merge order of the open stack, carried-forward gaps, and the
  docking points and acceptance criteria of the remaining MVP work.
- [PR 0 control-plane contract](control-plane/README.md): Normative decisions,
  capability inventory, transport ADR, and observer threat model.
- [`CLAUDE.md`](../CLAUDE.md): Architecture, determinism, dependency direction,
  and the "operable without a hand" rule.
- [Local architecture review](../.claude/skills/arch-review/SKILL.md): The
  second-operator contract, including Act, Read, Discover, authorship, and
  authority.
- [New extension guide](../.claude/skills/new-extension/SKILL.md): Docking
  ports, performance budgets, and the definition of a modular extension.
- [`ChartState`](../crates/app/src/state.rs): Existing headless chart state.
- [Feed protocol](../crates/app/src/feed/mod.rs): Existing events, commands,
  notices, and capabilities.
- [Drawing registry](../crates/app/src/drawings/mod.rs): An existing shared,
  extensible registry.
- [OpenAI Model Context Protocol documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli):
  STDIO and Streamable HTTP transports, plus MCP configuration in Codex.
- [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp): Local
  STDIO registration, scopes, and server verification.
