# AI-review completion evidence

Task: [Q1 / #331](https://github.com/milocaetano/quantick/issues/331), child of
[campaign #330](https://github.com/milocaetano/quantick/issues/330). Branch:
`fix/ai-review-completion`. Worktree:
`C:/src/quantick-worktrees/fix-ai-review-completion`.

This document records implementation and local validation. Independent final
reviews, durable PR review reports, private review markers, publication and
exact-head CI remain coordinator-owned closing steps. Their absence here is
not a claim that those gates passed. No score increment is claimed; campaign
reassessment owns that conclusion.

## Change and trust boundary

Before this change, readiness could see current architecture/delivery evidence
and zero open AI threads when AI review had never run. Now readiness and
campaign merge also require `ai-review-complete` in the effective worktree's
private git directory. Its one LF-terminated line is `<branch> <shared-key>`.
The optional branch argument to the existing `require_marker` checks this
record shape and then uses its existing hexadecimal key comparator. The
architecture/delivery marker format, review-key calculation and tier behavior
remain unchanged.

The branch field closes a distinct case: switching to another task branch can
leave the raw main-based diff key equal. The main fixture asserts that equality
before asserting AI denial. Campaign keys already bind the explicit base ref
and tip; fixtures advance each while refreshing only the architecture/delivery
pair, so those earlier guards cannot account for the AI denial.

Completion does not mean zero findings. The canonical AI skill requires the
review to complete, publish required finding threads and a durable report, and
compare worktree/branch/head/clean status/base/key before and after the review
and publication. The report includes full HEAD SHA, explicit base ref and tip,
key, scope, six verdicts and finding IDs/count. Only then does its procedure
write the local projection. The existing unresolved-thread gate stays separate.
No-PR reviews print a report and do not record completion.

This is the repository's existing operational trust model: the hook validates
a local recording, not reviewer reasoning or GitHub report provenance. No new
skip, override, parser expansion, remote authority source or generic review
subsystem was added. Existing thread-count unavailability behavior and shell
command limitations remain. Main merging is exclusively the user's action.

## Scope and rates

| Paths | Rate and impact |
| --- | --- |
| `.claude/hooks/guardrails.sh` | Rare developer command. One small private marker read and branch query at readiness/merge. Existing comparison and campaign key reused. |
| `.claude/hooks/guardrails_test.sh`, `campaign_context_test.sh` | Rare validation. Disposable real Git histories and stubbed thread/GitHub responses; no credentials or live mutation. |
| `.claude/hooks/README.md` | Documentation of producer, consumer and preserved boundaries. |
| `.claude/skills/ai-review/SKILL.md` | On-demand instructions; completion procedure and report identity. |
| `.claude/skills/ship/SKILL.md`, `mission/SKILL.md` | On-demand instructions. Explicit draft/AI/readiness ordering; shorter existing ship prose pays for the producer instructions. |
| `docs/campaign/integration.md` | Documentation linking to the canonical producer with the same campaign key. |
| `.claude/GOAL-archive-ai-review-completion.md`, this file | Mission and evidence bookkeeping, no runtime path. |

No Rust source, Cargo manifest/lock, financial behavior, product contract,
hot path, UI surface or runtime dependency changes. No baseline is raised.
The mission description's pre-existing angle-bracket placeholder was rewritten
without changing its meaning because the affected skill validator rejects it.

## Local validation artifacts

Raw logs are retained outside the repository at
`C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q1-validation/`.
The temporary `run.py` executes each named command in this worktree and records
merged stdout/stderr and its exit code. Git `sh.exe` is resolved explicitly on
Windows. Skill validation uses PyYAML installed only in that directory's
`python-deps`; the repository and product dependency graph are unchanged.

| Log | Exact command / result |
| --- | --- |
| `01-arm-guards.log` | `cargo build -p quantick-guards`: exit 0, 6.05 seconds. Original PowerShell redirection mixed encodings; preserved as received, with that limitation disclosed. |
| `02-arm-app.log` | `cargo check -p quantick-app --all-targets`: exit 0, 1 minute 43 seconds. |
| `03-guards-batch1.log` | `cargo test -p quantick-guards`: exit 0. |
| `04-guards-batch2.log` | Same guard command: exit 101, context total 238244, 320 bytes beyond allowed headroom. |
| `05-guards-context-repair.log` | Same guard command: exit 101, context total 237928, 4 bytes beyond allowed headroom. |
| `06-guards-context-repair.log` | Same guard command: exit 0 after further concise prose repair. |
| `07-shell-suite.log` | Bare `sh` launch failed before the shell test ran; command-only log retained. |
| `08-shell-suite.log` | `C:/Program Files/Git/bin/sh.exe .claude/hooks/guardrails_test.sh`: exit 0; 223 passed, 0 failed, plus 51 campaign-context tests. |
| `09-skill-ai.log`, `10-skill-ship.log`, `11-skill-mission.log` | Affected `quick_validate.py` calls initially failed at import: PyYAML absent. |
| `12-validator-dependency.log` | `python -m pip install --disable-pip-version-check --target .../Q1-validation/python-deps PyYAML`: exit 0; PyYAML 6.0.3 installed privately. |
| `13-skill-ai-review.log`, `14-skill-ship.log` | `python -X utf8 C:/Users/camil/.codex/skills/.system/skill-creator/scripts/quick_validate.py .claude/skills/ai-review` / `ship`: exit 0. |
| `15-skill-mission.log` | Same validator for `mission`: exit 1 on its pre-existing angle brackets in the description. |
| `16-skill-mission-repair.log` | Same mission validator: exit 0 after wording repair. |
| `17-local-links.log` | `python -X utf8 .../Q1-validation/check_links.py`: exit 0; 6 local Markdown links/anchors resolve. |
| `18-guards-evidence.log` | `cargo test -p quantick-guards`: exit 0 after evidence and skill validation repairs. |
| `19-guards-archive.log` | `cargo test -p quantick-guards`: exit 0 after mission archival. |
| `20-campaign-base-update.log` | Temporary update script stopped before mutation because its status parser removed the first line's leading space. |
| `21-campaign-base-update.log` | Corrected update script: exit 0. Explicit owned-path stash, verified fast-forward and restoration preserved all 10 owned files byte-for-byte with no incoming path overlap. |

The original document was frozen before the final loop. Its completed results
are now indexed below under the explicitly authorized first delivery repair.
That earlier green loop validates the original committed tree only; this repair
requires its own fresh guards and full ordered loop before its next commit.

## Fixture coverage

The main suite runs the new boundary cases under both `Bash` and `exec_command`
payloads. Each tier (`small`, `medium`, `high`, `max`) denies absent completion
at readiness/merge despite zero threads; current completion allows readiness
and still denies unresolved findings. Small's positive fixture removes delivery
evidence to prove the existing bounded exemption stays narrow.

Additional cases isolate stale keys; empty, malformed and multiline records;
an unterminated appended record; another worktree's identical record; a branch
switch with an equal raw key; same-branch reword; and committed source change
after the other two reviews refresh. Current AI evidence cannot replace missing
architecture or delivery evidence. Draft creation with no markers stays open.

Campaign fixtures isolate base-tip and equivalent-base-ref changes for both
readiness and merge. Current evidence permits the explicitly authorized,
head-pinned, green-CI merge; current AI evidence cannot supply a missing grant
or pending CI. Existing main, auto/admin, alternate-repository, compound-command,
remote identity and unresolved-thread tests are retained. Recording-owner drift
checks now include AI and its shared key/identity/report obligations.

## Coordination and conditional obligations

The coordinator prepared this task at campaign base
`a808b2d87b36d73041027e4d20c053544b454a96`. Q1 initially remained read-only
while F1 owned the host. The coordinator explicitly released the host after
F1 checks completed; both arming commands above passed before the first edit.
Q1 touches none of F1's runtime code or unique mission/evidence files.

F1 then integrated at campaign SHA
`c3a92d58bb8a41ec4d78d73e60312b5f765b4da5`. The coordinator reported the
verified merge and authorized the update. The worker checked the fetched ref,
verified no incoming path overlapped Q1's owned set, preserved only those edits
in a task-specific stash, fast-forwarded without creating a merge commit, and
restored all 10 files with identical SHA-256 hashes. The owned stash was removed
only after restoration succeeded. Fresh guard and full workspace validation
against this base are required; old-base review stamps are not reused.

The implementation worker has made no GitHub write, publication, checkpoint,
review marker, review verdict or merge. Before committing, it must notify the
coordinator so publication intent can be recorded. The coordinator receives
the exact clean committed head, base and review dossier, preserves the archive
before reviews, and alone publishes/reviews/integrates under campaign authority.
No child host goal or subagent was created.

Cumulative failure signatures are retained: context-budget 2; shell-launch 2
(one tool shell launch, one bare subprocess executable lookup, both before test
execution); logging-format 1; missing validator dependency 3 independent calls;
mission-description validator 1; temporary status-parser assertion 1 (before
any stash or base mutation). These are not reset by repairs. The current
successful results above supersede failures as validation, while the raw failed
artifacts remain part of the audit.

Independent architecture (bug and full shape), AI and delivery reviews,
resolution of all required findings, exact-head CI, readiness and authorized
campaign integration remain mandatory closing obligations. Main integration
and campaign score reassessment remain outside this child implementation.


## First delivery repair and evidence provenance

The coordinator authorized this two-document repair in
[the first repair intent](https://github.com/milocaetano/quantick/issues/330#issuecomment-5564702713).
The independent [source-first report](https://github.com/milocaetano/quantick/pull/334#issuecomment-5564702199)
fixed 95 asks before seeing the ledger and found 17 unledgered portions.
The balanced criteria report (`Q1-review/criteria-review.md` in the external
artifact root) found no further gap except A22's failed exact-head Linux CI.
This repair adds explicit obligations and evidence; adding rows does not prove
their delivery. Existing IDs and both verbatim source quotations are retained.
New checklist boxes remain unclaimed for the next independent review.

Only this evidence document and the Q1 mission archive change in the repair.
The original ten paths all belong to the assigned envelope; no additional path
was needed in either implementation or repair. The conditional overlap notice
therefore did not trigger. If expansion becomes necessary, the worker must
report overlap before it. No rubric file, source, test, threshold, manifest,
lockfile or runtime contract changes. AD4's rubric and score remain untouched.

### Reading inventory

This is a reconstructed record of earlier tool reads from the implementation
conversation, written during delivery repair. There was no dedicated timestamped
pre-edit reading log. The table identifies the actual earlier reads and their
starting revision `a808b2d87b36d73041027e4d20c053544b454a96`; it does not present
newly calculated hashes as proof of earlier reading or invent earlier timestamps.

| Required input | Recorded earlier reading |
| --- | --- |
| `AGENTS.md`, `CLAUDE.md` | Initial PowerShell `Get-Content` reads of the assigned worktree's rules; later CLAUDE tail read supplemented the combined output. |
| `.agents/skills/mission/SKILL.md`, `new-task/SKILL.md`, `ship/SKILL.md`, `ai-review/SKILL.md` | Codex wrappers read first, followed to canonical `.claude/skills/` owners; `.agents/references/codex-compatibility.md` also read. |
| `.claude/skills/mission/SKILL.md`, `new-task/SKILL.md`, `ship/SKILL.md`, `ai-review/SKILL.md` | Canonical instruction contents read before implementation and used for arming, archive and producer/consumer design. |
| `.claude/skills/mission/references/why.md` | Read because Q1 changes the review workflow; this conditional reading applied. |
| `docs/campaign/integration.md` | Full current integration instructions read with the shared campaign code. |
| `.claude/hooks/guardrails.sh` | Full-file read followed by focused reads of the marker/comparator and publication gate regions (approximately lines 380–834 in that revision); existing parser/gate context was inspected before modification. |
| `.claude/hooks/campaign_context.sh` | Full current shared-key/authority implementation read; reused without changing this file. |
| `.claude/hooks/guardrails_test.sh` | Initial combined test output was truncated. Subsequent chunk reads covered the fixture setup and current cases (first 210, 210–470, 470–670, 690–1080, 1080–1198 plus remaining initial output), rather than treating truncated output as a full read. |
| `.claude/hooks/campaign_context_test.sh`, `.claude/hooks/README.md` | Full current campaign tests and hook documentation read; relevant README portions reread. |
| `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q1-start-issue.json` | Captured authoritative issue body read before mission creation; retained unchanged as input. |
| `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q1-investigation.md` | Supplied read-only investigation read before implementation, including the AD4 diagnosis and existing key/gate ownership. |

Additional context-guard source/baseline inspection explained the failed context
budget without increasing its allowance. During this repair the worker read
`Q1-derived-asks.md`, the full completeness mapping, criteria findings, current
archive/evidence, original dossier and later journal metadata. The external
`Q1-validation/repair-readback.json` records repair-time source hashes, git
identity and tier at `2026-09-07T03:45:59.792088+00:00`. Those hashes identify
current files, not a claim that a hash operation reviewed their contents.

### Ordered execution and handoffs

The following initial sequence is reconstructed from the ordered conversation
and actual raw command outputs. No exact release/first-edit timestamp was saved.

1. Q1 began with read-only planning while F1 held the host.
2. The coordinator said: "F1 has released the build/test host: all its checks
   passed, committing archive-only repair now. Q1 may acquire host immediately,
   arm guards and app all-target check, then edit/implement."
3. `01-arm-guards.log` records the successful guards build; `02-arm-app.log`
   records the successful all-target app check. Both finished before editing.
4. The first repository edit created `.claude/GOAL.md` with tier high. The
   mission was later archived at the assigned unique path before review. Thus
   both the first build and first edit followed the explicit host release.
5. The verified F1 integration prompted the coordinated fast-forward and fresh
   checks described above. No unintegrated F1 implementation was consumed.
6. After the frozen green retry, the worker sent the coordinator its precommit
   notice, exact outputs and pending external reviews. The coordinator's
   [commit-intent checkpoint](https://github.com/milocaetano/quantick/issues/330#issuecomment-5564562513)
   was created at `2026-09-07T03:21:17Z`. Commit `efa0c880bff675ffbba7cfd9ae78c9b4ced1e0c8`
   has committer time `2026-09-07T03:24:46Z`. Live readback artifacts
   `42-original-commit-intent-readback.json` and `repair-readback.json` retain
   these independent timestamps; log39 records the actual commit command.
7. Log40 and `final-handoff.json`, together with `Q1-review-dossier.md`, were
   returned to the coordinator. They identify clean HEAD `efa0c880...`, tree
   `ee801dce1324b9984ff618d3a8bfd1f7342d2dfb`, base `c3a92d58...` and key
   `81276d48392b425990db02c638107122bad9bc16`.

The repair-time readback confirms the assigned worktree
`C:/src/quantick-worktrees/fix-ai-review-completion`, branch
`fix/ai-review-completion`, private tier `fix/ai-review-completion high`, clean
original head and unchanged integrated base. The current repair authorization
specifically allows these owned documentation edits while Q2 holds the host;
guards/build/test wait for a new coordinator release. It does not retroactively
waive the initial release-before-build-and-edit rule. Each subsequent commit
needs a fresh completed validation loop, worker notice and recorded intent.

The implementation action record contains no routine user plan/command approval
request, child host goal creation, or `sandbox_permissions` option. This is an
explicit reconstruction from the worker's conversation/tool actions, not a
generated audit claiming an unavailable full exported event log. Coordinator
host releases and publication handoffs were retained. Independent coordinator
review agents are outside the child-host-goal prohibition. The worker performed
no GitHub write, review/marker write, checkpoint, publication or merge; the API
requests in this repair are read-only journal/intent retrievals.

### Completed original validation and retained failures

Logs below are under `Q1-validation/`; each runner log contains its exact command
and exit. The original green results belong to tree `ee801dce...` only.

| Artifacts | Observed result |
| --- | --- |
| 22 current-base guards; 23 fmt; 24 clippy; 25 build | All exit 0 after the base advance. |
| 26 full workspace test | Exit 101: unchanged `observer_core_capture_stays_within_the_ui_budget`, selected median 276 us against 250 us; three medians 274/276/283 us, selected p99 435 us, worst 760 us. App 1907 passed, 1 failed, 4 ignored. |
| 27 whitespace; 28 archive; 29 build-input equivalence | Exit 0. The only edit during the initial loop removed spaces on blank Markdown quote lines. The later retry used one frozen tree. Log28 checked the original 30 R mappings; it did not establish exhaustive source completeness. Rust/Cargo/toolchain/configuration inputs equal integrated `c3a92d58...`. |
| 30 isolated observer diagnostic | Exit 0 for the exact named test, median 83 us in the selected batch. Targeted binary `quantick_app-0ece8c1e3dae95db.exe` differs from the failed workspace binary, so this is not isolation of that binary or proof of a baseline defect. |
| 31 fmt; 32 clippy; 33 build; 34 full workspace test | Authorized unchanged repair attempt 1: all exit 0 in required order; 3458 passed, 0 failed, 12 ignored. App 1908/0/4 in 4.77 seconds using `quantick_app-d59c24e9655c0239.exe`, the same workspace binary as the failure. Successful default capture hides microsecond timings; none are claimed. |
| 35 final guards; 36 staged hygiene; 37 links; 38 precommit verification | All exit 0; `final-snapshot.json` and `precommit-verification.json` bind the ten frozen files and logs. |
| 39 local commit; 40 final handoff | Exit 0; exact original head/tree/base/key and clean status returned. |

The observer test kept its 250 us limit, all assertions and default full-suite
concurrency. Prior F1 same-test history is in
`docs/architecture/f1-indicator-operations.md:187`; it also resolved unchanged
and was not established as a baseline defect. Retain Q1's one observer failure
and its successful repair attempt 1, plus quoted-blank whitespace 1, alongside
all earlier cumulative signatures above. No count resets.

At the original head, [architecture](https://github.com/milocaetano/quantick/pull/334#issuecomment-5564629693)
and [AI review](https://github.com/milocaetano/quantick/pull/334#issuecomment-5564640970)
passed independently. Reviewers recorded their own current markers; this worker
does not modify them. [Exact-head CI](https://github.com/milocaetano/quantick/actions/runs/34079732246)
failed Linux while Windows succeeded: unchanged
`layers_tests::the_trade_paint_layer_switch_stops_the_marks`, line 565,
`marks_off=309`, `marks_on=224`. Raw `Q1-review/ci-initial-failed.log` is retained.
This cause is unresolved; no diagnosed baseline defect is claimed and no CI job
retry has been spent. This docs repair is source-request-ledger repair attempt
1 for 17 omissions. A22 remains partial, and the new diff requires fresh reviews
and new exact-head CI; the original passes/markers cannot attest the new diff.
The repair's temporary mapping generator initially matched two summary-table
rows before the exhaustive inventory and stopped at its assertion before any
evidence-file write. Limiting parsing to the named exhaustive table resolved
that one preflight failure; log45 records success. This is one additional
temporary mapping-parser signature, not another product or CI retry.

### Dossier and coordinator evidence trail

The handoff index is `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/`:

| Review input or evidence | Artifact / publication owner |
| --- | --- |
| Source and authority | `Q1-start-issue.json`, `Q1-investigation.md`, `authority-D3.md`; the D3 English translation is explicitly attributed, never labelled verbatim original. |
| Fixed asks and review inputs | `Q1-derived-asks.md`, `Q1-review/mission.md`, `branch.diff`, `branch.stat`, `branch.log`, `identity.json`. The coordinator refreshes snapshots for the repaired head. |
| Mission and implementation account | Committed Q1 archive, this document, `Q1-review-dossier.md`; original dossier remains historical and the repair handoff adds its new tree/logs. |
| Local validation and failures | `Q1-validation/` raw logs, scripts, source snapshots, precommit and final-handoff JSON; repair readbacks 41–44 and forthcoming repair validation logs. |
| Independent reviews and CI | `Q1-review/arch-review.md`, `ai-report.md`, publication/identity JSON, `completeness-review.md`, `criteria-review.md`, `ci-initial-failed.log`; published PR reports linked above. |
| Issue and PR trail | [Issue #331](https://github.com/milocaetano/quantick/issues/331), [PR #334](https://github.com/milocaetano/quantick/pull/334), parent commit/repair intent comments linked above. C5 requires the coordinator to publish the repaired artifact index in the child issue comments. |
| Pending closure | New exact committed identity and clean status, fresh architecture/AI/delivery, finding disposition, full green exact-head CI, gated readiness and serialized campaign-only integration. |

This index supplies the full review handoff rather than only test logs. The
worker will return the frozen repair tree and exact validation records before
the coordinator records a new commit intent, then return its verified commit
identity. C5 publication is coordinator-owned and remains explicitly pending;
no worker GitHub write is needed to repair traceability.

Later journal metadata was inspected through read-only GitHub API retrieval
after the historical checkpoint in the request. `43-journal-readback.json`
retains the paginated source; `latest-journal-readback.md` retains the latest
five inspected bodies and `repair-readback.json` their IDs/timestamps. The
latest inspected [comment 5564723774](https://github.com/milocaetano/quantick/issues/330#issuecomment-5564723774)
is dated `2026-09-07T03:44:03Z`. Its current coordination records keep Q1 in
delivery repair, Q2 on the build host and the campaign base at `c3a92d58...`.
The historical request checkpoint and initial zero retry counters are not
treated as current authority. Further base/authority/host updates must be read
back and reconciled before dependent action.

### Complete source-to-ledger inventory

The following S IDs and ask labels come from the independent source-first
inventory fixed before its reviewer saw the mission. This table maps all 95
asks to actual R/A/G/C entries in the repaired archive. It is a traceability
map, not a delivered/PASS verdict. Every row retains its full source meaning,
including both clients, all tiers, every commit and conditional obligations;
the unchanged verbatim request remains the authority.

| Fixed source ask | Repaired ledger mapping |
| --- | --- |
| S01 Actual completed review | R11/R14 → A8/A9 |
| S02 Existing shared review key | R12/R19 → A8/A13 |
| S03 Separate completion/finding disposition | R4/R14/R17 → A2/A9/A11 |
| S04 HEAD stability around review | R13 → A9 |
| S05 Base stability around review | R13 → A9 |
| S06 Key stability around review | R13 → A9 |
| S07 Branch stability around review | R13 → A9 |
| S08 Status stability around review | R13 → A9 |
| S09 Canonical producer durable PR report | R11/R16 → A8/A10 |
| S10 Private worktree projection | R9/R14 → A6/A9 |
| S11 Full head SHA in report | R11 → A8 |
| S12 Explicit base/ref tip in report | R12 → A8 |
| S13 Review key in report | R12 → A8 |
| S14 Claude missing/readiness | R1/R15 → A1/A10 |
| S15 Codex missing/readiness | R1/R15 → A1/A10 |
| S16 Claude stale/readiness | R5–R8/R15 → A3–A5/A10 |
| S17 Codex stale/readiness | R5–R8/R15 → A3–A5/A10 |
| S18 Claude missing/campaign merge | R2/R15 → A1/A10 |
| S19 Codex missing/campaign merge | R2/R15 → A1/A10 |
| S20 Claude stale/campaign merge | R5–R8/R15 → A3–A5/A10 |
| S21 Codex stale/campaign merge | R5–R8/R15 → A3–A5/A10 |
| S22 Missing denial with zero threads | R1/R2 → A1; R4 → A2 preserves independent thread gate |
| S23 All-tier missing/stale denial | R1/R2/R5–R8/R17 → A1/A3–A5/A11 |
| S24 Clean current review permits supported gate | R3 → A2 |
| S25 Current completion plus unresolved threads denies | R4 → A2 |
| S26 Source-diff invalidation | R5 → A3 |
| S27 Branch-only invalidation at equal raw key | R6 → A4 |
| S28 Campaign base-tip invalidation | R7 → A5 |
| S29 Target/base-ref invalidation | R8 → A5 |
| S30 Unrelated-worktree isolation | R9 → A6 |
| S31 Draft creation remains possible | R10 → A7/A11 |
| S32 Preserve other reviews | R17 → A11; R28 → A22/G4 |
| S33 Preserve tier constraints | R17 → A11 |
| S34 Preserve authority/human-only main merge | R18 → A12; C4 repeats handoff boundary |
| S35 Preserve command limits | R18 → A12 |
| S36 No skip/override/new exception | R19 → A13; R17/R18 → A11/A12 |
| S37 Minimal branch-binding rationale/proof | R6/R19 → A4/A13 |
| S38 Hermetic absent-completion tests on both routes | R15/R1/R2 → A1/A10 |
| S39 Hermetic stale-completion tests on both routes | R15/R5–R8 → A3–A5/A10 |
| S40 Hermetic clean/current tests on both routes | R15/R3 → A2/A10 |
| S41 Hermetic findings/open-thread tests on both routes | R15/R4 → A2/A10 |
| S42 Hermetic branch-change tests on both routes | R15/R6 → A4/A10 |
| S43 Hermetic unrelated-worktree tests on both routes | R15/R9 → A6/A10 |
| S44 Hermetic base-tip tests on both routes | R15/R7 → A5/A10 |
| S45 Hermetic target/base-ref tests on both routes | R15/R8 → A5/A10 |
| S46 Document exact producer | R16 → A8–A10 |
| S47 Document exact consumer | R16 → A10 |
| S48 Do not duplicate policy | R16 → A10 |
| S49 Read full current code | R31 → A24 |
| S50 No copied comparator/new generic subsystem | R19 → A13 |
| S51 Required rules/skills/rationale reading | R32 → A25 |
| S52 Read issue capture/investigation | R33 → A26 |
| S53 Independent Q1 within #330/#331 | R20/R21 → A14/A15, scoped by mission objective and D1 |
| S54 Assigned branch/worktree | R34 → A27 |
| S55 Assigned path envelope/unique outputs | R20/R25 → A14/A19, assigned evidence destination |
| S56 Exclude crates/Cargo/F1 unique files | R20 → A14 |
| S57 No F1 unintegrated-code dependency | R21 → A15 |
| S58 Conditional overlap notice before path expansion | R20 → A14 |
| S59 Coordinator release before builds and edits | R21/R22 → A15/A16 |
| S60 Arm guards build and app all-target before edit | R22 → A16 |
| S61 Create high-tier mission | R35 → A28 |
| S62 Exhaustive atomic R/A/G including conditionals | R36 → A29 |
| S63 Include verbatim request | R37 → A30 |
| S64 Preserve IDs/archive before reviews | R25 → A19 |
| S65 No new runtime/financial/public contracts | R29 → A14/G3 |
| S66 Do not change rubric | R30 → A23 |
| S67 No score increment before reassessment | R30 → A23 |
| S68 D3 two-independent/nonconflicting cap and serialized merges | R18/R21 → A12/A15; C4, with D2 resolving the grant |
| S69 No routine user plan/command permissions | R38 → A31 |
| S70 No child host goals | R39 → A32 |
| S71 Never sandbox_permissions | R40 → A33 |
| S72 Ordered four checks before every green-only commit | R23 → A17/G2 |
| S73 Shell guardrails suite passes | R23 → A17 |
| S74 Affected skill validators pass | R23 → A17 |
| S75 Affected link checks pass | R23 → A17 |
| S76 Affected language checks pass | R23 → A17/G1 |
| S77 Affected context checks pass | R23 → A17/G1 |
| S78 External raw logs with exact commands/exits | R24 → A18; R23 → A17 |
| S79 No weak tests/stale stamps | R15/R23 → A1–A10/A17/G2 |
| S80 Independent architecture review at child head | R28 → A22/G4; C1 duplicates sequencing |
| S81 Independent AI review at child head | R28 → A22; C2 sequences publication/review |
| S82 Independent delivery review at child head | R28 → C1 (own-delivery-PASS exception) |
| S83 Full CI at exact child head | R28 → A22; C3 sequences readiness |
| S84 Notify before every commit | R41 → A34 |
| S85 No worker publication/review-marker/GitHub/checkpoint actions before handoff | R27 → A21; C1/C2 delegate reviews/publication to coordinator |
| S86 Coordinator owns checkpoints/pushes/PRs/merges | R27 → A21; C1–C4 |
| S87 Return verified local head | R25 → A19, with R23/A17/G2 verification |
| S88 Return full review dossier | R42 → A35 |
| S89 If base advances, notify/coordinate update | R26 → A20 |
| S90 If base advances, rerun affected validation before reviews | R26 → A20 |
| S91 Latest campaign base | R26 → A20/G2; C4 verifies current base |
| S92 PR base exactly campaign/architecture-a | R27 → C2, whose text names that exact PR base |
| S93 Progress/failure signatures/cumulative nonreset counters | R24 → A18, A15 coordination record/A21 action dossier |
| S94 Issue/archive/raw/linked-PR evidence trail | R24/R25/R43 → A18/A19/A36; C2/C5 |
| S95 Inspect later campaign journal metadata | R44 → A37; R26 → A20 |
