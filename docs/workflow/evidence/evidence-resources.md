# Retained evidence resources: implementation evidence

Issue [#490](https://github.com/milocaetano/quantick/issues/490), bounded child
A1R of [#478](https://github.com/milocaetano/quantick/issues/478) and campaign
[#472](https://github.com/milocaetano/quantick/issues/472). This report records
source movement and proof, not a score or completion of A1, the campaign or main.

Status: full ordered local verification passed at synchronized campaign base
`4b13b64a0f4556470cbf6b7a5e1c7f1b000d0f50`, executed source tree
`4e2148b5c2141c97a3c7e6ba0778eeaf846df40e`. Workspace tests passed 3924,
failed 0 and ignored 21. All 64 required host and 44 required app cases executed
successfully within that run. Independent current reviews, exact-head CI and
canonical ship completion remain pending. No new visual execution is claimed.

## Source and preflight

The source-first high-tier [independent preflight](https://github.com/milocaetano/quantick/issues/490#issuecomment-5671417195)
accepted source S0 and GOAL SHA256
`0CF58E05DA51CE39ACEA4F321571DE060ED88ABB14B6C9BEBDA7E3DC57F1D58B`
before product edits. The archived mission retains the full original user
request, delegated issue and implementation authority. The private git-dir
`evidence/bootstrap/` retains full source, environment/cache/input receipt and
observed compiler output: own-target guards build exit 0 (15.57 s), then leased
target app/control-host all-targets check exit 0 (50.20 s). These were pre-edit
baseline checks, not validation of this implementation.

## Ownership and actual behavior

The former `crates/app/src/control/evidence/store.rs` is now
`crates/control-host/src/evidence.rs`, with its eight existing tests in
`crates/control-host/src/evidence/tests.rs`. The same owner also holds
`EvidenceChunk`, `EvidenceChunkPage`, media/resource identity constants and the
derived maximum chunk count. No new crate, dependency or generic framework.

`RetainedBundle::new` still derives digest, byte count and chunks from supplied
bytes; invariant-bearing fields remain private. `EvidenceStore` still receives
caller time, capture epoch and current permissions. The shared mutex/poison
behavior, whole-deque expiry sweep, admission order, count/byte bounds and page
cursor validation are unchanged. `known_error` was already owned by host
admission; its direct import preserves the identical error constructor. The
private usize conversion retains the same saturating conversion.

The caller still supplies canonical capture bytes, capture time, current time
and current permissions. This store does not validate canonical JSON, perform
capture/redaction or replace capability admission. Retention expiry is evaluated
when the caller invokes the existing lifecycle methods, not by a hidden clock.

Real app consumers remain manifest construction, response-worker page reads
and access-withdrawal clearing. Stable app reexports keep the contract/gateway
call sites unchanged. Capture, configuration redaction, system/scene collection,
PNG encoding and image geometry remain in app. The image source changes only
its digest-helper import. Existing wire DTO names, serde/schemars attributes,
constants and validation/refusal order are retained.

Initial source comparison found the moved store implementation token-identical
after excluding intended visibility widening, comments, formatting and optional
trailing commas. All eight existing test bodies/assertions match under the same
normalization. Two old comments describing a front-only expiry sweep were
corrected to describe the already-existing whole-deque sweep; no behavior change.
App compilation subsequently exposed one missed existing test consumer:
`gateway/screenshot.rs::retained_evidence_for_test` needs `retained()` when host
is a dependency. The existing count method is now public without `cfg(test)`;
its documented result is physical storage count, not a live-resource guarantee
or expiry sweep. App helper/assertions stay unchanged. No wire capability,
production invocation, lock or timer was added.

Rate class remains rare/on-demand response-worker work and withdrawal clearing.
Hash passes, copies and locks stay at their existing call sites; no additional
per-trade/depth/frame work or UI-thread work. No renderer/capture behavior changes,
so no new visual surface or harness hook is introduced and no new screenshot PASS
is claimed. The actual diff remains subject to independent applicability review.

## Public second-consumer proof

`crates/control-host/tests/evidence_resource.rs` uses only public control/host
types. Its independent producer emits a fixed 1,000,000-byte canonical JSON
string; a separate .NET SHA256 computation established the literal whole and
chunk hashes. Expectations do not call the implementation's digest helper.
This fixture is not described as a second production host.

The five tests cover:

- derived metadata and fixed hashes; six chunks across two pages; exact byte
  reassembly, offsets, tail length, wire integer strings and typed round-trip;
- foreign query, instance and resource refusal, stale revision and past-end
  cursor refusal with their unchanged per-code retry advice;
- grant reduction between pages, including the exact typed error envelope;
- expiry at the deadline, shared-clone withdrawal, obsolete in-flight capture
  refusal and successful insertion under the new epoch;
- physical retained counts across insertion/clear/expiry, including an expired
  entry counted until a caller-triggered sweep removes it.

The eight moved store tests retain count/byte/age eviction, overlarge refusal,
backwards-clock retention, cursor checks, reduced grants, clear, obsolete epoch
and full expiry sweep/byte reclamation assertions. Existing app/socket evidence
tests remain untouched, including scope laundering, leakage, screenshot-region
correlation, registered capture hook and no-idle-frame-cost proof. Generated
schema/catalog tests and the full suite passed at the synchronized base below.

## Validation record and pending work

- Initial `cargo fmt --all -- --check` and `git diff --check`: PASS.
- Initial guards: one downward app UI-free ratchet finding, 47225 against 47665.
  Raw output is retained in private `evidence/guards-before-sync.log`. The
  coordinator authorized exact recomputation after MS1 synchronization; no
  ceiling increase or guessed baseline adjustment is permitted.
- First targeted host execution: 47 unit tests (including the moved eight) and
  12 existing admission fixtures passed; the new evidence fixtures were 3/4.
  A1R-T1 corrects a new fixture expectation: the pre-existing read-only
  PAGE_STALE error is retryable, while CURSOR_INVALID is not. No runtime error
  policy changed. Raw failed output is retained in private
  `evidence/targeted-host-attempt1.log`; corrected host attempt2 passed all63.
- App attempt1 failed before tests because a stale borrowed-cache host variant
  omitted the new module. A1R-T2's authorized content-identical `lib.rs` patch
  changed only its timestamp; SHA256 equality and rebuilt dep-info are retained.
  No target deletion/clean/config edit. App attempt2 then exposed the distinct
  A1R-T3 `cfg(test)` count-method seam, fixed as described above.
- Final targeted runs: host attempt3 passed47 unit +12 admission +5 public
  evidence tests; identical app union attempt3 passed44 tests, including real
  socket paging, grant/leakage, screenshot, launch-hook count, no-idle-frame-cost
  and generated schema/catalog checks. Compiler times4.45s and3m36s respectively;
  app tests0.91s. The borrowed target lease was released after completion.
- Private `evidence/targeted-status.md`, `targeted-failures.md`, before/after
  input receipts and complete attempt logs preserve commands, source hashes,
  cache identity, all failures and their distinct corrections. These initial
  development failures are not erased or passed off as final review evidence.
- Those targeted attempts used the historical bootstrap base `432632239f924883c07e76904551be0faf0f37eb`.
  They are not substituted for the fresh synchronized-base execution below.

## Synchronized-base verification

After source inspection of MS1's entire incoming 36-path delta, the owned branch
fast-forwarded to `4b13b64a0f4556470cbf6b7a5e1c7f1b000d0f50` (base tree
`43b6ed4dc79b37a5808202fca082d1ded5baec38`) on 2026-09-15 at 00:49:58 UTC.
There was no overlapping dirty path. All owned source hashes, the then-current
GOAL and original S0 remained identical across adoption; private
`ms1-adoption-before.log` and `ms1-adoption-after.log` retain the comparison.
S0 SHA256 remains `33592A8F66AE7E60C330B87760CCA94AE88A3038435103307179895E5AA96AC1`.
The original preflight and bootstrap chronology are retained, not restamped.

The unchanged own-worktree guard measured 46911 against MS1's incoming 47351
UI-free ceiling. The initial exit1 is preserved; the authorized exact downward
entry and budget now both equal 46911, and guards passed. No ceiling increase,
measurement modification, guessed subtraction or score execution occurred.

Fresh commands below ran in the A1R worktree against staged input tree
`4e2148b5c2141c97a3c7e6ba0778eeaf846df40e`. Cargo compilation used the exclusively
leased `C:/src/quantick-worktrees/fix-mutation-retry-truth/target`, jobs1, without
cleaning, deleting artifacts or stopping other processes. Times are UTC on
2026-09-15; they describe correctness validation, not a performance benchmark.

| Command | Start / terminal finish | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 00:51:21 / 00:51:23 | exit0 |
| `cargo clippy --workspace --all-targets --jobs 1 --target-dir <leased-target>` | 00:51:26 / 00:52:34 | exit0 |
| `cargo build --workspace --jobs 1 --target-dir <leased-target>` | 00:53:13 / 00:58:00 | exit0 |
| `cargo test --workspace --jobs 1 --target-dir <leased-target> -- --test-threads 1` | 00:58:47 / 01:14:44 | exit0; 3924 passed, 0 failed, 21 ignored |

Full test output includes 108 unit/integration/doc summaries. Its compilation
finished in 13m52s. Exact-name reconciliation against all 64 prior host cases
(47 unit, 12 admission, 5 evidence fixtures) and all 44 required app cases found
108 fresh `... ok` lines and zero missing/filtered-only/ignored required cases.
`current-targeted-coverage.md` indexes each raw line. Under the coordinator's
explicit coverage instruction, identical tests were not rerun just to create
another log. The target was explicitly released after terminal proof and process
exit verification at 01:15:14 UTC; it is available to the coordinator for C2.

The selected runtime fixture is the unchanged committed
`crates/app/config/bubbles.toml`, SHA256
`5DB6B44A4564F7E0CD26482A3F1BADD17EB41788E6959453330F2738BB37CD4D`,
set process-locally through `QUANTICK_BUBBLES`. Earlier non-runtime fmt/clippy/build
inherited the user config (SHA256
`356E0FE04464D7AA05025D097FD38E14D802E652FB85C3140F0414121C8C11CE`);
that environment is retained separately, never relabelled. No user config was
edited. Toolchain: cargo1.98.0 (797e8a9bc), rustc1.98.0 (88d9e12ae).
Dependency/config/environment hashes are in `current-base-inputs.log`.

The seven frozen working-file/base blob comparisons passed. Published schemas,
capability inventory/catalog, Cargo manifests/lock and CI files have no changed
input. The unchanged Python read-cost tests passed9 (00:52:53 to00:52:59, exit0).
Source comparison reconfirmed all eight moved test bodies and the store behavior,
apart from the documented visibility/test-seam repair. Actual full-suite results
also cover Binance/MT5 loss reporting, mutation uncertainty, MCP idempotency,
ticket and replay regressions. No manual/ignored benchmark is called a PASS.

All raw artifacts below are in the owned worktree's private git-dir `evidence/`.
They retain complete observed output, exact commands, timestamps and exits.

| Artifact | SHA256 |
| --- | --- |
| `current-fmt-attempt1.log` | `B3A53432CF2CA86A5A0960ED9247EAA9445649B631566D00E87DAD22B76A68BE` |
| `current-clippy-attempt1.log` | `6406DFAC0C3D9AF54F7DC4049B730D63E674E73B06826D49174A072D18D02DAF` |
| `current-build-attempt1.log` | `59A32C7EF9E2F4AA2F635CDEBBF1DAB8326A1C35D5F1349EB3F9C3180C7156A7` |
| `current-test-attempt1.log` | `DF2E611BA9061DAF269B20998862FB62936F6448861D54D6C9896F4FDFC4EAB4` |
| `current-targeted-coverage.md` | `059E0CD6B99BED6057FD43315973B3FCF025D5FDC87453C3726BFCD6F9136E62` |
| `current-frozen-contract-readcost.log` | `C44CA903BC7530917994A227F65E144A453AC47FCD6C5553D88B0FAE4C073C98` |
| `current-source-compatibility.log` | `89CF089B46B23D1DB118CD2BC97B1353CC0B36B65D06020DAE5D6BD8D188BA45` |
| `current-implementation-comparison.log` | `140216DAD03019677388F2021BADAB80EC69A5457221EC4F5120459BE92F19A2` |

Only this evidence record and the mission archive change after the executed
tree; the entire delta, unchanged source/config/generated inputs, guards,
relative links and hygiene are checked before commit in private
`current-final-inputs.log` and `current-reuse-receipt.md`. Runtime evidence is
reused from the explicit executed tree, not claimed as final-commit execution.
Current independent architecture/AI/high delivery reviews, exact-head CI and
canonical ship completion remain coordinator-owned and pending. Independent
main movement after MS1 is not silently incorporated into this unchanged-base run.

All three integrated defect fixes and their regressions remain in scope for
preservation: Binance/MT5 loss disclosure, non-retryable potentially-executed
mutation uncertainty, and generic MCP invocation idempotency keys. No ticket,
replay, financial behavior, public permission, test, guard or review relaxation.

## Remaining responsibilities

This slice removes the real retained-resource owner, not the broader evidence
family. The old store was 679 raw lines including its tests; the host module is
447 raw lines, its eight moved tests are 294, and five new public fixtures are
288. The app evidence facade loses 50 net raw lines; its image adapter changes
one import. Host registration adds one line and its README adds15. These are
actual source spans/diff counts, not a scored metric or credit for test movement.

App capture/assembly/redaction/image work remains; admission, gateway protocol,
observer projection/wire ownership and semantic ownership are separate decisions.
A1's remaining owner inventory and parent-owned independent assessment remain
open. This child does not promise a numeric threshold or close A1.
