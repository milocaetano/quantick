# PR 5c evidence: evidence bundles

**Branch:** `feat/control-evidence`, cut from `origin/main`.

**Plan:** [PR 5c](../mcp-control-plane-development-plan.md) §8 · **Roadmap:**
§5.4 · **Contract:** §3 (canonical digests), §5 (`retained_resource`
pagination), §7 (limits), §8 (tool surface) ·
**Threat model:** O-17, O-18, O-21, O-25.

The last item of the MVP. Every other read answers one question; a bundle is
one *instant* — the chart, the feed, the frame cost, the scene and the events
around them, taken together, hashed, and handed back by identifier. It is what
turns "it looked wrong" into something reproducible.

## Rate class and tier

On-demand captures. Nothing here runs unless a client asks:

| Path | Rate | What this change does there |
| --- | --- | --- |
| Aggregator, tick ingest | per trade | nothing |
| Book state, depth projection | per depth update | nothing |
| Renderer, per-frame view | ~60 Hz | one boolean check (`screenshot_armed`) and one `is_empty` on the waiter queue, both in the gateway's own frame service, which only runs while local access is enabled. With none armed, the input scan does not run |
| Evidence capture and chunk read | one per client call | the work of this change |

`evidence_costs_the_frame_nothing_until_a_client_asks_for_it` runs thirty
frames with access enabled and the tier granted and asserts nothing was ever
armed.

**Where the work happens.** The application thread does only what needs
application state: the projection pass (already budgeted and guarded), a
bounded journal read, a copy of the effective configuration, and — when one
was asked for — a move of the frame's pixels. Encoding, PNG compression,
canonical JSON, hashing, chunking and retention all happen after the capture
leaves that thread, on the same response worker that already serializes a
snapshot. That is why the capture is a `DeferredUiRead` like `SnapshotCapture`
and not a value assembled in place.

Reading a bundle back costs the frame nothing at all: `evidence.read` is a
*worker* read. Paging a retained resource needs no application state, so a
client pulling a bundle down chunk by chunk never enters the UI queue.

## The tier the trader grants

| Scope | Opens | Default |
| --- | --- | --- |
| `observe.evidence` | `evidence.capture` and `evidence.read` | prompt (sensitive) |
| `observe.screenshot` | rasterising the window into a bundle | prompt (sensitive) |

Both are declared sensitive and off by default, and both were already declared
in `contract.rs` before this change — a bundle is every granted scope at once
in one durable object, and a picture of the window is its own decision.

**A bundle cannot launder a scope.** `evidence.capture` requires the two
permissions above *plus* every permission the scopes it names require, added
per request as dynamic permissions. Aggregation is not a way in:
`evidence_capture_is_refused_without_its_own_scope_and_cannot_launder_another`
proves the first half, and the redaction test's `interaction.selection` attempt
was refused with `control.scope_denied` naming `observe.paper` — the mechanism
working on a test that was not written to find it.

**A resource identifier is an address, not an authorization.** Every chunk read
rechecks the bundle's own `source_scopes`, recorded in its manifest, against
the connection's current grant, so a scope taken away closes an open resource
mid-read (`a_chunk_read_rechecks_the_grant_the_bundle_aggregated`, threat
O-21/O-25).

## Acceptance: criterion → test

| Roadmap 5.4 criterion | Test |
| --- | --- |
| 1. An agent explains the running session without a screenshot | `an_evidence_bundle_explains_the_session_without_an_image_and_its_events_keep_reading` — connects over the loopback socket, captures, pages the bundle back, verifies the digest, and reads instance, session, build, host, capture revision and the workspace/feed/chart/health/scene projections out of it |
| 2. Feed, replay, indicator and connection changes appear through the cursor | same test: the bundle's `events.next_cursor` is fed straight back into `events.read` and continues the same journal |
| 3. The bundle reports omitted information and coverage gaps | `an_evidence_bundle_names_what_it_omitted_and_why_as_codes_not_prose` — every registered scope the caller did not name is in `omitted_scopes`, the five fixed gaps are present, and every reason is asserted to be a lower-case code rather than a sentence |
| 4. A bundle with a screenshot maps every named control to a region of the image | `a_bundle_with_a_screenshot_maps_every_named_control_to_a_region_of_the_image` — the image carries the scene's own `capture_revision`, the bytes are a real PNG hashed as the descriptor says, every control the scene gave bounds for has a region `within_image`, and every control without bounds is listed with the scene's own reason. `a_capture_that_wants_an_image_waits_for_the_frame_instead_of_answering_blind` proves the capture waits for the window rather than answering without it |
| 5. A validation skill reads and asserts through the live control plane | `ui-harness` gained *Reading the running app through the control plane* (driving `quantick-mcp` over STDIO, and what `coverage` and `screenshot.control_regions` mean); `visual-qa` gained §3, *Ask the app what it believes, then look*, which now takes a structured reading before the pixels and treats a scene/image disagreement as a FAIL. The deterministic fixture is `QUANTICK_CONTROL_EVIDENCE`, proved by `the_evidence_launch_hook_captures_through_the_same_read_a_client_calls` |
| 6. No token, user path, user drawing text or config key in the bundle | `no_token_user_path_user_text_or_redacted_config_key_reaches_an_evidence_bundle` — plants the trader's own note text, a configured journal path, a bridge command naming their home, a routable bind address, and reads the connection's real bearer token out of the published descriptor; then hunts all six through the whole reassembled bundle *and* the manifest |
| 7. Retention and size bounded by named constants | `retention_evicts_by_the_earlier_of_count_bytes_and_age`, `a_bundle_larger_than_its_own_share_is_refused_instead_of_emptying_the_store` (`control.backpressure`), `a_bundle_is_paged_by_its_cursor_and_a_foreign_cursor_is_refused` (`control.cursor_invalid`), `withdrawing_access_forgets_every_retained_bundle`. An expired or unknown bundle is `control.resource_gone` — every code from the existing vocabulary |

## What a bundle carries, against plan §8

| Plan §8 asks for | Where it is |
| --- | --- |
| `evidence_id` and integrity hash | `manifest.evidence_id`, `manifest.content_digest` (canonical JSON v1 digest of the whole document) plus a raw-byte digest per chunk, in order |
| Version, commit, protocol version | `environment.system` — the `system.info` projection verbatim, from the same function that scope publishes, so a bundle and a snapshot cannot disagree about the build |
| Operating system and graphics backend | `environment.system.target_os` / `target_arch`, `environment.graphics_backend` |
| Instance and session IDs | `manifest.instance_id`, `manifest.session_id` (the process nonce) |
| A consistent capture revision | `manifest.capture_revision` — one pass through the projection registry, the same value stamped on the screenshot |
| Workspace, chart window, scene | ordinary registered scopes, captured by name |
| Exact projection data | `snapshot.scopes`, each with its module and schema version |
| Recent events and actions | `events`, a page of the semantic journal with the cursor that continues it |
| Frame, feed, book and worker metrics | the `health.summary` scope |
| Effective configuration with redaction | `configuration`, below |
| Gaps, inferred data and unavailable fields | `coverage`, below |
| An explicit list of what was not captured | `coverage.not_captured` |
| Stays in memory, returned as a resource | bounded store, `retained_resource` cursor; no disk write anywhere in the change |

Two things in that table are deliberately *not* where a reader might look for
them:

**Relevant structured logs.** Not captured, and the bundle says so:
`diagnostic_logs` / `not_captured_in_this_tier`. `observe.diagnostic_logs` is
declared and ungranted; a log scope is its own projection with its own
allowlist, and inventing one inside the evidence module would be exactly the
hand-kept second list this codebase keeps refusing.

**Inferred data.** Reported *in place*, not in `coverage`. A value that is
inferred rather than measured carries its own provenance where it lives — the
aggressor side under `feed.status`, the tick-rule delta under `orderflow.tape`
— because a label separated from its value is a label that drifts. Coverage
answers the other question: what is absent, and why.

## Coverage: derived, never hand-kept

```text
coverage.omitted_scopes      every registered scope the caller did not name
coverage.unavailable_fields  { pointer, reason } per field a projection could not fill
coverage.not_captured        { subject, reason } — what this tier never carries
coverage.complete            false, always, and it says so
```

`unavailable_fields` is *derived*: the walk looks for the
`{ "available": false, "reason": "<code>" }` shape `AvailabilitySnapshot` gives
every scope, and reports each one with the JSON Pointer that finds it. A module
that adds a new unavailable field joins the report without touching
`evidence.rs`. It walks the scope values only — they are already `Value`, so
the walk costs no serialization and never traverses a megabyte of image looking
for a field an image cannot contain.

The fixed gaps are decisions recorded where a reader meets them, rather than
silences they would have to interpret: `diagnostic_logs`,
`user_authored_text`, `configuration_paths`, `disk_export`,
`chart_bars_beyond_the_visible_window`, plus the screenshot's own reason when
there is no image (`not_requested`, `frame_not_delivered`,
`exceeds_evidence_bundle_budget`, `image_encoding_failed`,
`frame_pixels_inconsistent`, `scene_scope_not_captured`).

## Redaction: what the configuration section keeps and drops

A path is the one configuration value that is never about Quantick: it is about
whose computer this is. Ports, provider kinds and flags explain a failure; a
home directory only identifies a person.

| Setting | In the bundle | Why |
| --- | --- | --- |
| `metatrader.listen_addr` | `listen_port` and `listen_host_is_loopback` | the port and whether it is loopback is everything a bridge-will-not-connect investigation needs; the host names the user's network |
| `metatrader.bridge_command` | `bridge_command_configured: bool` | a program path into the user's machine |
| `metatrader.ports` | `symbol_port_count` | |
| `paper.trades_dir` | `trades_dir_configured: bool` | a path |
| feeds | id, name, provider, symbol count and catalogue (bounded), declared layout and bars, whether a bubble preset is set | none of it names the machine |

What is dropped is listed by key in `configuration.redacted_keys`, so a reader
is told a setting exists and was withheld rather than left to conclude it is
unset. User-authored text never reaches a bundle at all: the projections redact
it before evidence sees them, which is why the note canary is absent even
though `analysis.drawings` was captured.

## The screenshot, and why the revision is the whole point

A picture with no names invites the assistant to guess. The image is stamped
with the *same* `capture_revision` as the scene captured in the same pass, and
`screenshot.control_regions` gives each named control its rectangle in that
image, scaled from the logical points the scene reports by the frame's own
`pixels_per_point`. A control the window is clipping is reported with its real
numbers and `within_image: false` rather than trimmed — a clipped control is
exactly what a screenshot gets asked about.

**How the frame is obtained.** A capture that asks for an image and finds none
does not answer without it: the request parks, the window is asked to
rasterise through `ViewportCommand::Screenshot`, and the next frame harvests
the reply *before* the drain — so the scene taken beside it describes the frame
that was photographed, not the one after. The queue is bounded
(`CONTROL_MAX_SCREENSHOT_WAITERS`), each waiter keeps its own request deadline,
and a window that never presents produces a bundle with an honest gap rather
than a hang.

**The visible indicator (threat O-18).** Pixels enter the control plane through
exactly one function, `ControlAccess::accept_screenshot`, and it raises the
acknowledgement-lane notice every time. There is no path that takes a picture
quietly — the test fixture goes through the same door, which is what lets the
notice be asserted at all.

## Blast radius

| Added | Edited |
| --- | --- |
| `crates/app/src/control/evidence.rs` | `contract.rs` — two capability registrations, two prepare handlers, the `UiReadContext` port, `readable_scopes` |
| `docs/control-plane/pr5c-evidence.md` | `gateway.rs` — the store handle, the screenshot slot and its harvest, the deferral, `invoke_local_read`, clearing on disable and exit |
| `schemas/control/evidence-*.schema.json` (5 files) | `mod.rs`, `schema_catalog.rs`, `system.rs` — registration lines |
| | `crates/control/src/limits.rs` — three named bounds; `wire.rs` — `Base64Bytes` |
| | `crates/mcp/src/tools.rs`, `fake.rs`, `server.rs` — `quantick_capture_evidence` and its guards |
| | `crates/app/Cargo.toml` — `png`, already in this crate's tree through `eframe → image → png` |
| | `.claude/skills/ui-harness`, `.claude/skills/visual-qa`, `docs/control-plane/roadmap.md`, `control-contract.md` |

No new crate; nothing depends on `evidence.rs` but the contract that registers
it. `png` is a direct dependency now but not a new one in the dependency
graph — it was already compiled as part of `eframe`'s. It is named directly
because a raster in a format no viewer accepts would make an evidence image
useless to the human reading the bundle, and the alternative was a hand-rolled
encoder in the control path.

## Deliberate deviations

**No named MCP tool for the chunk read.** Contract §8 names
`quantick_capture_evidence` and no companion, and says the long tail uses
`quantick_invoke`. A chunk read is not a path a client walks constantly, so
`evidence.read` is reached through `invoke`, which enforces the same
permissions a named tool would. The capture tool's description says so, and
`EVIDENCE_READ_CAPABILITY` carries the reasoning where a future reader will
meet it.

**`evidence.bundle` is a cursor scope, not a projection scope.** The contract's
`PageCursor` declares a `scope_id`; the value there names the *retained
resource* being walked. No module registers it and no capture builds it, and
the constant says so.

**`read_only: true` on a capability that retains something.** In the effect
policy's sense: a bundle changes no application state, touches no position and
takes nothing away from the trader. What it creates is the answer itself,
bounded by its own named limits, expiring on its own, and gone the moment
access is withdrawn. The threat model already places evidence capture in the
observer's *Allowed* list for the same reason.

## Verification

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace` — see the pull request body.
- Schemas and the capability catalog regenerated
  (`QUANTICK_UPDATE_CONTROL_SCHEMAS=1`); the snapshot tests and the
  no-egui-on-the-wire guard are green with 46 committed documents.
- The MCP tool list is fixed at ten for the observer profile, guarded in
  `tools.rs`, `server.rs` and the STDIO smoke test.

## What this closes

Plan §18's last open criterion, *an evidence bundle can reproduce an
investigation*. With 5.1, 5.2 and 5.3 already on `main`, the MVP is complete
when this merges. The next document is the PR 6 (cockpit) plan, which does not
start before the owner decides on the authority layer of plan §9.2.
