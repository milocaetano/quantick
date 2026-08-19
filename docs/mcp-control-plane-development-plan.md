# Quantick control plane and MCP development plan

**Status:** Proposal for review, revised after architecture review

**Date:** 2026-08-19

**Authorship:** The first draft was written by Codex. This revision was made by
Claude (Claude Code) on 2026-08-19, at the repository owner's request, as a
review of that draft rather than a replacement for it.

**Why it was revised:** The draft's architecture was accepted as proposed and
is unchanged. What the review changed is scope, schedule, and six technical
details, because the draft did not yet satisfy the objective it was written for:
the owner wants to point at something on a running chart and be understood, and
to have the assistant answer on the chart rather than only in prose. A read-only
snapshot API cannot do either. Section 0 lists every change with its reasoning,
and section 3.1 gives the measurements each change rests on, so a reviewer can
check the claims instead of trusting them.

**Primary goal:** Let Codex, Claude, and other MCP-compatible clients observe a
running Quantick instance, understand what the user is pointing at inside it,
answer on the chart as well as in prose, and collect reproducible evidence
without relying on screenshots.

**Repository language:** English. Code, schemas, documentation, examples,
public messages, and contributor-facing artifacts must be written in English.

## 0. What this revision changed

The first draft's architecture is unchanged: a vendor-neutral control contract,
MCP as one adapter, a single registry shared with the interface, and authority
that starts small. The review kept that spine and changed the scope and the
schedule.

Three gaps against the stated objective:

- **Nothing published where the user is looking.** The draft gave pull
  snapshots and a scene tree of controls, but no cursor, selection, or way to
  mark a target, so an agent could read the whole chart and still not know which
  bar the user meant. Section 6.5 adds a resolved cursor, a published selection,
  and a mark, all inside the observe tier.
- **Every write deferred, including the harmless ones.** The draft had one write
  cliff. Section 2.6 replaces it with tiers by effect, and PR 5b ships the tier
  that cannot lose the user's work inside the MVP, so the agent can answer on
  the chart instead of only reading it.
- **Live indicator authoring unnamed.** Compiling a script from prose, reading
  structured diagnostics, and attaching it is among the highest-value
  capabilities on the objective list, and the draft mentioned indicators only as
  settings actions. PR 5b makes it explicit, using compile diagnostics that
  already exist.

Six technical corrections, each stated where it belongs: identifiers are
registry strings rather than enumerations (5.5); schema compatibility is
enforced by snapshot-tested files (5.6); the frame budget is milliseconds rather
than a request count (10.2); `wait_for_change` parks off the UI thread and holds
no request slot (6.4); the MCP tool list is not the capability registry (7.1);
and the determinism consequence of an agent acting during a replay is a decision
PR 0 must record (PR 0).

Two smaller ones: a screenshot is correlated with the capture revision rather
than treated as a bare fallback (section 8), and the adapter never starts the
application (9.1).

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
   instead of reading pixels.
6. The agent answers on the chart as well as in prose. It can attach a label,
   an arrow, a popup, a sound, or a compiled indicator, and then read the
   result back through the same snapshot the user sees.
7. When needed, the agent creates an evidence bundle with a stable ID that can
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
by the damage it can do gives three tiers plus one asymmetry, and the tiers,
not the calendar, decide what may ship together.

| Tier | Examples | Property that sets the tier |
| --- | --- | --- |
| Observe | snapshot, chart window, scene, events, cursor, marks | Changes nothing |
| Annotate and notify | label, arrow, popup, sound, attach a script | Additive, reversible, no money |
| Cockpit | tab, focus, viewport, bar spec, layers | Can discard work done by hand |
| Financial | paper orders, strategies, then live trading | Moves money or its record |

The observe tier needs no protection. The annotate tier needs attribution and a
one-action undo. The cockpit tier needs `expected_revision` because the user can
lose work. The financial tier needs everything in section 9.

The asymmetry: an operation that only ever *reduces* authority — lock entries,
flatten, disarm a strategy, kill switch — cannot create exposure, and refusing
it has a worse failure mode than allowing it. Such operations may ship with the
annotate tier even though they touch trading, and section 9.4 states the rule
that keeps that narrow.

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
- The application defines 89 distinct `QUANTICK_*` hooks. Those hooks are
  already a control plane with three defects: startup only, write only, and
  unobservable. That list is the starting capability inventory for PR 0, not a
  guess about scope.
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
mcp ----------> control

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
- `observer`, `developer`, and `paper` profiles;
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
idempotent
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
requested_at_ms
```

Drawings, presets, strategies, and orders created by an agent must retain that
authorship in application state, the interface, and the journal. Authorship
cannot exist only in an MCP log.

### 5.3 Revisions and concurrency

Snapshots carry a monotonic revision for each module and one revision for the
capture. Future writes accept `expected_revision`. If state changes after an
agent observes it, the operation fails with a structured error instead of
acting on a stale assumption.

### 5.4 Agent-readable errors

Errors use a predictable shape:

```text
code
message
retryable
current_revision
violated_precondition
details
next_steps
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

- Adding a field, a scope, an event kind, or a capability is additive and does
  not bump a version.
- Removing a field, renaming one, changing its unit, or changing what a value
  means is breaking and bumps the capability version.

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
needed, and all three are observation, not authority: the application reports
what the human did and changes nothing on the human's behalf. They belong to
the `observer` profile.

**A resolved cursor.** Not a coordinate pair. The application already knows how
to turn a position into meaning during a frame, and the cursor scope publishes
that meaning:

```text
pane, tab
screen position
slot index, bar timestamp_ms, price
the bar under the cursor: OHLCV, delta, trade count, progress
the order-flow cell under the cursor, when a flow layer is on
the drawing, anchor, or handle under the cursor
the control under the cursor, by the same stable ID the scene uses
```

**A published selection.** Selected drawings, focused pane, active tab, and the
selected row in a trade history or event table. Drawings already carry a
selected index internally; this exposes it, with its ID, through the contract.

**A mark.** One hotkey that appends an event carrying the fully resolved target
above, plus an optional note the user types. This is the primitive that makes
the rest work. It converts "look at this" from a gesture the agent cannot see
into a durable, structured referent the agent can quote back. Marks are
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
split: `observer` covers the observe tier, including the cursor, selection, and
marks of section 6.5, because none of it changes anything. `developer` adds the
annotate tier and cockpit actions. `paper` adds simulated trading. Live trading
has no profile in this plan.

### 9.2 Future writes

- Use `dry_run` whenever an operation can be validated first.
- Require an `idempotency_key` for safe retries.
- Use `expected_revision` to reject stale state.
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

This is what makes "block my entries" a capability the assistant can be trusted
with early, while "let me trade again" stays a decision the user makes.

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
- Leave over-budget requests in the bounded queue or return backpressure.

A request count is the wrong unit. One capture of a wide chart window with
footprints and plot series can cost more than twenty small reads, so a cap of
"N requests per frame" bounds the wrong quantity and still lets a single
oversized capture blow a frame. The executor therefore checks a clock against a
budget and stops, and any capture large enough to exceed the budget is either
paginated by contract, as section 6.2 already requires, or resumed on the next
frame.

The proposed opening budget is 1 ms at p99 with a hard stop well under a 16 ms
frame. The number is calibrated in PR 2 against a measured baseline, but a
number is stated so that "as fast as possible" is never the specification.

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
- Create an initial capability inventory by module. Start from the 89
  `QUANTICK_*` hooks of section 3.1: each one is an existing action with no
  name, no result, and no discovery, so the inventory begins as a real list
  with a migration target rather than as an estimate.
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
durable record of what happened. PR 0 picks one of two answers and writes it
down:

- agent-initiated actions are recorded into the replay session, so a run driven
  by an agent stays reproducible; or
- a session in which an agent acted is documented as not reproducible, and the
  backtest harness refuses to treat it as a golden fixture.

Either is defensible. Leaving it unstated is not, because the first agent action
during a recorded replay would silently break the repository's primary
invariant. Note that the observe tier, including marks, does not raise this
question at all: it changes no input. Only the annotate tier and beyond do.

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

### PR 2: Snapshot registry in the application

**Branch:** `feat/control-observer`

**Rate class:** On demand.

Deliverables:

- Add `crates/app/src/control/`.
- Add a projection registry.
- Add system, workspace, feed, chart, replay, paper, and health snapshots.
- Add the cursor and selection scopes of section 6.5. This is where the
  application gains a pointer model it does not have today, so the work is a
  new concept rather than the exposure of an existing one: resolving a position
  into a slot, a price, a bar, a flow cell, and an object under the cursor.
- Add a paginated chart window.
- Include explicit units and provenance in DTOs.
- Produce a consistent revision for each capture.
- Measure the baseline and calibrate the per-frame budget of section 10.2.
- Do not add a socket yet.

Acceptance criteria:

- A headless test creates `QuantickApp`, changes state through the normal path,
  and verifies the snapshot.
- A two-pane capture preserves focus and provenance correctly.
- Every registered snapshot validates against its own declared schema, checked
  by a test per module, so hand-written DTO mapping cannot drift from the
  contract in silence.
- A resolved cursor over a known bar reports that bar, verified headlessly
  against a fixture rather than by eye.
- No request means no per-frame cost, measured as `frame_cpu_ms` against
  `origin/main` under the method in section 10.3.
- No egui type appears in the wire schema.

### PR 3: Local gateway for the running instance

**Branch:** `feat/control-gateway`

**Rate class:** Startup and infrequent requests.

Deliverables:

- Implement the loopback or IPC endpoint selected by the ADR.
- Publish an instance descriptor.
- Create an ephemeral token.
- Add bounded queues.
- Execute request and response work on the UI thread.
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
- Add optional Streamable HTTP if the selected SDK is mature enough.
- Implement the MVP tools except advanced event and evidence support.
- Add instance selection.
- Add short, self-contained server instructions.
- Mark read-only tools correctly in their annotations.
- Add a local configuration generator or setup assistant.
- Add MCP client smoke tests.

Acceptance criteria:

- Codex and Claude run `describe`, `get_snapshot`, `get_chart_window`, and
  `get_diagnostics` against the same instance.
- The observer profile exposes no write operation.
- Disconnecting a client does not change application state.
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
- Register the hotkey as a named capability, so a mark can also be taken
  without a keyboard.

Acceptance criteria:

- An agent watching `wait_for_change` observes the user select an object and
  take a mark, and can name the bar, price, and object that was marked.
- A mark taken over a footprint cell reports the cell, not only the bar.
- The journal never allocates per trade; market events are aggregated first.
- An expired cursor reports `dropped_before`.
- A parked waiter does not delay any other request, proved by a test that waits
  and reads concurrently.
- Write requests remain impossible.

### PR 5b: The annotate and notify tier

**Branch:** `feat/control-annotate`

**Rate class:** Human or agent actions, never trade or frame frequency.

This is the first tier that writes, and it is deliberately the tier that cannot
lose the user's work. It is also what makes the loop bidirectional: until it
ships, the agent can see the chart and has no way to answer on it.

Deliverables:

- Implement the action registry port with the annotate tier as its first
  consumer, then reuse the same registry in PR 6.
- Add label, arrow, and zone annotations against a resolved chart target,
  attributed to the agent as author and removable in one action.
- Add popup, toast, and sound notification capabilities.
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
- The determinism decision from PR 0 is honoured by every action here, since
  this is the first pull request in which an agent changes session input.

### PR 5c: Evidence bundles

**Branch:** `feat/control-evidence`

**Rate class:** On-demand captures.

Deliverables:

- Add consistent captures and a temporary resource.
- Add redaction, an integrity hash, and a manifest.
- Add the first semantic scene representation.
- Correlate an optional screenshot with the capture revision, so pixel regions
  map to stable IDs (section 8).
- Optionally add an explicit export operation.
- Migrate the `ui-harness` and `visual-qa` skills onto the control plane. Both
  drive the application through environment variables and read it through
  window captures today; after PR 3 they can drive it live and read structured
  state, which is what starts retiring the 89 hooks instead of adding a
  parallel mechanism beside them.

MVP acceptance criteria:

- An agent explains a running session without a screenshot.
- Feed changes, replay changes, indicator changes, and connection errors appear
  through the cursor.
- The bundle reports omitted information and coverage gaps.
- A bundle containing a screenshot maps every named control to a region of that
  image.
- At least one existing validation skill runs against the control plane instead
  of against environment variables.

### PR 6: The cockpit tier

**Initial branch:** `feat/control-actions`

**Rate class:** Human or agent actions, never trade or frame frequency.

The action registry port itself lands in PR 5b with the annotate tier as its
first consumer, so this pull request adds no port. It adds the tier that can
discard work the user did by hand, which is why `expected_revision` and an undo
path stop being optional here. Once the port has carried two tiers, independent
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
- Undo or rollback is available when the human action already supports it.
- An existing hook, if any, calls the same handler.

### PR 7: Paper trading and strategies

**Branches:** One per module after the registry is stable.

Deliverables:

- Add complete snapshots for orders, queued orders, positions, brackets, P&L,
  and the journal.
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
```

Keeping this sequence linear reduces protocol churn. Once the action registry
is stable, indicator, drawing, replay, and workspace actions can be developed
in parallel when they do not share files.

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
future developer profile, a stale action fails, while an accepted action records
its actor and result.

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
| An agent action breaks replay reproducibility | PR 0 records the decision either way |
| Hand-mapped DTOs drift from the contract | Each snapshot validates against its schema |
| One oversized capture blows a frame | A millisecond budget, not a request count |
| A long poll starves other requests | Waiters park off the executor (section 6.4) |
| The risk-reducing exception becomes a loophole | Every outcome must lower risk (section 9.4) |
| Snapshots consume frame time | Build on demand, limit ranges, and serialize off-thread |
| The tool list becomes too large | Use profiles, capability search, and module-based exposure |
| Modules produce an inconsistent snapshot | Capture in one UI-thread pass and attach revisions |
| A client acts on stale state | Require `expected_revision` |
| A retry duplicates an action | Require `idempotency_key` |
| Local data leaks | Require opt-in, local endpoints, ephemeral tokens, and redaction |
| An agent gets a trading shortcut | Reuse UI confirmation and use two phases for live trading |
| DTOs freeze internal models | Keep external DTOs explicit and versioned |
| Refactoring overwhelms `app.rs` | Extract incrementally, one capability per pull request |
| MCP clients diverge | Run Codex and Claude conformance checks for every adapter release |

## 16. Recommended starting decisions

PR 0 should adopt these defaults unless the ADR or threat model finds a
specific reason to change one:

1. **Local first:** Use an authenticated loopback endpoint. Evaluate named
   pipes and Unix sockets in the ADR without blocking the MVP.
2. **Local MCP over STDIO:** Prefer broad client compatibility and simple
   configuration.
3. **Observer by default:** Expose no writes in the first release.
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
   selection, and a mark are part of the MVP. Pointing is what makes every
   later exchange specific.
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

After this plan is reviewed and accepted:

1. Complete PR 0 in the current worktree with the capability inventory seeded
   from the 89 existing hooks, the local transport ADR, the observer threat
   model including the cursor and selection scopes, the tool surface decision,
   the owner of a trade annotation, and the determinism decision.
2. Validate the documentation, complete the architecture review, and merge
   PR 0.
3. Create `feat/control-contract` in its own worktree.
4. Implement only `quantick-control`, the fake host and client, and their tests.
5. Run all four repository checks and the architecture review.
6. Merge the contract before changing the application.

This sequence establishes a small, testable boundary before touching the large
`QuantickApp` state. It also keeps MCP SDK decisions out of domain code.

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
