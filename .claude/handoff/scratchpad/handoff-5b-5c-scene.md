# Handoff: what the next PRs build on (5b annotate, 5c evidence, scene + snapshot modules)

Stack as left: `main` ← #213 (PR 2) ← #221 (PR 3) ← #222 PR 4 (`feat/mcp-observer`) ← #223 PR 5a (`feat/control-events`). Merge in order; GitHub retargets each PR when its base lands. Re-stack a child after rewriting its base with `git rebase --onto <new-base> <old-base-head>`.

## PR 5b — annotate and notify tier (`feat/control-annotate`, stack on `feat/control-events`)

Build on:
- `crates/app/src/control/actions.rs` — `ActionRegistry` + `attention.mark.create`: add `annotate.label.create / arrow / zone` (drawings attributed to the actor — the drawing needs an `author`/actor field shown in the inspector and removable in one action: `annotate.remove` by the returned annotation ID), `notify.popup / toast / sound`, `indicator.script.attach / detach`. Every handler is `fn(&mut QuantickApp, &mut ControlAccess, &ActorContext, &Value)`; the mark shows the shape.
- Remote dispatch of actions: today actions run only locally (`QuantickApp::control_action`). For an agent to call them, the gateway's UI drain must execute actions with `&mut QuantickApp`: `ControlAccess::begin_frame(&QuantickApp)` becomes `&mut`, `PreparedDispatch` gains an `Action(...)` variant prepared by `ObserverContract::prepare` when the capability is a registered action (today `prepare` finds no read handler and fails closed), and `execute_on_ui` routes it to `invoke_local_action` with the connection's trusted `ActorContext` (actor_kind `agent`, principal/connection from the handshake). The permission check already exists (`required_permissions ⊆ effective_scopes`).
- Profiles and scopes: `annotator` profile and `annotate` / `annotate.attention` permissions are declared in `contract.rs`; add `annotate.chart`, `annotate.notification`, `annotate.sound` (off by default), `annotate.script`; let the panel grant them (the scope checkboxes are generated from `selectable_permissions`); let the adapter accept `--profile annotator` (`AVAILABLE_PROFILES` in `crates/mcp/src/main.rs`) with the conservative `invoke` hints (already implemented in `tools.rs`).
- Notification rate limits: `CONTROL_NOTIFICATION_RATE_PER_MINUTE` / `CONTROL_NOTIFICATION_BURST` exist in `quantick_control::limits`; the per-client limiter pattern is `ClientRateLimiter` in `gateway.rs`. The `annotate` effect policy has `irreversible_transient_risk: None`; notifications should get their own effect policy (e.g. `notify`) requiring `user_interrupt` / `audible_output` flags, so the mark does not have to lie about interrupting.
- `attach_script`: `quantick_pine::compile` returns `Vec<PineError>` with code/span/message/notes — return them as structured `details`; `IndicatorHost` is headless; the indicators scope (`observe.indicators`) reads the result back.
- Control trace: every annotate action during replay must go through `invoke_local_action` (it already appends intent/result); remote actions from the gateway must too.
- MCP: `quantick_annotate`, `quantick_notify`, `quantick_attach_script` named tools (contract §8) — add to `crates/mcp/src/tools.rs` like `quantick_read_events`.

## PR 5c — evidence bundles (`feat/control-evidence`)

Build on: `SnapshotCapture` (coherent capture with revision), `EventPage`, the describe result, `APP_HEALTH_SUMMARY` fields in `health.rs`; keep bundles in memory (`CONTROL_EVIDENCE_*` limits exist); a `resource` read capability chunked by `CONTROL_MAX_RESPONSE_BYTES`; `observe.evidence` permission already declared (sensitive, Prompt). Screenshot correlation needs the semantic scene first.

## Scene and the remaining snapshot modules

`ProjectionRegistry::register_module` + `register_scope` in `crates/app/src/control/registry.rs` is the port: one file per module (`scene.rs`, `indicators.rs`, `drawings.rs`, `orderflow.rs`, `replay.rs`, `paper.rs`) with a `register(registry)` call in `standard_registry`. Each scope declares its permissions; the health scope shows the DTO conventions (exact decimals, `_unix_ms`, provenance). Events for those modules dock into `emit_semantic_changes` as one more key comparison each.

## Known gaps carried in the PR bodies
- Module revisions are capture-derived (deep compare) — PR 5a's successor should make them change counters driven by the journal, which also gives events a `module_revision`.
- Stale descriptor cleanup and PID-reuse detection (ADR 0001 §5) — the adapter's housekeeping.
- Control trace: the sidecar handle is opened per action with two `sync_data` on the UI thread (per gesture); keep it open per session and move the result sync off the frame once measured (deferred in #223).
- Semantic events for a selected drawing's property change (lock/hide/rename while still selected) no longer fire `interaction.selection.changed` (the emitter compares a zero-allocation identity); a drawing module's own events should carry those.
- The strict p99 capture tests are `#[ignore]`d readings; the always-on guards judge the median.
- Live Codex/Claude check against a running desktop instance was not run in this session (no app-launch authorization); `quantick-mcp setup --client codex|claude` prints the registration commands.
