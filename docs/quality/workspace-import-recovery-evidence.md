# Workspace import recovery evidence

Campaign [#330](https://github.com/milocaetano/quantick/issues/330), child
[#336](https://github.com/milocaetano/quantick/issues/336). This evidence concerns
rare user-initiated workspace imports. It makes no hot-path performance or
group-transaction claim.

## Source identity and reproduction

Initial tested campaign base:
`0bd50f9b815a05e2ba8d0c9804324dbb415f6658`.
Branch: `feat/workspace-import-recovery`.
The implementation and this evidence are reviewed together. The focused tests
ran against these exact Git blob SHA-1 identities (obtain them independently
with `git hash-object <path>`):

| Source | Tested blob SHA-1 |
| --- | --- |
| `crates/app/src/workspace_bundle.rs` | `1e86005dadfd604c57ff982ea5f01c277e79006f` |
| `crates/app/src/workspace_bundle/recovery_tests.rs` | `52cac02a920aaa569c5b36ac2ade73c6463bc7fb` |

The final commit SHA is available from the containing PR and
`git log -1 --format=%H -- crates/app/src/workspace_bundle/recovery_tests.rs`.
This avoids a self-referential commit identifier in the committed document;
the base and tested source bytes are pinned above.

Run from the owned worktree with its local target directory:

```text
cargo test -p quantick-guards
cargo test -p quantick-app workspace_bundle::recovery_tests -- --nocapture
cargo test -p quantick-app workspace_bundle::
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo build --workspace
cargo test --workspace
```

The focused `--nocapture` command prints each actual live/staged file's contents
at every installation boundary, operation paths/order and exact returned errors.
Scratch directory names vary per run; expected values, operation order and
failure ordinals do not. `ScratchDir` owns and removes all fixture files. No
test resolves the trader's real store paths.

For isolated normal/failure/recovery/repeat transcripts without interleaved
parallel fixture output, these exact commands were also run:

```text
cargo test -p quantick-app workspace_bundle::recovery_tests::second_rename_failure_recovers_through_ordinary_import -- --exact --nocapture
cargo test -p quantick-app workspace_bundle::recovery_tests::fourth_rename_failure_recovers_through_ordinary_import -- --exact --nocapture
cargo test -p quantick-app workspace_bundle::recovery_tests::successful_install_stages_every_group_before_renaming -- --exact --nocapture
```

Exact command output is retained externally at
`C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q5-validation/`.
The mission archive records source authority and the R/A/G/C ledger. Independent
architecture, AI, source-first delivery and exact-head CI remain coordinator
gates before an authorized merge into the campaign branch. Main merge remains
exclusively the user's action.

## Independent fixture oracles

The fixture registers five included stores in this deliberately nonalphabetical
order, followed by the real excluded paper store. Real stores retain their
actual registered validators and local-key declarations. The foreign store has
its own validator and registration; production import learns no new identity.

| Ordinal | Store | Original | Independently specified recovered value |
| --- | --- | --- | --- |
| 1 | `ui_state` | Window 800 x 600 | Window 1200 x 900; all four local keys retained |
| 2 | `chart_layers` | Grid false, heatmap true | Grid true, heatmap false |
| 3 | `foreign_store` | Shape square, machine tag local | Shape round, machine tag local |
| 4 | `layouts` | Active layout 1 named Local | Active layout 1 named Imported |
| 5 | `symbols` | Binance BTCUSDT | Binance ETHUSDT |
| Excluded | `paper_state` | Opaque local bytes, including NUL and CRLF | Byte-for-byte original |

The imported UI source has foreign replay/recent/bookmark values and autosave
true. The independent recovered UI literal preserves `replay_folder`,
`recent_workspaces`, `saved` and `save_on_exit` from the local fixture. The
foreign store similarly preserves its `machine_tag`. Every original, incoming
and recovered included-store literal passes its registered validator during
setup. Recovered bytes are the ordinary TOML serialization of these independent
literals, never output derived from `apply`, `check` or `merge_local_keys`.

Capture must strip every declared local key and exclude paper. The fixture then
explicitly inserts an excluded paper section with an invalid version, writes
and reads the bundle, and verifies that import still never stages or modifies
paper. Original live bytes retain their fixture formatting; untouched groups
must match those exact bytes, not merely parse to equivalent values.

## Installation and disk boundaries

`apply` delegates to private `apply_with_rename`, which contains the original
validation, rendering, staging, installation and cleanup body. Only the rename
call is parameterized. The production closure invokes `std::fs::rename`; the
test adapter invokes that same operation except at its selected failure ordinal.
Neither adapter can skip the preceding real validation and staging body.

The tests record path resolution for each stage and the exact rename source and
destination. Before every rename callback, they read every live file and every
expected staging file. This establishes that all five stages exist before the
first replacement, and only the previously installed prefix has changed.

| Boundary | Changed live groups | Original live groups | Remaining `.importing` files |
| --- | --- | --- | --- |
| Before import | None | 1-5 | None |
| Before rename 1 | None | 1-5 | 1-5, all expected recovered bytes |
| Before rename 2 | 1 | 2-5 | 2-5 |
| After injected rename 2 failure | 1 | 2-5 | 3-5 |
| Before rename 3 in later fixture | 1-2 | 3-5 | 3-5 |
| Before rename 4 in later fixture | 1-3 | 4-5 | 4-5 |
| After injected rename 4 failure | 1-3 | 4-5 | 5 |
| After ordinary recovery | 1-5 | None | None |
| After repeated ordinary success | 1-5 | None | None |

Successful renames consume their staging paths. The failing rename's staging
file is removed. Later staging files survive that failure and are overwritten
by ordinary re-import staging before installation. The error therefore
describes a partial import honestly; it does not promise rollback.

The exact error oracle is:

```text
replaced 1 of 5 settings groups, then <owned chart-layers path> failed: injected rename failure 2. Open the file again to finish.
replaced 3 of 5 settings groups, then <owned layouts path> failed: injected rename failure 4. Open the file again to finish.
```

Each fixture then invokes ordinary production `apply` twice on the same bundle.
Both calls must return `ui_state`, `chart_layers`, `foreign_store`, `layouts`,
`symbols` in registry order and leave the independent recovered cockpit on disk.
Local keys and paper bytes are asserted at every boundary and after both calls.

## Earlier failures and normal control

`invalid_section_reaches_no_staging_or_rename` places version 99 in the real
chart-layer section. Path resolution and rename callbacks panic if called. The
test requires the real validator's exact error, every original live byte and no
staging artifacts.

`stage_write_failure_cleans_earlier_temps_without_installing` creates an owned
directory at the fourth staging path. The real `std::fs::write` fails there;
no rename callback may run. Every live file remains original, the first three
staging files are cleaned, and the pre-existing empty blocker survives. After
the fixture removes only its own blocker, ordinary recovery/repeat succeeds.
The operating system's write-error wording is intentionally not frozen; the
existing app prefix, failing live path and `Nothing was changed.` suffix are.

`successful_install_stages_every_group_before_renaming` is the no-fault control:
five stage path resolutions precede five real renames, with the same per-step
disk assertions. Existing workspace tests are retained without changes and run
alongside these five focused tests.

## Cost and limits

Affected rate: rare import operations. The production adapter is a capture-free
closure passed to a generic `impl FnMut`; its concrete type is resolved at
compile time. There is no trait object, heap allocation, application-root state
or per-file identity switch introduced by the seam. The additional source-level
wrapper/closure calls are statically dispatched; no end-to-end timing reduction
or guaranteed total execution time is claimed.

For N included sections, the same body validates N sections, renders/stages N
files, then attempts N renames on success. Failure on rename K attempts K
renames, retains K-1 installed groups, removes the failed staging file and
leaves N-K later stages. A phase-one failure still invokes zero renames and
cleans only successfully registered earlier stages. Existing reads for local
keys, serialization, filesystem writes/removals and their ordering are unchanged.
The fixture traces verify the five-store instances of these counts and order.

## Verification results

- Arming before mission/source edits: `cargo build -p quantick-guards` passed,
  exit 0, 4.84s; `cargo check -p quantick-app --all-targets` passed, exit 0,
  1m39s. Fresh worktree-local target; logs `01-arm-guards.log` and
  `02-arm-app-check.log`.
- Post-edit guards: PASS, 172 tests (149 unit + 18 integration + 5 agreement),
  exit 0, `03-batch-guards.log`. Formatting: PASS, exit 0, `04-format.log`.
  Post-format guards: PASS, the same 172 tests, `05-post-format-guards.log`.
- Focused recovery suite: PASS, 5 tests, exit 0, `06-recovery-tests.log`.
  Initial fresh test compilation took 3m59s; fixture execution took 0.12s.
  These are verification elapsed times, not import performance measurements.
- Complete workspace bundle suite: PASS, 20 tests (15 preserved + 5 new),
  exit 0, `07-workspace-tests.log`.
- Isolated second-failure, fourth-failure and normal-control commands above:
  PASS, one test each, exit 0. Logs `08-<full-test-name>.log` retain actual
  per-step disk contents and ordinary recovery/repeat readbacks. The disk
  boundary table above was verified by these assertions, not inferred from
  the expected return value alone.
- Initial complete ordered loop: fmt check PASS (`10-full-fmt.log`), workspace
  clippy all targets PASS, 36.13s (`11-full-clippy.log`), workspace build PASS,
  1m14s (`12-full-build.log`), workspace test FAIL, exit 101
  (`13-full-workspace-tests.log`). The app had 1912 passed, one failed and
  four ignored; all 20 workspace bundle tests passed. The failing existing
  `observer_core_capture_stays_within_the_ui_budget` observed medians
  270/258/269 us; its best 258 us exceeded the unchanged 250 us limit.
- Exact existing isolated diagnostic, with no source/threshold/runner change:
  `cargo test -p quantick-app app::tests::control_plane_tests::observer_core_capture_stays_within_the_ui_budget -- --exact --nocapture`
  PASS, one test, exit 0 (`14-observer-isolated-diagnostic.log`), medians
  79/81/79 us. This observation does not establish the cause of the earlier
  failure and does not substitute for the full suite.
- The coordinator authorized one unchanged full ordered retry on this final
  documentation tree. Its exact command outputs are retained as
  `15-retry1-guards.log`, `16-retry1-fmt.log`, `17-retry1-clippy.log`,
  `18-retry1-build.log` and `19-retry1-workspace-tests.log`; the frozen tree
  and actual exits belong to the coordinator handoff. A commit requires every
  mandatory command to exit 0. No passing retry is claimed in advance here.
- Failure history at retry preparation: observer failure occurrence 1,
  production repairs 0, full-suite retries not yet executed. Earlier tool
  signatures remain in the external attempt ledger: one rejected large
  PowerShell write, one failed source-string append followed by its successful
  repair, and one missing optional config-file read. No counter was reset.
- Independent reviews, exact-head CI and campaign integration: pending
  coordinator delivery; no marker or score credit is claimed here.
