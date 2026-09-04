# PR 5c evidence: evidence bundles

> **Archaeology, not current state.** This document records what was true
> when it was written and is kept for the reasoning it carries. For what
> has shipped, ask the registry — see [Precedence](../README.md#precedence).

**Branch:** `feat/control-evidence`, cut from `origin/main`.

**Plan:** [PR 5c](../../mcp-control-plane-development-plan.md) §8 · **Roadmap:**
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
| Renderer, per-frame view | ~60 Hz | one boolean check (`screenshot_armed`), one `is_empty` on the waiter queue and one `Option` clear, all in the gateway's own frame service, which runs only while local access is enabled. With nothing armed, the input scan does not run and no pixels are touched |
| Evidence capture and chunk read | one per client call | the work of this change |

`evidence_costs_the_frame_nothing_until_a_client_asks_for_it` runs thirty
frames with access enabled and the tier granted and asserts nothing was ever
armed.

**Where the work happens.** The application thread does only what needs
application state: the projection pass (already budgeted and guarded), a
bounded journal read, a copy of the effective configuration, and — when one
was asked for — a clone of the frame image's handle. The pixel conversion, PNG
compression, serialization, canonical JSON, hashing, chunking and retention all
happen after the capture leaves that thread, on the same response worker that
already serializes a snapshot. That is why the capture is a `DeferredUiRead`
like `SnapshotCapture` and not a value assembled in place, and why the frame's
rows travel as a closure rather than as a `Vec`.

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
permissions above, *plus* every permission the scopes it names require, *plus*
the scopes every bundle carries whatever was asked for. That last clause is
not decoration: a capture always embeds a page of the semantic event journal
and the effective feed configuration, so it always requires `observe.events`
and `observe.market`. Without it, a connection refused the journal could read
it by asking for a bundle of `system.info` — and because the manifest would
not have recorded the scope either, the read-time recheck would not have been
looking for it. Three tests hold the boundary:
`evidence_capture_is_refused_without_its_own_scope_and_cannot_launder_another`,
`a_bundle_requires_the_scopes_it_always_carries_however_few_were_named`, and
the redaction test's `interaction.selection` attempt, which was refused with
`control.scope_denied` naming `observe.paper` — the mechanism working on a
test that was not written to find it.

**A resource identifier is an address, not an authorization.** Every chunk read
rechecks the bundle's own `source_scopes`, recorded in its manifest, against
the connection's current grant, so a scope taken away closes an open resource
mid-read (`a_chunk_read_rechecks_the_grant_the_bundle_aggregated`, threat
O-21/O-25).

## Acceptance: criterion → test

| Roadmap 5.4 criterion | Test |
| --- | --- |
| 1. An agent explains the running session without a screenshot | `an_evidence_bundle_explains_the_session_without_an_image_and_its_events_keep_reading` — connects over the loopback socket, captures, pages the bundle back, verifies the digest, and reads instance, session, build, host, capture revision and the workspace/feed/chart/health/scene projections out of it |
| 2. Feed, replay, indicator and connection changes appear through the cursor | same test: the bundle's `events.next_cursor` is fed straight back into `events.read` and continues the same journal. `a_bundle_carries_the_events_around_the_capture_not_the_oldest_it_holds` pins the window itself — the page ends at the newest event the journal held, not at its oldest, which is the difference between "what just happened" and "the application starting up eleven pages ago" |
| 3. The bundle reports omitted information and coverage gaps | `an_evidence_bundle_names_what_it_omitted_and_why_as_codes_not_prose` — every registered scope the caller did not name is in `omitted_scopes`, the five fixed gaps are present, and every reason is asserted to be a lower-case code rather than a sentence |
| 4. A bundle with a screenshot maps every named control to a region of the image | `a_bundle_with_a_screenshot_maps_every_named_control_to_a_region_of_the_image` — the image carries the scene's own `capture_revision`, the bytes are a real PNG hashed as the descriptor says, every control the scene gave bounds for has a region `within_image`, and every control without bounds is listed with the scene's own reason. `a_capture_that_wants_an_image_waits_for_the_frame_instead_of_answering_blind` proves the capture waits for the window rather than answering without it |
| 5. A validation skill reads and asserts through the live control plane | `ui-harness` gained *Reading the running app through the control plane* (driving `quantick-mcp` over STDIO, and what `coverage` and `screenshot.control_regions` mean); `visual-qa` gained §3, *Ask the app what it believes, then look*, which now takes a structured reading before the pixels and treats a scene/image disagreement as a FAIL. The deterministic fixture is `QUANTICK_CONTROL_EVIDENCE`, proved by `the_evidence_launch_hook_captures_through_the_same_read_a_client_calls` |
| 6. No token, user path, user drawing text or config key in the bundle | `no_token_user_path_user_text_or_redacted_config_key_reaches_an_evidence_bundle` — plants the trader's own drawing text, a note typed at a mark (the journal path, which the projections do not touch), a configured journal path, a bridge command naming their home, a routable bind address, and reads the connection's real bearer token out of the published descriptor; then hunts all seven through the whole reassembled bundle *and* the manifest. `the_event_page_a_bundle_carries_holds_no_operator_prose` pins the redaction itself |
| 7. Retention and size bounded by named constants | `retention_evicts_by_the_earlier_of_count_bytes_and_age`, `a_bundle_past_its_retention_is_gone_even_when_it_is_not_at_the_front`, `a_bundle_larger_than_its_own_share_is_refused_instead_of_emptying_the_store` (`control.backpressure`), `a_bundle_is_paged_by_its_cursor_and_a_foreign_cursor_is_refused` (`control.cursor_invalid`), `withdrawing_access_forgets_every_retained_bundle`. An expired or unknown bundle is `control.resource_gone` — every code from the existing vocabulary. `a_bundle_too_large_for_one_chunk_still_pages_back_over_the_socket` proves the paging itself against an incompressible image, which is the fixture the deflating one hid |

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

Each scope gets its *own* share of the bound. One scope can hold hundreds of
these markers — the scene reports "bounds are not recorded" on nearly every
control it names — and a single shared budget spent in key order let that one
scope fill the report and silently drop the real gaps of every scope sorting
after it, which alphabetically is most of the registry.

The fixed gaps are decisions recorded where a reader meets them, rather than
silences they would have to interpret: `diagnostic_logs`,
`user_authored_text`, `configuration_paths`, `disk_export`,
`chart_bars_beyond_the_visible_window`, plus the screenshot's own reason when
there is no image (`not_requested`, `frame_not_delivered`,
`exceeds_evidence_bundle_budget`, `image_encoding_failed`,
`frame_pixels_inconsistent`, `frame_scale_not_representable`) and the region
list's own when there is an image but nothing to map onto it
(`scene_scope_not_captured`, `scene_scope_not_readable`).

Those last two are one distinction and it matters: a scene that is sitting
populated in the same document but could not be read back is reported as
unreadable, never as uncaptured. Telling a reader a scope was not captured
while it is right there would be a false statement made by the very section
that exists to be honest about what is missing.

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
unset.

**User-authored text takes two paths out, and both are closed.** The
projections strip it before it reaches a snapshot, which is why the drawing
canary is absent even though `analysis.drawings` was captured. But a bundle
also embeds a page of the *journal*, and the journal records prose verbatim —
the note a trader types at a mark, the message an assistant sends, the words on
a label — so the event page is stripped on its way into the bundle as well.
Redacted unconditionally rather than per grant: `events.read` still serves the
prose to a client holding `observe.events`, unchanged; what a bundle must not
become is the one durable object where every scope's text is gathered
together. The marker says redacted rather than empty, so a reader can tell a
withheld note from a mark that never had one.

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
(`CONTROL_MAX_SCREENSHOT_WAITERS`), and a window that never presents produces a
bundle with an honest gap rather than a hang.

Three rules keep that honest, each of which the review found missing:

- **An image is worth exactly one frame.** Whatever no capture claimed by the
  end of `begin_frame` is dropped. A frame harvested for a capture that had
  already timed out would otherwise sit there and be handed to an arbitrarily
  later one, which would stamp it with *its* revision and scale *its* scene
  onto a picture of a different chart — the one thing the capture revision
  promises cannot happen. It also returns the framebuffer instead of parking
  tens of megabytes for the rest of the session.
- **The request is re-sent while anyone waits.** A viewport command the
  platform swallows — minimised, occluded, between viewport states — used to
  latch the arming flag, and no command was ever sent again for the rest of
  the session while every later capture parked until its deadline.
- **Giving up happens before the deadline, not at it.**
  `CONTROL_SCREENSHOT_GRACE_MS` is the room the honest answer needs: the
  dispatcher refuses an expired request before running anything, so a capture
  that waited to the last millisecond could only ever have answered
  `control.timeout` — no bundle, no gap, and none of the text it could have
  collected all along.

**Where the pixels are paid for.** The frame clones the image's handle and
nothing else; the rows are converted by a closure the response worker calls,
beside the PNG encoding. Copying a 4K framebuffer element by element on the
application thread is eight million iterations inside a budget measured in
microseconds. The conversion unmultiplies alpha, because the toolkit stores
colours premultiplied and a PNG's are not.

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

## What the architecture review changed

Step 0 (`code-review` at `max`) ran over the branch and returned fifteen
findings, every one of them confirmed. Six were things that would have shipped
broken, and they are worth listing because each was invisible to the tests that
existed:

1. **No bundle over ~192 KiB could ever be read back.** The chunk size was
   picked against `CONTROL_MAX_RESPONSE_BYTES` when the bound that binds a
   chunk is `CONTROL_MAX_STRING_BYTES` — a chunk is one base64 *string*, and
   the codec prescans every string in every frame. Every page of every real
   bundle would have come back `control.payload_too_large`. Hidden because the
   screenshot fixture was a smooth ramp that deflated to a few kilobytes.
2. **The bundle laundered `observe.events` and `observe.market`**, as above.
3. **The event page was the wrong page** — the oldest the journal held rather
   than the newest.
4. **A picture could be stamped with a later capture's revision**, which is
   the correlation the whole tier is built on.
5. **One swallowed viewport command disabled screenshots for the session.**
6. **The give-up path answered `control.timeout`** where its own comment
   promised a bundle with a gap.

Two more were inert-by-omission: `serialize_ui_result` discarded the
`ControlError` the widened `DeferredUiRead` signature exists to carry, so no
client could ever see `control.backpressure`; and an oversized image failed the
whole capture instead of being dropped for the text, which is what this
document, the module and the limits all said happened.

The rest were quality: the coverage budget above, four ways the launch hook
could misbehave, two second copies of things the repo already owned (the
`host:port` splitter, and `evidence.read` declared in the adapter with no test
or fake arm), the double canonicalisation, the published scale not reproducing
the regions it was used to build, and the framebuffer copy on the application
thread.

A second pass over the opened pull request found five more, all confirmed and
all fixed:

1. **The event page carried the operator's own words** while `coverage`
   claimed user text was redacted — a false statement by the section whose job
   is honesty, and a hole in criterion 6 that the original canary could not
   find because it planted a *drawing* note, which the projections already
   strip. The redaction test now plants a mark note too, and fails without the
   fix.
2. **A capture still encoding when access was withdrawn was retained anyway.**
   The ingredients are collected on the application thread and the bundle is
   built on a response worker milliseconds later, so an insert could land
   after `clear()` and sit for its full retention in a store emptied
   *because* the grant behind it was gone. The store now carries an epoch.
3. **Coverage could report a truncation that never happened** — the budget was
   checked at the top of the walk rather than where a field is added, so a
   scope that fitted exactly but still had a scalar queued claimed fields were
   dropped.
4. **The screenshot waiters ran outside the frame budget**, so a frame could
   execute eight projection passes against a documented ceiling of four and
   leave the drain with nothing to spend.
5. **The launch hook raised the screenshot notice before the scope check**, so
   asking for an image without `observe.screenshot` told the trader their
   window had been captured on the way to refusing it — weakening the one
   indicator `visual-qa` now asserts on.

Nothing was deferred from either pass.

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
