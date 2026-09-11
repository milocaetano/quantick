# Windows descriptor authority evidence

Issue [#335](https://github.com/milocaetano/quantick/issues/335), campaign #330,
stable task Q4. Source base: `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`.
This record concerns the maintained tests in
`crates/control-local/src/discovery/windows_tests.rs` and the separate Windows
control-local CI step. The discovery production diff is only a test-module registration behind
`cfg(all(test, windows))`. A subsequently approved ordering repair in the
existing `client.rs` fake gateway is also behind `cfg(test)`; no runtime policy
changes.

## Boundaries and independent oracles

Windows PowerShell 5.1, shipped with Windows, runs without profiles or an
interactive window. The executable is resolved beneath `SystemRoot`, not through
PATH. Paths and restoration SDDL travel through child environment variables,
never interpolated shell code. The process inherits the test process token;
no impersonation, accounts, credentials or privilege enablement are involved.

PowerShell `Get-Acl` and .NET `ObjectSecurity` inspect the actual owner SID,
protected DACL and access rules. `.NET WindowsIdentity` reports both User and
Owner. These are independent of Quantick's `GetNamedSecurityInfoW` verifier and
SID helpers. `Get-Item` independently reports actual reparse attributes,
junction type and target. Every setup failure is fatal; a generic access-denied
error cannot satisfy an exact policy-message assertion. Oracle JSON and logs
contain identities and filesystem conditions, never descriptor contents or
bearer tokens.

| Test suffix under `discovery::windows_tests::` | Actual boundary and oracle |
| --- | --- |
| `windows_private_publication_discovery_read_and_removal` | Publish a valid descriptor, independently inspect directory/file owner, protected DACL, allow principals and user read access, then read/discover/remove through production functions. |
| `windows_everyone_read_descriptor_is_refused_without_hiding_valid_peer` | Independently verify an actual Everyone (`S-1-1-0`) Read allow ACE; enumeration reports the exact untrusted-principal issue for that descriptor while retaining another valid descriptor. Restore DACL and remove both. |
| `windows_everyone_read_directory_is_refused_before_enumeration` | Independently verify the same untrusted allow on the owned directory, then call discovery without publishing again. The exact policy error precedes enumeration. Restore DACL before cleanup. |
| `windows_junction_directory_and_json_entry_are_refused_without_target_changes` | Actual directory junctions link two owned scratch roots; independently confirm reparse attribute, Junction type and expected target. Explicit-directory and `.json` entry paths receive the exact redirect refusal. Canary and target descriptor bytes remain unchanged. Nonrecursive link removal precedes scratch-root cleanup. |
| `windows_foreign_os_directory_is_refused_before_enumeration` | Read-only `SystemRoot/System32` owner differs from both token identities. Discovery returns the exact foreign-owner reason before enumeration. |
| `windows_foreign_os_file_is_refused_before_descriptor_parsing` | Read-only `SystemRoot/System32/kernel32.dll` owner differs from both identities. The private production descriptor reader returns the exact foreign-owner reason before parsing. |

Scratch objects use the existing crate-owned RAII helper. ACL restoration is
armed before mutation and junction ownership before creation, so unwinding
also attempts restoration and removes links before scratch roots. Only owned
scratch objects are mutated. The foreign OS objects are never modified.

## Prerequisites and execution record

Required: Windows with inbox PowerShell, a scratch filesystem supporting DACLs
and directory junctions, permission to create/change owned scratch ACLs and
junctions, and readable security information for the two foreign-owned OS
objects. No symlink privilege or Developer Mode is required. Missing fixtures,
redirected fixtures, or an OS owner equal to either token identity fail with a
fixture-prerequisite error; there are no ignored tests or silent skips.

Executed locally on 2026-09-07: Windows NT 10.0.26200.0, 64-bit process/OS,
PowerShell 5.1.26100.9168, NTFS. The tests independently reported TokenUser and
TokenOwner both `S-1-5-21-621640551-2506351434-2223236169-1001` and scratch
ownership matching that identity. Both OS fixtures reported TrustedInstaller
SID `S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464`, distinct
from both token identities. `windows-prerequisites.json` and the test logs
retain those observations; no elevated-token variant is claimed.

Raw logs and command/exit records are retained at
`C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q4-validation/`.
Arming commands `cargo build -p quantick-guards`,
`cargo check -p quantick-app --all-targets` and
`cargo check -p quantick-control-local --all-targets` each exited 0 before source
edits. Current executed validation:

| Exact command | Exit and raw log |
| --- | --- |
| `cargo test -p quantick-guards` | 0; `04-batch-guards.log`, then `08-repair-guards.log` (149 unit, 18 integration, 5 agreement tests). |
| `cargo fmt --all` | 0; `05-format.log`, `07-repair-format.log`. |
| `cargo test -p quantick-control-local windows_tests -- --nocapture` | Initial attempt failed during ACL setup/unwind (`06-windows-authority.log`); repair 1 exited 0 with all 6 passed, none ignored (`09-windows-authority-repair1.log`). |
| `cargo test -p quantick-control-local -- --nocapture` | 0; all 18 passed, none ignored, plus doc tests (`10-control-local-ci-command.log`). |

`commands.jsonl` preserves exact start/finish UTC, command arrays, working
directory and process exit values. `tested-source-hashes.json` identifies the
test/workflow sources for these executions. The coordinator's `coordinator-workflow-verification.json` records actual
PyYAML 6.0.3 parsing with duplicate-key rejection using an existing private Q1
dependency (no installation or repository dependency change). Top-level
triggers/env/name, the full Linux job, Windows runner and all original ordered
steps match the source base; the only appended step is the exact control-local
invocation. Verified workflow SHA256:
`ebbb038d25ce854e48c67ccf8a88fee082e52e1213df1534f2a702767b7b8466`.
This is scoped structural/command verification, not actionlint or GitHub CI.

The amended-helper run of `cargo test -p quantick-control-local -- --nocapture`
exited 0 with all 18 tests passed, none ignored, and no `panicked` output
(`11-amended-control-local.log`). The final ordered fmt-check, workspace
clippy/all-targets, workspace build and workspace-test commands and exits are
recorded in `commands.jsonl` and `final-ordered-results.json` after execution.
The first ordered sequence exited 0 for fmt, Clippy and build (logs 13-15),
then workspace tests exited 101 (log 16): the existing app test
`observer_core_capture_stays_within_the_ui_budget` measured batch medians
262/270/278 microseconds against the unchanged 250 microsecond limit. App
results were 1,907 passed, 1 failed and 4 ignored; later workspace crates did
not execute in that command. The tested six-file index tree was
`81908ff5543a0d0f9148dfbdef3635acb2c80218`; both the index tree and file bytes
were confirmed unchanged after the failure in `final-ordered-results.json`.
The failed app executable's actual path/hash is retained in
`failed-app-executable.json`.

This failure is not dismissed as harmless or attributed to a particular host
process. The existing capture-budget diagnosis describes historical failures
and measurement limits; it does not prove this occurrence's cause. Q4 stopped
before retry, released the host, and preserved local signature
`Q4-APP-CORE-CAPTURE-BUDGET` occurrence 1/repair 0 alongside the campaign history.
No capture code, threshold, sample count, existing ignore or runner concurrency
was changed. The coordinator scheduled the exact existing test in isolation
for diagnosis and one subsequent unchanged full ordered retry at Q4's next
host grant. The isolated diagnostic then executed the original failing binary, verified
SHA256 `790a3245a5053f928d80dc511ef9ceeb27efd220a856b4f48da30b87cc01b921`, with
the exact existing test name and `--exact --nocapture` (log 17). It exited 0:
median/p99/worst batches were 85/180/241, 85/150/176 and 90/145/190 microseconds.
The unchanged helper selects the lowest-p99 batch, so its selected median was
90, not the minimum median of 85. This diagnostic establishes that this same
executable passed in isolation; it neither identifies the earlier failure's
cause nor replaces the full workspace gate. The single authorized full ordered
retry follows documentation/guards and retains ordinary runner concurrency;
its exact commands/exits are recorded in `retry1-ordered-results.json` after
execution, with no pending outcome claimed as green. No commit has been made. The local
handoff requires all four successful exits, and the coordinator owns final
SHA/CI/review linkage before integration.

The first ACL setup used PowerShell `Set-Acl`, which attempted an unrelated
security section requiring `SeSecurityPrivilege`; no privilege was enabled.
Restoration hit the same setup error during unwinding and aborted that first
test process (`0xc0000409`). Repair 1 uses .NET access-section-only persistence
and prevents a second cleanup panic from aborting an existing unwind. Normal
restoration failure still fails the test. Failure counters retain one failed
invocation, two ACL setup occurrences and one cleanup-abort occurrence, with
one repair attempt per signature in `failure-ledger.json`.

That aborted attempt left four identified owned scratch roots. The real
junction was removed nonrecursively after checking its owned target. Automatic
approval review rejected both computed and literal recursive-root cleanup
commands with only `blocked by policy`; no rejected command executed, and no
further deletion workaround was attempted. Exact retained paths and commands
are in `failed-run-cleanup-roots.json` and `cleanup-policy-rejections.md`.
This retained failed-run scratch is disclosed separately from the repaired
maintained test result.

The full crate command also printed an existing detached client-test helper
panic at `client.rs:539`, `seen_tx.send(request).unwrap()` (`SendError`), despite
exit 0. Read-only inspection found response-write-before-channel-send and
callers that can drop the receiver without waiting for the helper; this explains
the race as an inference, not identification of the observed caller. At that execution the helper
and callers were unchanged, and discarding their JoinHandle permitted a background
panic to escape libtest failure accounting. This is an existing harness
observability limitation, not a claim that the panic is harmless. The focused
six-test authority log has no such panic and awaits each oracle process.
`existing-client-thread-diagnosis.md` retains the original diagnosis. The
coordinator subsequently authorized only the existing `cfg(test)` fake-gateway
helper's ordering change in [the scope extension](https://github.com/milocaetano/quantick/issues/330#issuecomment-5569365534):
move `seen_tx.send(request).unwrap()` immediately before
`stream.write_all(&frame).unwrap()`. All five fake-gateway call sites bind the
receiver (`seen`, `_seen`, `_a`, `_b`) before waiting for `LocalClient::connect`;
the channel send now completes before the reply can release that wait and let
the caller finish. Encoding, both unwraps and every existing assertion remain.
This removes the observed successful-return ordering race without ignoring
channel errors or changing the production protocol. The amended helper subsequently passed the exact
control-local CI command with 18 tests and no printed panic (log 11). This
single observed run plus the source ordering argument addresses the reported
race; it does not claim exhaustive scheduling exploration or joined helper
threads. The original failure-bearing exit-0 log remains preserved.

The coordinator records the final reviewed SHA, exact-head CI URL and executed
test names in the linked issue/PR. Windows CI now invokes
`cargo test -p quantick-control-local -- --nocapture` separately, retaining the
workspace build and engine/guards tests; the pinned toolchain and full Linux
workflow remain required. Exact-head CI and independent architecture, AI and
source-first delivery reviews remain pending before campaign integration.

## Limitations

The foreign-object tests prove real filesystem ownership checks used before
directory enumeration and descriptor parsing. They **do not prove a
foreign-created valid JSON descriptor traversing enumeration and connection**.
No connection is attempted by these policy fixtures. Optional null/missing-DACL
and missing-user-read variants are not included or claimed. This evidence does
not automatically raise AP3; the integrated SHA must be reassessed under the
unchanged rubric, retaining any stronger foreign-descriptor evidence gap.

The first source patch preceded persistence of the mission archive, despite
completed arming, an earlier written preparation plan and a coordinator-recorded
private tier. This historical ordering deviation is preserved in the mission
and external chronology evidence, remains open for independent delivery review,
and has not been self-waived.

## CI fixture repair 1: inherited PowerShell modules

Initial candidate `3ceaf72298db097f17c3e83f23a887441fc3364f`, tree
`3817b89b7e6945735800bde5f643a231a334c1c5`, passed its authorized full local
retry (logs 19-22, 3,464 passed/0 failed/12 existing ignored, all six Windows
authority tests executed). It was committed only after coordinator release and
published as draft [PR #342](https://github.com/milocaetano/quantick/pull/342).
The subsequent exact-head [Windows CI job](https://github.com/milocaetano/quantick/actions/runs/34116356314/job/101723991450)
failed all six new tests at `Get-Acl` module autoload
(`CouldNotAutoloadMatchingModule`); 12 existing tests passed. Linux and the
other Windows steps passed. Independent architecture review retained this as
Blocker B1. Local success did not discharge maintained Windows CI execution.
Raw evidence: `ci-34116356314-failed.log` and
`Q4-review-3ceaf72298db/arch-review.md`.

[Microsoft's primary guidance](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psmodulepath?view=powershell-7.6#starting-windows-powershell-from-powershell-7)
explains that PowerShell 7 module paths can survive an intermediate process and
make inbox Windows PowerShell load incompatible modules. Removing PSModulePath
from the child's environment makes Windows PowerShell reconstruct its default
paths. The test-only repair adds precisely `Command::env_remove("PSModulePath")`
to the oracle subprocess. The parent environment, process token, filesystem
policy and every existing test assertion remain unchanged. The original CI
runner's inherited environment was not logged, so this is a supported repair
hypothesis, not a claim that its exact cause was independently observed.

A controlled local negative/positive establishes the inherited-path behavior:

- Retain the original unfixed test executable and its source identity before
  editing. Preserved binary SHA256:
  `48c9bb048473921d312fcd16db377d0bcda0000fa1f8754d1320ea0ba582153b`;
  source `windows_tests.rs` SHA256:
  `1248995f2e06ae723123923f3f183c23a0e543d2a89eb78a31127c84e50efeff`.
- Create an owned external evidence fixture named
  `ci-repair1-module-fixture/Microsoft.PowerShell.Security`. Its manifest names
  a script exporting Get-Acl, module version 9.9.9 and required PowerShell
  version 99.0, deliberately incompatible with inbox 5.1. It is not installed
  in any module store. Files and hashes are retained in
  `ci-repair1-unfixed-proof.json`.
- Launch only the test subprocess with PSModulePath equal to this fixture
  directory followed by the inbox Modules directory. Run the existing
  `discovery::windows_tests::` filter with `--nocapture`. The original binary
  exits 101: all six tests fail with the same specific module-autoload error
  (log 25). This is an expected negative experiment, retained separately from
  unexpected validation failures.
- Recompile the private test child with the one environment-removal change.
  Under the identical inherited module-path value and unchanged fixture bytes,
  the full 18-test executable exits 0; all six policy tests pass (log 28).
  Parent environment equality is checked before/after. Positive binary/source
  hashes and build profile are in `ci-repair1-fixed-proof.json`; the retained
  negative binary is the prior workspace artifact, and the positive artifact
  is the targeted crate build, so their build identities are stated separately.
- The exact new CI command `cargo test -p quantick-control-local -- --nocapture`
  also exits 0 after the repair (18/18 plus doc tests, log 29). Guards pass in
  log 26. The final document-inclusive ordered check sequence is recorded in
  `ci-repair1-ordered-results.json` after execution, before commit release.

No module installation, process-global environment mutation, machine-policy
change, privilege enablement or cleanup retry occurred. The controlled fixture
and unfixed binary remain deliberate evidence artifacts. New-head Windows CI
and fresh independent review are still required; the local experiment is not
claimed as executed CI. Historical A10 remains UNMET and the four previously
policy-blocked scratch roots remain disclosed above.
