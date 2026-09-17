# Capability registration and admission ownership evidence

Issue [#521](https://github.com/milocaetano/quantick/issues/521), campaign [#472](https://github.com/milocaetano/quantick/issues/472), stable key A1C, tier high. Implementation and observations are complete for review; this is not formal architecture/AI/delivery acceptance, exact-head CI, campaign completion or a score claim. Original criteria and authority remain in the [complete mission archive](../../.claude/GOAL-archive-capability-admission-owner.md).

All implementation evidence below is bound to the uncommitted change over base `a6e554386b037bb2ec49fe29ff6494e4fef0460a`, not a fabricated candidate commit. This evidence-only follow-up changes the archive and this document; runtime, fixtures, dependencies and build inputs remain frozen.

## Durable chronology and requirement map

The [independent source-first map](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706253618), [D9-D11 follow-up](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706285424), [actual harness review](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706387850) and [root baseline audit](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706387994) preceded runtime implementation. [D12](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706370091) permitted only exact-byte test relocation; the [whitespace disposition](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706415634) preserved the designated binary. [D13/runtime dossier](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706422744) approved the narrow borrowed ActionRegistry schema view before the [runtime grant](https://github.com/milocaetano/quantick/issues/472#issuecomment-5706426398).

The [runtime receipt](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706659437) contains source, tests, failures, full checks, byte parity and scratch probe. The separately authorized [observation result](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706696942), [final independent observation review](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706752907) and [D14 disposition](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706750579) retain the measured costs. The final review supersedes 5706746496: that earlier publication lacked the final external-output scope paragraph; readback mismatch was reconciled. None substitutes for current full formal reviews.

| Original criterion / ledger | Concrete evidence | Status at this archive |
| --- | --- | --- |
| A1 / R1 | Eight generic collections and whole transaction moved to private headless owner; app keeps concrete composition/effects | Implemented, awaiting formal judgment |
| A2 / R1,R2 | Public second-host transaction tests, defensive fail-closed tests, exact full errors and permission unions | Local tests and recorded parity pass |
| A3 / R1,R2,R3 | Four full-byte signatures; generated documents/schemas unchanged; external-provider precedence tests | Recorded observable equality, opaque invocation limit below |
| A4 / R4 | Three original test bodies moved intact; eleven public second-host tests; retained primitive/concrete app tests | Full local suite passes |
| A5 / R5 | Frozen six-case corpus plus startup, all paired observations/five allocation fields, source-cost review and explicit D14 acceptance | Trigger disclosed/resolved for proceeding, not no-regression or formal PASS |
| A6 / R6 | Actual fields/decisions/callers/test inventory and external scratch edit-cost probe | Measured source facts, no score/root reduction claim |
| A7 / R7 | Full local checks and frozen-seven identity; formal reviews and exact-head CI still due | Incomplete; A7/G4/G5 remain unchecked |

G1/G2/G3 and applicable G6 have local evidence below. G4 also requires current CI; G5 and G-AI1..4 require current canonical reports/projections and zero unresolved threads. C1..C4, literal What done means, final verifier and reviewed campaign-only integration remain pending. Main/settings stay user-only. No acceptance wording or threshold was weakened.

## Actual ownership and interfaces

[CapabilityContract](../../crates/control-host/src/contract.rs) owns the registry, explicit binding/token map, compiled input/output indexes, ordered profiles, ordered permissions, sorted scope descriptors and scope-permission index. All eight collections are private. `ContractBuilder` registers the supplied ordered authority through existing ControlRegistry, permits explicit module/effect registration, then derives the validated scope catalogue. It does not accept unrelated pre-populated registry/vector pairs or expose `registry_mut`/Deref.

`register_read` reuses atomic admission registration; `register_external` establishes an explicit External binding. Failed schema/duplicate registration cannot leave reachable partial state. Read absence is not an action route. `admit` owns envelope/static admission, binding/schema/key/tier checks, read preparation, dynamic authorization and final permission union. The callback receives only token, payload and borrowed `ScopeCatalogue`; it prepares but does not execute effects. A value prepared successfully is dropped inside the owner when dynamic authorization fails. App receives only the finalized route. Existing admission/catalogue primitives are reused, not forked.

Output validation first proves exact registration through the read output index or explicit external binding. Matching external schemas retain precedence; mismatched identity or missing external provider fails closed. Read with no provider uses its registered validator. Provider identity is trusted host composition, not cryptographic protection against a malicious host lying about its validators. App's ActionRegistry registration already binds actual descriptors and compiled schemas.

[ObserverContract](../../crates/app/src/control/contract.rs) changes from ten fields (eight generic collections plus actions/evidence) to three (owner, same ActionRegistry Arc, same EvidenceStore). It retains vocabulary, declaration order, safe grants/sensitivity, concrete handlers, build/instance describe assembly, action execution/canonical input and effects. [Reads](../../crates/app/src/control/contract/reads.rs) query narrow scope permissions. [ActionRegistry::schemas](../../crates/app/src/control/actions.rs) borrows stored descriptor identity/input/output; existing owned lookup and execution callers are unchanged. Pointer-identity/wrong-version tests cover this D13 support.

Production consumers remain gateway handshake registry access, gateway prepare/output checks, inventory/retry-matrix discovery and concrete worker/UI reads. Public second-host integration tests are a real API consumer, not a second production application. QuantickApp fields, root branches, impl spread, dependency direction and concrete UI/financial/feed/series/indicator/layout behavior are unchanged. No new dependency, clock, lock, GUI construction, generic framework or per-request catalogue/schema compilation is introduced. Rate is per control request plus construction, not per trade/depth/frame; request frequency was not measured.

Physical extents including signature/braces: app `prepare` 54 -> 32 lines, new headless `admit` 68; original app contract snapshot 1457 lines (includes three preparation-registration lines), candidate 1304; headless owner 307. These are source extents, not scanner grades or a claim of universal code reduction.

## Tests, observable parity and UI applicability

[Public second-host tests](../../crates/control-host/tests/capability_contract.rs) exercise two opaque reads and one writable external action via production construction/registration/admission/output APIs. Eleven tests include static denial with zero callbacks, envelope/payload/version precedence, key/dry-run/revision policy, dynamic permission union and sorted complete denial details, denied-value Drop and zero effects, invalid input/output/duplicate registration atomicity, missing/mismatched external schemas, output precedence, ordered metadata and scope queries. [Internal defensive tests](../../crates/control-host/src/contract/tests/fail_closed.rs) inject otherwise-unconstructible missing binding/input/output states and prove fail-closed behavior.

Three original app generic tests moved with exact bodies/assertions, apart from one nesting indent: forbidden read idempotency key, strict dry-run and supplied expected revision (textual comparison fbbd00). Five original concrete app tests remain, adapting only private collection access/registration to the owner. Real second-handler worker execution remains. Original primitive `admission_contract.rs` tests are unchanged. Existing gateway queues/timeouts/late-work/cancellation/idempotency/uncertainty, token refusals, observer-action denial, paper/replay and coherent captures remain covered by the full suite; no assertion was weakened.

Frozen additive [fixtures](../../crates/app/src/control/contract/tests/fixtures.rs), [observations](../../crates/app/src/control/contract/tests/observations.rs) and [preparation](../../crates/app/src/control/contract/tests/preparation.rs) use the real standard projection registry, actions, evidence and contract without a window. Complete parity includes original and returned envelopes, full ControlError including details/retryable/next steps, required-permission union, route labels, describe and permission policy. Worker/Ui trait-object invocation internals cannot be serialized; admission parity is not a proof of all execution contents. Preserved concrete execution tests are separate evidence.

Optimized parity ran with `A1C_SIGNATURE_DIR` unset and `A1C_COMPARE_DIR` the immutable original baseline: exit 0, one test passed. Separately emitted candidate output into a fresh distinct directory and independently compared every byte of all four files through bijective Base64 equality, not hash-only equality. Baseline hashes before/after stayed unchanged. Both raw and normalized describe match; normalization only replaces actual build commit with `<actual-build-commit>`.

| Signature | Bytes | SHA256, identical A/B |
| --- | ---: | --- |
| describe.raw.json | 514633 | 39B4E8F04BD1D5A9BD5346F56E58B04E51B3814AA7BB3C9FA9BEA868471D43FA |
| describe.normalized.json | 514614 | CD3931693D18792D74519DB7E497370074B02B0B37803C889A1BB8A802742B0E |
| admission-outcomes.json | 40067 | 64B13A7A60F0223FD5A97C1D84112F7D903B83C53861BA52D54A2EBA1EDB2B09 |
| permissions.json | 8011 | 1AC6667690096E7B866893C8CDE3D72423D38A3C8C45A072BE03AD2FF5D98049 |

Generated [capability inventory](../control-plane/capability-inventory.md), [catalogue](../../schemas/control/observer-capability-catalog-v1.json), every file under [schemas/control](../../schemas/control), [UI behaviour matrix](../control-plane/ui-behaviour-matrix.md), [retry matrix](../control-plane/retry-matrix.md) and [generated hook registry](../../.claude/skills/ui-harness/references/hook-registry.md) are unchanged. Full-suite generated-contract equality tests/guards passed. This changes no visual behavior or mouse-only surface; public contract parity and retained app integration are applicable evidence, not a screenshot or visual-QA PASS. Existing ordinary core/max-chart-window capture-budget tests pass; pre-existing manual p99 variants remain ignored.

## Frozen observation protocol and complete results

Timings are **matched instrumented app-test batch averages**: the existing thread-local work_meter counter is always active. No new allocator, CPU/priority/power change or GUI/action execution. Setup/serialization/logging are outside windows; requests include the same envelope clone, prepare, black-box and Result drop. Startup constructs/drops the whole standard composition, not isolated schema compiler time.

Baseline-only calibration performed three untimed operations, started at 1000 iterations (startup 1), doubled with a clamped ceiling 1048576 (startup 128), and froze the first block >=250ms. All pilot samples were retained; no ceiling was exceeded or recalibration used. D12 relocated the three harness files byte-identically under `contract/tests`; retained original calibration/allocations, rebuilt designated baseline and proved all signature bytes unchanged. Designated compiled contract differs from final baseline source only by removal of one additive blank line, with both snapshots retained. The redundant later build was stopped and never replaced the designated binary.

| Case | Fixed operation / grant | Count |
| --- | --- | ---: |
| Small read | control.describe `{}`, default grant -> Worker | 512000 |
| Dynamic snapshot | snapshot.read scopes system.info/health.summary; observe, observe.system, observe.health, observe.indicators, observe.orderflow -> Ui/full union | 128000 |
| Static denial | snapshot.read `{scopes:42}`, empty grant -> PERMISSION_DENIED | 1024000 |
| Malformed payload | same malformed payload, default grant -> INVALID_REQUEST | 512000 |
| External action | attention.mark.create `{}`, annotate + annotate.attention -> Action, no execution | 512000 |
| Output validation | complete describe and invalid `{}` -> true/false, two validations/operation | 4000 |
| Startup | fresh whole standard composition + destruction | 8 |

External mark output true/false and version failure are mandatory deterministic parity only, not part of timed output validation. Other parity cases cover malformed envelope, strict key/dry-run/revision, version mismatch and sensitive dynamic denial.

Comparison001, root handle 86887 exit 0, ran 2026-09-17 00:42:18.0012314Z–00:43:42.0608087Z. Three independent-process pairs AB/BA/AB per case, three same-count warmup blocks then one measured block per process; afterwards one separate allocation process/case/side with three windows. All 56 child receipts exited 0: 126 warmup rows, 42 timing rows and 42 allocation rows retained/audited. No retry, timeout, capture/lifecycle error, exclusion or concurrent build. The review trigger is per-case paired median >1.05, not pooled acceptance.

| Case | Pair 1 B/A | Pair 2 B/A | Pair 3 B/A | Median | Observed range |
| --- | ---: | ---: | ---: | ---: | --- |
| Small read | 0.9048323362 | 0.8914585853 | 0.9289942436 | 0.9048323362 | 0.8914585853–0.9289942436 |
| Dynamic snapshot | 0.9467705134 | 0.9935195054 | 0.9612668295 | 0.9612668295 | 0.9467705134–0.9935195054 |
| Static denial | 1.0542905900 | 0.8909156832 | 1.0563268557 | **1.0542905900** | 0.8909156832–1.0563268557 |
| Malformed payload | 0.9572481681 | 0.9250070448 | 0.9134939836 | 0.9250070448 | 0.9134939836–0.9572481681 |
| External admission | 0.9455076913 | 1.0042421232 | 0.9960646944 | 0.9960646944 | 0.9455076913–1.0042421232 |
| Describe output pair | 1.0086089166 | 1.0125273431 | 1.0380057530 | 1.0125273431 | 1.0086089166–1.0380057530 |
| Whole startup | 1.0324104601 | 0.9708549869 | 1.0517278474 | 1.0324104601 | 0.9708549869–1.0517278474 |

All three allocation windows agree for every side/case. Counter order below: allocation calls / requested allocation bytes / realloc calls / copied realloc bytes / largest realloc copy. First four are exact per-operation quotients without truncation; fifth is the raw **undivided window maximum**. These are not live/peak heap, deallocation or total requested realloc bytes.

| Case | Baseline five fields | Candidate five fields |
| --- | --- | --- |
| Small read | 9 / 399 / 0 / 0 / 0 | 8 / 383 / 0 / 0 / 0 |
| Dynamic snapshot | 31 / 2575 / 1 / 8 / 8 | 30 / 2562 / 1 / 8 / 8 |
| Static denial | 10 / 914 / 0 / 0 / 0 | 10 / 914 / 0 / 0 / 0 |
| Malformed payload | 14 / 1255 / 1 / 17 / 17 | 14 / 1255 / 1 / 17 / 17 |
| External admission | 10 / 438 / 0 / 0 / 0 | 11 / 459 / 0 / 0 / 0 |
| Describe output pair | 5 / 381 / 0 / 0 / 0 | 5 / 381 / 0 / 0 / 0 |
| Whole startup | 252048 / 26575169 / 22882 / 3769655 / 9024 | 252100 / 26579351 / 22882 / 3769655 / 9024 |

### Observed costs and explicit D14 disposition

Static denial triggered review: median +5.429%. Raw A/B block nanoseconds are 456898700/481704000, 514929200/458758500, 447023000/472202400. At fixed count, ns/request are 446.1901/470.4141, 502.8605/448.0063, 436.5459/461.1352. The two AB increases are about 24ns/request; the BA pair reverses by about 54.9ns. All remain. Unchanged primitive denial returns before new binding/provider/schema/preparation work; the external ID clone cannot explain this case. Identical allocations do not prove identical instructions. Codegen/layout/environment contributions are unknown; no disassembly/diagnostic was performed. Three pairs establish neither p95/CV/noise nor a production/worst-case deadline bound, and request frequency is unmeasured.

External **admission** adds one owned ID-key allocation before borrowed action lookup: measured +1 call/+21 bytes for attention.mark.create, with validated ID maximum 128 bytes. Original action lookup already owned an ID; the added explicit-route map explains the extra allocation. Removing an Arc clone does not remove a heap allocation. Existing payload/schema/history cloning is unchanged. External **output** also adds an explicit binding ID clone by source inspection but was only checked deterministically; neither these allocation numbers nor describe-output timings measure the full external round trip.

Startup adds 52 allocations/4182 bytes. The binding map now holds nine reads plus 45 external versions; the extra 45 ID keys account for 894 bytes. Remaining seven allocations/3288 bytes are consistent with enlarged BTree storage, not allocation-stack-traced node attribution. No extra external schema compilation or realloc growth. This scales with configured capabilities at construction, not request history/market data. Read paths no longer probe ActionRegistry; faster observations do not offset slower cases.

D14 explicitly accepts the observed timing and bounded allocation costs **for proceeding to formal review**: a real headless transaction owner encloses preparation and dynamic authorization, and source inspection found no new input-amplified work, request-time compilation, lock, GUI, catalogue reconstruction or payload/history clone. Original D11's >1.05 trigger was review, not a zero-cost ceiling. This is not no-regression/flat/noise, automatic PASS because the delta is small, or causal proof. Formal reviewers retain independent judgment. No new timing retry, sample rejection, threshold adjustment or speculative repair was used.

## A6 localized additive registration probe

Actual external before/after source copies and diffs added `control.probe` using the existing describe handler/schema at the corresponding production registration site. Baseline: one file, one site, 15 insertions/0 deletions, four mutable collection arguments. Candidate: one file, one site, 11 insertions/0 deletions, `owner.register_read`. New caller/field/branch sites: zero on both sides. The registration site remains; no root ownership reduction is claimed.

Artifacts `a1c-probe-baseline/{before.rs,after.rs}`, `a1c-probe-candidate/{before.rs,after.rs}` and matching `.diff` files are retained with the runtime receipt. Baseline after SHA256 `F8397F7E27FF08B859576AB6F796EB15EDB28E6E5DCFEC4725F4F60578D981E5`; candidate after `4C6EBD197AC69FA9DA05FE595FBC4836A3ECA01EE9E46C4BA1084B8334E954BC`. This probe was uncompiled and never changed the worktree: edit-cost evidence only. The preserved actual second-handler execution test is separate executable docking proof. No score, denominator padding or incremental rebuild result is inferred.

## Validation, failures and evidence reuse

Full ordered runtime loop handle 18204 exited 0: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `cargo test --workspace`. Totals: 4016 passed, 0 failed, 24 ignored across 120 suite summaries; app 2085 passed/13 ignored, control-host 76 tests, guards 258 passed. Formatting was silent; exit recorded in the handle, not an invented log. Clippy/build/test logs and hashes are below. Optimized candidate build handle 13174 exited 0, `cargo test -p quantick-app --release --no-run`, same actual `QUANTICK_GIT_COMMIT`, followed by exact-byte parity. No Cargo.lock, manifest, build-script, hook, guard or rubric change; unrelated Python/MT5 checks and cargo-deny were not triggered.

Preserved initial failures: preparation fixed guessed MarkResult enum strings before measurements and one additive formatting difference. Guard initially found +447 UI-free lines from test placement; D12 moved exact bytes into recognized tests. First relocated build 52213 failed E0583 (explicit-path sibling module resolution), fixed only module paths. Next guard was +1 from an additive blank, then removed; guards 258/fmt 0. Designated binary kept its compiled snapshot with that blank, final source snapshot without it. Redundant build 75286 was safely stopped at its own verified rustc process, terminal 101; it was not a source failure or substitute baseline. Original receipts remain.

Runtime owner attempts 1–3 each initially failed eleven synthetic tests for scope namespace, capability namespace, then inherited examples inconsistent with integer output schema. Attempt 4 passed ten/failed one because a required-revision fixture remained read-only. Corrections affected only new synthetic descriptors/examples, never production validation or frozen measurement fixtures. Attempt 5 passed the then-75 owner tests, before the final defensive test brought the suite to 76. Two patch context/order failures were atomic/no edit. Initial app check 31757 and initial Clippy 49427 passed. All attempt logs remain; no formal repair counter was reset or failed sample excluded.

The archive follow-up reuses the full runtime loop and parity **only after rehashing all eleven frozen files** and checking that no executable/build/dependency/generated input changed. New execution is proportional repository guards plus diff hygiene, local-link/mission-preservation and input-identity checks. Exact follow-up receipts are recorded in the coordinator's archive report; final formal reviews/CI are not reused or claimed. Skill-driven archiving preserves the original mission and separates historical stage authorization from current readiness.

Archive checks: `cargo test -p quantick-guards` terminal a330fe exited 0, all 258 tests passed. Identity/link script terminal 01f9c7 exited 0: HEAD plus all eleven source hashes and seven frozen blobs match; exhaustive changed/untracked-path allowlist contains only those eleven files and these two Markdown documents; staged index unchanged; tracked diff hygiene, both new documents' UTF-8/no-BOM/whitespace, all 17 local links, and complete original mission text-prefix preservation pass (line endings normalized). All 125 generated/schema paths listed in the receipt are unchanged from base, including released schemas. Remote links use coordinator-provided durable readbacks, not new network requests. The first script launch was blocked by local PowerShell execution policy before execution; its wrapper's exit 0 was not a check PASS. A separate process-local `-ExecutionPolicy Bypass` launch executed the owned read-only check script successfully without changing persistent settings. No full suite or application measurement was repeated.

## Provenance and immutable identities

External raw artifacts live under `C:/src/outside-eight-coordination/`; durable linked issue reports above preserve the findings and hashes. Toolchain: rustc 1.98.0 (`88d9e12ae178fab0fb5cc050a94da85685d449ea`, 2026-08-18), LLVM 22.1.8; cargo 1.98.0 (`797e8a9bc`, 2026-08-05), x86_64-pc-windows-msvc. Host Intel i5-12400F, six physical/twelve logical cores. No CPU/priority/power changes.

| Artifact | SHA256 |
| --- | --- |
| Original calibration binary | 3625C846797496A413A6C92171514F5A6E163F2D3CEF81B7CC264A6192E5F50A |
| Designated D12 baseline binary | 0320A9824D897188902FFB79A2AFE9EE1C1FD260432105462BE340C20DFFFF81 |
| Candidate binary | A5A326156E3BEADA893DCC5E4E4954EF415514D49BDE449FA7E92B754C82A50F |
| Frozen pair runner | 1AFCED7FD295DC7731016BD63C02D070CF2F86B18EC2D10F75A46957B4542B2D |
| Candidate manifest | E9E6D319EAD9566ABDF657DFD9A5D36841F7EE7529A8923C8FE134C3E75AECDF |
| Runtime result | 3492357F5CF58AE2F1072C5ED8674C25E49D2CBD25645EA691E09192389BCE7C |
| Final observation review | 9D1F5A2BA31F99C95D029BA9B02E7C95BAD1EC8650B3262DBA3836CD20C5E8F3 |
| Full Clippy log | 2CA563724493462CC07FF9BD4F8A9AE863A18EFC1153205B4DAC1F6D2A94A616 |
| Full build log | D5130DB9923AED750018C9E5000E095D1E015668C99E93D4E33ED3D3C04FCE87 |
| Full test log | 8A372514EE9B980AD7D0EA9F0B069B7F8DD26986F3037CF1D5546E001E70153F |
| comparison001 receipts.json | 8BD37A1C7652BFFD071C927F58902C4F12A344B6E81419CEE4D407F531E506F4 |
| timing-samples.json | EF551E343A09E539368DD4957AF026688A57735C2B28B1DB6ED04FA326850F1D |
| allocation-samples.json | 26A33448AE5CF5D842786D4E2B7BDF40D8246D2619782F042E681E279CEAFCA2 |
| summary.json | 1846A131D681AE1D728E5C7A12C443C80DB16AECC401F4F27F19EB1E033897B9 |

All binaries are named `quantick_app-3e589fbc2400c2b9.exe` inside the separate `a1c-baseline-bin`, `a1c-baseline-d12-bin`, `a1c-candidate-bin` directories. The frozen parity filter is `control::contract::preparation::complete_contract_and_admission_signatures`. Raw calibration/allocation/log/source manifests retain every row and count; baseline emission alone was never labelled parity PASS.

| Frozen file (under crates/) | SHA256 |
| --- | --- |
| app/src/control/actions.rs | 227D1A474EBE7000BBFA52C4907AFF5D469372FA59CB05B37766979A6D3BD2B6 |
| app/src/control/contract.rs | C711649C06BA0BBAD46408785D32DDDDF43DE5788423E37A8D04C823E5D2C25E |
| app/src/control/contract/reads.rs | 11D45C111E54CCCC9AAE937FC4EA389C63A43D0E4E35C0D3A07A7DDE83624928 |
| control-host/src/contract.rs | E9A52E0A1171BA9F67B05826421586E6983A0645219873F5D3B2A5843BA5BAAD |
| control-host/src/contract/tests/fail_closed.rs | 5E70DA17E9006CA3C55317BE82788DA4EA0F61F1A04FD9F84A8C003323964A56 |
| control-host/tests/capability_contract.rs | D65AC6D74BB6C708FF095C00D5BB059D901BB35FB80F821000085443355468C4 |
| control-host/src/lib.rs | 4C112D111D055197EC0DB80FAA838ED3A39379A20B026D3E1100BAF653ABB93A |
| control-host/README.md | 738B0B4525381B589C57D917A7BBDC76E35B0C4439605EBCAF1B19DB12F40F5A |
| app/src/control/contract/tests/fixtures.rs | 0EFAC8BD1A699C1D49BC3B243492F90FBFDC3062A0C2577F91113B99F3624AF8 |
| app/src/control/contract/tests/observations.rs | CD6C0C6999BC5DD8858A7142D23436BF5A4282159EF2664BA264873CE04B946C |
| app/src/control/contract/tests/preparation.rs | F07953F89E2E96635348C82E99799C021DD0E23080140E55D0A1D757206BE9B4 |

Frozen campaign seven are unchanged Git blob IDs (SHA-1, not SHA256):

| Path | Git blob |
| --- | --- |
| docs/quality/outside-score-rubric.md | 0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76 |
| tools/outside_score/measure.py | 494ff11e6231f513d500fe935fe27fe3c2d2e2e5 |
| .claude/skills/outside-score/SKILL.md | 81d101d7e0fad73f1390977be7671545da46ec51 |
| .agents/skills/outside-score/SKILL.md | 45cc224fa29f2881601e03fe462e242bd50e8521 |
| .claude/skills/quantick-score/SKILL.md | a0d008fbab2565fcfa1fc1b6894b075bf47f3526 |
| .agents/skills/quantick-score/SKILL.md | 5489246f5748b8b8abc80696d88cb75e331d6179 |
| docs/quality/quantick-score-rubric.md | c7fd2af3aa108abdff7b3083b4321fd941d8a813 |

No rubric/scanner run or score improvement is asserted here. Aggregate campaign/main assessments, current formal review gates and final exact-head CI remain outstanding.
