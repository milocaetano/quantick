# Mission: prove Windows descriptor authority boundaries

**Objective:** Prove the existing Windows descriptor authority policy over actual filesystem objects with independent fixture oracles and maintained Windows CI execution.

**Why:** Campaign #330 Q4 / issue #335 supplies missing Windows execution evidence without changing policy or claiming stronger foreign-descriptor coverage.

**Tier:** high, because the work proves a security boundary and changes its CI execution gate.

## Request ledger

- **R1**: Prove valid private publication, discovery, read and removal with independently inspected owner, protected DACL and principals.
- **R2**: Prove Everyone-read descriptor refusal while a valid peer survives.
- **R3**: Prove untrusted directory ACL refusal before enumeration without publisher repair.
- **R4**: Prove actual explicit-directory and .json-entry junction rejection, unchanged target canary and safe cleanup.
- **R5**: Prove actual read-only foreign-owned OS directory/file rejection before enumeration/parsing, independently comparing both token identities and retaining the weaker-proof limitation.
- **R6**: Distinguish policy refusal from setup/IO failures; preserve all runtime, public, security and financial contracts with test-only machinery and no skips.
- **R7**: Add control-local execution to pinned Windows CI while preserving existing Windows build/tests and Linux workflow.
- **R8**: Preserve final-revision evidence, prerequisites, commands/exits, logs/CI URLs, local verification and independent review closure before campaign integration.
- **R9**: Supply reproducible evidence for unchanged-rubric reassessment without automatically increasing AP3.

- **R10**: Persist the mission before source implementation; retain any historical deviation without backdating or self-waiver.
- **R11**: Edit only assigned Q4 paths; report indispensable ownership adjustments and exclude Q2/Q3 paths.
- **R12**: Retain explicit host release before arming and serialize all builds, tests and measurements.
- **R13**: Complete ordered fmt/clippy/build/workspace-test before every commit and guards after edit batches.
- **R14**: Report failures and cumulative signature counters before retries and preserve campaign stop limits.
- **R15**: Worker produces local validated commit/handoff; coordinator owns all GitHub writes, review markers and merges.
- **R16**: Never print descriptor tokens or credentials in oracle output, diagnostics or evidence.
- **R17**: Create no account and enable no privilege for Windows fixtures.
- **R18**: Request/use no credentials and mutate no system ownership or ACL.
- **R19**: Preserve owner-or-token-owner compatibility, trusted principals and reparse policy.
- **R20**: Preserve descriptor format, transport, runtime/public contracts and all financial rules.
- **R21**: Keep test machinery in a cfg(test), cfg(windows) discovery child with no public test API or alternate production implementation.
- **R22**: Use latest integrated campaign base at start; update and revalidate when it advances before merge.
- **R23**: Preserve original sources verbatim and stable ledger/counters; do not manufacture history.
- **R24**: Archive mission before independent review and retain architecture, AI, source-first delivery and exact-head CI closure before campaign merge.
- **R25**: Use no child host goal or routine user permission prompt.
- **R26**: Restore scratch access and remove junction links before recursive cleanup; never follow an unowned target.
- **R27**: Use clear fixture-prerequisite errors with no ignored tests, silent skips or weakened assertions/guards.
- **R28**: Use no paid service or deployment and preserve human-exclusive main merge.

- **R29**: Apply only the approved cfg(test) fake_gateway send-before-reply ordering fix; preserve encode, unwraps, all caller assertions and production protocol. Verify all callers and retain the original panic evidence.

- **R30**: At the next explicit host grant, execute the exact existing capture-budget test in isolation as diagnosis, retaining executable identity and all output; do not substitute it for workspace acceptance.
- **R31**: After that diagnosis, execute only the authorized single unchanged full ordered retry, preserving source, runner concurrency, thresholds and cumulative failure history; stop/report a failure without another retry.

- **R32**: Repair Windows CI fixture availability by removing only the oracle child PSModulePath; preserve all policy and assertions. Actual CI module-path cause remains unproven without runner environment evidence.
- **R33**: Prove a controlled inherited-incompatible-module-path negative/positive through existing oracle/tests, retaining unfixed binary/source identity and logs; no global environment mutation, module installation or machine policy change.
- **R34**: After bounded CI repair1, execute six Windows tests, full crate, guards and ordered workspace checks, then freeze/report before commit; coordinator obtains new-head CI/reviews. Stop/report first unexpected failure, preserve all signature budgets and cleanup restrictions.

## Decisions and authority

- **D1**: Issue #335 is the task source; parent campaign #330 and D4 https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109 authorize independent implementations with serialized host use and campaign merges. Main merge remains exclusively human.
- **D2**: Coordinator owns remote writes, independent reviews, review markers, CI and merge; this worker delivers a validated local commit/handoff.
- **D3**: Host release https://github.com/milocaetano/quantick/issues/335#issuecomment-5568958659 was retained with UTC receipt in external Q4-validation/host-release-received.txt before arming. Three arming commands exited 0, then the host was released to Q3 before edits.

## Assumptions

- **S1**: Inbox Windows PowerShell's .NET ObjectSecurity/WindowsIdentity APIs are an independent oracle from production GetNamedSecurityInfoW verification. Its child process inherits the test process token; no impersonation or privilege changes occur.
- **S2**: Existing System32 and kernel32.dll are prerequisite fixtures, never mutated. If either is unavailable, redirected, or not foreign to both token identities, the test fails explicitly.
- **S3**: Optional null/missing-DACL and missing-user-read cases are omitted; no coverage is claimed for them.
- **S4**: All changed execution rates are tests, CI and rare development. No runtime or performance change; benchmark/UI/financial gates are inapplicable.
- **S5**: The committed evidence identifies the source revision and executed tree; final commit identity and exact-head CI URLs are supplied by coordinator's durable issue/PR evidence, avoiding a self-referential commit hash.

## Acceptance criteria

- [x] **A1**: Actual valid publication/read/discovery/removal plus independent owner/protected-DACL/principal proof. Evidence: windows_private_publication_discovery_read_and_removal and raw Windows test log -> docs/control-plane/windows-authority-evidence.md. *(R1)*
- [x] **A2**: Everyone-read descriptor gets exact untrusted-principal issue and valid peer survives. Evidence: windows_everyone_read_descriptor_is_refused_without_hiding_valid_peer -> evidence document/log. *(R2)*
- [x] **A3**: Independently verified untrusted directory gets exact policy refusal before enumeration and ACL is restored. Evidence: windows_everyone_read_directory_is_refused_before_enumeration -> evidence document/log. *(R3)*
- [x] **A4**: Independently confirmed owned junctions rejected at both paths, target bytes unchanged, links removed before scratch roots. Evidence: windows_junction_directory_and_json_entry_are_refused_without_target_changes -> evidence document/log. *(R4)*
- [x] **A5**: Existing foreign OS directory/file each differs from TokenUser and TokenOwner and gets exact ownership refusal; stronger foreign-valid-JSON claim explicitly excluded. Evidence: both windows_foreign_os_* tests -> evidence document/log. *(R5)*
- [ ] **A6**: Test-only implementation preserves policy/contracts and fixtures cannot pass on setup errors or skip. Evidence: production diff and independent review -> evidence document/PR. *(R6)*
- [ ] **A7**: Pinned Windows CI actually executes all positive/adversarial tests at reviewed head; original build/engine/guards/Linux remain. Evidence: workflow diff and exact-head CI log/URL -> evidence document/issue/PR. *(R7)*
- [ ] **A8**: Revision, prerequisites, exact commands/exits, logs, fixture limits and review evidence retained. Evidence: committed evidence and external Q4-validation/commands.log plus coordinator CI/review links. *(R8)*
- [ ] **A9**: Reassessment evidence explicitly retains AP3/stronger-foreign-descriptor limitation. Evidence: committed evidence and coordinator reassessment. *(R9)*
- [ ] **A10**: UNMET: first source patch preceded GOAL archive. Earlier preparation and tier do not discharge this ordering obligation. Evidence: external ordering-file-times.json, first-source-child-as-applied.rs, mission-ordering-deviation.md and tool chronology. *(R10)*
- [ ] **A11**: Changed paths stay within assigned discovery registration/Windows child, workflow, evidence and unique mission archive, plus the separately coordinator-approved cfg(test) fake_gateway ordering change in client.rs (R29). Evidence: final diff/independent review. *(R11)*
- [ ] **A12**: Release retained before arming; further cargo waits for coordinator windows. Evidence: host-release-received.txt, commands.log and coordinator releases. *(R12)*
- [ ] **A13**: Every commit has preceding successful ordered checks; batch guards pass. Evidence: timestamped raw logs/commands.log. *(R13)*
- [ ] **A14**: Failure/repair signatures and reports are recorded before each retry; no limit is reset. Evidence: external failure-ledger.json and coordinator messages. *(R14)*
- [ ] **A15**: Worker performs no remote writes or marker projections. Evidence: tool history and coordinator handoff. *(R15)*
- [ ] **A16**: No token-bearing JSON is emitted; byte equality assertions avoid printing descriptor bytes. Evidence: source inspection and logs. *(R16)*
- [ ] **A17**: Fixtures use inherited existing token with no account/privilege operations. Evidence: source inspection. *(R17)*
- [ ] **A18**: Only owned scratch DACLs mutate; OS fixture access is inspection only. Evidence: source inspection. *(R18)*
- [ ] **A19**: Production policy bodies unchanged; independent fixture checks accept current compatibility. Evidence: final diff and positive/adversarial execution. *(R19)*
- [ ] **A20**: No production implementation changes beyond cfg(test, windows) registration and no dependency/schema changes. Evidence: final diff and workspace gates. *(R20)*
- [ ] **A21**: All new test machinery is private in windows_tests.rs and registration has both cfg conditions. Evidence: source diff. *(R21)*
- [ ] **A22**: Start base matches 0bd50f9b815a05e2ba8d0c9804324dbb415f6658; coordinator rebase/revalidation and fresh gates remain pending if base advances. Evidence: git SHA and merge preflight. *(R22)*
- [ ] **A23**: Issue, received delegation and host release are quoted; appended IDs preserve R1-R9/A1-A9. Historical deviation retained. Evidence: archive and source comparison. *(R23)*
- [ ] **A24**: Archive is in local commit; coordinator final reviews/CI remain pending. Evidence: reviewed diff plus coordinator gate records. *(R24)*
- [ ] **A25**: No create_goal/update_goal or user permission tool is called. Evidence: tool history. *(R25)*
- [ ] **A26**: RAII restoration and nonrecursive junction removal retain both owned roots, including unwinding. Evidence: source review and junction/ACL tests. *(R26)*
- [ ] **A27**: All six tests execute; independent preconditions and exact reason assertions are mandatory. Existing assertions/guards/thresholds unchanged. Evidence: source diff and test log. *(R27)*
- [ ] **A28**: Worker performs local filesystem/code/test work only; coordinator campaign scope does not authorize main merge. Evidence: tool history/handoff. *(R28)*
- [x] **A29**: Existing request-channel send occurs immediately before response write; all callers retain the receiver while awaiting connect, preserving assertions and no production changes. Evidence: client-helper-extension-source.txt, client.rs diff, read-only caller analysis and amended crate run 11 (18/18, exit0, no printed panic); final workspace sequence recorded externally. *(R29)*
- [x] **A30**: Exact isolated capture-budget diagnostic output/exits and executable identity retained externally after explicit host grant; not claimed as full workspace proof. *(R30)*
- [ ] **A31**: One authorized unchanged ordered retry recorded with all commands/exits and first-failure stop; no threshold/runner relaxation or counter reset. *(R31)*
- [x] **A32**: Oracle subprocess removes only inherited PSModulePath so inbox Windows PowerShell constructs default module paths; policy/assertions unchanged. Evidence: Windows child diff and Microsoft primary guidance. *(R32)*
- [x] **A33**: Owned incompatible module fixture makes original binary fail at exact Get-Acl autoload prerequisite and repaired binary pass through the same existing tests under identical parent environment. Evidence: external fixture, hashes and raw negative/positive logs. *(R33)*
- [ ] **A34**: Bounded CI repair1 has guards, all six tests/full crate and ordered four results; frozen tree reported before commit, new-head CI/reviews pending coordinator. Evidence: raw logs, CI URLs and freeze record. *(R34)*
- [ ] **G1**: Repository artifacts are English. Evidence: guards and architecture dimension 8.
- [ ] **G2**: Full ordered fmt/clippy/build/workspace-test passes before every commit; guards after edit batches; affected workflow verification. Evidence: Q4-validation raw logs and commands.log.
- [ ] **G3**: Architecture review closes with all Blocker/Should-fix findings resolved or accepted under policy. Evidence: coordinator's independent review record at final diff/base.

## Final local evidence freeze

Coordinator workflow structural verification actually passed using private PyYAML 6.0.3 with duplicate-key rejection; original Linux job and Windows ordered steps unchanged, one appended control-local step. Source: Q4-validation/coordinator-workflow-verification.json. This is not actionlint or exact-head CI.

Amended helper crate execution (11-amended-control-local.log) exited 0, 18/18 tests, no ignored tests or printed panic. Source reasoning and original SendError log remain in the evidence document. The first final ordered workspace sequence passed fmt/clippy/build and failed workspace tests (exit101) on the existing 250us capture budget, medians262/270/278us, with1907 app tests passed/1failed/4ignored. Frozen index81908ff5543a0d0f9148dfbdef3635acb2c80218 and owned file bytes were unchanged after failure. Actual commands/exits remain in commands.jsonl/final-ordered-results.json. Q4-APP-CORE-CAPTURE-BUDGET is occurrence1/repair0. The coordinator authorized an isolated diagnostic plus one unchanged ordered retry. Isolated log17 passed on verified original failing executable790a3245a5053f928d80dc511ef9ceeb27efd220a856b4f48da30b87cc01b921: median/p99/worst85/180/241,85/150/176,90/145/190us; existing lowest-p99 selection chooses median90. The diagnostic is not full-workspace acceptance or cause attribution. Full ordered retry1 follows this documented tree and guards; actual results remain external and unexecuted outcomes are not claimed as green. No source/threshold/runner relaxation; worker may hand off only after all four pass. The coordinator has instructed no commit yet. A10 remains explicitly UNMET; no delivery marker is recorded. Four policy-blocked failed-run roots remain; no further cleanup attempt occurred.

## Closing steps

- **C1**: Coordinator publishes the validated result as a draft PR targeting campaign/architecture-a, then obtains independent architecture, AI and source-first delivery closure at the final head/base.
- **C2**: Coordinator closes exact-head CI, confirms fresh campaign base and authorization, then prepares authorized campaign integration. Main merge remains human-exclusive.

## Workflow chronology and counters

Read-only preparation preceded host release and arming. Guards build, app all-target check and control-local all-target check exited 0. The first source patch (test child and registration) accidentally preceded persistence of this mission archive (GOAL). This ordering defect was reported to the coordinator immediately; the earlier preparation already recorded tier, source, ownership, fixture plan and limitations. This archive preserves the actual sequence; no reset or false timestamp is used. The coordinator had already persisted the private mission-tier at claim; this worker rewrote the same tier after the patch. The missing prerequisite was the mission archive, not the tier. The archive now precedes further edits. This historical deviation remains open for independent delivery review; no self-granted deferral or compliance claim is made. No tests or commit had occurred at that point.

Initial validation operation failures 0; repair attempts 0. Current validation failures 2 (initial ACL/unwind failure and first final workspace capture-budget failure); repair 1 passed all six focused tests and all 18 crate tests. Signature history: ACL setup privilege request occurred twice, cleanup abort once, each repaired in attempt 1; no privilege enabled. Separate ancillary operations: two automatic cleanup policy rejections (neither executed), one corrected read-only PowerShell syntax error. Four owned failed-run roots remain after successful nonrecursive junction removal; recursive deletion attempts stopped. Full command logs, source hashes, cleanup restrictions and read-only diagnosis of the existing detached client SendError panic are retained in external Q4-validation and summarized in the evidence document. A10 remains unmet. Incidental read of absent .cargo/config.toml was a read-only lookup miss with no retry or resulting edit. Record each subsequent failure signature and cumulative repair before retry in external Q4-validation.

## Original request as received

Complete attributed issue #335 source from Q4-start-issue.json, verbatim:

```text
<!-- campaign-task:milocaetano/quantick#330/Q4 -->
## Context

Parent: https://github.com/milocaetano/quantick/issues/330. Stable key Q4. The unchanged quantick-score v1.0 assessment at campaign SHA c3a92d58bb8a41ec4d78d73e60312b5f765b4da5 leaves AP3 at4/5 because actual Windows owner/ACL/reparse adversarial execution is missing. `crates/control-local/src/discovery.rs` implements these checks, but Windows CI currently tests only engine and guards. Linux execution cannot demonstrate the Windows branches. Evidence plan: https://github.com/milocaetano/quantick/issues/330#issuecomment-5564255845; this is a dated investigation, not executed acceptance.

## Scope

Add maintained Windows-only tests over real filesystem objects through the existing production descriptor publication/discovery/read boundaries, then execute quantick-control-local tests in Windows CI. Use owned scratch fixtures and an independently inspected read-only foreign-owned OS object. Preserve current owner-or-token-owner compatibility, trusted ACL principals, reparse policy, descriptor format, transport behavior and all runtime/public/financial contracts. No account creation, privilege enablement, credentials, system ownership/ACL mutation, security-policy redesign, paid services or deployment. Test machinery belongs in a cfg(test), cfg(windows) child of discovery; no public test API or alternate production implementation.

## Acceptance criteria

- [ ] Actual valid private descriptor publication/discovery/removal succeeds. An independent Windows API inspection proves owner, protected DACL and allowed principals; do not use the verifier under test as the fixture oracle or print descriptor tokens.
- [ ] A descriptor with an independently verified Everyone-read allow ACE is refused with the specific untrusted-principal reason while another valid descriptor in the same private directory remains discoverable.
- [ ] An owned discovery directory with an independently verified untrusted allow ACE is refused for that policy reason before enumeration. Publication must not repair the adversarial fixture before the assertion. Restore scratch access for cleanup.
- [ ] A real owned directory junction is independently confirmed as a reparse point. Both an explicit discovery-directory junction and a .json-named junction entry are rejected through production checks; target canary content remains unchanged. Cleanup removes the link itself before scratch roots and never follows a target outside verified ownership.
- [ ] Existing read-only foreign-owned Windows directory/file fixtures have owners independently shown to differ from both TokenUser and TokenOwner. Production directory/file checks refuse them for foreign ownership before enumeration/parsing. Fail with a clear fixture-prerequisite error if unavailable; no ignored test or silent skip. Explicitly distinguish this proof from a foreign-owned valid JSON descriptor traversing enumeration and connection; do not claim the stronger variant.
- [ ] All new policy fixtures distinguish intended refusal from generic access-denied/setup failures. No test weakens a policy, threshold, guard or existing assertion. Null/missing-DACL or no-user-read variants are optional only if deterministic setup and independent oracles can prove their intended branch; do not inflate acceptance with unexecuted variants.
- [ ] Windows CI executes `cargo test -p quantick-control-local` in addition to existing engine/guards checks, under the pinned toolchain. Existing Windows workspace build and full Linux workflow remain required. Fresh exact-head CI proves positive and adversarial tests actually executed, not merely compiled.
- [ ] Record final SHA, actual Windows filesystem/token prerequisites, test names, exact commands/exits and fixture limitations in committed evidence plus raw logs/CI URLs. Full local ordered fmt/clippy/build/workspace-test passes before every commit; guards after edits and affected workflow verification pass. Independent architecture, AI and source-first delivery reviews and exact-head CI close before campaign merge.

## Campaign record

Owner class: autonomous. Priority:6. Risk:high (security-boundary proof and CI). Estimated scope: discovery Windows test child and registration, Windows CI test command/comments, unique mission/evidence documentation. Dependencies:none; start only from latest integrated campaign branch. Intended ownership excludes Q2 schema files and Q3 lane implementation. Claimed branch `feat/windows-local-authority`, worktree `C:/src/quantick-worktrees/feat-windows-local-authority`, owner `codex/01a07841-8868-75a1-b6f6-9cb93837d72d/implement_q4`, startSHA `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`, campaign Project item `PVTI_lAHOA0fkv84Bipkmzg5uTfY`; PR publication follows validation. Base latest origin/campaign/architecture-a; PR target exactly campaign/architecture-a. D4 (https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109) permits up to three independent implementations with disjoint worktrees and serialized campaign merges; update/revalidate after base advances. Main merge exclusively human.

Validation rates: tests/CI/rare developer work; no runtime hot-path change or performance claim. Evidence destination:this issue, committed mission/evidence and linked PR. Initial operation/repair counters0; preserve cumulative per-signature history and campaign retry limits. Completion is not an automatic AP3 increment: reassess the integrated SHA under unchanged rubric, retaining any stronger foreign-descriptor evidence gap.

```

## Received coordinator delegation (verbatim attributed source)

```text
Implement campaign #330 task Q4 / issue #335, source C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q4-start-issue.json. Worktree C:/src/quantick-worktrees/feat-windows-local-authority, branch feat/windows-local-authority, start integrated0bd50f9b815a05e2ba8d0c9804324dbb415f6658, PR target campaign/architecture-a. Read current repo instructions/skills. Own crates/control-local/src/discovery.rs + discovery/windows_tests.rs, .github/workflows/ci.yml, docs/control-plane/windows-authority-evidence.md and unique .claude/GOAL-archive-windows-local-authority.md. No Q2/Q3 paths; report necessary ownership adjustments. D4 actualuserauthority https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109 permits max3 independentmissions, serializedbuild/benchmarkhost and campaignmerges. Begin READ-ONLY preparation; wait explicit coordinator hostrelease beforearming, tests or repo edits. Fulfill complete issue acceptance through realWindowsfilesystem and independentoracles, preserve runtime/public/securitypolicy/financialrules. Originalsrcplan Q4-spec.md/product-boundary-plan artifact optional whereavailable; issue issource. Follow existing mission/ship workflow; preserve originalsource/stableledger/counters, rawcommands/exits under tempcampaignQ4-validation. Localvalidatedcommit/handoff only; coordinator handles remotewrites/reviewmarkers/merges. No childhostgoal or routine userpermissionprompt. Send concrete test-boundary/fixture plan and indispensable uncertainty, then wait forhost.
```

## Received host release (verbatim attributed source)

```text
Q3 released the build/test host after its targeted compile exited. Q4 may acquire the host NOW, retain this release before arming, arm guards and app all-target check, then create the mission and implement its owned Windows authority tests. Q3 performs source/dossier work only until Q4 releases the host. No builds or benchmarks may overlap. Report failures and cumulative signature counters before retries; all required checks must pass before any commit. No routine user permission is needed. Coordinator owns GitHub writes, independent reviews and serialized campaign merges. Durable release https://github.com/milocaetano/quantick/issues/335#issuecomment-5568958659, campaign checkpoint5568959425; source also Q4-host-release-source.txt. Continue existing Q4 preparation at C:/src/quantick-worktrees/feat-windows-local-authority base0bd50f9, no reset/new mission branch. Save literal received release + UTC receipt in external Q4-validation before arming. Independent fixtures and ownership restrictions in your preparation remain. Q2 is now separately delivery-stalled, no dependency. Please give me a host release after arming if useful to let Q3 finish compile/tests while you implement; serialize builds/measurements.
```

## Approved bounded test-helper extension (verbatim attributed source)

```text
Coordinator approved one bounded source-only extension from observed existing helper panic, intent https://github.com/milocaetano/quantick/issues/330#issuecomment-5569365534. Add ownership ONLY crates/control-local/src/client.rs cfg(test) fake_gateway (around528-543). All callers bind seen/_seen/_a/_b while LocalClient::connect awaits the response. Move existing seen_tx.send(request).unwrap() immediately BEFORE stream.write_all(&frame).unwrap(), preserving encode, unwraps and every existing assertion. This orders request observation before reply allows caller completion/drop, removing the observed successful-return race without ignoring failed sends or changing production protocol. Verify all callers/source rationale; if another blocking cause exists, report rather than generalizing. Add this exact directive to mission/source with stable appended R/A and disclose original log; no new features/runtime/public changes. No cargo now (Q5 host), full validation at next assigned window. A10 historical unmet and four policy-blocked cleanup roots remain explicit; no further cleanup attempts. Coordinator will update issue ownership projection.
```

## Post-failure coordinator disposition (verbatim attributed source)

```text
Q5 now owns final-check host. Root independently read initial test failure and unchanged tree. Preserve Q4-APP-CORE-CAPTURE-BUDGET occurrence1/repair0 alongside all historical signatures; next Q4 window will run exact isolated existing test then one unchanged full ordered suite retry (no source/runner/threshold relaxation). You may prepare truthful evidence/counter update source-only while waiting, then final guards + four validate that document tree at next host grant. No cleanup retries. Please return idle status after source-only handoff to free review slot.
```

## Retry1 host release (verbatim attributed source)

```text
EXCLUSIVE HOST RELEASE NOW to Q4: Q3 all four archive correction checks passed and cargo exited; checkpoint https://github.com/milocaetano/quantick/issues/330#issuecomment-5569810260. Record literal receipt/time. Run exact existing isolated observer budget diagnostic (preserve failing exe identity); if pass, one unchanged full ordered fmt/clippy/build/test retry1 on final documented tree, guards after doc edits. Source/runner/threshold unchanged. Report first failure before another attempt; counters/history retained, A10 remains UNMET. Freeze validated tree and logs then report before commit; no cleanup or remote writes.

```

## Bounded CI repair1 directive (verbatim attributed source)

```text
Bounded CI repair1 AND exclusive host released NOW, journal https://github.com/milocaetano/quantick/issues/330#issuecomment-5570034309. Q4 arch completed BLOCKED with one new fixture-availability Blocker; report Q4-review-3ceaf72298db/arch-review.md. Exact Windows CI34116356314 all6fail Get-Acl module autoload; raw Q4-validation/ci-34116356314-failed.log. Microsoft primary guidance https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psmodulepath?view=powershell-7.6#starting-windows-powershell-from-powershell-7 describes PS7->intermediateprocess->WindowsPowerShell inherited PSModulePath loading incompatible modules; remove ONLY child PSModulePath to rebuild defaults. Actual runner env wasn't logged, so don't claim proven cause yet. Implement narrow cfgtest Command.env_remove("PSModulePath") with rationale, all policy/assertions unchanged. Before edit record directive in existing stable mission ledger; no rewritten original source/history/A10. Establish meaningful local negative/positive inherited-module-path execution (owned incompatible module fixture or equivalent controlled subprocess, no process-global environment mutation/module install/machine policy change) through same existing oracle/tests; preserve unfixed output and exact binary/source identity. Then all6tests/fullcrate/guards/ordered4/newhead CI via coordinator. Stop/report first failure within same signaturebudget, no cleanup retries. Freeze exact validated tree/report BEFORE commit; no remote writes. No other agent edits this WT; all other cargo stopped.

```

## CI repair1 execution before final freeze

Initial candidate3ceaf72298db097f17c3e83f23a887441fc3364f was locally validated, committed and published by coordinator as draft342. Windows CI34116356314 then failed all6fixtures at Get-Acl autoload; architecture B1 is open. Raw failure and review retained. Local reproduction uses owned incompatible Security module requiring PowerShell99.0 and child-only PSModulePath: preserved original binary48c9bb048473921d312fcd16db377d0bcda0000fa1f8754d1320ea0ba582153b fails all6 with matching autoload error (expected negative25). Three added test-only lines remove only the oracle child's inherited PSModulePath. Identical controlled parent environment/fixture then passes18/18, including all6policy tests (positive28); exact crate CI command also passes18/18 plus doctests (29), guards26 pass. Actual CI inherited environment was not logged, so its precise cause remains a hypothesis. No global environment/module install/policy/assertion change. Remote failure signatureQ4-CI-GETACL-MODULE-AUTOLOAD: one run,6test occurrences, repair1; controlled expected-negative experiment retained separately. Document-inclusive final ordered checks and new-head CI/reviews remain pending in their respective raw records. A10 is still UNMET; no cleanup retry or self-waiver.

## Completeness round 1: bounded ledger repair 1

The independent source-first completeness report for head
`fce869e328929984e9f09ed7945faadb613351fd` found seven omissions U1-U7 across
eight of 53 source groups. The report is retained at
`Q4-review-fce869e32892/completeness-review.md` and published at
https://github.com/milocaetano/quantick/pull/342#issuecomment-5570557082.
Repair intent: https://github.com/milocaetano/quantick/issues/330#issuecomment-5570562977.
This append records those received obligations without changing the preceding
archive bytes, original requests, existing IDs or scope. All added criteria
remain unchecked for independent grading. Representation of a historical
obligation does not establish that it was fulfilled. A10 remains explicitly
UNMET; a promise or ledger entry added now cannot repair that history.

### Appended request ledger

- **R35**: Work on the assigned branch `feat/windows-local-authority` in `C:/src/quantick-worktrees/feat-windows-local-authority`, starting from integrated `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`. Source: U1/S27, original issue campaign record and received delegation; continuing base freshness remains R22.
- **R36**: Target the Q4 pull request exactly at `campaign/architecture-a`. Source: U1/S27, original issue campaign record and received delegation.
- **R37**: Keep campaign concurrency within the authorized ceiling of three independent implementation missions. Source: U2/S29, original issue D4 campaign record and received delegation.
- **R38**: Give independent implementation missions disjoint worktrees. Source: U2/S29, original issue D4 campaign record.
- **R39**: Serialize campaign merges through the coordinator. Source: U2/S29, original issue D4 campaign record and received delegation; R15 retains merge ownership.
- **R40**: During the bounded CI repair, no other agent edits this worktree. Source: U2/S53, received bounded CI repair1 directive; stopped and serialized Cargo remains R12.
- **R41**: Read the current repository instructions and applicable skills for Q4. Source: U3/S33, received delegation.
- **R42**: Follow the existing mission and ship workflow with the campaign integration contract and Codex host mappings. Source: U3/S33, received delegation and the named workflows; historical deviations remain recorded under R10/R23.
- **R43**: Begin with read-only preparation and wait for explicit coordinator host release before arming, tests or repository edits. Source: U4/S34, received delegation; retain the original preparation boundary and timing.
- **R44**: Send the coordinator a concrete test-boundary/fixture plan and indispensable uncertainty, then wait for the host before execution. Source: U4/S34, received delegation; a locally saved plan alone is not proof of its handoff.
- **R45**: After the initial explicit host release, successfully run `cargo build -p quantick-guards` and then `cargo check -p quantick-app --all-targets` before creating the mission and implementing the owned Windows authority tests. Source: U5/S37, received initial host release; this adds the missing arming prerequisite without discharging R10/A10.
- **R46**: After the requested source-only handoff during Q5's final-check host window, return idle status to free the review slot. Source: U6/S46, received post-failure coordinator disposition; later test or review success is not evidence of this historical return.
- **R47**: Record the received bounded CI repair1 directive in the existing stable mission ledger before editing its implementation, preserving original source/history and A10. Source: U7/S47, received bounded CI repair1 directive; preserve the original before-edit prerequisite without backdating this entry.

### Appended acceptance criteria

External paths below are relative to `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a`.

- [ ] **A35**: Q4's branch/worktree and integrated starting SHA match R35. Evidence: original claim/delegation and read-only Git identity records -> `Q4-validation/ledger-repair1-before.json` and coordinator claim record. *(R35)*
- [ ] **A36**: The published Q4 PR has base `campaign/architecture-a`. Evidence: coordinator's actual PR head/base readback -> issue #335/PR #342 evidence; final preflight remains C3. *(R36)*
- [ ] **A37**: Campaign implementation concurrency never exceeds three independent missions during Q4's execution. Evidence: actual campaign claim/release records and worker lifecycle records -> campaign #330 concurrency evidence; authorization text alone does not prove execution. *(R37)*
- [ ] **A38**: Concurrent independent implementations use disjoint worktrees. Evidence: actual branch/worktree claim identities and lifecycle records -> campaign #330 concurrency evidence. *(R38)*
- [ ] **A39**: Campaign merges performed during Q4's execution are serialized by the coordinator. Evidence: actual merge operation records and campaign ref readbacks -> campaign #330 integration evidence; unperformed Q4 integration remains C4. *(R39)*
- [ ] **A40**: No other agent edits Q4's worktree during bounded CI repair1. Evidence: bounded repair worker lifecycle, completed file-change/command records and frozen tracked-file identities -> `Q4-action-audit/95eb7299094dd3a5/execution-events.jsonl` plus coordinator ownership records; an instruction alone is not execution proof. *(R40)*
- [ ] **A41**: Current repository instructions and applicable mission/ship skills were read for Q4. Evidence: completed instruction-read commands -> `Q4-action-audit/95eb7299094dd3a5/execution-events.jsonl`; repair reads are recorded separately and do not replace missing historical reads. *(R41)*
- [ ] **A42**: Q4 follows the applicable mission/ship workflow and campaign/Codex mappings, with actual ordering and every deviation retained. Evidence: command/file-change events, ordered raw logs, commits and independent gate records -> `Q4-action-audit/95eb7299094dd3a5/` and `Q4-validation/`; A10 remains UNMET and this criterion claims no exception. Future closing duties remain C1-C4. *(R42)*
- [ ] **A43**: Initial preparation is read-only until the explicit host release, with no arming, test or repository edit preceding it. Evidence: completed operations and actual release receipt -> `Q4-action-audit/95eb7299094dd3a5/execution-events.jsonl`, `Q4-validation/host-release-received.txt` and initial raw arming logs. *(R43)*
- [ ] **A44**: The concrete boundary/fixture plan and indispensable uncertainty were handed to the coordinator before execution, followed by waiting for the host. Evidence: original plan artifact plus actual handoff/lifecycle record and release ordering -> `Q4-validation/preparation.md` and coordinator's retained receipt evidence; no handoff or wait time is inferred from narrative status. *(R44)*
- [ ] **A45**: Initial guards build and app all-target check both exit 0 after release and before mission creation and implementation. Evidence: raw `01-arm-guards.log`, `02-arm-app.log`, `commands.log` and completed command/file-change events -> `Q4-validation/` and `Q4-action-audit/95eb7299094dd3a5/execution-events.jsonl`; the separate mission-before-source obligation A10 remains UNMET. *(R45)*
- [ ] **A46**: The requested historical source-only handoff was followed by an idle-status return that freed the review slot. Evidence: actual original handoff and worker completion/lifecycle records -> coordinator's retained Q4/Q5 slot records; no original message content or timestamp is reconstructed from tool-name or receipt indexes. *(R46)*
- [ ] **A47**: The bounded CI repair1 directive was written into the existing stable mission ledger before its implementation edit, with source/history/A10 preserved. Evidence: completed ledger-write and subsequent source-patch operations -> `Q4-action-audit/95eb7299094dd3a5/execution-events.jsonl`, source lines 878 and 897, plus the retained directive and archive. This appended criterion does not assert historical fulfillment. *(R47)*

### Continuing closing obligations

- **C3**: Before readiness or integration, the coordinator verifies the final Q4 PR head and exact base `campaign/architecture-a` and completes every still-pending mission/ship review and exact-head CI requirement under C1/C2. This remains future work; the ledger repair does not grant readiness or a delivery marker. *(R36, R42)*
- **C4**: If Q4 reaches authorized campaign integration, the coordinator performs its merge in the serialized campaign merge window after the fresh-base and authorization checks, then retains merge and campaign-ref readbacks. Main merge remains human-exclusive. This remains future work. *(R39, R42)*

### Objective chronology and retained limitations

The action-audit manifest identifies an immutable prefix of completed command
and file-change events. It excludes implementing narration and message bodies.
Its `source_line` values address the captured source records, not physical
line numbers in the extracted file. Event times below are observed completion
times from that audit, not newly invented receipt or handoff times:

- Source line 86 completes the release-recording/guards command at `2026-09-07T10:05:10.576Z`, exit 0. Source line 138 completes the app all-target check at `2026-09-07T10:06:58.837Z`, exit 0. Inspect the commands and original raw logs for their actual sequence.
- Source line 176 completes the initial source patch at `2026-09-07T10:10:06.698Z`; source line 192 completes mission persistence at `2026-09-07T10:11:21.563Z`. The source edit preceded the mission. A10 remains UNMET.
- Source line 878 completes the command appending R32-R34/A32-A34 and the literal CI repair directive to the existing archive at `2026-09-07T11:38:57.250Z`, exit 0. Source line 897 completes the `.env_remove("PSModulePath")` source patch at `2026-09-07T11:40:08.216Z`. These are objective evidence pointers for independent grading; adding R47 now does not change their history.

Historical failure records and raw logs remain unchanged: initial Set-Acl
privilege setup failures and cleanup double panic; the existing fake-gateway
send-order race and its bounded test-only repair; observer capture-budget
failure with its exact isolated diagnostic and single unchanged full retry1;
missing-sh review-key tooling and its bounded resolution; and Windows CI
module-autoload failure with CI repair1. The original CI runner environment
was not logged, so its precise cause remains unproven. Successful later tests
cannot establish that missing environment. The controlled negative experiment
remains distinct from unexpected failures. Completeness round 1 and this
ledger repair1 are recorded separately from architecture/CI repair rounds;
no historical counter is reset.

Four owned failed-run scratch roots remain pending after the automatic cleanup
policy rejections. No cleanup attempt is authorized by this repair. This
append changes only the mission archive. Existing code, tests, raw logs and
prior evidence retain their bytes. Source-only mapping, original prefix,
tracked-file hashes, evidence hashes and exact patch are retained as
`Q4-validation/ledger-repair1-*`. Guards, ordered validation, commit and fresh
reviews for the appended tree remain pending the coordinator's host/workflow
release; none is claimed here.


## Deferred

These narrowly scoped retrospective process exceptions implement the authenticated user's [standing process-resolution instruction](https://github.com/milocaetano/quantick/issues/330#issuecomment-5584842294), granted after the Q2-Q5 blockers were disclosed. The user delegated their resolution without repeated approval prompts. The coordinator records the specified historical exceptions under that instruction; it does not claim the original actions were delivered. Original request and criterion IDs, failure signatures, counters and adverse evidence remain retained.

- **A10 / R10 - historical mission-before-source order.** Historical grade remains MISSING: the completed source edit at 2026-09-07 10:10:06.698 UTC preceded mission persistence at 10:11:21.563 UTC. Later documentation cannot repair that chronology.
- **A25 / R25 - complete historical tool-call absence proof.** Historical grade remains UNPROVEN for omitted nested/rejected/lifecycle events; no prohibited action is inferred. Current authority and tool limits remain mandatory.
- **A37 / R37 and A38 / R38 - complete historical concurrency and disjoint-worktree census.** Historical grades remain UNPROVEN where full claim/release records are missing. Current recovery is serial and worktrees are explicitly identified.
- **A40 / R40 - complete cross-agent write coverage during historical CI repair1.** Historical grade remains UNPROVEN; actual saved trees and recorded writes remain evidence within their scope.
- **A44 / R44 and A46 / R46 - original plan handoff, waiting and idle/slot-release records.** Historical grades remain UNPROVEN where content/lifecycle records are unavailable. No retrospective message or idle timestamp is invented.

All current product requirements, deterministic behavior, public/financial contracts, independent reviews, tests, exact-head CI and score criteria remain unchanged. The linked R obligations are exempt only to the extent discharged by the historical portions listed here; their other current/observable portions still require proof. Missing encrypted/private originals are not reconstructed from authored quotations.

The recovery applies these recorded exceptions and validates the latest campaign integration through one bounded repair attempt for the historical-process signature, preserving the previous attempts. Further actual failures retain their own existing finite per-signature limits; the same unavailable-history search is not repeated. Only reviewed green intermediate PRs may merge into `campaign/architecture-a`. Main merge remains exclusively the user's action.
