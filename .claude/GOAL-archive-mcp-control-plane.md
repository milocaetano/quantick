# Mission: finish the MCP control-plane implementation (MVP)

**Objective (one sentence):** Bring `docs/mcp-control-plane-development-plan.md` to its MVP definition of done (section 18), by finishing the work Codex left in the tree and delivering the remaining pull requests in the plan's order — each on its own branch and worktree, stacked on the previous one while the earlier ones wait for the owner's merge.

**Classification:** code change · adds capabilities (crate `mcp`, gateway, events, annotate tier) · touches hot paths (app frame loop via the gateway executor and journal) · touches user-visible surfaces (enable toggle / clients panel, mark hotkey, annotations) · engine/determinism territory (control trace) · adds things a trader does (mark, annotate, notify, attach script).

## State found on 2026-08-21

| Plan item | Branch / PR | State |
| --- | --- | --- |
| PR 0 docs | `docs/mcp-control-plane-*` | merged (`docs/control-plane/`) |
| PR 1 `quantick-control` | `feat/control-contract` | merged |
| PR 1 hardening | `fix/control-contract-hardening` (local only) | 3 commits + uncommitted test/schema edits + stash (`wire.rs` `deny_unknown_fields`); no PR |
| PR 2 observer projections | `feat/control-observer` → PR #213 | **draft**, CI green, 34 commits behind `main` |
| PR 3 gateway | `feat/control-gateway` worktree | ~4.1k new + ~1k modified lines **uncommitted** on top of PR 2 (`69ae891e`); `docs/control-plane/pr3-gateway-evidence.md` claims 15 gateway + 11 observer tests pass; full gates not run; no PR |
| PR 4 `quantick-mcp` | #222 | open, base `feat/control-gateway`, head f2ee943a, **CI pass** |
| PR 5a events / cursor / mark | #223 | open, base `feat/mcp-observer`, head d3a5fa5c, **CI pass** |
| PR 5b annotate tier | — | not started |
| PR 5c evidence | — | not started |
| scene + analysis/orderflow/session snapshot modules | — | not started |

`control-main` (detached at `c82f5106`) is a perf-baseline checkout; leave it.

## Progress log

- 2026-08-21 · D1: stash applied, `deny_unknown_fields` replaced by a reserved-names schema transform (contract §6 tolerant reader), 4 commits, four checks green before fmt; code-review step 0 launched (agent lost its finders to a concurrent review — re-run after #213's review finishes).
- 2026-08-21 · D2: #213 rebased on main (fold rename + lane-reference fixes, schema regenerated), four checks green, pushed; code-review step 0 running.
- 2026-08-21 · D3: Codex's gateway committed, rebased on PR 2; discovery + loopback client extracted to `crates/control-local`; hooks `QUANTICK_CONTROL_PANEL` / `QUANTICK_CONTROL_ACCESS`; audit findings fixed (idle-aware read timeout, duplicate request_id, advertised timeout, bounded discovery, 7 new tests); idle benchmark 8 pairs, candidate never slower. Pending: gateway tests, four checks, arch-review, PR (base `feat/control-observer`).
- Decision: PR 2/PR 3 stay stacked on `main`/PR 2, **not** on D1 (avoids a merge-into-wrong-base footgun on #213); D1 is an independent small PR.
- 2026-08-21 · **D1 PR open: https://github.com/milocaetano/quantick/pull/220** (step 0 findings resolved/deferred in the body; four checks green at 87907656; CI watch running).
- 2026-08-21 · D3: audit fixes + 7 tests + p99 guard split (median always-on, p99 ignored) at 53e98e55; four checks running; step 0 pending (serialized behind #213's review to avoid the code-review name collision).
- 2026-08-21 · D4 (`feat/mcp-observer`, worktree `feat-mcp-observer`): crate `quantick-mcp` written — 27 tests green (21 unit, 3 fake-gateway e2e, 3 stdio smoke); rebased on PR 3 head; four checks pending rerun after PR 3's.
- 2026-08-21 · **D2 done: #213 ready for review** (https://github.com/milocaetano/quantick/pull/213, head 48cec863, four checks green, review section in body, **CI pass 5m17s**).
- 2026-08-21 · #220 CI **pass** (5m17s). #213 step 0 closed (9 findings; 8 resolved on branch at 48cec863, 1 deferred with reason); PR 2 four checks rerunning; then push + ready. PR 3 step 0 launched (`high feat/control-gateway`). Stack: main ← PR2 48cec863 ← PR3 6179cc4b ← PR4 964aca12. Lesson: after rewriting a stacked base, re-stack the child with `git rebase --onto <new-base> <old-base-head>`; a plain rebase replays the old base's commits and conflicts.

- 2026-08-21 · D3 final head f661641e (four checks green, pushed); D4 final head d83dafca (four checks green, pushed); both await PR 3's step 0 (`code-review [f66d63]`, finders gw-angle*) before marker + `gh pr create`. Disk hit 0 GB mid-session (rustc ICE) — removed `target/` of verified worktrees, 49 GB free.
- 2026-08-21 · D5 (`feat/control-events`, worktree `feat-control-events`, stacked on D4): journal + cursor + parked `events.wait` (waiter manager, no UI lock) + frame emitter (tab/focus/selection/feed/replay) + `attention.mark.create` via `ActionRegistry` (Ctrl+M, `QUANTICK_CONTROL_MARK`, annotate permissions/profile declared, observer refused) + control trace sidecar with re-injection + MCP `quantick_read_events`/`quantick_wait_for_change`; schemas regenerated; tests written; evidence doc `pr5a-events-evidence.md`. Gateway 20/20, control 21/21, mcp 27/27, trace replay test green; committed; four checks running.

- 2026-08-22 · **D3 PR open: https://github.com/milocaetano/quantick/pull/221** (base `feat/control-observer`, head f661641e; step 0: 6 plausible/low findings all deferred with reason in the body; CI watch running). PR 4 step 0 launched (`high feat/mcp-observer`). #221 **CI pass** (6m50s). PR 5a: four checks green at b12f87c7, pushed (`feat/control-events`); step 0 after PR 4's (PR 4's reviewer is waiting on finders again — nudge to close if silent).

## Deliverables, in order (each = own worktree, four checks, arch-review, PR open)

- [x] **D1 — hardening PR** (#220, head 87907656, CI pass): finish `fix/control-contract-hardening` (apply the stash, keep the schema/codec agreement test, regenerate schemas), four checks, arch-review, push, PR against `main`.
- [x] **D2 — PR #213 ready** (head 48cec863, ready, CI pass): rebase `feat/control-observer` on latest `main`, four checks, arch-review (step 0 included), promote from draft to ready-for-review with evidence in the body.
- [x] **D3 — PR 3 gateway** (#221, head 60425039, CI pass): commit Codex's work on `feat/control-gateway`, rebase onto the rebased PR 2, check every ADR-0001 "required test" and every PR 3 acceptance criterion (present, or listed as a gap in the PR body), four checks, hot-path evidence (frame_cpu_ms / capture budget vs a control run, same conditions), arch-review, PR with base `feat/control-observer`.
- [x] **D4 — PR 4 `quantick-mcp`** (#222, head f2ee943a, CI pass): leaf crate + binary, STDIO only, named observer tools (`describe`, `get_snapshot`, `get_chart_window`, `get_diagnostics`, `search_capabilities`, `invoke`; `get_scene` only when the scene module exists), instance selection, ≤512-char instructions lead, read-only annotations per contract §8, stdout purity smoke test, setup assistant, `workspace_deps` ALLOWED + `CLAUDE.md` entries, fake second host in tests, blast radius in body, PR stacked on D3.
- [x] **D5 — PR 5a events + pointing** (#223, head d3a5fa5c, CI pass): journal ring buffer + cursor, `read_events` / `wait_for_change` parked off the UI queue, selection events, mark hotkey → `attention.mark.create` through the action-registry port, control-trace port; ui-harness hook for the hotkey; PR stacked on D4.
- [ ] **D6 — PR 5b annotate tier**; **D7 — PR 5c evidence**; **D8 — scene + snapshot modules** — **explicitly not started** (handoff: `scratchpad/handoff-5b-5c-scene.md`, final report §2) (as far as the session reaches; each a complete PR or explicitly reported as not started).

## Standard gates (every deliverable)

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` — all exit 0 after rebasing on the latest base.
- Performance impact declared by rate class (per-trade / per-depth / per-frame / rare) in the plan and the PR body; hot-path touches carry measured evidence, not belief.
- `arch-review` run over `git diff <base>...HEAD`, every Blocker / Should-fix resolved or deferred in the PR body; marker recorded (`git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"`) before `gh pr create`.
- New capability: port named, registration-only edits, defaults preserve today's behaviour, fake second implementation tested, blast radius (added vs edited files) in the PR body.
- Trader-facing action: drivable without a mouse — act (named call), read (structured result), discover (registry entry).
- User-visible surface: env hook registered per `ui-harness`; `visual-qa` PASS or defects explicitly accepted; `trader-ux-review` without unresolved Blocker (visual passes need the owner's authorization to launch the app — otherwise reported BLOCKED, not skipped silently).
- PR opened with green CI and the evidence in its body. **Merging is never part of the mission** — the owner merges; stacked PRs retarget when their base lands.

## Out of scope

PR 6 (cockpit), PR 7 (paper), PR 8 (public API/live) — later phases by the plan's own words. No general refactor of `app.rs`.
- 2026-08-22 · PR 4 step 0 closed: 6 findings (1 confirmed: error `structuredContent` vs `outputSchema`; 5 plausible: chart-window wording, LocalLink rediscovery churn, unbounded/non-UTF-8 stdin line, search limit bytes vs chars, `id: null` + other-mode flags) — all six fixed in `feat-mcp-observer` (22 unit + 3 + 3 green; four checks running). PR 5a step 0 launched (`high feat/control-events`, agent a9d784ea) — re-stack of 5a onto the new PR 4 head waits for it.
- 2026-08-22 · **D4 PR open: https://github.com/milocaetano/quantick/pull/222** (base `feat/control-gateway`, head 24dc0ccc; step 0: 6 findings all fixed in `fix(mcp)`; four checks green; marker recorded; CI watch running). PR 5a reviewer stalled on finders — nudged to run the pass itself; re-stack of `feat/control-events` onto 24dc0ccc after it reports.
- 2026-08-22 · #222 first CI failed on `a_capture_that_exhausts_the_budget_ends_the_frame_drain` (PR 3 code: `budget_exceeded` used `>` vs the loop's `>=`, equality after `as_micros` truncation). Fixed on #221 (60425039, four checks green, pushed, **CI pass** 5m37s); PR 4 re-stacked to f2ee943a (four checks green, pushed, CI watch). PR 5a step 0 reported 10 findings (4 confirmed: trace re-injection per active tab, wait thread spawn leak, parked wait not in in_flight_ids, dropped_before lost; 6 plausible): 9 fixed, fsync-per-gesture deferred and declared; committed, re-stacked onto f2ee943a, four checks running; `pr5a-body.md` filled.
- 2026-08-22 · **D5 PR open: https://github.com/milocaetano/quantick/pull/223** (base `feat/mcp-observer`, head 028f426b; step 0: 10 findings, 9 fixed, 1 deferred with reason in the body; four checks green; marker recorded; CI watch running). Stack: main ← #213 ← #221 ← #222 ← #223. D6 (5b annotate), D7 (5c evidence), D8 (scene + remaining snapshot modules): not started — handoff notes in `scratchpad/handoff-5b-5c-scene.md` and in the final report.
- 2026-08-22 · #222 **CI pass** on f2ee943a (6m11s). #223 CI watch running.
- 2026-08-22 · #223 first CI failed (7m07s): the new bounded-slots test raced — the late client sent its wait before the other connections' 12 waits were parked on the Linux runner, got parked instead of refused, and its read expired (`control.instance_gone`). Test fixed: each connection proves its four waits parked by reading its own per-connection overflow refusal before the next connection sends (3/3 green locally); four checks running, then commit + push + CI watch.
- 2026-08-22 · #223 test fix committed (d3a5fa5c, four checks green, pushed, marker refreshed); CI watch running on the new head.
- 2026-08-22 · #223 **CI pass** on d3a5fa5c (5m47s). Mission closed: #220, #213, #221, #222, #223 open with green CI; D6/D7/D8 not started (handoff in the final report). GOAL.md archived as GOAL-archive-mcp-control-plane.md.
- 2026-08-22 · Post-close second pass (user asked for a review): `code-review 223 high` → 6 plausible, 5 fixed + 1 wording (f982d7e8: one walk per recording, worker-counted rewinds with target, this run's marks join the walk, capability version honoured, replayed mark without target refused); four checks green; pushed; body updated; CI watch. `code-review 222 high` running. Roadmap doc `.claude/roadmap/CP-01-control-plane-fase-2.md` written for the remaining work (A snapshots, B scene, C 5b, D 5c).
- 2026-08-22 · #223 **CI pass** on f982d7e8 (6m06s). `code-review 222 high` second pass: 2 confirmed (search matched `scope_id` instead of the contract's `id`; a frame with id but no method/result/error was dropped) + 6 plausible; 6 fixed, 2 deferred (listing handshakes every instance; stale descriptor skips the reuse shortcut) — 119a5b05, four checks green, pushed, body updated, CI watch; re-stacking #223 onto it. #221 second pass: reviewer fork ended waiting for finders; asked three finders for their lists directly; the delta since its step 0 is the one-line `budget_exceeded >=` fix.
- 2026-08-22 · #222 **CI pass** on 119a5b05 (5m32s); #223 re-stacked onto it (b6adbe83, four checks green, pushed, **CI pass** 6m01s). #221 second pass via the three bug finders (reviewer fork had ended): 11 findings, 7 fixed (2 s exit wait with access disabled; transient accept errors killing the gateway; capacity rejection closing with RST; Accepted before admission; drain stop reason + scheduler-proof tests; client protocol range + envelope validation + sub-ms timeout; Windows TokenOwner ownership) and 4 deferred with reason — c16a803b, four checks green, pushed, body updated, CI watch; re-stacking #222 then #223 onto it.
- 2026-08-22 · #221 **CI pass** on c16a803b (5m49s). Disk hit 0 again mid re-stack (targets of two stale worktrees removed, 24 GB free). #222 re-stacked onto c16a803b → 35fb4d12, checks rerunning; then #223 re-stacks onto it.
- 2026-08-22 · #222 re-stacked onto c16a803b → 35fb4d12 (four checks green, pushed, CI watch). #223 re-stacked onto 35fb4d12 → db7a828f (two conflict hunks in gateway.rs resolved: PR 3's admission-before-accept kept, 5a's ConnectionSlots kept, the removed test helper dropped); four checks running.
- 2026-08-22 · #222 **CI pass** on 35fb4d12 (5m34s). #223 db7a828f four checks green, pushed, marker, CI watch.
- 2026-08-22 · #223 **CI pass** on db7a828f (6m14s). Stack final: #220 87907656, #213 48cec863, #221 c16a803b, #222 35fb4d12, #223 db7a828f — all green. Handoff branch `docs/control-plane-handoff` pushed (roadmap, GOAL archives, agent memory, reports); this machine is being discarded.
