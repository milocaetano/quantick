# Mission: roadmap 5.1 — the remaining snapshot modules

**Objective:** an agent can read indicators, drawings, order flow, replay and
paper through the control plane's snapshot scopes.

**Branch:** `feat/control-snapshots`, base `origin/feat/control-annotate`
(9ae2c89).

**Recorded deviation.** The brief said "cut from `origin/main`". It cannot be:
`emit_semantic_changes`, `gateway.rs`, `journal.rs`, `crates/mcp` and
`crates/control-local` do not exist on `origin/main` — PRs #221/#222/#223/#231
each merged into the branch below them, never into the trunk. Base is the stack
tip, per the roadmap's own stacked-base rule (section 4). Owner chose this.

**Out of scope, by owner's instruction:** landing the stack on `main` — that
belongs to the `feat/control-annotate` session. **Open exactly one PR, against
`feat/control-annotate`. Never merge. Never push to `feat/control-annotate`.**

## The nine scopes

`register_scope` requires `scope_id` to start with `<module_id>.`.

| Module | Scopes |
| --- | --- |
| `analysis` | `analysis.indicators`, `analysis.drawings` |
| `orderflow` | `orderflow.tape`, `.footprint`, `.bubbles`, `.heatmap`, `.l2` |
| `session` | `session.replay`, `session.paper` |

## The port (exists; a module only registers)

`crates/app/src/control/registry.rs`:

- `register_module(ModuleDescriptor { id, title, description }, revision)`,
  `revision: fn(&QuantickApp) -> K`, `K: Eq + Send + 'static`. The registry
  diffs this key between captures, so it must exclude anything moving every
  frame (`health.rs::revision` drops the frame averages for that reason).
- `register_scope(scope_id, module_id, schema_version, title, description,
  &[permission ids], project)`,
  `project: fn(&QuantickApp, CaptureContext) -> T`,
  `T: JsonSchema + Serialize + Send`. Schema is generated from `T`.
- One line in `standard_registry()` (`control/mod.rs`).

## DTO rules (exemplar `control/health.rs`)

- Exact decimals as strings: `CanonicalDecimal` via `types.rs` helpers
  `canonical_decimal` / `canonical_u64` / `canonical_i64` / `canonical_f64` /
  `canonical_f32(v, places)`; counts `WireU64` / `wire_usize`.
- Timestamps: `_unix_ms` suffix +
  `#[schemars(extend("x-unit" = "unix_milliseconds"))]`; durations carry their
  own `x-unit`.
- Derive `Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema`.
- No egui type on the wire. No user drawing text on the wire.
- Provenance declared, not implied (`paper_trading_session_ledger`).
- Inferred data labelled: delta is the tick rule; venue bid/ask are band limits.
- Page limits named in `quantick_control::limits`.

## Where each scope's data already lives (never recompute in a capture)

- **indicators** — `pane.indicators.all()` -> `IndicatorView`
  (`app/src/indicators/mod.rs:34`): `slot`, `kind` (`native.*`/`script.*`),
  `ordinal`, `descriptor` (`IndicatorDescriptor`: title, short_title, overlay,
  plots, inputs, fills), `label`, `columns`, `rows`, `preview`,
  `error: Option<EvalError>`, `stale: Option<String>`, `hidden`,
  `input_values: Vec<InputValue>`, `objects`. `InputSpec`
  (`indicators/src/input.rs:133`) has name/title/default/min/max/step/options
  per variant — "effective inputs" = spec paired with `input_values`.
- **drawings** — `pane.drawings.items()` -> `Drawing`
  (`app/src/drawings/mod.rs:1229`): `id`,
  `author: Option<DrawingAuthor { actor_kind, client_name }>`,
  `name` (**user text — never on the wire**; publish `user_label_present`),
  `tool`, `band`, `scope`, `locked`, `hidden`, `foreign_market`, `off_series`.
  Band wire name: `drawing_band_name`. Guard test already exists:
  `observer_resolves_mirrored_drawings_without_leaking_user_text`.
- **orderflow** — `pane.orderflow: Option<OrderflowView>`
  (`app/src/orderflow_view.rs`): `cached_health()`, `enabled()`,
  `depth_visible()`, `bubbles_enabled()`, `lane_*`, `tape_age()`,
  `base_capture_grouping()`, `live_lane_window()`.
- **replay** — `tab.replay: Option<ReplayLink>`
  (`app/src/feed/replay.rs:248`): `session: Arc<Session>`, `status:
  Arc<ReplayStatus>` -> `position_ms`, `start_ms`, `end_ms`, `played`,
  `total`, `speed`, `is_playing`, `is_finished`, `elapsed_ms`, `rewinds`,
  `progress`. "Trace present and complete" = the **control** trace:
  `control/trace.rs`, `ControlTrace::path_for` / `load` / `is_complete`.
- **paper** — `tab.paper: PaperTrading` (`app/src/paper_trading.rs:975`):
  `position_summary()`, `working_orders()`, `session_trades()`,
  `selected_trade_index()`, `is_flat()`.

## Events — `emit_semantic_changes` (`control/gateway.rs:1063`)

Model: `replay_key(tab) -> Option<(bool, bool)>` compared in place against
`SemanticBaseline`, allocating only when changed; `replay.state.changed` is the
shipped exemplar. The #223 review removed a version that allocated per frame —
do not reintroduce it. Add one comparison per new module: indicators (attach,
detach, compile error), drawings (created, removed, edited, with author).

## Schemas

    QUANTICK_UPDATE_CONTROL_SCHEMAS=1 cargo test -p quantick-app --bin quantick-app observer_schemas_are_versioned_valid_and_ui_framework_free
    QUANTICK_UPDATE_CONTROL_SCHEMAS=1 cargo test -p quantick-app --bin quantick-app observer_capability_catalog_is_registry_derived_and_versioned
    QUANTICK_UPDATE_SCHEMAS=1 cargo test -p quantick-control --test schema_snapshots

## Acceptance (roadmap 5.1, 1-7)

1. [ ] headless test per scope: build `QuantickApp`, change state the normal
       way, verify the snapshot
2. [ ] two-pane capture preserves focus and provenance
3. [ ] every scope validates against its schema
       (`observer_modules_project_headless_state_that_matches_their_schemas`)
4. [ ] no egui type on the wire
5. [ ] no request, no per-frame cost: `control_idle_dense_replay_benchmark`
       plus the `observer_*_stays_within_the_ui_budget` guards
6. [ ] one journal event per relevant module change
7. [ ] blast radius in the PR body

## Gates

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace` (known local red `the_bridge_paging_tests_pass`:
      Windows `python3` Store alias; run the two .py files directly)
- [ ] perf declared: cold path, on-demand capture only
- [ ] `arch-review`, findings resolved or deferred in the body
- [ ] one PR against `feat/control-annotate`

Not user-visible: no `visual-qa`, no `trader-ux-review`, no `ui-harness` hook.
