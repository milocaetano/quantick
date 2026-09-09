# Maintained campaign recovery evidence

Q11 [issue 352](https://github.com/milocaetano/quantick/issues/352) maintains the
adopted helper and drift regressions. Its source/map was independently checked
before edits; the [public source dossier](https://github.com/milocaetano/quantick/issues/352#issuecomment-5609545319)
and [claim checkpoint 951](https://github.com/milocaetano/quantick/issues/330#issuecomment-5609579122)
retain the source, authority and ownership. The implementation author previously
performed that preflight and is not eligible for the independent final reviews.

## Implementation and regression map

The [maintained source and provenance](../../tools/campaign/README.md) identify
the exact adopted helper and 53 unchanged original tests. No helper algorithm,
assertion, retry policy, business authority or product runtime is changed.

| Criterion | Inspectable implementation or required evidence |
| --- | --- |
| A1 | Exact adopted source/tests in `tools/campaign/`; their initial hashes and operational entry points in the README; both suites registered in the existing guardrail runner; existing CI Python lint includes all `tools/`. |
| A2 | `FreshProcessRecoveryTests.test_fresh_process_reconciles_once_and_completes_dependency_audit`: two processes/empty caches, actual partition/journal/checkpoint path, literal null/exhausted/history/main oracles, one original effect, useful computed audit and released ownership. |
| A3 | Original partition/identity/lease/unknown-history refusal tests; fresh-process broken-partition/identity/owner/lease variants; `test_changed_business_readback_refuses_result_and_dependent_publication` checks the caller's actual external readback obligation. |
| A4 | `CliPaginationTests` keeps `GhTransport -> data -> gh -> subprocess` intact, replacing only raw CLI I/O. Three bounded pages cover cursor chaining, unique chronological rows, boundary discovery and missing-boundary refusal. The original 53 tests and separate PR-record tests remain independently runnable. |
| A5 | This source/evidence/transfer record plus source-bound local receipts, current-head CI and independent review-input dossier. Actual review PASS and campaign integration are C1/C2, not prerequisites for grading their own evidence. |

The fixture audit uses reconstructed dependency state and current fake merge/CI
readbacks. A negative case changes the upstream CI result and verifies that the
audit is not published even though recovery itself completes. Neither helper
task scheduling nor mechanical business authorization is claimed. Literal Q7
null/repair11, Q10 review4 and Q8 failed-experiment history are synthetic retained
state in the offline fixture, not assertions that a live replacement ran.

## Validation and closing evidence

The focused checks and full ordered workspace validation below passed on
2026-09-09. Current-head CI, final source/architecture/AI/delivery reviews and
campaign integration remain pending; these local results do not replace them.

The coordinator dossier `q11-implementation/batch1/` holds command JSON, raw logs
and `source-manifest.json` (SHA256
`b42843c33b26c6e3d07bbc40848a19cbaa6d99196b49d8117599bed48103f2c4`).
Its base/head is `98c1955ba1d0e5dc78d16f0bd25ac13cce1c21b4` plus the
manifest-bound, uncommitted Q11 files. Every completed command records unchanged
input hashes. Publish these receipts on issue352 with the final review dossier.

| Local command | Observed result | Receipt / log SHA256 |
| --- | --- | --- |
| `cargo test -p quantick-guards` | PASS | `00.json` / `8725f75ed189861be239977e551a3898d624ce5508bb0e52f44801a5f53f2d42` |
| Original coordinator suite | 53 tests PASS | `01.json` / `ca802e870ca11e46af06464c5796429246df5854c4a37ce2b9451be0f612bee8` |
| New recovery/pagination suite | 6 tests PASS, including five fresh-process refusal variants | `02.json` / `005d32e90c3ca366bd6f0a91506873f4ee9c7ca9f34419b21c4df6f67d3ee814` |
| Separate review-progress suite | 20 tests PASS | `03.json` / `fba0b0b45b6efd7e1b87e0ac582dbdcbdb99300297e49102a722eec5d660f544` |
| Ruff 0.16.6, `check --select F tools/campaign/` | PASS | `04-isolated-lint.json` / `82b3e6a6c090a57601d22943bd23fca9218d1031dbe5a7b754092f9a156b4f18` |
| Shared `guardrails_test.sh` | 225 passed, 0 failed; includes both coordinator suites | `05.json` / `14a081b1a91fc49c87f5e94fac6a838b9c5bd4ba77a7ed68dd033d7b7a2dd844` |

The initial lint launch failed because `ruff` was absent from PATH; preserve
`04-launch-error.json`. The successful run used an isolated coordinator-cache
installation, adding no repository dependency. The hook suite took about four
and a half minutes on Windows; it completed without a rerun. This receipt-only
document update follows the frozen checks; no tested helper, test, hook, config
or assertion changed. Its guards/diff check belongs with the final local dossier.

Root's `q11-validation-full1/source-manifest.json` binds all 1,114 tracked and
relevant untracked source files at the same base/head plus the uncommitted Q11
implementation. Its SHA256 is
`b17aeab5b12b946c76c872fb33a585b0be09684cf05c32000a90d651bf4fdb4c`.
The following ordered commands ran from 22:54:38Z to 23:04:14Z. Each receipt
records exit zero and unchanged source hashes; every raw log hash was checked.

| Full local command | Observed result | Receipt / log SHA256 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | `00-fmt.json` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --workspace --all-targets` | PASS | `01-clippy.json` / `cf2cee264f064c754545e5ee6df7130cbf621fc422595ce7133fafdc50dbd507` |
| `cargo build --workspace` | PASS | `02-build.json` / `ddc3f35ba65edc1ba04883ee6d7f45f87a69562ef02a9d5503789b97dab2ee27` |
| `cargo test --workspace` | 3,523 passed, 0 failed, 13 ignored; `RUST_TEST_THREADS=1` | `03-test.json` / `4977918232d9170ff848b440cd6020a58cdb787a01ff9a05c3ad32a4fa9c8af3` |

The independent working-tree report `q11-independent-source-preflight.md`,
SHA256 `d1e742f13e8751fccb99009f9a201a9aa2a722092c2d9431b96cd7475e8c8fb9`,
found no confirmed issues in its bounded source/diff inspection at 22:46:31Z.
It predates the full workspace run and this archive update. It is a review
input, not a final committed-diff architecture/AI/delivery verdict or CI result.

The [mission archive](../../.claude/GOAL-archive-campaign-recovery-regressions.md)
retains every original R/A/G/C clause and the verbatim delegated issue;
the [original source](../../.claude/GOAL-source-campaign-recovery.md) is unchanged.
A1-A4 record the implemented evidence. A5 remains open for current-head CI;
closing reviews and integration remain open too. Q12's transferred live
outcomes retain their separate owner and dependency below.

The post-validation batch changes only this evidence prose and archives the
mission with implementation status and evidence references. The complete
`q11-implementation/archive-delta/` record compares all full-manifest hashes,
tracks the active mission separately, records the full name/status and content
delta, checks relative links, and binds the resulting tree. Reused local
runtime results are from the manifest above, not newly executed at the archived
tree. Base, executable/test/hook/config inputs, toolchain and dependencies are
unchanged; no toolchain/environment mutation or new related failure occurred
during this documentation batch. Final archive guards remain root-owned before
commit. Evidence reuse does not replace current reviews or final-head CI.

- G1: source preflight and checkpoint951 establish the planning/claim record;
  coordinator arming receipts belong in the final source-bound dossier.
- G2: initial helper/test hashes match the adopted publications. Inspect the
  final diff and independently specified oracles for preserved assertions and
  scope. All new paths run rarely/offline; no trade/depth/frame path changes.
- G3: run guards, both coordinator suites, independent PR-progress tests,
  `ruff check --select F tools/campaign/`, the shared hook suite and the ordered
  workspace fmt/clippy/build/test loop. Final-head CI remains mandatory.
- G4: preserve cumulative/unknown histories, explicit build ownership and main
  exclusions in campaign checkpoints. Q8's exclusive build/measurement window
  takes precedence; Q11 claims no runtime measurement.
- C1: archive the mission/source before independent exact-source architecture/
  bug, AI and source-first delivery reviews. Retain their head/base keys and
  stable findings; a review is not complete until its actual verdict exists.
- C2: after fresh reviews/green CI, root may integrate only into the authorized
  campaign branch and must record exact source/merge reachability.
- C3: provide that reviewed repository entry point and provenance to Q12;
  pending live evidence remains pending.

## Preserved Q12 transfer

[Q12 issue 353](https://github.com/milocaetano/quantick/issues/353) depends on
Q11's reviewed campaign integration with green CI. The retained combined
proposal's clauses have these explicit owners:

| Original combined criterion | Required owner |
| --- | --- |
| A1-A4 maintained/offline code outcomes | Q11 A1-A4 |
| A1 actual live use of the maintained reviewed file | Q12 A1 |
| A5 target-bound URL-only replacement and legitimate ownership/reconciliation/history proof | Q12 A2 |
| A6 useful current live audit, next action, complete handoff/release | Q12 A3 |
| A7 premerge validation/review package and reviewed integration | Q11 A5 and C1-C2 |
| A7 actual integrated independent assessment and parent limitations | Q12 A4 |

Q12 must preserve actual prompt provenance, target/helper hashes, activation,
old-writer inactivity/legitimate ownership, exact one-effect readback and
before/after cumulative histories. Offline same-writer restart cannot discharge
that live obligation. Final campaign-wide reviews/CI, dimension floors, A+ gates
and user-only main integration remain parent obligations. No score increase is
claimed by this implementation or its proposal.
