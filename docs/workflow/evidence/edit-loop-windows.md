# Edit-loop and Windows parity: implementation evidence

Issue [#477](https://github.com/milocaetano/quantick/issues/477), C2 of campaign
[#472](https://github.com/milocaetano/quantick/issues/472).

This is an implementation/draft handoff, not mission completion, a score or
accepted calibration. Hosted Linux/Windows checks, five real touched samples
for each actual top-three crate, reviewed numeric fixed budgets, independent
current reviews and final-head green CI remain required. The first hosted
measurement intentionally fails the uncalibrated budget gate after collecting
its complete valid series; that failure must be retained.

## Source and ownership

The [archived mission](../../../.claude/GOAL-archive-edit-loop-windows.md)
retains the complete original request, delegated source and stable R1-R5/A1-A5,
G1-G6, four literal AI gates and closing steps. Exact source, original/revised
preflight reports and conformance are retained in the
[artifact index](../../../.claude/evidence/edit-loop-windows/README.md).
Preflight accepted source SHA256
D13F57B1E16E4D654CD37E0C7E503398417C282FC534886F60789B8916167702,
map revision 2 SHA256
02B0B688512ACB78EDA70D66C6B5D5DA8B0602755274C36D81795C3F2D2416B3,
and root actual-GOAL conformance at original SHA256
8BE9B8971F2CD0F18694485DBED81C845623DA94AF7AEBF6A7275DF476539C89.

Only 16 CI/tooling paths implement C2. No Rust product code, financial rule,
observer contract, ticket/replay behavior, frozen rubric, measure.py or score
skill is changed by this child. All three integrated feed/mutation/MCP fixes
remain in its base. Parent owns GitHub settings tasks, publication, review
producers, campaign integration and independent scoring; main merge is the
trader's. No setting change has been needed here.

| Owner | Responsibility |
| --- | --- |
| .github/workflows/ci.yml | Windows fmt/clippy/build/full tests, retaining explicit authority diagnostic; Linux offline safety fixtures |
| .github/workflows/edit-loop.yml | Same PR/scheduled measurement and fixed-budget check, pinned toolchain, always-upload failure evidence |
| tools/edit_loop/inputs.py and evidence.py | Frozen-lexer exact-SHA ranking, representative production module, complete input/raw manifest validation |
| sampling.py, source_time.py | Timed full package tests, five mtime-only touches, source identity/byte/metadata restoration |
| process_owner.py, supervisor.py, worker.py, snapshot.py | Owned process tree, fail-closed uncertain cleanup, allowlisted host/environment/cache identity |
| budgets.py and budgets.json | Explicitly uncalibrated bootstrap, reviewed fixed baseline protocol and rejecting drift/invalid/over-budget data |
| test_measure.py and test_evidence.py | Offline Windows/Linux safety, evidence corruption, selection and workflow behavior fixtures |

All touched paths are rare/offline CI/tooling work. There is no per-trade,
per-depth or per-frame change, no additional user-visible surface, new trading
action, financial calculation, capability or crate. No dense-runtime or pixel
PASS is claimed.

## Current source and base

Original local code commit 331947ab001025e3e296706c5343a492714a70b4 had tree
1e76097a22d620a9f3839ff0c0d49f1f3d636d68 at campaign base 4b13b64a.
Its completed old-base execution included one actual app test failure,
bounded diagnostics and an explicitly authorized successful workspace-test
retry. These remain historical evidence, never relabelled as new-base runs.

After A1R integration, the full incoming 10-path delta was inspected: retained
resources moved below app with their tests, imports, downward ratchet and
evidence. There was no overlapping C2 path. The clean unpushed commit rebased
onto 9ff57501249f51d8f75c21f52094ec3dc3c39af2 at
2026-09-15T02:07:35.8582717Z through 02:07:37.4059336Z, producing code head
0ca889b9e3c3b6b60dd52002f737d17af56082d8 and tree
71c407d226dc6fc80858506d4eb1831729cbd97b. All 16 tooling Git blobs remained
identical; pre-archive GOAL SHA256 stayed
8225DD2784E0D5455C8C4096F752246F1A408BAB857813D3E0B981EFC4050D8E.
No reset, clean, unowned stash or overwrite occurred. Original commit remains
in the private reflog and retained receipts.

Selection-only inspection at this clean code head found app 76683 production
lines (control/gateway/server.rs, 1207), orderflow 5045 (engine.rs, 1020), and
pine 4939 (parser.rs, 815). This is neither timing nor a score. The actual
measurement must independently recompute selection at its own clean H and
bind its raw report to H; the later archival head E is not silently called H.

## Current verification

Fresh full ordered execution, not reused from the old base:

| Command | UTC start/end (2026-09-15) | Result |
| --- | --- | --- |
| cargo fmt --all -- --check | 02:08:42.0001152 / 02:08:46.6717826 | exit 0 |
| cargo clippy --workspace --all-targets | 02:08:46.8441083 / 02:09:36.9785130 | exit 0 |
| cargo build --workspace | 02:09:37.1062509 / 02:18:51.9516404 | exit 0 |
| cargo test --workspace | 02:18:52.6421945 / 02:28:33.5527777 | exit 0 |

Session 4128, wrapper PID 28944, ran at code head 0ca889b9/tree71c407d2,
with process-local Cargo jobs 1 and default libtest threads. All 108 unit,
integration and doc-test summaries passed: 3924 passed, 0 failed, 21 ignored.
The app had 2056 passed/11 ignored; the eight retained-store tests moved to
control-host in A1R. No failure or retry occurred in this new-base loop.
Original failures remain in history.

Toolchain: cargo 1.98.0 (797e8a9bc), rustc 1.98.0 (88d9e12ae). Cargo.lock
SHA256 8826F73F0589B91F318C40316C18C1925B926BD7A03C0DDB393CA0A84EC795FA;
toolchain file SHA256
C979712EF4AF4226A5E600DBB6F76708E7416D716E353A030C93500E7D85DB12.
QUANTICK_BUBBLES named this checkout's committed fixture, SHA256
5DB6B44A4564F7E0CD26482A3F1BADD17EB41788E6959453330F2738BB37CD4D.

The root-leased R1 compile target was exclusive to this loop, not the physical
machine. It was never cleaned. Release was explicit after terminal status and
wrapper absence at 02:29:06.2061384Z. Raw output chunks 0-9 preserve each
command, step exit and full test output; empty polls have empty result objects,
not missing output. These durations are correctness-verification observations,
not the edit-loop protocol or a claim of uncontended performance.

Current-base offline execution at the same code tree passed 30 edit-loop
fixtures, 9 C1 read-cost fixtures, 17 frozen-lexer fixtures and ruff F lint.
It ran from 02:09:33.1261830Z through 02:11:16.3498640Z; all exit codes were 0.
These are fake-Cargo/controlled-subprocess regression tests, not actual Cargo
timings or Linux execution. Old-base fixture outputs remain under history.

The seven initial frozen blobs match the current HEAD and working files.
Cargo.lock, toolchain and committed bubbles fixture remain identical; no
schema/inventory generated input changed. The archival delta consists only
of this report, the mission archive and input-bound evidence records. Its
guard/hygiene/link/manifest checks and explicit reuse proof are retained in
the artifact index. Runtime evidence is reused from tree 71c407d2 only after
that full evidence-only delta is inspected. Independent current review keys
and exact-head CI remain due.

## Failure and feedback chronology

C2-PF1/PF2 are the original preflight source-map findings, closed at reviewed
revision 2. C2-PF3/PF4/PF5 are pre-freeze implementation feedback, not new IDs or
published repair batches: descendant ownership, exact-SHA ranking/complete
input coverage, and explicit quiescence receipts. Source fixes and their
negative fixtures are retained; Windows proves only its own branch. Linux
execution remains pending CI.

The original development failures include missing-module fixture-first TDD,
Git/source CRLF disagreement, unsupported Windows timestamp API behavior,
a fixture accidentally clearing PATH, an unused import, and an intermediate
EOF whitespace diagnostic. Their original command/result receipts and
corrections are retained; the whitespace wrapper exited 0 despite its inner
diagnostic. No numerical inner exit that was not printed is invented.

The bootstrap Project selector failed from shell quoting, then succeeded
through local JSON parsing. Its original outer wrapper exited 0; the failing
gh subcommand's numerical exit was not printed. Narrow original invocation/
result evidence excludes unrelated successful parent text. PowerShell's
NativeCommandError formatting on ordinary Cargo stderr was not itself a
failed Cargo check: the actual pre-edit guard/app-check exits were 0.

The old-base full test run failed at profile_pointer_tests.rs:221, actual
Default versus expected NotAllowed cursor. The test file was byte-identical
to the original campaign baseline; root cause remains unproven. Same-binary
exact/four-neighbor serial runs passed. An explicit inspector-hook run and
20 fixed-count concurrent pairs also passed, so the suspected unshared-lock
environment interaction was not reproduced or repaired by inference. One
complete default-thread app run then passed; root explicitly authorized one
unchanged-input workspace-test retry, which passed. No assertions, required
tests, ignore flags or thread defaults changed.

Two subsequent diagnostic invocation errors are retained: unsupported
PowerShell Get-Date -AsUTC and a refused direct script-policy launch. The
unchanged diagnostic script then ran with a process-only execution-policy
argument, never a persistent policy change. Private evidence-authoring typo/
patch diagnostics are disclosed in the capture provenance.

The original Start-Transcript file captured ambient concurrent console text.
It is private, unsafe, excluded from this archive and never used as scoped
validation evidence. Original validation is proven by complete stored tool
output chunks; current runs use scoped output without Start-Transcript.
Empty polls and display-only abbreviation are distinguished from stored
truncation. No missing output is reconstructed.

The code commit under prior standing authority completed just before root's
later hold message was delivered; the chronology was reported immediately.
No push occurred, and further commits/base adoption waited for the next
explicit instruction. Campaign operation failure count remains 1 (rejected
initial ci/ prefix); formal review-repair batches remain 0. Development/tool
diagnostics and the later authorized test retry are not silently erased.

## Remaining acceptance and handoff

| Criterion | Evidence now | Still required |
| --- | --- | --- |
| A1 | Workflow ordering/retained checks and local fixture proof | Actual exact-head Linux/Windows steps |
| A2 | Safe runner, selection/ownership/restore/evidence fixtures | Five real touched samples per actual top-three crate, identified host, under-30-day exact H, committed raw artifact |
| A3 | Rejecting checker, fixed protocol, uncalibrated status, failure-upload path | Initial failing calibration artifact, reviewed justified numeric budgets, separate same-job PASS |
| A4 | All observed applicable failure histories and unchanged current validation | Diagnose any future CI failure; no speculative fix or hidden retry |
| A5 | Seven frozen blobs, disjoint code diff, full inherited regressions | Current independent reviews; campaign blind assessment remains parent-owned |

First candidate publication must remain a draft. Download and preserve the
complete initial hosted artifact even when the budget gate correctly fails;
inspect it with the committed checker, then independently review the proposed
fixed baseline and commit it without discarding slow samples or auto-learning
a passing limit. Observe a separate checked run at its actual head. The first
real default-branch cron execution can occur only after the trader's merge;
a PR-triggered run never substitutes for that final main obligation.

No architecture/AI/delivery PASS, ready state, final verifier, main score,
quantick-score preservation or campaign completion is self-awarded here.
