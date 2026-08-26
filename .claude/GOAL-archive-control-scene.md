# Mission: roadmap 5.2 — the semantic scene

Implement §5.2 of `docs/control-plane/roadmap.md` on `feat/control-scene`, cut
from `origin/main` (95cb0b7 — the control stack landed on main through #233, so
the roadmap §4 stacked-base exception no longer applies). PR targets `main`.

The tree of what is on screen without rasterising: visible controls, a label
and an ID stable across frames, enabled/selected state, the unavailability
reason as data, bounds where useful, owner, and the related registered
capability.

## Mission-specific criteria

1. **Stable IDs across frames.** Two frames, same tree, proved by test. IDs
   derive from identity (tab ID, pane ID, `DrawingTool::id()`), never from
   iteration index or screen position.
2. **Unavailability as data.** `AvailabilitySnapshot { available, reason }` is
   the mould — never a rendered string for the client to parse back.
3. **One ID, shared with the cursor.** `PointerSnapshot.control_id` is filled
   by the *same* resolution the scene uses, and
   `CursorSnapshot.semantic_scene` stops being
   `unavailable("semantic_scene_not_registered_in_this_release")`. Cross-test
   with `observer_cursor_*`.
4. **One source of controls.** `drawings::DRAWING_TOOLS`, the toolbar and the
   panels' own state. No parallel "for the agent" list beside a registry —
   that is an `arch-review` finding.
5. **Owner and bounds.** Owner (panel, dialog, tab, pane) and the rectangle
   where useful, as canonical decimals.
6. **`quantick_get_scene`.** One `Tool` entry, one arm in `tools::call`, an
   embedded schema and the `ErrorResponse` arm of the `oneOf` like the others
   in `crates/mcp/src/tools.rs`; `the_tool_list_is_fixed_and_named_as_the_contract_says`
   updated. The capability is registered in the mould of
   `health.diagnostics.read` (`contract.rs`).
7. **Schema and catalog.** Versioned schema, no egui type on the wire,
   catalog regenerated:
   `observer_schemas_are_versioned_valid_and_ui_framework_free` and
   `observer_capability_catalog_is_registry_derived_and_versioned`.

Full acceptance list: roadmap §5.2 criteria 1-5 (its criterion 5 pulls in
criteria 3, 5 and 7 of §5.1: schema, no egui, no cost without a request,
blast radius).

## Injected gates

- **Code change**: four checks green on the branch head; performance declared
  by rate class; `arch-review` run (step 0 `code-review <PR> high`) with every
  Blocker and Should-fix resolved or deferred with a reason in the PR body;
  **PR opened** (merging is the owner's call).
- **Hot path — the real risk here**: `bounds` must come from state the frame
  already records (`last_chart_area`, `last_bands`). If any new per-frame
  bookkeeping is needed, measure it: `control_idle_dense_replay_benchmark` in
  candidate/control pairs under one window, plus the
  `observer_*_stays_within_the_ui_budget` guards (median; p99 is an
  `#[ignore]`d reading). Numbers in the body.
- **Adds a capability**: `new-extension` — the port is
  `ProjectionRegistry::register_module` / `register_scope` and
  `register_capability` (all exist); registration-only edits; defaults
  preserve today's behaviour; a fake second implementation exercises the port;
  blast radius (added vs edited files) in the body.
- **Second operator**: a named call with a registered ID and schemas, a
  readable result, discoverable through `describe`.
- **Determinism**: fixture-first for the stable-ID test; deterministic
  ordering (`BTreeMap` / `Vec`, never `HashMap` iteration order).
- **UI**: the scene *reads* the interface and paints nothing, so there is no
  new surface and no new `ui-harness` hook. `visual-qa` and
  `trader-ux-review` are declared not applicable in writing in the PR body —
  never skipped in silence.
- Test names in the house style; every confirmed review finding gets a test
  that fails without the fix; English throughout.
