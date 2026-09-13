# Mission: move the UI-free control-plane host core out of `crates/app` into the headless crate `quantick-control-host` — slice 1 of #442

## Objective

Create `crates/control-host` (package `quantick-control-host`), a headless
crate beside `control`, and move into it the first owner slice of the UI-free
control-plane host code that lives in `crates/app/src/control/`: the snapshot
projection registry, the contract's capability-admission checks, and — the
slice measured small enough to carry it — the idempotency store and the event
journal. Time is injected, never read. `app` keeps every egui-bound part, the
authority table, the capability handlers, the gateway server loop and the
evidence store, reaching the moved code through the same paths it uses today.
Behaviour is identical and every generated artifact is byte-identical.

Why it matters: the gateway's registry, idempotency store, journal and
contract validation are UI-free yet sit in the UI crate, where the headless
guard never scans them and every change recompiles `app`. Campaign task Q17 of
#367, phase 2; the issue splits the work by owner, and this is the first PR.

**Tier:** `high`, as the campaign assigned it. A new crate, a moved public
surface and a published contract whose schemas must not move: `delivery-review`
runs in full, `arch-review` step 0 runs `code-review` at `medium`. The tier is
fixed by the coordinator and recorded in the git dir; it is not lowered.

## Request ledger

- **R1** — Move the UI-free host code into a headless crate — "extend
  `crates/control`, or a new `control-host` crate beside it; record the choice
  against `AGENTS.md`'s map and `crates/guards/src/graph.rs`".
- **R2** — "with time injected (the idempotency retention, deadlines) instead
  of read, so it satisfies the headless rules".
- **R3** — "`app` keeps the egui-bound parts (panel, frame service, screenshot
  capture, scene/layout projection) behind a narrow port."
- **R4** — "Behaviour identical: every control-plane, retry-matrix,
  schema-compatibility and authority test passes unchanged; generated
  inventories and matrices unchanged" — the coordinator adds "(prove with a
  byte diff of the generated files)".
- **R5** — Split by owner. This mission delivers "the **first slice**
  (registry/contract validation and whatever is needed for it to compile
  headless), as one PR"; "if after measuring the first slice is small, you may
  include the idempotency/journal/recovery slice too, but keep the PR
  reviewable (aim under ~3,000 moved lines)".
- **R6** — Record "in the PR body and on #442 (one comment) the remaining
  slices (idempotency/journal/recovery, then the server loop) with their file
  lists and line counts, so the coordinator can dispatch them next".
- **R7** — Issue A1: "the moved crate is in `HEADLESS_CRATES`; `cargo test -p
  quantick-guards` green; the dependency graph gains no reverse edge."
- **R8** — Issue A3: "incremental rebuild time for a one-line change in a
  moved module measured before/after, reported"; coordinator: "state the load
  you observed, repeat three times, report the median".
- **R9** — Issue A4: merged into `campaign/lean-a-plus` — the coordinator's
  act. This mission ends at a ready PR: "**Never merge**".
- **R10** — Parallel work: Q16 (#441) and Q18 (#443) run beside this. "Keep
  edits to `AGENTS.md`'s map, `crates/guards/src/graph.rs`, `HEADLESS_CRATES`
  and the workspace `Cargo.toml` minimal and line-local. Do not touch
  `crates/app/src/paper_account*`."
- **R11** — Purpose, which judges the rest: the moved code is scanned by the
  headless guard and can be built and tested without recompiling `app`.
- **R12** — Process: the `/mission` workflow at tier high — draft PR against
  `campaign/lean-a-plus`, arch-review (step 0 code-review), ai-review with
  resolved threads (two repair rounds), delivery-review last, CI green,
  `gh pr ready` once, `mission_ship_gate.sh ship`; return a handoff block.

## Decisions taken by the trader

None. This mission runs as a campaign subagent and cannot ask; every question
the full round would have asked is an assumption below marked *wanted to ask*.

## Assumptions

- **S1** *(wanted to ask)* — **A new crate, `control-host`, not an extension of
  `control`.** `control` is the transport-neutral contract that the client
  side (`control-local`, `mcp`) links; the projection registry, the
  idempotency store and the journal are the *host's* machinery and bring
  `crossbeam-channel`. Putting them in `control` would make every MCP client
  compile the host. The edge is `control-host → control`, one-way, recorded in
  `graph.rs`, `AGENTS.md` and `CLAUDE.md`. Safe: the crate name is one rename
  away and nothing outside `app` depends on it.
- **S2** — **What slice 1 moves, measured.** Registry (`control/registry.rs`,
  444 lines) and the contract's admission checks are small (~750 lines), so
  the idempotency store (`gateway/idempotency.rs`, 1,194) and the event journal
  (`journal.rs`, 370) come too: about 2,300 moved lines, under the ~3,000
  ceiling. `recovery.rs` does **not** move: it is the cockpit tier's
  `feed.recover`/`feed.reload` action handler, driving `QuantickApp` and
  `quantick_feed::stall` — application behaviour, not host plumbing, the same
  class as `trade.rs`. The word "recovery" in the issue's list is recorded as
  read that way. Safe: moving it later is additive.
- **S3** — **The authority table stays in `app` for now.** `ObserverContract::new`
  (profiles, permissions, effects, the read-capability registrations) is
  UI-free, but it names constants owned by ten app modules (`actions`,
  `annotate`, `trade`, `notify`, `script`, `events`, `evidence`, `layout`, …).
  Moving it drags those modules' vocabulary; it is recorded as a remaining
  slice. What moves is the machinery the table feeds: capability
  registration with compiled schemas, the admission order (envelope,
  registration, permission, schema, idempotency, tier), the dynamic-scope
  check, output validation and the snapshot-scope catalogue.
- **S4** — **Paths do not move for consumers.** `app` keeps
  `control/registry.rs` as the registry specialised to `QuantickApp` plus
  re-exports (a forwarding newtype rather than the planned type alias — see
  S9), and `control/mod.rs` / `gateway.rs` name the moved `journal` and
  `idempotency` modules with a `use`, so every `super::journal::…` and
  `super::idempotency::…` path in `app` compiles unchanged. Tests that lived
  inside the moved files travel with them; tests under
  `crates/app/src/app/tests/` do not change at all.
- **S5** — **The clock is one port.** `quantick_control_host::clock::HostClock`
  (`unix_ms`, `monotonic`) is handed to the projection registry at
  construction; `app` supplies the system implementation. The registry's
  `captured_at_unix_ms` and its capture stopwatch read the port, so the crate
  has zero `SystemTime::now`/`Instant::now` and no allowlist entry. The
  idempotency store already takes `now_ms` from its caller and keeps doing so.
- **S6** — **`describe()`'s application version stays in `app`.** It reads
  `env!("CARGO_PKG_VERSION")` and `option_env!("QUANTICK_GIT_COMMIT")`, which
  name the *application's* build; evaluated inside another crate they would
  name that crate. It is the one reason `describe` does not move.
- **S7** — **The A3 measurement.** One-line change = flipping one string
  literal in the moved `journal` module. *Before*: `cargo build -p quantick-app`
  and `cargo test -p quantick-app --no-run` after the edit. *After*: the same
  two for `app`, plus `cargo build -p quantick-control-host` and `cargo test -p
  quantick-control-host --no-run` — the loop a change in the moved module now
  needs. Three runs each, median reported, CPU load and concurrent
  `rustc`/`cargo` process count recorded beside each.
- **S8** — **Visibility.** Items that cross the crate boundary go from
  `pub(crate)` to `pub`; this is the boundary, not behaviour. One exception
  found while building: the journal's test-only `len` became private, because
  a public `len` without `is_empty` is a clippy error once the type is really
  exported.
- **S9** — **`app`'s `ProjectionRegistry` is a forwarding newtype, not a type
  alias.** Found while building: the extension-boundary guard
  (`crates/guards/src/extension_boundary/scan.rs`) refuses every `type` alias
  whose right-hand side names `QuantickApp` ("unsupported scope must be
  diagnosed, not exempted"), and a `Deref` impl would need exactly such an
  alias in `type Target`. So `control/registry.rs` holds
  `struct ProjectionRegistry(projection::ProjectionRegistry<QuantickApp>)` with
  one forwarding method per registry method; the guard is not touched.
- **S10** — **`AGENTS.md` pays for its map entry.** The file sat 9 bytes under
  its context ceiling; the node, the edge and the table row cost ~230 bytes.
  Four sentences were tightened — the `setup` paragraph, the discoverability
  sentence, and the restatements under rules 4 and 5 — with no rule removed,
  so the ceiling and the budget do not move. `CLAUDE.md` gains 34 bytes, within
  its ceiling. `deny.toml`'s count of `quantick-*` crates goes from eighteen to
  nineteen.
- **S12** — **Admission is proved inside its own crate.** Found by the shape
  pass: the moved checks were covered only by `app`'s tests, so a change to
  them could not be tested without building `app` — against R11.
  `crates/control-host/tests/admission_contract.rs` (9 tests over
  `quantick_control::fake`'s reference registry) proves the order and every
  refusal from outside the crate.
- **S11** — **`contract.rs`'s own test module is untouched.** The tests reach
  `json!` through `use super::*`; production no longer uses it, so the
  import is kept under `#[cfg(test)]` rather than editing the tests.

## Acceptance criteria

- [x] **A1** — `crates/control-host` exists as package `quantick-control-host`,
      workspace-inherited edition and lints, depending on `quantick-control`,
      `crossbeam-channel`, `schemars`, `serde`, `serde_json` and nothing that
      is UI, async or network; it is in `HEADLESS_CRATES`, in `CLAUDE.md`'s
      headless sentence and dependency line, in `graph.rs` `ALLOWED` as
      `("control-host", &["control"])`, and in `AGENTS.md`'s map (node, edges,
      row); `cargo test -p quantick-guards` is green.
      *Evidence:* the manifest and `cargo tree -p quantick-control-host -e normal --depth 1`;
      the guards summary line. → PR body. *(R1, R7)*
      *Result:* `cargo tree` lists exactly `crossbeam-channel`,
      `quantick-control`, `schemars`, `serde`, `serde_json`;
      `cargo test -p quantick-guards` 172 + 14 + 13 + 24 + 5 passed.
- [x] **A2** — The projection registry, the admission checks, the idempotency
      store and the event journal live under `crates/control-host/src/`; `app`
      keeps `recovery.rs`, the authority table, every handler, the gateway
      server and every egui-bound file; the moved and remaining files are
      listed with line counts.
      *Evidence:* `git diff --stat` against the base and `wc -l` of both sides.
      → PR body. *(R1, R3, R5)*
- [x] **A3** — No clock is read in `control-host` production source: the
      registry takes a `HostClock`, the idempotency store takes `now_ms`; a
      test in `control-host` drives capture with a fixed clock and sees its
      time in the capture; `headless-allowlist.txt` gains no line.
      *Evidence:* the test name and result; `git diff` of the allowlist empty;
      `grep -rnE 'Instant::now|SystemTime::now|egui|eframe|tokio' crates/control-host/src`
      with every hit shown to be in a test module. → PR body. *(R2, R11)*
      *Result:* `.claude/evidence/control-host-headless/headless.txt` — one hit,
      a `thread::spawn` inside `idempotency.rs`'s test module; tests
      `projection::tests::a_capture_is_stamped_and_timed_by_the_injected_clock`
      and two siblings pass.
- [x] **A4** — Behaviour identical: `crates/app/src/app/tests/` has an empty
      diff; every test inside a moved file runs in `quantick-control-host`,
      the same count before and after; the full workspace suite is green.
      *Evidence:* `git diff --stat -- crates/app/src/app/tests` empty; the two
      `#[test]` counts; the test summary lines. → PR body. *(R4)*
      *Result:* `.claude/evidence/control-host-headless/tests.txt` — 26 moved
      tests, 26 in `control-host`; the app lists 2,143 → 2,117 and is otherwise
      identical; the workspace suite is green.
- [x] **A5** — Generated artifacts byte-identical: the capability inventory,
      the retry matrix, the UI behaviour matrix and the hook registry dumped
      from the branch equal the committed files, and the control-schema test
      run with `QUANTICK_UPDATE_CONTROL_SCHEMAS=1` leaves `schemas/` and
      `docs/control-plane/` without a diff.
      *Evidence:* the four `cmp` results and `git status --short -- schemas docs`
      empty. → PR body. *(R4)*
      *Result:* `.claude/evidence/control-host-headless/generated-artifacts.txt`
      — all four identical, status empty.
- [x] **A6** — Incremental rebuild for a one-line change in a moved module,
      before and after, three runs each, median, with the observed load.
      *Evidence:* the table. → PR body. *(R8, R11)*
      *Result:* `.claude/evidence/control-host-headless/a3-rebuild.txt`; medians
      app build 25.2 s → 15.8 s, app test build 47.7 s → 24.0 s, and the
      moved module's own loop 1.3 s (build) / 2.6 s (test build), under
      59–100 % CPU load from other agents' builds.
- [ ] **A7** — The remaining slices, with file lists and line counts, are
      recorded in the PR body and in exactly one comment on #442.
      *Evidence:* the comment URL. → PR body. *(R5, R6)*
- [x] **A8** — Parallel-work hygiene: no path under
      `crates/app/src/paper_account*` in the diff; the edits to `AGENTS.md`,
      `graph.rs`, `headless.rs` and the root `Cargo.toml` are line-local.
      *Evidence:* `git diff --stat` for those files, and the files likely to
      conflict with Q16/Q18 named. → PR body. *(R10)*
- [x] **A9** — `cargo test -p quantick-control-host` builds and passes without
      compiling `quantick-app`.
      *Evidence:* the command's output has no `Compiling quantick-app` line.
      → PR body. *(R11)*
      *Result:* `tests.txt` — only `Compiling quantick-control-host`, and
      `quantick-app` appears 0 times in its full dependency tree.

- [ ] **G1** — Every artifact in English; `arch-review` dimension 8 and the
      language guard pass. → arch-review report.
- [ ] **G2** — The four checks green at the final head, each run on its own
      (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`)
      plus `cargo test -p quantick-guards`. → PR body.
- [x] **G3** — Performance impact declared: projection capture runs per
      control request on the UI thread (rare, request-rate), and gains one
      dynamic call through the clock port where it called `Instant::now()`
      directly; idempotency and journal are unchanged code in a new crate
      (per request / per semantic event, rare). No per-trade, per-depth or
      per-frame path is touched. → PR body.
- [ ] **G4** — `arch-review` run over `origin/campaign/lean-a-plus`, step 0 at
      `medium`, every Blocker and Should-fix resolved or deferred in the PR
      body. → PR comment via `review_report.sh`.
- [x] **G5** — `new-extension` for a new crate: the port is named
      (`HostClock`), registration edits only in the shared files, defaults
      preserve today's behaviour, a fake second implementation (a fixed clock)
      is tested, blast radius stated. → PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence destinations: the AI-review report comment on the PR, the thread
list output in the PR body, and the marker under the campaign review key.

## Closing steps

- **C1** — `delivery-review` PASS, published through `review_report.sh`.
- **C2** — The PR is open against `campaign/lean-a-plus`, non-draft, exact-head
  CI green.
- **C3** — `sh .claude/hooks/mission_ship_gate.sh ship <n>` PASS.

## Not applicable

- *Touches a hot path* — no per-trade, per-depth or per-frame path changes (G3).
- *Touches anything user-visible* — no surface, string or pixel changes; the
  generated matrices prove it (A5).
- *Adds something a trader does* — no new action; the capability set is the
  generated inventory, unchanged.
- *Engine / determinism territory* — the engine is not touched.
- *A4 of the issue (merge into the campaign branch)* — the coordinator's act
  (R9).

## Verbatim request

> Execute campaign task **Q17**, issue https://github.com/milocaetano/quantick/issues/442 (read it in full with `gh issue view 442`), of campaign #367 (https://github.com/milocaetano/quantick/issues/367), phase 2, mission tier **high**, through the `/mission` workflow of this repository (`.claude/skills/mission/SKILL.md` in the worktree). You are a subagent and cannot ask the trader; make routine judgment calls, record them in the goal file and PR body.
>
> Worktree: `C:\src\quantick-worktrees\refactor-control-host-headless` (Git Bash `/c/src/quantick-worktrees/refactor-control-host-headless`), branch `refactor/control-host-headless` at campaign tip `317772a5`; `mission-base` and `mission-tier` (high) are already recorded — do not rewrite them. Start every command with `cd /c/src/quantick-worktrees/refactor-control-host-headless &&`, and first `cargo build -p quantick-guards`. Never write to `C:\src\quantick` or another worktree.
>
> The issue allows splitting into several PRs by owner. Do that: this mission delivers the **first slice** (registry/contract validation and whatever is needed for it to compile headless), as one PR, and records in the PR body and on #442 (one comment) the remaining slices (idempotency/journal/recovery, then the server loop) with their file lists and line counts, so the coordinator can dispatch them next. If after measuring the first slice is small, you may include the idempotency/journal/recovery slice too, but keep the PR reviewable (aim under ~3,000 moved lines).
>
> Parallel work: Q16 (#441, moving the paper account out of `crates/app` into a headless crate) and Q18 (#443, a ratchet on `app.lines.without_egui`) run at the same time in other worktrees. Keep edits to `AGENTS.md`'s map, `crates/guards/src/graph.rs`, `HEADLESS_CRATES` and the workspace `Cargo.toml` minimal and line-local. Do not touch `crates/app/src/paper_account*`.
>
> Rules that bind this repo (from CLAUDE.md): dependency direction one-way (never a reverse edge; `control-local` → `control`), headless crates get time told not read (inject the clock for retention/deadlines), English in the repo, behaviour identical — every control-plane, retry-matrix, schema-compatibility and authority test passes unchanged, generated inventories/matrices/schemas unchanged (prove with a byte diff of the generated files). Size ratchet 1,500 production lines, context ratchet. Run the four checks each on its own (`cargo fmt --all -- --check` — use rustfmt directly if the shim is blocked; `cargo clippy --workspace --all-targets`; `cargo build --workspace`; `env -u QUANTICK_BUBBLES cargo test --workspace`) and `cargo test -p quantick-guards`. Never truncate test output with head. A3 (incremental rebuild before/after): other agents are building, so state the load you observed, repeat three times, report the median.
>
> Delivery: draft PR `cd <wt> && gh pr create --draft --base campaign/lean-a-plus ...` with a heredoc body (tier high, "Campaign task Q17 of #367, slice 1", `Refs #442`), ending with `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`. Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`. Reviews as the tier requires: arch-review (step 0 code-review), ai-review with resolved threads (two repair rounds budget), delivery-review last from fresh context; markers via the shared key (`sh .claude/hooks/campaign_context.sh key <wt>`), reports through `review_report.sh`. Then CI green, `gh pr ready <n>` once, `sh .claude/hooks/mission_ship_gate.sh ship <n>`. **Never merge; never use the PowerShell tool for `gh pr` commands; never touch main.** Use `python`, not `python3`.
>
> Return a HANDOFF BLOCK: PR URL; head SHA; what moved and where; each acceptance criterion with evidence (full or partial for this slice); A3 numbers; remaining slices; review verdicts and URLs; markers and key; CI; ship-gate output; files likely to conflict with Q16/Q18.

The coordinator's task is issue #442, quoted from `gh issue view 442`:

> ## Scope
>
> - Move the UI-free host code into a headless crate (extend `crates/control`, or a new `control-host` crate beside it; record the choice against `AGENTS.md`'s map and `crates/guards/src/graph.rs`), with time injected (the idempotency retention, deadlines) instead of read, so it satisfies the headless rules; `app` keeps the egui-bound parts (panel, frame service, screenshot capture, scene/layout projection) behind a narrow port.
> - Behaviour identical: every control-plane, retry-matrix, schema-compatibility and authority test passes unchanged; generated inventories and matrices unchanged.
> - Split into several PRs by owner if one is too large (registry/contract first, then idempotency/journal/recovery, then the server loop).
>
> ## Acceptance criteria
>
> - [ ] A1: the moved crate is in `HEADLESS_CRATES`; `cargo test -p quantick-guards` green; the dependency graph gains no reverse edge.
> - [ ] A2: all control-plane tests pass unchanged; the retry matrix, UI behaviour matrix, capability inventory and published schemas unchanged.
> - [ ] A3: incremental rebuild time for a one-line change in a moved module measured before/after, reported.
> - [ ] A4: merged into `campaign/lean-a-plus`, after the first consolidated PR is cut.
