# UI model port

`Model::update(&mut State, Intent) -> Effects` is the headless boundary.
State stores cached presentation data. Inputs include time and changed domain
snapshots; paint never builds projections or runs domain decisions. Domain
crates remain authoritative. A refresh intent should carry a revision so the
model can retain its projection when the source has not changed.

`src/catalog.rs` is the only registration list. Move a legacy entry into
`models { [] [] name: Name => model_module = "model_module.rs",
view_module = "view_module.rs", view_module::effects, Overlay, Host; }`.
The two metadata lists apply to the model and view declarations respectively;
empty lists ship normally, while fixtures use conditional compilation.
This declares `src/model_module.rs` here and `app/src/surfaces/view_module.rs`
in the app, creates the stored adapter, and routes its slot and effects.
Neither crate root needs another module declaration. The model module exports
`Model`; the view module exports a default-constructible `View` implementing
`surfaces::View<Model>`. Model state must implement `Default`; effects implement `Debug` and `PartialEq`
for the catalog-generated, non-exhaustive headless effect enum.

The view borrows state and returns `Option<Intent>`; `None` paints without
updating. Batch related changes in one intent when needed. A view owns only
UI mechanics. `Adapter::update` is the same path for host/agent input.
`effects` converts typed model effects into the existing `SurfaceResponse`.
The existing frame surface host applies that response through domain ports;
do not add another dispatcher or make a model reach into the application.

`Surfaces::draw_all` drives overlays. Existing chrome owners call `draw_slot`
with `Slot::{PaneHeader,TitleBar,Toolbar,MenuBar,TabStrip,StatusLine}`, a `ViewContext` containing
their borrowed `Ui`, and explicit `SurfaceEnv` inputs. Forward the returned
response to the existing host effect application. Slot calls do not consume
another slot's intents. Keyboard capture and capture hooks are registered by
the same catalog; a view hook can stage an intent for its next paint.

Compatibility entries retain their concrete types, state, call order, keyboard
capture and hooks. Delete replaced code/tests when migrating a row. The two
catalog fixtures are test-only and are not shipped consumers or UI features.

Scoped hosts borrow the existing registry once. `update_scoped(scope, HostIntent)`
refreshes a cached model before `draw_slot_scoped(scope, slot, ViewContext)` paints.
The catalog generates `HostIntent` and `HostEffect` variants from the row's `Name`;
no handwritten central match or feature field is added. Model effects travel in
`SurfaceResponse::model_effects` to the existing owning adapter. Scope identifiers
are stable host IDs, not positions. The host retains scopes while their owner exists.
Unscoped overlays retain the compatibility environment; scoped views receive no
legacy environment and read explicit model snapshots. Snapshot equality may serve
as the revision when the domain has no revision token, retaining projections across
identical refreshes. The pane pilot caches identity, segments and window options.

Pane-header test disposition is recorded in [coverage](src/pane_header/coverage.md).

Rustfmt does not discover modules declared by the catalog macro. In addition to
`cargo fmt --all -- --check`, run `rustfmt --edition 2024 --check` on each changed
catalog-loaded model/view source (and its nested tests). Successor migrations must
include that explicit check in their validation evidence.

The two app View fixtures are test-only catalog rows under
`app/src/surfaces/tests/fixtures/`. Their relocation from `surfaces/fixtures/`
corrects production classification; it is not behavioral extraction and removes
no app-owned test. Compare final feature counts to the original pinned baseline,
and report this classification correction separately from policy migration.

The four window chrome slots (`TitleBar`, `TabStrip`, `Toolbar`, `MenuBar`)
use the same scoped adapter and catalog as pane headers. Chrome scopes are stable
window IDs; pane retention only visits `PaneHeader`, so owners cannot erase one
another. Host snapshots enter as refresh intents; models cache projections and
validate actions before returning typed effects to existing domain adapters.
The menu model owns navigation, availability and row projection. Its view keeps
only native rectangles and formatted shortcuts. Test-only catalog items exercise
ordering, visibility and effects in all four slots.

Editors whose input changes rows within one frame override `View::paint_updated`.
The generic `UpdateSession` borrows the adapter's actual state; `update(intent)`
runs the authoritative model once and immediately folds its effects into the host
response. `state()` then borrows the resulting projection for dependent painting.
The session stores no second state or effect queue. Ordinary views keep `paint`;
the default staged method forwards its optional intent through the same session.
Scoped and unscoped hosts share this path. Emit updates only for changed input,
never rebuild projections during idle painting.

Each catalog row ends in `Host` or `Local`. `Host` opts into generated external
`HostIntent` dispatch; `Local` owns only view/hook inputs and generates no unused
host variant. Both retain identical model storage, slots, hooks and effect routing.

A catalog row can declare several existing frame passes with `PinnedInspector |
DrawingChrome`. The registration gives one adapter a static slot list. A host
uses `adapter.with_pass(slot, sink, |view, session| ...)` to borrow pass-specific
facts directly into the view; an undeclared slot invokes neither closure nor
sink. Both passes see the same state and view, and session intents deliver
converted effects once. Borrowed geometry and application references stay on
that stack, so the host can refresh them after the canvas without copying them
into the model. Ordinary views continue through `draw_slot` with the same
catalog membership check.
