# Regenerating published artifacts

Run `sh tools/regen.sh --check all` to compare every ordinary generated artifact
byte for byte, or `sh tools/regen.sh --write all` to update them. A failed producer
leaves the committed files untouched; each successful replacement uses a complete
temporary file and rename. Check mode never changes repository artifacts.

| Target | Producer | Outputs |
| --- | --- | --- |
| `control` | `cargo run -p quantick-control-handlers --bin regen-control` | Current protocol and observer schemas, reference and observer catalogs, capability inventory and retry matrix |
| `ui` | `cargo run -p quantick-operability --example export_ui_behaviour_matrix` | UI behaviour matrix |
| `hooks` | `quantick-app --features harness --dump-hook-registry` | `.claude/skills/ui-harness/references/hook-registry.md` |
| `pine-oracle` | `npm --prefix tools/pine-oracle run regenerate` | External Pine oracle fixtures; explicit `--write` only |

`control` and `ui` never build the desktop app. `hooks` builds its existing harness
producer, which exits before configuration or display access. `all` runs the first
three targets. The external Pine oracle is never invoked by ordinary regeneration
or merge; install its pinned dependencies separately before opting into its refresh.

`regen-control --check|--write|--list [--root DIRECTORY]` also works directly.
`--list` prints the exact owned path set. Shared descriptor factories provide the
same metadata to live handlers and offline composition; typed projection specs
retain the live DTO schema. Headless composition validates metadata, authority,
scope ownership and retry readbacks without constructing a fake window or host.
The caller supplies build identity. Catalog files omit instance-specific build
fields as before. App wiring tests compare independently assembled live registry
bytes with the headless output.

Current generated JSON lives directly in `schemas/control`. The authored
`canonical-json-v1-vectors.json` and frozen `released/v1` snapshots and manifest
are compatibility inputs, not generator outputs. Their bytes remain unchanged:
the regeneration tests snapshot them, and the existing published-schema tests
verify the released inventory and pinned hashes. There is no `schemas/ui` producer.
Only current outputs carry `linguist-generated merge=binary` attributes.

Existing generated banners and app dump flags remain compatible; those control
flags delegate to headless composition. The older protocol `export_schemas`
example remains available for its single-file and patch output. The app-only
ignored observer regeneration test has been replaced by `regen-control`.

`sh tools/merge_main.sh` resolves generated conflicts after handwritten sources
are resolved and staged, runs every ordinary generator, stages generated files
only, and then commits the pending merge. Generator failure preserves the pending
merge for a retry with `--continue`.
