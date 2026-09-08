# Published schema compatibility

The ordinary snapshot tests detect generated schemas that differ from their
current fixtures. The published compatibility tests additionally compare the
actual generated documents against a separately retained release. Regenerating
the current snapshots therefore cannot hide a same-version breaking change.

## Released source and integrity

[`schemas/control/released/v1/manifest.json`](../../schemas/control/released/v1/manifest.json)
records all 56 JSON Schema documents published at original main commit
`a808b2d87b36d73041027e4d20c053544b454a96` in
<https://github.com/milocaetano/quantick>: 10 core schemas and 46 application
schemas. Each record identifies its filename, original repository path,
generator owner, version and SHA-256 over the original Git blob bytes.

The files were copied with `git show SOURCE_COMMIT:SOURCE_PATH`, without parsing
or rendering them. The source core and application schema catalogs were checked
against the complete `*.schema.json` inventory at that commit. This is a
historical main baseline, not a claim that the campaign candidate is on main.

The manifest itself has the pinned raw-byte SHA-256
`fff12d5cc40eb0219ae336693b7aaea2e3f52695964d5da3a99b71c04ee87079`.
The shared test helper verifies that digest, source identity, each file digest
and the complete released directory inventory. Removing a manifest entry, losing
a released file or editing its bytes fails before compatibility is evaluated.
Repository LF normalization keeps raw-byte checks portable across Windows and CI.

Treat this directory as immutable release evidence. Existing
`QUANTICK_UPDATE_SCHEMAS` and `QUANTICK_UPDATE_CONTROL_SCHEMAS` regeneration paths
write only their existing files directly under `schemas/control`; neither writes
the release directory or manifest. A future release needs its own explicitly
reviewed baseline and provenance. Updating current snapshots is not permission
to replace this baseline. Hash pins detect accidental drift; repository review
is still required to prevent a deliberate edit to both fixtures and pins.

## Enforcement and coverage

The [core suite](../../crates/control/tests/published_schema_compatibility.rs)
calls `public_schema_documents()`. The
[application suite](../../crates/app/src/app/tests/published_schema_compatibility_tests.rs)
calls the application's `control::schema_catalog::documents()`. Neither uses
current snapshot JSON as the candidate schema.

Both compile the same
[test-only support](../../crates/control/tests/support/published_schema.rs).
It first requires every released identity for that owner to be present in the
generated set, then calls the existing production `require_compatible_version`
for every pair. No comparison algorithm or runtime validator is duplicated.
Duplicate generated identities and unknown owners are rejected. Each suite
removes every retained published document in turn and checks for an error
naming that missing published schema; an empty generated set fails too. A
successor-document fixture proves that adding v2 can coexist with v1 while
replacing the v1 route fails coverage.

Identity includes the versioned filename. Adding a v2 document does not excuse
removing a retained v1 route. This gate protects the retained v1 set; new release
baselines require deliberate registration. The existing snapshot tests still
cover the full current generated set and retain their original regeneration
behavior.

Real-schema variants demonstrate the production version policy:

- Requiring the previously optional request-envelope `reason` fails at v1.
- Lowering observer events-read `limit.maximum` from 256 to 128 fails at v1.
- Adding an optional `client_note` property passes at v1 for both schemas.
- Both breaking variants are accepted at explicit v2 while still classified
  `Breaking`; zero and decreasing versions are rejected.

The first two variants run through the same complete generated-document gate
as the normal tests, so they fail even if a current snapshot were regenerated
to match the incompatible candidate.

## Scope and limits

This change preserves all existing generated JSON fixtures and runtime
request/result/error/permission behavior. Rust additions compile only in tests;
the sole existing-code edit registers an application test module. It adds no
runtime validation, per-request cost, dependency, product capability or policy
relaxation. Every touched path is test/CI-only or rare documentation tooling;
there is no per-trade, per-depth or per-frame performance claim to benchmark.

The capability catalogs and canonical JSON vectors are data documents, not
JSON Schemas, and are not release-comparison inputs. Schema compatibility does
not prove retry metadata, idempotency/retry behavior, consent behavior or every
runtime invariant. For example, request payload float rejection is enforced by
runtime validation, not fully expressed by the payload schema. Existing
registry, codec and runtime tests remain necessary; this change neither fixes
nor claims to verify unrelated retry metadata. The existing conservative
compatibility policy defines what this gate can detect.

## Reproduce the evidence

Run these standalone tests from the repository root without update flags:

```sh
cargo test -p quantick-control --test published_schema_compatibility
cargo test -p quantick-app published_schema_compatibility_tests
cargo test -p quantick-control --test schema_snapshots --test schema_compatibility --test registry_contract
cargo test -p quantick-app observer_schemas_are_versioned_valid_and_ui_framework_free
cargo test -p quantick-app observer_capability_catalog_is_registry_derived_and_versioned
```

Both new suites are unignored and run in normal `cargo test --workspace` and CI.
Before every commit, run `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets`, `cargo build --workspace`, then
`cargo test --workspace` in that order. Run `cargo test -p quantick-guards`
after each edit batch.

The implementation base is `origin/campaign/architecture-a` at
`c3a92d58bb8a41ec4d78d73e60312b5f765b4da5`. The committed
[mission archive](../../.claude/GOAL-archive-published-schema-compatibility.md)
retains the source requests and acceptance ledger. Raw UTF-8 commands, merged
output and exit codes live in
`C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q2-validation/` for
coordinator handoff. The coordinator records the final immutable head, log
hashes, independent architecture/AI/delivery outcomes and exact-head CI in
[issue #333](https://github.com/milocaetano/quantick/issues/333) and its PR.
Those review and CI outcomes are pending at implementation handoff; local test
results do not substitute for them. Campaign merges remain serialized and main
merge remains exclusively the user's action.
