# Docking a control family

`quantick_control::control_families!` is the membership list. Add one row in
strict ID order; each group is explicit, including empty groups. Schema-only,
projection-only and action-only families need no placeholder implementation.
The control crate exports names, never references to its consumers.

Each consumer resolves the names beside the code it owns:

| Binding | Owner and convention |
| --- | --- |
| `schemas` | `control-schema/src/<binding>.rs`, `documents()` |
| `actions` | `control-handlers/src/<binding>.rs`, `register_actions()` and `action_descriptors()` |
| `projections` | handler `register()`, `module_descriptor()` and `scope_descriptors()`; existing app-local adapters keep their local binding |
| `authority` | `control-host/src/authority/families/<binding>.rs`, `SCOPES` and `bind()` |
| `readbacks` | schema family `READBACKS` |
| `behaviours` | `operability/src/registry/<binding>.rs`, `BINDING` |
| `samples` | `app/src/app/tests/retry_readback_tests/samples/<binding>.rs`, `samples()` |

Action families with projections share one handler module. A projection-only
family gets its own module. Existing app-local projection adapters keep their
metadata in `control-handlers::generation`; their local bindings select that
owner. Schemas, authority, behaviour mappings and retry declarations remain
with their actual owners. Generated artifacts use the production descriptors
and a caller-supplied `BuildIdentity`.

Behaviour groups and gateway scenarios carry local ordinals to preserve the
existing document and scenario order. These ordinals do not choose membership;
the manifest does. Duplicate scenario positions or behaviour group positions
fail. The family guard scans every Rust file under `crates`, including nested
and test modules, for `const` or `static READBACKS`. The one aggregate export is
recognized by path and its exact declaration; every other owner must dock.

Run `python tools/control_docking_test.py` with Cargo dependencies available
locally. It copies the current source into a disposable directory and uses
Cargo offline. The chart phase removes only `chart.context.read`, checks that
the remaining app builds, then restores the capability and compares every
emitted path and byte. It measures existing handwritten lines and all changed
files separately. The earlier drawing and strategy adapters remain in place.

The mutation phase adds a new synthetic family through the real manifest and
conventional owner files. Its named Rust test executes production action,
admission, authority, readback, projection metadata and idempotency consumers;
the app gateway test executes the production registration and sample consumer.
The fixture has no live feed or trading connection. Its added rows and summary
counts are checked, and every unrelated emitted byte must stay identical.
This synthetic run is separate from the ordinary workspace test suite.
