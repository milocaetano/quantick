# Quantick control plane and MCP development plan

**Status:** Accepted for phased implementation; PR 0 and PR 1 merged, PR 2–5b
implemented and landing on `main` together as one stack. The delivery record
and the earlier revisions live in git history.

**Date:** 2026-08-19

**History:** Codex produced the first draft. Claude Code reviewed and expanded
it on 2026-08-19 at the repository owner's request. The PR 0 contract review
then reconciled the plan with the measured source inventory, authority model,
transport ADR, and observer threat model.

**Why it was revised:** The vendor-neutral control-plane spine was retained, but
the first draft did not yet satisfy the workflow it was written for. The owner
wants to point at something on a running chart and be understood, and to have
the assistant answer on the chart rather than only in prose. A read-only
snapshot API cannot do both.

**Primary goal:** Let Codex, Claude, and other MCP-compatible clients observe a
running Quantick instance, understand what the user is pointing at inside it,
answer on the chart as well as in prose, and collect reproducible evidence
without relying on screenshots.

**Repository language:** English. Code, schemas, documentation, examples,
public messages, and contributor-facing artifacts must be written in English.

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

No pointer model existed when this plan was written. Three pieces are
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

"Idle" needs a precise definition here. As measured when this plan was written,
the application repaints unconditionally every 16 ms, so there is no idle application to hide cost in.
Idle means only that no request is pending and no semantic event is being
recorded, and the acceptance condition in that state is exact: no new
allocation, no new lock, and no measurable change in `frame_cpu_ms` against
`origin/main`.

## 19. References

- [PR 0 control-plane contract](control-plane/README.md): Normative decisions,
  capability inventory, transport ADR, and observer threat model.
- [`CLAUDE.md`](../CLAUDE.md): Architecture, determinism, dependency direction,
  and the "operable without a hand" rule.
- Local architecture review (the `arch-review` skill, since retired): The
  second-operator contract, including Act, Read, Discover, authorship, and
  authority.
- [New extension guide](../.claude/skills/new-extension/SKILL.md): Docking
  ports, performance budgets, and the definition of a modular extension.
- [`ChartState`](../crates/chart/src/state.rs): Existing headless chart state.
- [Feed protocol](../crates/feed/src/lib.rs): Existing events, commands,
  notices, and capabilities.
- [Drawing registry](../crates/app/src/drawings/mod.rs): An existing shared,
  extensible registry.
- [OpenAI Model Context Protocol documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli):
  STDIO and Streamable HTTP transports, plus MCP configuration in Codex.
- [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp): Local
  STDIO registration, scopes, and server verification.
