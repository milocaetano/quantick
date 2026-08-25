# Control plane roadmap: what shipped, what is open, what is left

**Status:** Living document. Updated when a control-plane pull request opens,
merges, or is retired.

**Date:** 2026-08-24

**Owner document:** the [development plan](../mcp-control-plane-development-plan.md)
owns scope and ordering; the [control contract](control-contract.md), ADR 0001
and the threat model own implementation detail. This file is the ledger between
the two: which plan item is at which stage, in which pull request, with which
gaps carried forward. It adds no new decisions.

## 1. Where the plan stands

| Plan item | Branch | PR | State |
| --- | --- | --- | --- |
| PR 0: contract and inventory | `docs/mcp-control-plane-plan`, `docs/mcp-control-plane-contract` | merged | done |
| PR 1: `quantick-control` crate | `feat/control-contract` | merged | done |
| PR 1 hardening (handshake, idempotency, codec, schema guards) | `fix/control-contract-hardening` | [#220](https://github.com/milocaetano/quantick/pull/220) | open, base `main` |
| PR 2: projection registry, core scopes, cursor, chart window | `feat/control-observer` | [#213](https://github.com/milocaetano/quantick/pull/213) | open, base `main` |
| PR 3: local gateway, `quantick-control-local` | `feat/control-gateway` | [#221](https://github.com/milocaetano/quantick/pull/221) | open, stacked on #213 |
| PR 4: `quantick-mcp` STDIO adapter | `feat/mcp-observer` | [#222](https://github.com/milocaetano/quantick/pull/222) | open, stacked on #221 |
| PR 5a: events, cursor, human mark, action registry, control trace | `feat/control-events` | [#223](https://github.com/milocaetano/quantick/pull/223) | open, stacked on #222 |
| Snapshot modules: analysis, order flow, session | `feat/control-snapshots-*` | — | not started |
| Semantic scene | `feat/control-scene` | — | not started |
| PR 5b: annotate and notify tier, `attach_script` | `feat/control-annotate` | — | not started |
| PR 5c: evidence bundles | `feat/control-evidence` | — | not started |
| PR 6: cockpit tier | `feat/control-actions` | — | not started; gated on the owner's §9.2 authority decision |
| PR 7: paper trading and strategies | one branch per module | — | not started |
| PR 8: public API, further adapters | — | — | not started |

The MVP definition of done (plan §18) closes with PR 5c. Everything from PR 6
on is post-MVP.

## 2. Merge order for the open stack

The four application pull requests are stacked and GitHub retargets each one to
`main` when its base lands, so they merge in order:

1. #213 (`feat/control-observer`)
2. #221 (`feat/control-gateway`)
3. #222 (`feat/mcp-observer`)
4. #223 (`feat/control-events`)

#220 is independent of the stack on paper but not in the tree: it rewrites
`crates/control/src/codec.rs` and `crates/control/src/schema.rs`, and so does
the stack (typed handshake reads, the pre-authentication codec ceiling, the
schema catalog). Merged in either order, the second one conflicts in those two
files. The cheaper resolution is to merge the stack first and rebase #220 on
the new `main`: #220 is eleven files and one author, the stack is fifty and
four pull requests. Its two public signature changes (`to_base64url` returning
`Zeroizing<String>`; `read` and `decode_frame` no longer `pub`) touch nothing
the stack calls, so the rebase is a textual merge of the two files, not a
redesign. After the rebase, #220 still needs the architecture review its body
left blank.

Re-stacking rule when a base is rewritten (a rebase or a fixup on #213, say):
the child carries the base's old commits, and a plain `git rebase <base>`
replays them and conflicts in files the child never touched. Use
`git rebase --onto <new-base> <old-base-head> <child>` with the base's head
noted *before* rewriting it.

## 3. What each open pull request carries forward

Every deferred item below was stated in the pull request body with its reason;
none is hidden. They are listed here so the next pull request that touches the
area picks them up instead of rediscovering them.

### #213 (PR 2)

- Module revisions are capture-derived: each requested scope is built and
  deep-compared with the previous capture, so every scope is materialised
  twice per capture. Measured at 22–28 µs p99 for all seven scopes, one tenth
  of the 250 µs budget, on demand only. The successor to PR 5a replaces the
  key projections with journal-driven change counters, which is also what
  gives events a `module_revision`.
- The strict p99 capture guard is an `#[ignore]`d reading; the always-on
  guard judges the median of the best of three batches.

### #221 (PR 3)

- One response thread per UI-bound request (bounded by 8 in flight × 8
  connections) and a 5 ms accept-loop poll while enabled — gateway threads
  only, idle-frame cost measured as nil.
- Stale descriptors of a crashed instance are listed until a connect fails;
  PID reuse is not detected. ADR 0001 §5 permits cleanup rather than requiring
  it; the adapter's housekeeping owns it.
- Requests queued when the user disables access wait out the request timeout
  instead of being refused at once.
- A client that connects and sends nothing until the handshake timeout is
  rejected as `control.invalid_request`, not `control.timeout`.
- `effective_limits.default_page_items` / `max_page_items` are protocol-level
  defaults while `chart.window.read` caps its page at 32; the per-capability
  ceiling lives in the descriptor.
- Visual QA of the Local agent access panel was not run (no authorization to
  launch the desktop app); `QUANTICK_CONTROL_PANEL=1` / `QUANTICK_CONTROL_ACCESS`
  reach it.

### #222 (PR 4)

- `quantick_get_scene` is not in the tool list because the semantic scene does
  not exist yet; it lands with the scene module (§5.2 below).
- The instance listing connects and handshakes every advertised instance to
  prove liveness; a lazy variant belongs with stale-descriptor cleanup.
- The live Codex / Claude Code check against a running desktop instance was
  not run; `quantick-mcp setup --client codex|claude` prints the registration
  commands. The first session with authorization to launch the app runs it and
  records the result in the PR 4 evidence document.

### #223 (PR 5a)

- Typing a note at the mark hotkey: the hotkey marks at once; the note travels
  through the hook and the action input. An inline prompt is a UI affordance
  on the same action and can land alone.
- Per-event module revisions (see #213).
- Indicator, drawing, order/position and health events: the emitter is one
  in-place comparison per key; the remaining keys dock one line each when
  their modules register scopes.
- The control-trace sidecar is opened per action with two `sync_data` on the
  UI thread — per gesture, never per frame. Keep the handle open per session
  and move the result sync off the frame if a slow-disk measurement says it
  matters.
- `interaction.selection.changed` no longer fires for a property change of the
  selected drawing (lock, hide, rename): the emitter compares a
  zero-allocation identity. The drawings module's own events carry those.
- An action that resolves state at call time (a mark without a `target`
  resolves the pointer) has its `canonical_input` traced *before* the handler,
  so a remote caller that omits the target would leave an intent replay cannot
  reproduce (replay refuses it today). PR 5b gives the action port a `resolve`
  step so the trace records the effective input; label, arrow and zone need it
  too.
- The mark result digest still carries the journal `sequence`, so it depends
  on the mark's place in the journal; PR 5c decides which result fields are
  identity for fixture comparison.
- Visual QA of the Ctrl+M gesture was not run; `QUANTICK_CONTROL_MARK` reaches
  the action.

## 4. Base for the next work

Until the stack merges, a new branch is cut from `feat/control-events` and its
pull request targets that branch; when the stack lands, the next branch is cut
from `main` as CLAUDE.md requires. The pull request body says which.

Read first, in this order: the bodies of #221, #222 and #223; the
[control contract](control-contract.md) (§2.6, §5, §8, §11 are the vocabulary
— no package invents a new capability ID, permission, effect or error code);
[ADR 0001](adr-0001-local-transport-and-instance-discovery.md); the
[observer threat model](observer-threat-model.md); and, while they still live on
the stack's branches, `pr2-performance.md`, `pr3-gateway-evidence.md`,
`pr4-mcp-evidence.md` and `pr5a-events-evidence.md` in this directory.

## 5. Remaining MVP work

```text
5.1 snapshot modules (analysis, orderflow, session) ─┐
5.2 semantic scene                                   ├─→ 5.4 evidence bundles (PR 5c)
5.3 annotate / notify / attach_script (PR 5b) ───────┘
```

5.1, 5.2 and 5.3 can run in parallel — one worktree and one agent each,
disjoint files (a module file plus a registration line; a scene module; the
action registry). The one bridge: 5.3's "attach is readable in the indicators
scope" criterion needs 5.1's `indicators` module. If 5.3 lands first, its body
names that criterion as a gap and the proof goes in the 5.1 pull request. 5.4
comes last because it consumes the scene, the modules and the events.

Each item is one pull request (5.1 is three small ones). None is a large pull
request: the blast radius of each is a new file plus registration lines.

### 5.1 Remaining snapshot modules (three pull requests)

Branches: `feat/control-snapshots-analysis`, `feat/control-snapshots-orderflow`,
`feat/control-snapshots-session`.

Depends on nothing. Blocks 5.4 (evidence acceptance) and the read-back
criterion of `attach_script` in 5.3.

Where to dock (the port exists; a module only registers):

- `crates/app/src/control/registry.rs`: `ProjectionRegistry::register_module`
  and `register_scope`; the canonical list is `standard_registry()` in the
  control `mod.rs` — one line per module. The `system.health` scope
  (`crates/app/src/control/health.rs`) is the DTO exemplar: exact decimals as
  strings, `_unix_ms` suffix, declared provenance, no egui type.
- Each scope declares the permissions it requires (`observe.*` today); the
  `observer` profile and the safe default grant live in `contract.rs`.
- Events: a module whose state changes gets one comparison in
  `emit_semantic_changes` (`gateway.rs`), in the mould of `TabKey` — compared
  in place, allocating only when something changed. The #223 review removed
  the version that allocated per frame; do not bring it back.
- Schemas: `schemas/control/*.schema.json` plus the catalog, regenerated with
  `QUANTICK_UPDATE_CONTROL_SCHEMAS=1 cargo test -p quantick-app observer_schemas_are_versioned_valid_and_ui_framework_free`
  and `… observer_capability_catalog_is_registry_derived_and_versioned`; the
  snapshot test fails when this is forgotten.

Modules:

- **analysis** — `indicators` (the headless host: descriptor, effective inputs,
  current series readings per pane, pending compile errors) and `drawings`
  (objects per pane and side with stable ID, tool, band, this/all-charts scope,
  `locked` / `hidden`, author — see 5.3 — and user text *not* leaked, as
  `observer_resolves_mirrored_drawings_without_leaking_user_text` already
  proves).
- **orderflow** — `tape`, `footprint`, `bubbles`, `heatmap`, `l2`: only what
  the chart already computes, never recomputed in the snapshot; page limits as
  named constants in `quantick_control::limits`; inferred side labelled as
  such (delta is tick rule; venue bid/ask are band limits).
- **session** — `replay` (session, position and elapsed, playing/finished,
  speed, trace present and complete) and `paper` (position, orders, history
  with `paper_trading_session_ledger` provenance).

Acceptance per pull request — plan PR 2's criteria applied to the module:

1. A headless test creates `QuantickApp`, changes state through the normal
   path, and verifies the snapshot.
2. A two-pane capture preserves focus and provenance.
3. Every scope validates against its declared schema by test
   (`observer_modules_project_headless_state_that_matches_their_schemas`
   covers what is registered; the new module joins it).
4. No egui type on the wire
   (`observer_schemas_are_versioned_valid_and_ui_framework_free`).
5. No request, no per-frame cost: `control_idle_dense_replay_benchmark`
   (candidate × control pairs under one window of conditions, numbers in the
   body) and the `observer_*_stays_within_the_ui_budget` guards (median; p99 is
   an `#[ignore]`d reading).
6. One journal event per relevant module change (`replay.state.changed` is the
   exemplar; indicators: attach, detach, compile error; drawings: created,
   removed, edited, with author).
7. Blast radius in the body: new file(s), registration line, schemas.

### 5.2 Semantic scene

Branch: `feat/control-scene`.

Depends on nothing. Blocks 5.4 (screenshot correlation) and the adapter's
`quantick_get_scene` tool.

What it is (plan §6.3): the tree of what is on screen without rasterising —
visible controls, label and an ID stable across frames, enabled/selected
state, the unavailability reason as data (never rendered text), bounds where
useful, owner (panel, dialog, tab, pane), and the related registered
capability.

Where to dock: a `scene.rs` module registered through `register_module` like
the 5.1 modules. The source of controls is the same place that already feeds
the `ui-harness` hooks (the `DRAWING_TOOLS` rail, the toolbar, the panels) —
one registry, never a parallel "for the agent" list (a hand-kept list beside a
registry is an `arch-review` finding). The cursor (§6.5) already returns the
control under the pointer by ID; the scene uses the same ID.

Acceptance:

1. Stable IDs across frames, proved by test (two frames, same tree).
2. Explicit unavailability reasons without text parsing
   (`AvailabilitySnapshot { available, reason }` is the mould).
3. A control the scene names is the one the cursor returns when pointing at it
   (cross-test with `observer_cursor_*`).
4. `quantick_get_scene` enters `crates/mcp/src/tools.rs` (one `Tool` entry, one
   arm in `tools::call`, embedded schema, the `ErrorResponse` arm of the
   `oneOf` like the others) and
   `the_tool_list_is_fixed_and_named_as_the_contract_says` is updated.
5. Criteria 3, 5 and 7 of 5.1 (schema, no egui, no cost without a request,
   blast radius).

### 5.3 PR 5b: annotate, notify and `attach_script`

Branch: `feat/control-annotate`.

Depends on #223 (the `ActionRegistry`, the declared `annotator` profile, the
control trace). Blocks 5.4.

The first tier that writes, and deliberately the one that cannot lose the
user's work (plan §2.6: annotate means the user can undo; nothing cockpit,
nothing financial). Rate class: human or agent action, never per trade or per
frame.

Where to dock (everything exists as a port or a declaration):

- `crates/app/src/control/actions.rs` — `ActionRegistry::register(descriptor, handler)`
  with handler
  `fn(&mut QuantickApp, &mut ControlAccess, &ActorContext, &Value) -> Result<Value, ControlError>`;
  `attention.mark.create` (`create_mark`) shows the full shape (descriptor,
  schemas generated from structs, input/output validation, journal event with
  `actor` and `target_source`). `ANNOTATE_EFFECT_ID`, `ANNOTATE_PERMISSION_ID`,
  `ANNOTATE_ATTENTION_PERMISSION_ID` and `ANNOTATOR_PROFILE_ID` are declared.
- Remote dispatch of actions (today actions run only locally through
  `QuantickApp::control_action`): `ObserverContract::prepare` (`contract.rs`)
  gains a `PreparedDispatch::Action(...)` when the capability is a registered
  action and `required_permissions ⊆ effective_scopes` (the check exists for
  reads); `execute_on_ui` (`gateway.rs`) routes to
  `ControlAccess::invoke_local_action(app, id, input, origin)` with an
  `ActionOrigin::Remote` carrying the connection's *trusted* `ActorContext`
  (actor kind `agent`, principal from the handshake). `begin_frame(&QuantickApp)`
  becomes `&mut QuantickApp`; `app.rs` already takes and puts back
  `control_access`, so the `&mut` is cheap. `invoke_local_action` already
  writes the trace (§11); remote actions go through the same path — that is
  the "every action uses the control trace" criterion.
- Profiles and scopes: `annotator` exists; add `annotate.chart`,
  `annotate.notification`, `annotate.sound` (off by default) and
  `annotate.script`. The access panel generates its checkboxes from
  `selectable_permissions` (nothing to draw by hand); the adapter accepts
  `--profile annotator` in `crates/mcp/src/main.rs` (`AVAILABLE_PROFILES`),
  with the conservative `invoke` hints `tools.rs` already implements.
- Limits: `CONTROL_NOTIFICATION_RATE_PER_MINUTE` / `CONTROL_NOTIFICATION_BURST`
  are in `quantick_control::limits`; the per-client limiter is
  `ClientRateLimiter` (`gateway.rs`) — reuse the pattern, do not duplicate it.
  Give notifications their own effect policy (`notify`) with the
  `user_interrupt` / `audible_output` risk flags, so the mark never claims to
  interrupt.
- `attach_script`: `quantick_pine::compile` returns `Vec<PineError>` with
  `code`, `span`, `message`, `notes` — they go out as structured `details` of
  the error (`ControlError` has `context.details`); `IndicatorHost` is
  headless; attach and detach go through the *same* named function the UI
  uses to add an indicator to a pane (the button calls it — act/read/discover),
  and the result is read back through the `indicators` scope (5.1).
- Drawings: an agent's annotations are drawings with an author field (actor
  kind plus client name) visible in the inspector and the context bar — an
  agent object indistinguishable from the trader's is a Blocker — and
  removable in one action by the user (`annotate.remove` by ID, plus one UI
  gesture removing every annotation by that author).
- The `resolve` step carried from #223: canonicalise the input before the
  intent line so the trace records the effective input for label, arrow and
  zone as well as the mark.
- MCP: the contract's named tools — `quantick_annotate`, `quantick_notify`,
  `quantick_attach_script` (plus detach) — in `crates/mcp/src/tools.rs`, as
  `quantick_read_events` was added in #223 (embedded schema, `ErrorResponse`
  in the `oneOf`, hints per profile).

Acceptance (plan PR 5b plus `arch-review`'s second operator):

1. The same handler serves the UI and the agent, shown in the pull request
   (test: the function called by the gesture and by a remote `invoke` under
   the `annotator` profile produces the same event/object, attributed to
   different actors).
2. An agent-created annotation is visibly attributed and the user removes it
   in one action (headless test plus a `ui-harness` hook for the capture;
   `visual-qa` with the owner's authorization, otherwise BLOCKED in writing).
3. A failed compile returns `PineError` code, span and notes as structured
   data, never a rendered string.
4. A successful attach is readable in the `indicators` scope; detach restores
   the prior state exactly (snapshot before == snapshot after).
5. No capability in the tier discards user-created state or touches a
   position — reviewed against the §2.6 table, stated in the body.
6. Notification flood tests prove per-client rate and burst; a client without
   `annotate.sound` produces no audio.
7. Every remote action appears in the control trace during replay (test in
   the mould of
   `a_mark_during_replay_is_traced_and_replayed_at_the_same_logical_time`:
   re-injection with `target_source: replayed`, actor `automation`).
8. Observer still reaches none of it
   (`gateway_observer_cannot_create_a_mark_remotely_…` gains a sibling per new
   action).
9. `ui-harness` hooks for every new surface (author in the inspector, removal,
   toast/popup) — a row in the skill's table, like `QUANTICK_CONTROL_MARK`.

Out of scope: cockpit (PR 6), paper and strategies (PR 7), disk export, any
market write.

### 5.4 PR 5c: evidence bundles

Branch: `feat/control-evidence`.

Depends on 5.1, 5.2 and 5.3. Rate class: on-demand captures.

What it is (plan §8): a coherent capture — `evidence_id` plus integrity hash,
version/commit/protocol, OS and graphics backend, instance and session IDs, a
consistent capture revision, workspace, chart window and scene, exact
projection data, recent events and actions, relevant structured logs,
frame/feed/book/worker metrics, effective configuration with redaction, gaps,
inferred data and unavailable fields, and the explicit list of what was not
captured. It stays in memory under bounded retention and is returned as a
resource; disk export is a separate cockpit action outside this item.

Where to dock: `SnapshotCapture` (coherent capture with revision) and
`EventPage` exist; `APP_HEALTH_SUMMARY` / `health.rs` hold the metrics;
`CONTROL_EVIDENCE_*` are in `limits`; the `observe.evidence` permission
(sensitive, Prompt confirmation) is declared in `contract.rs`; the resource
read is a read capability paged by `CONTROL_MAX_RESPONSE_BYTES` with the
`retained_resource` cursor (`PaginationConsistency::RetainedResource` exists in
the contract). An optional screenshot is stamped with the *same* revision as
the scene, so every pixel region maps to a control or object ID — which is why
5.2 comes first.

Acceptance (plan PR 5c = MVP acceptance):

1. An agent explains the running session without a screenshot (true since
   #222; the bundle packages the same information).
2. Feed, replay, indicator changes and connection errors appear through the
   cursor (`events.read` / `events.wait`): the 5.1 events feed the bundle.
3. The bundle reports omitted information and coverage gaps.
4. A bundle with a screenshot maps every named control to a region of the
   image.
5. At least one existing validation skill (`ui-harness` or `visual-qa`) reads
   and asserts through the live control plane; the fixture may still come from
   a deterministic hook until the equivalent action exists in PR 6.
6. Redaction: no token, user path, user drawing text or config key in the
   bundle — a test looks for them.
7. Retention and size bounded by named constants; overflow is
   `control.backpressure` / `control.resource_*` from the existing vocabulary.

## 6. Gates for every pull request in section 5

Beyond CLAUDE.md's four checks, `arch-review` with step 0 `code-review <PR> high`,
the `pr-gate` marker written before `gh pr create`, and CI watched with
`gh pr checks <n> --watch`:

- Schemas and catalog regenerated and versioned; snapshot tests green; no egui
  type on the wire.
- Performance by rate class in the body; hot paths measured
  (`control_idle_dense_replay_benchmark` in pairs under one window; budget
  guards by the median). No request, zero per-frame cost.
- Second operator (`arch-review` dimension 7): every action is a named call
  with schemas and a registered ID, a readable result, discoverable through
  `describe`; authorship recorded and visible; no market or safety action
  reachable by a path shorter than the trader's (Blocker).
- `workspace_deps` and `CLAUDE.md` if a crate appears (none should: everything
  here docks in `app`, `mcp` and `control`).
- `ui-harness`: every new surface gets its hook in the same change;
  `visual-qa` / `trader-ux-review` only with the owner's authorization to
  launch the app — otherwise BLOCKED in writing in the body, never skipped in
  silence.
- Test names in the house style (a declarative sentence with a contrast), and
  every confirmed review finding gets a test that fails without the fix.
- Pull request body in the mould of #221–#223: *Summary · Rate class and tier ·
  Docking / blast radius · Acceptance (criterion → test table) · Deferred ·
  Verification · Architecture review (step 0 plus shape)*.

## 7. MVP closure map (plan §18)

| §18 criterion | Closes with |
| --- | --- |
| Opt-in local access; Codex and Claude connect through MCP; documented read schemas; explain the session without a screenshot; point at a bar, cell or object and have it named; follow changes with a cursor; no cockpit or financial write; per-frame budget enforced and measured; idle hot paths untouched; health metrics flat; checks, review and CI green | the open stack (#213 → #223) |
| The agent answers on the chart, authorship visible, removable in one action | 5.3 |
| An indicator described in prose, compiled, corrected from diagnostics, attached | 5.3, read back through 5.1 |
| Feed, replay, indicator and connection changes through the cursor | 5.1 |
| `quantick_get_scene` | 5.2 |
| An evidence bundle reproduces an investigation | 5.4 |

When 5.1–5.4 are merged the plan's MVP is complete. The next document is the
PR 6 (cockpit) plan, which does not start before the owner decides on the
authority layer of plan §9.2; PR 7 (paper trading, strategies, the trade
annotation store of §6.6) and PR 8 (public API, further adapters) keep the
plan's order and their threat-model extensions.
