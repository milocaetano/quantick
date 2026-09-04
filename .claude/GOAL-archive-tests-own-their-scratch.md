# Mission: tests own their scratch

**Objective** — Make every disk-writing test own a unique, self-removing
scratch directory, so a red test means the change is wrong rather than that
Windows reused a process id.

**Why it matters** — `docs/agentic-development.md` §The gates rests on one
property: a red means the change is wrong, so an agent can trust the signal
instead of first proving the repository innocent. Two recorded flakes break it
today, both rooted in a temp path keyed on `process::id()` alone and never
removed: three `paper_trading` tests fail on a reused pid, and CI drops one app
test at random on `main` too. The leak is also large — `%TEMP%` on the host
held 450,227 `quantick-*` entries on 2026-09-04.

**Tier:** `medium`. The work spans roughly forty call sites across five crates,
which is past the `small` ceiling, and the criterion that actually matters —
the suite leaves the temp directory as it found it — needs `delivery-review` to
check it was *proved* rather than asserted. Not `high`: the change is
mechanical once the helper exists, touches no trading logic, no UI and no hot
path.

## Request ledger

| # | Ask |
| --- | --- |
| **R1** | Give each crate that needs it **one** scratch helper yielding a unique-per-run directory that **removes the tree on `Drop`** — verbatim, "a `ScratchDir` that builds `temp_dir()/quantick-<crate>-<pid>-<nanos-since-epoch>-<counter>` and removes the tree on `Drop`". `app` and the `mcp` tests need one; `replay` already has `src/scratch.rs`. |
| **R2** | Add no new dependency unless the mission shows `std` cannot do it; a reach for `tempfile` goes through `[workspace.dependencies]` and runs `cargo deny check bans licenses`. |
| **R3** | Route **every test-side temp path** through the helper — each site in evidence-ledger #4 that is test-side. |
| **R4** | Leave the legitimate production sites of evidence-ledger #5 untouched: the `control-local` instance descriptor and client, the MCP link, the guards' own scratch. |
| **R5** | Prove the suite is clean: count `quantick-*` entries in `temp_dir()` before and after the suite and assert the delta is zero. |
| **R6** | A test that must leave a file for a later assertion keeps its `ScratchDir` alive until then — verbatim, "it keeps the `ScratchDir` alive until then". |
| **R7** | Put **a one-line sweep in the PR body, not in code**, for the operator to clear the 450k leftovers once. The mission deletes nothing on the host. |
| **R8** | Retire the two memory-level workarounds by pointing at the fix: the PR body says `paper-trading-tests-flake-on-pid-reuse` and `gateway-wait-for-change-ci-flake` no longer apply. |
| **R9** | `grep -rn 'process::id()' crates --include=*.rs` returns only production sites, **each named in the PR body with why it is legitimate**. |
| **R10** | The three tests of evidence-ledger #2 pass **ten consecutive runs** of `cargo test -p quantick-app paper_trading` on this host. |
| **R11** | The temp-directory delta check exists, runs in the ordinary verification loop, and is green. |
| **R12** | `cargo test --workspace` twice in a row from one shell leaves `ls $TEMP \| grep -c quantick-` unchanged between runs. |
| **R13** | The four-check loop green, `cargo test -p quantick-guards` green, **no size ceiling raised** — helpers stay small and live in test modules or `tests/`. |
| **R14** | Verify every claim in the brief's evidence ledger before acting, rather than trusting it. |
| **R15** | Respect the parallel branches: nothing in `pane.rs` tests (`refactor/pane-tests-out`), nothing in `paper_trading.rs` beyond `test_scratch_dir` and its callers plus that file's own test module (`fix/generated-truth`), no dependence on `refactor/orderflow-crate`. |
| **R16** | *(purpose, and the ask that judges the others)* A red means the change is wrong: the two recorded flakes stop happening, so an agent can trust the signal instead of proving the repository innocent. |

## Decisions taken by the trader

- **D1** *(answers the contradiction between the brief's scope 2 and its
  criterion 1)* — The three test-only paths that live in **production files
  behind `cfg!(test)`** are **in scope**: `store_home::test_path`
  (`store_home.rs:281`), `paper_home::startup_home` (`paper_home.rs:107`) and
  `paper_state::scratch_path` (`paper_state.rs:163`). They are test-side in
  fact and are the largest leak — one cockpit home per app a test builds — and
  criterion R9 cannot be met without them. Cleanup there needs an owner; a
  test-only `Drop` on `QuantickApp` plus an explicit release in
  `store_home::next_test_home` is the expected shape.
- **D2** *(answers how R5 is proved, since no test inside the suite can observe
  the state **after** the suite)* — Both halves: a **new static guard** in
  `quantick-guards` that fails any raw `temp_dir()` in test-side code, so the
  next regression is caught on every commit and in CI; **and** a measured
  before/after count around `cargo test --workspace`, whose output goes in the
  PR body. The guard covers the future, the measurement proves the present.

## Assumptions

- **S1** — The unique-per-run token is computed **once per process**
  (`pid` + nanos since the epoch, in a `OnceLock`), not once per call. A
  per-call nanos read would make two resolutions of a stable path disagree,
  which is exactly what `store_home::test_path`'s comment forbids. The
  per-call counter still distinguishes sibling directories within a run.
- **S2** — The scratch helper stays `std`-only. `std::fs::remove_dir_all`,
  `SystemTime::UNIX_EPOCH` and `AtomicU64` cover every requirement, so R2's
  escape hatch is not taken and `cargo deny` is not needed. Recorded rather
  than asked because R2 states the default and the code answers it.
- **S3** — The helper is **per crate**, not a shared crate. A new workspace
  crate for eight lines of test support would add a dependency edge from every
  crate that tests to a new leaf, which `CLAUDE.md`'s dependency rule and the
  context budget both argue against. `replay`'s existing `src/scratch.rs` is
  the precedent the mission follows.
- **S4** *(wanted to ask; the reading taken)* — The helper **never deletes a
  directory it did not create**. A sweep of stale `quantick-*` entries at test
  start would drain the host's 450k backlog automatically, but R7 says the
  operator does that once, by hand, from the PR body; a helper that deletes
  other runs' folders could also delete a live parallel run's. Unasked because
  the brief's "the mission does not delete anything on the host" answers it,
  and reversing it is a one-line change.
- **S5** *(wanted to ask; the reading taken)* — A directory that a test
  deliberately hands to a **spawned child process** or to the gateway keeps its
  guard alive for the whole test body (R6), rather than being detached. If a
  specific test turns out to need the directory to outlive the test, that test
  is listed in the PR body as an exception with its reason.
- **S7** *(supersedes D1's parenthetical about the cleanup owner)* — The
  owner for the `cfg!(test)` paths is a **thread-local guard**, not a
  test-only `Drop` on `QuantickApp`. libtest runs each test on a thread it
  spawned, and a spawned thread runs its thread-local destructors on exit, so
  the thread is an owner that already exists — where a `Drop` on `QuantickApp`
  would have meant touching every test that moves out of an app, for the same
  effect. The trader's decision was that the three paths are in scope; the
  mechanism was mine. It leaks in `--test-threads=1`, where libtest uses the
  main thread and the process exits without running its destructors: a bounded
  handful of directories, in a mode neither CI nor an agent runs.
- **S8** — The guard's rule is **"only a crate's own scratch module may ask
  for the temporary directory"**, not "no `temp_dir()` inside `#[cfg(test)]`".
  Deciding whether a line is test-side needs a Rust parser, and the guard crate
  has no dependencies at all; the module-allowlist rule needs none, is
  reviewable at a glance, and catches strictly more — including
  `store_home::test_path` and its siblings, which are test-only paths in
  production files that no `#[cfg(test)]` rule would have seen.
- **S9** — **Five scratch modules, one per crate that needs one**, rather than
  a shared crate. A shared one would be a new workspace crate that every crate
  with tests depends on, which `CLAUDE.md`'s dependency rule argues against and
  R2's "no new dependency" forbids outright; `guards` may depend on nothing at
  all, and an `mcp` integration test links the crate as a dependency and cannot
  see its `#[cfg(test)]` items, so `mcp` needs two. The guard names all five,
  which is what keeps them from drifting.
- **S10** — Two directories are deliberately **not created**: the gateway's
  instances directory and `control-local`'s discovery directory. Discovery
  refuses a descriptor folder whose ACL is not already private, and a folder
  `ScratchDir` creates carries the default one. Found by `control-local`
  failing outright, and it explains a gateway test that was dropping at random.
- **S6** — "Removes the tree on `Drop`" is best-effort: `Drop` ignores I/O
  errors, exactly as `replay`'s `Scratch` does today. A test binary killed
  mid-run still leaks, and no `std` mechanism prevents that.

## Acceptance criteria

- [ ] **A1** — `crates/app` has exactly one scratch helper, a `ScratchDir`
      building `temp_dir()/quantick-app-<pid>-<nanos>-<counter>` and removing
      the tree on `Drop`; `crates/mcp`'s integration tests have one; `replay`'s
      existing `Scratch` is aligned to the same unique-per-run token.
      *Evidence:* the helper source, plus a unit test asserting two `ScratchDir`
      values differ and that the path is gone after the value drops.
      → the new `scratch` modules and their tests. *(R1)*
- [ ] **A2** — No new workspace dependency; `Cargo.toml`'s
      `[workspace.dependencies]` is unchanged.
      *Evidence:* `git diff origin/main -- Cargo.toml Cargo.lock` empty.
      → PR body. *(R2)*
- [ ] **A3** — Every test-side temp path resolves through a helper: no raw
      `std::env::temp_dir()` remains in test-side code across the workspace.
      *Evidence:* the new guard's output, green.
      → PR body. *(R3, R5)*
- [ ] **A4** — The production sites of evidence-ledger #5 are untouched, and
      `grep -rn 'process::id()' crates --include=*.rs` returns only production
      sites, each named in the PR body with why it is legitimate.
      *Evidence:* the grep output and the diff over those files.
      → PR body. *(R4, R9)*
- [ ] **A5** — A new `quantick-guards` check fails any raw `temp_dir()` in
      test-side code, runs in `cargo test -p quantick-guards`, and is green on
      this branch.
      *Evidence:* the guard's name, its own unit tests, and the run output.
      → PR body. *(R5, R11, D2)*
- [ ] **A6** — The three tests of evidence-ledger #2 —
      `an_in_session_rerun_opens_its_own_file`,
      `the_ledger_never_lists_this_sessions_trades_twice_after_a_retarget`,
      `a_close_refreshes_an_open_report_by_itself` — pass ten consecutive runs
      of `cargo test -p quantick-app paper_trading`.
      *Evidence:* the loop command and its ten results.
      → PR body. *(R10, R16)*
- [ ] **A7** — `cargo test --workspace` run twice from one shell leaves the
      `quantick-*` count in `temp_dir()` unchanged between the two runs, and
      the delta over the whole exercise is zero.
      *Evidence:* the three counts (before, between, after).
      → PR body. *(R5, R12, R16)*
- [ ] **A8** — A test that must outlive its writer keeps its `ScratchDir`
      alive; any test that genuinely cannot is listed as an exception with its
      reason.
      *Evidence:* the exception list, or the statement that there is none.
      → PR body. *(R6)*
- [ ] **A9** — The PR body carries the operator's one-line sweep for the 450k
      leftovers, and states that
      `paper-trading-tests-flake-on-pid-reuse` and
      `gateway-wait-for-change-ci-flake` no longer apply.
      *Evidence:* the PR body itself.
      → PR body. *(R7, R8)*
- [ ] **A10** — Every claim in the brief's evidence ledger was re-checked
      against `origin/main` before acting, with the corrections stated.
      *Evidence:* the verification table.
      → PR body. *(R14)*
- [ ] **A11** — The diff touches no `pane.rs` test, and nothing in
      `paper_trading.rs` beyond `test_scratch_dir`, its callers and that file's
      own test module.
      *Evidence:* `git diff --stat origin/main...HEAD` and the per-file diff.
      → PR body. *(R15)*
- [ ] **G1** — Every artifact in English. *Evidence:* `arch-review` dimension 8
      and `cargo test -p quantick-guards` (`language.rs`) green. → PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`; each run on its own,
      never chained behind `||`. *Evidence:* the four outputs. → PR body.
- [ ] **G3** — Performance impact declared. Every touched path is **test-side
      or `cfg!(test)`-gated**: rate class *rare*, and none of it exists in a
      release build. *Evidence:* this line, plus the diff showing no
      production code path changed. → PR body.
- [ ] **G4** — `cargo test -p quantick-guards` green, and **no size ceiling
      raised** in `crates/guards/size-baseline.txt`. *Evidence:* the guards
      output and an unchanged baseline. → PR body. *(R13)*
- [ ] **G5** — `arch-review` run over `git diff origin/main...HEAD` with every
      Blocker and Should-fix resolved, or deferred in the PR body with its
      severity. *Evidence:* the verdict. → PR body.

## Not applicable, and why

- **Hot path** — nothing the diff touches runs in a release build; every edited
  path is behind `#[cfg(test)]` or `cfg!(test)`. No `APP_HEALTH_SUMMARY` run
  and no bench: there is no production code to measure. G3 records the
  classification rather than a measurement.
- **User-visible surfaces** — no surface changes, so `ui-harness`, `visual-qa`
  and `trader-ux-review` do not apply. `QuantickApp` gains a `Drop` that exists
  only in test builds.
- **Adds a capability** — `new-extension` does not apply: nothing new docks,
  and the scratch helpers are test support, not a registered capability.
- **Something a trader does** — no new action, tool, trade or lock, so *The
  second operator*'s act/read/discover criteria have nothing to grade.
- **Engine / determinism** — no engine code. The new guard is nonetheless
  written test-first, matching the repository's habit.
- **Docs/skills only** — does not apply; this is a code change and takes the
  full shape pass.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, with the tier named beside the four verification
  boxes.

## The request as received

Quoted verbatim, in the trader's own words, because the ledger above must not
become its own source of truth — an ask dropped while writing the ledger is one
no reviewer could find. This is the single attributed quotation `CLAUDE.md`'s
English rule exempts; every other line of this file is English.

> /mission medium fix/tests-own-their-scratch — every test that writes to disk
> names its folder after the process id and never removes it, so Windows PID
> reuse makes three paper-trading tests fail on stale files and CI drops one app
> test at random. Give each crate one scratch helper that yields a
> unique-per-run directory and removes it on drop, route every test-side temp
> path through it, and prove the suite leaves the temp directory as it found it.
> Read C:\src\mission-tests-own-their-scratch.md in full before anything else
> and build the request ledger from it.

The referenced brief, `C:\src\mission-tests-own-their-scratch.md`, is the
mission's second source; its scope, evidence ledger, acceptance criteria and
out-of-scope list are folded into R1–R16 above.
