# Validation journal — incomplete mission

All commands below ran in feat-sync-main-chart-layout. No commit or remote
write has been performed by this executor. Full final-head checks remain open.

## Passed focused checks

- Core: 13 tests PASS, no dependencies, local target; repeated after formatting.
- Control: 2 annotation tests PASS (session93129 initial; focused repeat2.39s).
  Added a Decimal::MAX roundtrip assertion, observed FAIL before the guard in
  parse_price, then PASS after rejecting a nonrepresentable f64 wire roundtrip.
- Generated guard tests: 16 PASS, session81079. The scan includes both original
  app/control declarations and exact new control/annotation.rs; no exemption.
- Feature app all-target check: session38837 PASS21.29s, historical candidate.
- Guard command: PASS after context prose compaction preserving rules and
  explicit extracted-contract scan. Later rendering owner refinement reduced
  root count to10215, requiring/taking a two-line ratchet tightening; fresh
  guard command PASS. UI-free46911 remains unchanged, no annotation exemption.

## Full app discovery run (not a pass)

Session95325, `cargo test -p quantick-app -- --nocapture`: compile1m56s,
tests18.37s, 2065 passed / 5 failed / 11 ignored. Failures:

- observer_capability_catalog_is_registry_derived_and_versioned: expected
  current catalog regeneration after additive exact-reference input.
- hooks::tests::the_committed_registry_is_what_the_generator_emits: expected
  hook prose regeneration after feature-gating documentation.
- quick_range_never_starts_in_the_live_tape_lane: fixture lacked a live edge,
  so its lane visibility precondition failed. Adding only enabled=true was
  insufficient. Corrected fixture supplies a live print and flushes the worker;
  rerun is pending in session69772.
- orderflow_view::tests::bubbles_project_while_book_capture_stays_off and
  the_live_strip_alone_keeps_the_aggression_pipeline_running: both reproduced
  unchanged in baseline and candidate standalone suites (21PASS/2FAIL).

## Environment diagnosis: no orderflow code fix

Inherited QUANTICK_BUBBLES referenced the trader's external bubbles.toml.
The baseline orderflow suite becomes23/23PASS when ONLY this variable is
removed from its child test shell. No trader configuration was read for
diagnosis, modified, removed or replaced. Future controlled checks/perf clear
the inherited override in their child environment and state this explicitly.
Prior failures remain evidence; this is not a waiver or an orderflow patch.

## Preserved merged baseline executable

Path: C:/src/quantick-worktrees/feat-evidence-resources/target/debug/deps/quantick_app-938f257d5e0579f7.exe

SHA256: C076FF05E14F8629B3850242426F80F6A743E7A8AD972A59E68A51133AD84ECB.
This is the pre-runtime-edit merged characterization artifact. A repeated
quick_range_ invocation confirms3 expected failures (stale slots, duplicate
timestamps199/199 versus80/160, persistent selection) and1 shipped Fib PASS.
No overwritten artifact is being relabeled as baseline. It remains available
for a controlled same-host performance comparison; no performance verdict yet.

## Subsequent controlled results

- Tape-lane fixture: one live print plus worker flush establishes live_end_ms;
  session69772 PASS1/1, compile1m46s, test0.07s, inherited bubble override cleared.
- New all-three-action control tests: session41013 PASS2/2, compile1m52s,
  tests0.22s. Checks pane/revision/layout/time/price refusal without partial
  insertion, real local-transport grant denial, authorship and forbidden-key
  idempotency policy. All grants use the existing panel/control admission path.
- Current capability catalog regenerated through its existing update test;
  only111 additive lines for optional ChartReference in the three current
  future-aware action schemas. Legacy v1 schema and released fixtures unchanged.
- Feature-enabled app build37908 PASS1m36s. Offline --dump-hook-registry
  regenerated the hook registry; exactly one authored-row change, no window.
- Ordered loop started after regeneration: fmt --all -- --check PASS;
  clippy --workspace --all-targets session56754 PASS35.89s. Workspace build
  is next/running; workspace test still pending. Do not treat this as full PASS.
- Seven frozen working-file blob hashes match the initial manifest reproduced
  in prior sync-main-bars/safety-and-frozen.md. No feed/feed-mt5, admission,
  gateway or control-host source delta against reviewed9ff in the named checks.
- Visual/UX not run: active desktop input16ms idle, no app window launched.
  See visual-status.md; headless checks do not replace pixel/UI acceptance.

Ordered workspace build session25643 PASS1m32s. cargo deny check bans licenses
PASS2.34s with existing duplicate-version warnings (bans ok, licenses ok).
Fresh git diff --check and fmt --all -- --check PASS. Ordered workspace test
now follows, with QUANTICK_BUBBLES cleared only in the child environment.

Workspace test77934 was not a full PASS: app2072PASS/0FAIL/11ignored and many
subsequent crate suites passed, then guard unit test
headless::tests::the_scanned_crates_are_the_ones_the_rule_names failed because
the prose compaction changed its documented `, and it binds` delimiter.
The delimiter was restored (operative headless rule unchanged), elapsed-time
wording compacted instead, and the exact guard test plus guard command PASS.

During that run a source review found the v2 legacy timestamp branch previously
ignored bar_position. A new core assertion failed before repair; validation
now consumes fallback positions only for exact references or future-only
anchors. Core13PASS after repair; a new all-three-action app compatibility test
is added. The earlier workspace/app artifact predates this repair and is not
restamped. A fresh ordered fmt/clippy/build/test loop is required and started.

Guardrails attempts: plain sh absent from Windows PATH; absolute Git sh without
its utility PATH failed before fixture creation (both accidental candidate
directories under Program Files/Git verified absent). Retried with Git usr/bin
and bin prepended only in the child environment; session95186 still running.

Latest compatibility-repair inputs: repeated ordered fmt and clippy PASS
(command4.69s, cached clippy0.67s); workspace build76060 PASS1m44s.
Workspace test follows in the same exclusive borrowed target with only the
process-local QUANTICK_BUBBLES override cleared. No app window is involved.
The other observed workspace build/test belongs to chore-agent-evidence-out
and uses its own target; it was neither interrupted nor adopted as evidence.

Workspace test7068 completed exit1 after compile3m20s: the app suite advanced
successfully, guard unit tests200PASS, but capability_documentation had
13PASS/1FAIL. Its independently authored orphan-message expectation still
named only the old app contract directory. Updated that single expected
literal to include the new control annotation source; the orphan detection
assertion and complete-finding-set comparison remain intact. No waiver.
Manifest candidate-inputs-v2.json predates this test-only literal repair;
SHA256 B018380804B2F9C7F495D257497B45F7A59CE360EE8523B1007903E0ABF74260.

Capability-documentation repair:14PASS,0FAIL,0.08s. Fresh ordered session19185:
fmt PASS; clippy PASS1.33s; build PASS1.09s; current app2073PASS,0FAIL,
11ignored,16.93s. The rest of workspace is still running at this receipt.
App test executable7d3f212efc7d7c85 SHA256:
BFC19329AFEAC5132A0DE11AD458A365EFA16B12B415D7656D815F6D8D3666B5.

Guardrails95186 completed exit0:277passed,0failed. Nested campaign-context
67PASS, review-report8PASS, review-progress20PASS, coordinator53PASS and
recovery6PASS are emitted by that same command. It made no product edits.

Ordered session19185 subsequently completed exit0: all workspace tests and
doc tests passed, including capability documentation14/14, guard units200/200,
the current legacy/exact operation tests, and released contract fixtures.
The complete local four-command loop now passes at these uncommitted inputs.
Feature-enabled app tests are a separate next command, not implied by this
default-feature result. CI and independent current-review gates remain pending.

Current source manifest v3 contains86 changed inputs and excludes only the
evolving quick-range evidence directory. SHA256:
282DB5E83D9337CE7ED29430ECF12E88A950106C10E822DC3E2FD497ED80D158.
It includes the repaired orphan-message expectation. All seven frozen
working-file blobs were rechecked and match their original manifest.
