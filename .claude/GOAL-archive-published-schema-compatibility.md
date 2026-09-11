# Published schema compatibility mission

Freeze the published v1 schema set and enforce compatibility against actual generated core and application schemas, so snapshot regeneration cannot conceal a same-version break.

**Tier:** high, because this enforces the public control contract while preserving runtime behavior.

Issue: https://github.com/milocaetano/quantick/issues/333. Campaign: https://github.com/milocaetano/quantick/issues/330.
Branch: `feat/published-schema-compatibility`; worktree: `C:/src/quantick-worktrees/feat-published-schema-compatibility`.
Declared base: `origin/campaign/architecture-a` at `c3a92d58bb8a41ec4d78d73e60312b5f765b4da5`.
Released source: original main `a808b2d87b36d73041027e4d20c053544b454a96`.

## Request ledger

- **R1** — Preserve runtime request behavior.
- **R2** — Preserve runtime result behavior.
- **R3** — Preserve runtime error behavior.
- **R4** — Preserve runtime permission behavior.
- **R5** — Preserve all currently generated public JSON schemas.
- **R6** — Freeze actual published v1 schemas from the known main SHA.
- **R7** — Record baseline provenance and exact source SHA.
- **R8** — Record schema document identities and source hashes.
- **R9** — Keep the released baseline separate from ordinary regeneration.
- **R10** — Compare actual generated core documents with released v1 schemas.
- **R11** — Compare actual generated observer/application documents with released v1 schemas.
- **R12** — Run compatibility enforcement in normal tests and CI.
- **R13** — Reuse require_compatible_version without a copied comparator.
- **R14** — Do not add a runtime validator fork.
- **R15** — Do not add per-request cost.
- **R16** — Cover every retained published document explicitly.
- **R17** — Fail coverage for missing or removed same-version documents.
- **R18** — Prove real-schema required-field narrowing fails unchanged version.
- **R19** — Prove real-schema numeric-bound narrowing fails unchanged version.
- **R20** — Prove a compatible additive optional property passes.
- **R21** — Prove explicitly bumped-version fixtures follow existing policy.
- **R22** — Do not alter the rubric.
- **R23** — Do not loosen compatibility rules or snapshot regeneration tests.
- **R24** — Do not add a product capability.
- **R25** — Do not correct unrelated retry metadata.
- **R26** — Keep normal snapshot, registry and schema tests passing.
- **R27** — State unsupported contract-metadata comparisons honestly.
- **R28** — Record exact base/head and baseline provenance evidence.
- **R29** — Record meaningful negative/variant assertions.
- **R30** — Provide independently runnable test commands.
- **R31** — Run local fmt/clippy/build/test before every commit.
- **R32** — Run guards after edit batches.
- **R33** — Obtain independent architecture review.
- **R34** — Obtain independent AI review.
- **R35** — Retain the issue's independent delivery-review obligation as a closing step.
- **R36** — Obtain exact-head green CI.
- **R37** — Use a separate high-tier campaign mission and owned paths.
- **R38** — Wait for host release and arm before edits.
- **R39** — Create GOAL after arming and before implementation; retain full source requests and archive before reviews.
- **R40** — Keep complete stable R/A/G traceability and explicit closing steps.
- **R41** — Preserve bounded retry history, stop and report to the coordinator before retries, and never weaken thresholds.
- **R42** — Leave publication, markers, remote state and serialized campaign merges to the coordinator: publish a draft PR, then complete independent review gates and CI before the authorized intermediate merge into `campaign/architecture-a`.
- **R43** — Update and revalidate if the campaign base advances.
- **R44** — Supply a source-based review dossier and raw logs.
- **R45** — Read AGENTS/CLAUDE and mission/new-task/ship/campaign instructions before implementation.
- **R46** — Do not ask for routine permission already granted.
- **R47** — Never set sandbox_permissions.
- **R48** — Report any further required path overlap before expanding ownership.
- **R49** — Do not create child host goals or prompts.
- **R50** — Do not perform self-reviews.
- **R51** — Save exact commands, merged output and exits through an external explicit UTF-8 Python runner.
- **R52** — Do not invent a benchmark obligation for test-only changes.
- **R53** — Respect the maximum of two independent disjoint implementation missions.
- **R54** — Do not depend on Q1 unintegrated results.
- **R55** — Preserve cumulative operation/repair counters and a maximum of three attempts per task/failure signature.
- **R56** — Do not weaken thresholds or claim an unsupported baseline after failures.
- **R57** — Verify private mission-base and high tier are bound to the owned branch.
- **R58** — Never edit archive checkboxes after reviews.
- **R59** — Notify the coordinator before every commit with exact frozen tree and check evidence.
- **R60** — Return the exact clean implementation head.

## Decisions

- **D1** — The coordinator owns remote state, publication, independent review orchestration and serialized merges into `campaign/architecture-a`; only the user merges main.
- **D2** — D3 permits two disjoint implementation missions. Q2 has no dependency on Q1 and owns only its assigned paths. Source: https://github.com/milocaetano/quantick/issues/330#issuecomment-5564210429.
- **D3** — The coordinator explicitly released the build/edit host after Q1's full checks passed.

## Assumptions

- **S1** — Published JSON Schemas are the 56 `*.schema.json` files at the named source SHA (10 core, 46 app), as verified against both source catalogs. Catalog instances and canonical vectors are not JSON Schemas; scope and existing catalogs settle this without a question.
- **S2** — Identity includes the versioned filename. Existing released v1 routes must remain available even if a new v2 route is added; an explicit checker fixture demonstrates bump policy without allowing a missing v1 route to disappear from coverage.
- **S3** — A shared test-only support module may be compiled by both test suites through a path attribute. This avoids production API or dependency changes and keeps one production comparator.
- **S4** — All touched Rust paths run only in tests; fixtures, docs and the archive are rare tooling paths. Runtime impact is zero; per-frame/per-trade/per-depth benchmarks do not apply.
- **S5** — No unresolved source ambiguity earned a user question. Stable naming, file placement and test fixtures are reversible implementation choices.

## Acceptance criteria

Evidence destination for code assertions: `docs/control-plane/published-schema-compatibility.md` and the named tests.
Raw command evidence: `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q2-validation/`.
Immutable final head, reviews and CI are recorded by the coordinator in issue #333 and the PR; this archive is not edited after reviews to claim future events.

- [ ] **A1** — No production code changes; codec and workspace regression tests pass. *Evidence:* source/test/log or coordinator report at the destinations above. *(R1)*
  *Recorded evidence:* source-evidence.json proves unchanged existing contracts; full runtime regression outcome is owed in 23-final-test.log before commit.
- [ ] **A2** — No production code changes; registry and workspace regression tests pass. *Evidence:* source/test/log or coordinator report at the destinations above. *(R2)*
  *Recorded evidence:* source-evidence.json proves unchanged existing contracts; full runtime regression outcome is owed in 23-final-test.log before commit.
- [ ] **A3** — No production code changes; registry and workspace regression tests pass. *Evidence:* source/test/log or coordinator report at the destinations above. *(R3)*
  *Recorded evidence:* source-evidence.json proves unchanged existing contracts; full runtime regression outcome is owed in 23-final-test.log before commit.
- [ ] **A4** — No production code changes; registry and workspace regression tests pass. *Evidence:* source/test/log or coordinator report at the destinations above. *(R4)*
  *Recorded evidence:* source-evidence.json proves unchanged existing contracts; full runtime regression outcome is owed in 23-final-test.log before commit.
- [x] **A5** — Existing schemas/control/*.json remain byte-identical to the declared campaign base and original main. *Evidence:* source/test/log or coordinator report at the destinations above. *(R5)*
  *Recorded evidence:* 11-source-provenance.log and source-evidence.json: 56 exact released blobs and 59 unchanged current JSON documents; manifest pins and unchanged snapshot writers.
- [x] **A6** — All 56 released schema blobs match original main a808b2d87b36d73041027e4d20c053544b454a96. *Evidence:* source/test/log or coordinator report at the destinations above. *(R6)*
  *Recorded evidence:* 11-source-provenance.log and source-evidence.json: 56 exact released blobs and 59 unchanged current JSON documents; manifest pins and unchanged snapshot writers.
- [x] **A7** — The released manifest and documentation identify source repository, commit and extraction method. *Evidence:* source/test/log or coordinator report at the destinations above. *(R7)*
  *Recorded evidence:* 11-source-provenance.log and source-evidence.json: 56 exact released blobs and 59 unchanged current JSON documents; manifest pins and unchanged snapshot writers.
- [x] **A8** — The manifest records every source path, owner, version and raw SHA-256; tests verify bytes against the pinned manifest. *Evidence:* source/test/log or coordinator report at the destinations above. *(R8)*
  *Recorded evidence:* 11-source-provenance.log and source-evidence.json: 56 exact released blobs and 59 unchanged current JSON documents; manifest pins and unchanged snapshot writers.
- [x] **A9** — Existing regeneration code writes only its existing top-level snapshot paths; released files are read-only test inputs. *Evidence:* source/test/log or coordinator report at the destinations above. *(R9)*
  *Recorded evidence:* 11-source-provenance.log and source-evidence.json: 56 exact released blobs and 59 unchanged current JSON documents; manifest pins and unchanged snapshot writers.
- [x] **A10** — The standalone control test feeds public_schema_documents() into the released compatibility gate. *Evidence:* source/test/log or coordinator report at the destinations above. *(R10)*
  *Recorded evidence:* 15-core-schemas.log: published_core_schemas_remain_compatible.
- [x] **A11** — The application test feeds control::schema_catalog::documents() into the same test-only gate. *Evidence:* source/test/log or coordinator report at the destinations above. *(R11)*
  *Recorded evidence:* 16-app-published.log: published_observer_schemas_remain_compatible.
- [ ] **A12** — Both unignored test suites run under cargo test --workspace without environment flags. *Evidence:* source/test/log or coordinator report at the destinations above. *(R12)*
  *Recorded evidence:* both unignored test modules and standard cargo test --workspace; final normal-suite result is owed in 23-final-test.log.
- [x] **A13** — The shared test helper calls the existing production checker; no production schema policy changes. *Evidence:* source/test/log or coordinator report at the destinations above. *(R13)*
  *Recorded evidence:* shared support/published_schema.rs calls require_compatible_version and raw_digest; manifest/inventory assertions; only test module registration edits existing Rust.
- [x] **A14** — Only test support, fixtures, documentation and one test registration change. *Evidence:* source/test/log or coordinator report at the destinations above. *(R14)*
  *Recorded evidence:* shared support/published_schema.rs calls require_compatible_version and raw_digest; manifest/inventory assertions; only test module registration edits existing Rust.
- [x] **A15** — All Rust additions compile only as tests; no per-trade, per-depth or per-frame changes. *Evidence:* source/test/log or coordinator report at the destinations above. *(R15)*
  *Recorded evidence:* shared support/published_schema.rs calls require_compatible_version and raw_digest; manifest/inventory assertions; only test module registration edits existing Rust.
- [x] **A16** — Pinned manifest and complete released inventory cover 10 core and 46 app schemas; every identity is checked before comparison. *Evidence:* source/test/log or coordinator report at the destinations above. *(R16)*
  *Recorded evidence:* shared support/published_schema.rs calls require_compatible_version and raw_digest; manifest/inventory assertions; only test module registration edits existing Rust.
- [x] **A17** — Both suites remove every retained published document in turn and require the named missing-document error; an empty collection also fails. *Evidence:* source/test/log or coordinator report at the destinations above. *(R17)*
  *Recorded evidence:* 15-core-schemas.log and 16-app-published.log: every_published_*_schema_is_required_in_generated_coverage, including empty sets and v2 coexistence/replacement.
- [x] **A18** — A real request-envelope variant makes optional reason required and fails through the actual generated-document gate. *Evidence:* source/test/log or coordinator report at the destinations above. *(R18)*
  *Recorded evidence:* 15-core-schemas.log: published_core_required_field_narrowing_fails_even_after_snapshot_regeneration.
- [x] **A19** — A real observer-events-read-input variant lowers limit maximum and fails through the actual generated-document gate. *Evidence:* source/test/log or coordinator report at the destinations above. *(R19)*
  *Recorded evidence:* 16-app-published.log: published_observer_numeric_narrowing_fails_even_after_snapshot_regeneration.
- [x] **A20** — Real published core and app schemas gain an optional property and pass the existing checker at v1. *Evidence:* source/test/log or coordinator report at the destinations above. *(R20)*
  *Recorded evidence:* 15-core-schemas.log and 16-app-published.log: published_*_variants_follow_existing_additive_and_version_policy.
- [x] **A21** — Both real breaking variants pass at v2 while remaining classified Breaking; zero and decreasing versions are refused. *Evidence:* source/test/log or coordinator report at the destinations above. *(R21)*
  *Recorded evidence:* 15-core-schemas.log and 16-app-published.log: published_*_variants_follow_existing_additive_and_version_policy.
- [x] **A22** — No rubric path changes; the task closes the published-schema enforcement gap without claiming a score. *Evidence:* source/test/log or coordinator report at the destinations above. *(R22)*
  *Recorded evidence:* source-evidence.json and scoped diff; no production, rubric, existing policy/test, dependency or lockfile edits.
- [x] **A23** — Existing checker, validators, policy and tests remain unchanged. *Evidence:* source/test/log or coordinator report at the destinations above. *(R23)*
  *Recorded evidence:* source-evidence.json and scoped diff; no production, rubric, existing policy/test, dependency or lockfile edits.
- [x] **A24** — No runtime or registry edits and no new capability registration. *Evidence:* source/test/log or coordinator report at the destinations above. *(R24)*
  *Recorded evidence:* source-evidence.json and scoped diff; no production, rubric, existing policy/test, dependency or lockfile edits.
- [x] **A25** — No contract metadata changes; documentation explicitly excludes retry/idempotency semantic proof. *Evidence:* source/test/log or coordinator report at the destinations above. *(R25)*
  *Recorded evidence:* source-evidence.json and scoped diff; no production, rubric, existing policy/test, dependency or lockfile edits.
- [x] **A26** — Existing suites pass as part of the full workspace run. *Evidence:* source/test/log or coordinator report at the destinations above. *(R26)*
  *Recorded evidence:* 15-core-schemas.log, 17-app-snapshots.log and 18-app-catalog.log; full suite is owed in 23-final-test.log.
- [x] **A27** — Documentation distinguishes JSON Schema compatibility from catalog values, retry behavior and runtime validation semantics. *Evidence:* source/test/log or coordinator report at the destinations above. *(R27)*
  *Recorded evidence:* docs/control-plane/published-schema-compatibility.md: named variants, standalone commands and Scope and limits.
- [ ] **A28** — Committed dossier names source/base and external validation evidence; coordinator records immutable final head and raw log hashes in the PR. *Evidence:* source/test/log or coordinator report at the destinations above. *(R28)*
  *Recorded evidence:* source-evidence.json records exact source and implementation base; precommit-evidence.json and coordinator handoff will record staged tree, final head and log hashes.
- [x] **A29** — Named tests prove removal, required-field and numeric-bound failures and additive/version-policy behavior. *Evidence:* source/test/log or coordinator report at the destinations above. *(R29)*
  *Recorded evidence:* docs/control-plane/published-schema-compatibility.md: named variants, standalone commands and Scope and limits.
- [x] **A30** — The documentation lists standalone core/app compatibility commands and existing regression commands. *Evidence:* source/test/log or coordinator report at the destinations above. *(R30)*
  *Recorded evidence:* docs/control-plane/published-schema-compatibility.md: named variants, standalone commands and Scope and limits.
- [ ] **A31** — Raw UTF-8 command logs show ordered exit-zero full checks before each commit. *Evidence:* source/test/log or coordinator report at the destinations above. *(R31)*
  *Recorded evidence:* 20-final-fmt.log through 23-final-test.log are required exit-zero evidence before the commit; no future result is claimed in this archive.
- [x] **A32** — Raw UTF-8 guard logs show exit zero after each edit batch. *Evidence:* source/test/log or coordinator report at the destinations above. *(R32)*
  *Recorded evidence:* 06-guards-batch1.log, 08-guards-docs.log, 14-guards-coverage.log; 19-guards-archive.log is owed after archive creation.
- [ ] **A33** — Coordinator supplies final-diff architecture review and resolved findings; pending at implementation handoff. *Evidence:* source/test/log or coordinator report at the destinations above. *(R33)*
  *Recorded evidence:* PENDING: coordinator independent report or exact-head CI at issue #333/PR; this archive does not claim a future verdict.
- [ ] **A34** — Coordinator supplies final-PR AI review and resolved findings; pending at implementation handoff. *Evidence:* source/test/log or coordinator report at the destinations above. *(R34)*
  *Recorded evidence:* PENDING: coordinator independent report or exact-head CI at issue #333/PR; this archive does not claim a future verdict.
- [x] **A35** — Closing step C1 explicitly requires independent final-diff PASS; its eventual verdict is a closing obligation, not a self-referential acceptance criterion. *Evidence:* source/test/log or coordinator report at the destinations above. *(R35)*
  *Recorded evidence:* C1 retains independent delivery PASS as a temporal closing step.
- [ ] **A36** — Coordinator supplies exact-head required CI proof; pending at implementation handoff. *Evidence:* source/test/log or coordinator report at the destinations above. *(R36)*
  *Recorded evidence:* PENDING: coordinator independent report or exact-head CI at issue #333/PR; this archive does not claim a future verdict.
- [x] **A37** — Branch/worktree and private base/tier are verified; no dependency on unintegrated Q1 changes. *Evidence:* source/test/log or coordinator report at the destinations above. *(R37)*
  *Recorded evidence:* 01-arm-guards.log, 02-arm-app.log, 03-mission.log, source-evidence.json and execution-record.md.
- [x] **A38** — Coordinator release precedes exit-zero guard build and app all-target check logs. *Evidence:* source/test/log or coordinator report at the destinations above. *(R38)*
  *Recorded evidence:* 01-arm-guards.log, 02-arm-app.log, 03-mission.log, source-evidence.json and execution-record.md.
- [x] **A39** — GOAL creation follows successful arming and precedes implementation; this archive retains the canonical issue and delegated request verbatim before reviews or markers. *Evidence:* original `Q2-validation/01-arm-guards.log`, `02-arm-app.log`, `03-mission.log`, and `04-freeze.log`, with their original ordered execution records; the source appendix below. *(R39)*
  *Recorded evidence:* this archive, Q2-start-issue.json and archive.py: complete issue/delegated quotations and 60 stable one-to-one R/A pairs plus G1-G4 and C1-C2.
- [x] **A40** — Every R maps to the same-numbered A; G items have no R tail; review/CI and closing evidence remain explicit. *Evidence:* source/test/log or coordinator report at the destinations above. *(R40)*
  *Recorded evidence:* this archive, Q2-start-issue.json and archive.py: complete issue/delegated quotations and 60 stable one-to-one R/A pairs plus G1-G4 and C1-C2.
- [x] **A41** — External logs preserve commands/exits and failure signatures; coordinator is notified before retries and commits. *Evidence:* source/test/log or coordinator report at the destinations above. *(R41)*
  *Recorded evidence:* execution-record.md: no failures/retries yet; coordinator owns remote actions and conditional campaign-base revalidation.
- [x] **A42** — Implementation handoff reports local clean head only; main merge remains exclusively human. *Evidence:* source/test/log or coordinator report at the destinations above. *(R42)*
  *Recorded evidence:* execution-record.md: no failures/retries yet; coordinator owns remote actions and conditional campaign-base revalidation.
- [x] **A43** — Coordinator checks current campaign tip before review/merge; any base advancement triggers update/revalidation. *Evidence:* source/test/log or coordinator report at the destinations above. *(R43)*
  *Recorded evidence:* execution-record.md: no failures/retries yet; coordinator owns remote actions and conditional campaign-base revalidation.
- [x] **A44** — Documentation, mission archive and external validation directory provide scoped diff, requirements, limitations and checks. *Evidence:* source/test/log or coordinator report at the destinations above. *(R44)*
  *Recorded evidence:* this archive, docs/control-plane/published-schema-compatibility.md, source-evidence.json and the raw validation directory.
- [x] **A45** — Read-only preparation reads the required working rules, canonical skills, Codex mappings and campaign integration override before arming. *Evidence:* source/test/log or coordinator report at the destinations above. *(R45)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A46** — No routine permission request is made; implementation and validation continue under the standing delegation. *Evidence:* source/test/log or coordinator report at the destinations above. *(R46)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A47** — No tool command supplies sandbox_permissions; the environment forbids that override. *Evidence:* source/test/log or coordinator report at the destinations above. *(R47)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A48** — Changes stay within enumerated owned paths; any newly required path is reported to the coordinator before editing it. *Evidence:* source/test/log or coordinator report at the destinations above. *(R48)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A49** — Only the repository mission file is created; no child host goal, continuation prompt or subagent is created. *Evidence:* source/test/log or coordinator report at the destinations above. *(R49)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A50** — Implementation verification is handed to independent reviewers; the implementer does not issue architecture/AI/delivery verdicts or markers. *Evidence:* source/test/log or coordinator report at the destinations above. *(R50)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A51** — Every build/test command is captured by Q2-validation/run.py with UTF-8 logs, stderr merged with stdout and explicit EXIT records. *Evidence:* source/test/log or coordinator report at the destinations above. *(R51)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A52** — S4 and the dossier classify runtime impact as zero without claiming benchmark evidence. *Evidence:* source/test/log or coordinator report at the destinations above. *(R52)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A53** — Only Q1 and Q2 implementation ownership is active under coordinator D3 scheduling; Q2 spawns no implementation agents. *Evidence:* source/test/log or coordinator report at the destinations above. *(R53)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A54** — Q2 starts at integrated c3a92d5 and edits no Q1 hooks, skills or integration documentation; independent schema behavior needs no Q1 output. *Evidence:* source/test/log or coordinator report at the destinations above. *(R54)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A55** — The coordinator receives any failure signature and cumulative attempt before a retry; logs retain failures rather than overwrite them. *Evidence:* source/test/log or coordinator report at the destinations above. *(R55)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A56** — No baseline, threshold or compatibility-policy relaxation is used to make validation pass; provenance is verified against exact source blobs. *Evidence:* source/test/log or coordinator report at the destinations above. *(R56)*
  *Recorded evidence:* execution-record.md and raw command logs; all owned implementation paths stay disjoint, no overlap expansion/failure retry was triggered.
- [x] **A57** — Pre-edit/precommit evidence reads this worktree's private mission-base and mission-tier, branch and declared base. *Evidence:* source/test/log or coordinator report at the destinations above. *(R57)*
  *Recorded evidence:* source-evidence.json records branch-bound mission-base and high mission-tier; coordinator confirmed parent #330 grant projection.
- [x] **A58** — Archive is frozen before reviews; later review/CI evidence is recorded outside the reviewed tree by the coordinator. *Evidence:* source/test/log or coordinator report at the destinations above. *(R58)*
  *Recorded evidence:* this archive is created before reviews; external coordinator reports carry later outcomes.
- [ ] **A59** — Precommit handoff includes intended commit, staged tree, declared base, full check exits and raw log hashes before git commit. *Evidence:* source/test/log or coordinator report at the destinations above. *(R59)*
  *Recorded evidence:* PENDING until local commit handoff: precommit-evidence.json, coordinator precommit message, final clean HEAD/status and raw log hashes.
- [ ] **A60** — Postcommit handoff records full HEAD, git status, staged tree equivalence and source dossier/raw-log locations. *Evidence:* source/test/log or coordinator report at the destinations above. *(R60)*
  *Recorded evidence:* PENDING until local commit handoff: precommit-evidence.json, coordinator precommit message, final clean HEAD/status and raw log hashes.

## Injected gates

- [ ] **G1** — English artifacts pass guards and architecture dimension 8. *Evidence:* guard logs and independent architecture report.
- [ ] **G2** — Ordered fmt/clippy/build/test pass on the final current campaign base before each commit. *Evidence:* raw command logs and coordinator final-head/base report.
- [x] **G3** — Performance classification is declared: test-only Rust, rare fixture/document tooling, zero runtime rates. *Evidence:* source diff and S4.
- [ ] **G4** — Independent architecture review resolves every Blocker/Should-fix or explicitly records permitted deferrals. *Evidence:* coordinator report and PR body; pending at implementation handoff.

## Not applicable

No runtime engine or determinism change, so engine test-first fixtures are not required. No UI surface, trader action, capability, port or registry is added; ui-harness, visual-qa, trader-ux-review and new-extension gates do not apply. No hot-path edit requires a benchmark. Cargo.lock, Python MT5 code and shared hooks are untouched, so cargo-deny, MT5 checks and hook tests are not change-specific gates. The full architecture shape review still applies to test code.

## Evidence state at archive freeze

All nine new compatibility tests and the existing targeted core schema/registry
and application schema/catalog tests pass. All 56 released blobs match original
main, and all 59 current contract JSON documents remain byte-identical to original
main and the declared integrated base. No validation failure or retry occurred.

The remaining unchecked local outcomes require final ordered checks and the
precommit/clean-head handoff. The coordinator supplies those logs and temporal
proof outside this frozen archive. Independent architecture/AI outcomes, exact-head
CI and C1/C2 remain expressly owed. This evidence state does not waive any ask.

## Closing steps

- **C1** — Independent delivery-review returns PASS against this archived mission and the final branch after architecture/AI findings and CI are discharged.
- **C2** — Coordinator publishes a draft PR against `campaign/architecture-a`, then completes independent review gates and CI before the authorized intermediate merge, records complete evidence and reports its URL. Implementation agent performs no remote writes.

## Verbatim source requests

Attributed quotation: canonical GitHub issue #333 captured before task implementation (title followed by its complete body).

```text
test(control): guard published schema compatibility

<!-- campaign-task:milocaetano/quantick#330/Q2 -->
## Context

Parent: https://github.com/milocaetano/quantick/issues/330. Stable key Q2. Rubric gap AP2 at a808b2d87b36d73041027e4d20c053544b454a96: generated core and app schema snapshot tests catch unregenerated drift, but regenerating the snapshots can accept a same-version breaking change. `require_compatible_version` is tested against synthetic fixtures and does not guard the actual published generated schema set against an independently retained released baseline.

## Scope

Preserve every existing runtime request/result/error/permission behavior and all currently generated public schemas. Freeze published v1 schema evidence from the known main SHA with provenance and source hashes, separate from the normal regeneration path. Apply the existing compatibility policy to actual core and observer/application generated documents in the normal test/CI path. Keep one compatibility implementation, no runtime validator fork or per-request cost. Define coverage so removal or silently skipping a previously published document fails. Do not alter the rubric, loosen compatibility rules, add a new product capability or correct unrelated retry metadata.

## Acceptance criteria

- [ ] The immutable published baseline records exact main source SHA and schema document identities/hashes; ordinary schema regeneration cannot silently overwrite that baseline.
- [ ] Normal standalone tests compare actual core and observer/application generated schemas against corresponding published v1 schemas using the existing compatibility checker, with explicit coverage for every retained published schema.
- [ ] Missing or removed same-version published schemas fail coverage instead of being skipped.
- [ ] Real published-schema variants prove a required-field or numeric-bound narrowing fails at unchanged version, an additive compatible property passes, and an explicitly bumped-version fixture follows the existing version policy. Tests exercise the production compatibility implementation, not a copied comparator.
- [ ] All current generated JSON fixtures and runtime behavior remain unchanged; normal snapshot/registry/schema tests still pass. State any unsupported contract-metadata comparison honestly rather than claiming schema checks prove retry semantics.
- [ ] Evidence records exact base/head, baseline provenance, meaningful negative/variant assertions, and independently runnable test commands. Full local fmt/clippy/build/test before every commit, guards after edit batches, independent architecture/AI/delivery reviews and exact-head CI pass.

## Campaign record

Owner class: autonomous. Priority: 4. Risk: high (public contract enforcement, no runtime contract change). Estimated scope: schema compatibility fixtures/test integration and concise provenance documentation. Dependencies: none identified; implementation waits for the single lane to be free and starts from latest origin/campaign/architecture-a, with PR base campaign/architecture-a. Rates: test/CI-only, no per-frame/per-trade/per-depth work. Owner, branch, worktree, PR URL/head, Project item: null until claimed/read back. Initial counters operation=0, repair=0; keep cumulative per-signature retry history. Evidence destination: this issue, committed mission/dossier and linked PR/CI. Existing related work #322/#323 and synthetic compatibility tests are reused; this task adds actual published-baseline enforcement rather than duplicating the rubric or comparator. Main merge remains exclusively human.
```

Attributed quotation: complete delegated implementation request from the campaign coordinator.

```text
Own Q2 implementation under Quantick campaign #330, issue https://github.com/milocaetano/quantick/issues/333. User D3 explicitly permits two independent disjoint implementation missions, coordinator serializes campaign merges; main exclusively user. Full command permission, never ask routine approval or set sandbox_permissions. WT C:/src/quantick-worktrees/feat-published-schema-compatibility, branch feat/published-schema-compatibility created at current integrated c3a92d58bb8a41ec4d78d73e60312b5f765b4da5 origin/campaign/architecture-a; private mission-base/tier high set. Read AGENTS/CLAUDE, mission/new-task/ship and campaign override. Canonical issue captured C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q2-start-issue.json; Q2-spec.json original scope; current score https://github.com/milocaetano/quantick/issues/330#issuecomment-5564523480 AP2=4. D3 retained authority-D3.md there and parentcomment5564210429.
Scope: preserve every current generated schema and runtime contract; freeze actual published v1 schema baseline from original main a808b2d87b36d73041027e4d20c053544b454a96 with file identity/hash/provenance, separately from normal regeneration. Normal tests compare REAL core and observer/application generated schemas with immutable released baseline using existing require_compatible_version comparator; no duplicated comparator or runtime validation fork. Explicit coverage prevents removed/missing same-version docs being skipped. Real-schema mutated required-field/numeric-bound narrowing fails unchanged version; compatible optional addition and bumped-version fixtures obey existing policy. No claim schemas prove retry metadata. Include complete stable R/A/G for every atomic/conditional issue obligation, full verbatim issue+delegated request in mission; actual arch/AI/CI requirements pending handoff, own delivery PASS/PR publication remain explicit closing steps per skill. Create GOAL after arming and before implementation, archive BEFORE reviews. No after-review checkbox edit.
OWN: crates/control/tests/ (including reusable test-only support if needed), crates/app/src/app/tests/published_schema_compatibility_tests.rs and bounded tests/mod.rs registration, schemas/control/released/, docs/control-plane/published-schema-compatibility.md, unique .claude/GOAL-archive-published-schema-compatibility.md. Exclude .claude/hooks/skills, docs/campaign/integration.md (Q1 owns these), current generated schemas/control/*.json, existing production schemas/validators/policy, Cargo.lock. If further path needed, report overlap before expansion. No feature/dependency on Q1 unintegrated work. No loosening existing snapshot regeneration/compatibility tests or schema policy.
START READ-ONLY analysis/atomic source-derived requirements now: Q1 owns build/test host through one unchanged budget-test diagnosis/full retry; WAIT for coordinator release before builds/edits. Then arm cargo build -p quantick-guards + cargo check -p quantick-app --all-targets before first edit; guards after edit batches; full fmt check, workspace clippy/build/test in order before every commit. Save exact commands/merged outputs/exits with explicit UTF8 Python subprocess runner outside repo (PowerShell default logs may UTF16/BOM). Do not invent benchmark needs for test-only code. Preserve signatures/counters (max3 per task/failure), stop/report before retries; no threshold weakening or unsupported baseline claims.
Allowed local implementation/tests/commit after green only; notify coordinator before commit for journal. NO push/PR/GitHub/checkpoint/marker writes or self-reviews; coordinator publishes draft then independent gates/CI and authorized intermediate merge. If integration branch advances, update preserved owned edits and revalidate before review/merge. No child host goals/prompts. Return full source-based review dossier/raw logs and exact clean head when done.
```


## Recovery decision, 2026-09-08

The authenticated user's [standing process-resolution instruction](https://github.com/milocaetano/quantick/issues/330#issuecomment-5584842294) authorizes the coordinator to resolve the disclosed Q2 delivery stall without another routine permission prompt. This is a concrete continuation decision, not a claim that the prior two missing ledger facets passed. R41, R42 and C2 now explicitly retain the original source obligations. The original creation-order omission was fixed previously; the two later facets were previously untouched and had no repair attempt. This recovery consumes their first repair attempt, with the existing per-signature maximum of three; all earlier signatures and counts remain in campaign checkpoints.

The child incorporates campaign base `4af299f7489d26631bb49a36674c133709e4a88a` through a history-preserving merge. Its registration conflict keeps both independently added test modules. Required validation, independent architecture/AI/full delivery reviews and exact-head CI remain mandatory. No implementation criterion, schema policy or evidence requirement is deferred. Only `campaign/architecture-a` may receive the reviewed intermediate merge; final main integration remains exclusively human. The original delegated implementer restrictions remain historical scope; the newly assigned coordinator performs publication and integration under D1 and this standing instruction.
