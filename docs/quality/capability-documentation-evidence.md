# Capability documentation evidence

Issue: [#348](https://github.com/milocaetano/quantick/issues/348), campaign Q9. Source base: `31c18053485779ddcfa96c7c299fc8b3f1cb8033`; branch: `fix/capability-documentation`; review tier: high.

AGENTS.md now points to the existing generated [inventory](../control-plane/capability-inventory.md) and [catalog](../../schemas/control/observer-capability-catalog-v1.json), with no copied capability/module/snapshot/permission totals. It retains `quantick_describe` as the running-instance authority.

The [new integration test](../../crates/guards/tests/capability_documentation.rs) uses the existing public `quantick_guards::generated::check` and shared `ScratchDir`. A complete literal fixture first passes, including every hook-side input. Adding the independent `orderflow.l2.read` declaration in `control/orderflow.rs` yields exactly its missing-row diagnostic. The literal second row/footer restores a clean result; removing only that declaration yields exactly the orphan row at inventory line 5. Separate cases assert the complete finding sets for missing marker, stale footer and missing inventory. No finding is filtered away.

The fixture covers the existing formatted, one-line `*_CAPABILITY_ID` source grammar. It does not claim to recognize inline registrations or to validate runtime registry membership. The expected IDs, inventory rows and diagnostics are authored literals, with no call to the scanner or renderer to produce an expected answer. The preparation proposal for an additional external mutation experiment was not selected by issue #348 and is not claimed as evidence.

Canonical authority is checked by existing application tests: `control::inventory::tests::the_committed_inventory_is_what_the_generator_emits` compares inventory bytes with the live registry renderer; `app::tests::control_plane_tests::observer_capability_catalog_is_registry_derived_and_versioned` compares the committed catalog JSON with `control::schema_catalog::capability_catalog()`. Both passed in the full workspace run below. The new orientation test checks canonical link destinations, their existence, the inventory generation marker and the discovery entry point without freezing a prose sentence. Production generation and comparison code is unchanged.

All touched paths run only during documentation reading or tests. There is no per-trade, per-depth or per-frame change, no runtime performance impact and no new product capability. Only AGENTS.md, the new integration test, the unique mission archive and this evidence document are owned. Runtime/financial logic, public contracts, schemas/catalog, inventory, registry/generator sources, production guards, thresholds/baselines, existing tests and the rubric retain their base bytes.

## Recorded validation

External receipts and complete logs are under `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q9-validation`. Each command has an immutable `.json` receipt, `.log` and `-source.json` manifest. The receipt records exact argument vector, worktree, UTC start/end, exit code, source HEAD/status, SHA-256 manifest/log hashes and any child environment override.

| Receipt | Exact command | UTC start | UTC end | Result |
| --- | --- | --- | --- | --- |
| `01-arm-build.json` | `cargo build -p quantick-guards` | 2026-09-08T07:11:28.550382+00:00 | 2026-09-08T07:11:41.913483+00:00 | exit 0 |
| `02-arm-check.json` | `cargo check -p quantick-guards --all-targets` | 2026-09-08T07:12:14.624427+00:00 | 2026-09-08T07:12:15.909553+00:00 | exit 0 |
| `03-batch-guards.json` | `cargo test -p quantick-guards` | 2026-09-08T07:15:12.623994+00:00 | 2026-09-08T07:15:20.100083+00:00 | exit 0 |
| `04-focused.json` | `cargo test -p quantick-guards --test capability_documentation` | 2026-09-08T07:15:34.244160+00:00 | 2026-09-08T07:15:34.710227+00:00 | exit 0 |
| `06-format-guards.json` | `cargo test -p quantick-guards` | 2026-09-08T07:16:09.869170+00:00 | 2026-09-08T07:16:12.352417+00:00 | exit 0 |
| `07-fmt.json` | `cargo fmt --all -- --check` | 2026-09-08T07:16:31.064508+00:00 | 2026-09-08T07:16:33.614047+00:00 | exit 0 |
| `08-clippy.json` | `cargo clippy --workspace --all-targets` | 2026-09-08T07:16:40.778883+00:00 | 2026-09-08T07:18:29.039872+00:00 | exit 0 |
| `09-build.json` | `cargo build --workspace` | 2026-09-08T07:18:34.668232+00:00 | 2026-09-08T07:22:11.729247+00:00 | exit 0 |
| `10-workspace-test.json` | `RUST_TEST_THREADS=1 cargo test --workspace (child process only)` | 2026-09-08T07:22:32.302644+00:00 | 2026-09-08T07:26:50.418597+00:00 | exit 0 |

The focused target passed all 7 tests: 5 capability/orientation cases and 2 tests from the shared scratch module. Ordinary guards also passed. Full workspace tests used the authorized child-only `RUST_TEST_THREADS=1`, with no test filters, ignored-test changes or budget changes. The original logs preserve the suite's existing ignored tests.

This table records the completed source-validation round before the evidence and checklist update. After these documentation edits, the final-source round repeats batch guards, the focused target and ordered fmt/clippy/build/workspace tests. Its authoritative receipts are `11-final-guards.json` through `16-final-workspace-test.json`; `frozen-verification.json` binds their actual results to the entire final Git tree and source manifest. This document does not claim its own recursive hash. Reviewers should require that final receipt to establish final-source success.

| Behavior source | SHA-256 |
| --- | --- |
| `AGENTS.md` | `4096f0653e82a3838b97013abf1620e37e5c4bf7b2cc66cd2250f297e5782976` |
| `crates/guards/tests/capability_documentation.rs` | `3d6fe73cbe48a0c414aeb1aca9f5c176e524f5979f6b5fa982fc41b3dc79118a` |

The immutable `mission-original.md` and `mission-receipt.json` were recorded after both arming checks and before the first source edit, against the clean base. They preserve the complete issue and root directive, high-tier parse, atomic R/A mapping, existing-authority assumptions and closing order. The original directive is separately retained as `Q9-original-directive.txt`. Three administrative read-path errors were retained in the mission receipt and corrected under the root's bounded administrative grant. There have been zero product/check failures. The independent early mission-source audit identified two missing ledger mappings, retained under `Q9-precommit-mission-source-mapping`, attempt 1, two findings awaiting independent source-completeness closure. [Repair grant 1](https://github.com/milocaetano/quantick/issues/348#issuecomment-5581028995) authorizes appended R15/A15 and R16/A16; the original mission/receipt remain unchanged. The correction adds no product scope and does not claim a delivery verdict.

## Authority reading and application

During preparation, the implementer read the actual issue, repository AGENTS/CLAUDE rules, canonical mission/new-task/ship skills and Codex mappings. This is an acknowledgment of the actual session reads and their application, not a reconstructed historical transcript or per-file timestamp record.

| Authority read | Application in this change |
| --- | --- |
| [Issue #348](https://github.com/milocaetano/quantick/issues/348) and saved root directive | Four owned files, high tier, public guard fixtures, literal diagnostics, source freeze and root-owned publication. |
| [AGENTS.md](../../AGENTS.md) and [CLAUDE.md](../../CLAUDE.md) | One-way dependency/leaf constraints preserved; English artifacts; pre-edit arming, batch guards and ordered mandatory checks. |
| [Canonical mission](../../.claude/skills/mission/SKILL.md) | High-tier parse, original source ledger, stable R/A/G/C identifiers, assumptions, immutable pre-source mission and archive in final source. Existing authority resolves routine questions. |
| [Canonical new-task](../../.claude/skills/new-task/SKILL.md) and [campaign integration](../campaign/integration.md) | Reused the root-created clean isolated worktree after checking branch, base, private mission context and ownership. Campaign base is explicit; root retains issue/Project publication and integration. |
| [Canonical ship](../../.claude/skills/ship/SKILL.md) | Ordered validation, source freeze before current reviews, exact-head CI and root-owned readiness/authorized campaign merge; no main merge. |
| [Codex mappings](../../.agents/references/codex-compatibility.md) through [.agents mission](../../.agents/skills/mission/SKILL.md), [new-task](../../.agents/skills/new-task/SKILL.md) and [ship](../../.agents/skills/ship/SKILL.md) | Canonical outcomes preserved while commands use the Windows host; no extra user question, child goal or permission flow. |

## Handoff and remaining external steps

The mission archive belongs to the frozen source. `handoff.json` records the actual final base readback, source tree, command receipts and release of implementer control. The implementer stops before commit, push, PR creation, GitHub writes and marker projection. Root owns current independent architecture/delivery/AI reviews, publication and exact-head CI. The architecture gate and closing steps remain external and are not claimed complete here.

After those reviews and current green CI, root may perform only the authorized serialized campaign merge. It must then reassess the actual integrated SHA under the unchanged rubric and synchronize issue/Project completion. No score is awarded by this fixture or document; no merge or future reassessment is a prerequisite to its own pre-merge source review. Main integration remains exclusively the user's action.
