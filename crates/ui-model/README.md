# UI model port

`Model::update(&mut State, Intent) -> Effects` is the headless boundary.
State stores cached presentation data. Inputs include time and changed domain
snapshots; paint never builds projections or runs domain decisions. Domain
crates remain authoritative. A refresh intent should carry a revision so the
model can retain its projection when the source has not changed.

`src/catalog.rs` is the only registration list. Move a legacy entry into
`models { [] [] name: model_module = "model_module.rs",
view_module = "view_module.rs", view_module::effects, Overlay; }`.
The two metadata lists apply to the model and view declarations respectively;
empty lists ship normally, while fixtures use conditional compilation.
This declares `src/model_module.rs` here and `app/src/surfaces/view_module.rs`
in the app, creates the stored adapter, and routes its slot and effects.
Neither crate root needs another module declaration. The model module exports
`Model`; the view module exports a default-constructible `View` implementing
`surfaces::View<Model>`. Model state must implement `Default`.

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
