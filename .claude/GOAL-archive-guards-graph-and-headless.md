# Mission: the crate graph and the headless rule become one-second guards

Two `CLAUDE.md` architecture invariants are enforced today by review or by a
test that costs a `pine` build. The one-way crate graph lives in
`crates/pine/tests/workspace_deps.rs`, so it runs only when a session builds
`pine` and its tests; the headless rule — "everything below `app` is headless
— no UI, no network, no async, no wall clock" — has no mechanical check at
all. Both become checks in `quantick-guards`, which has no dependencies and
answers in about a second, so an agent that breaks either learns it at edit
time instead of at review time.

**Tier:** `medium`. Two new guard modules with their own tests, a new signed
allowlist data file, a `--report` extension and a documentation edit — more
than a `small` diff, and `delivery-review`'s completeness pass is worth
paying for. It is not `high`: nothing here is a trader-facing surface, nothing
places or cancels an order, and the blast radius is one leaf crate plus the
deletion of one test file.

## The request ledger

- **R1** — Move the crate-graph enforcement into `quantick-guards` as
  `crates/guards/src/graph.rs`, carrying every one of the six checks
  `crates/pine/tests/workspace_deps.rs` performs.
- **R2** — Delete `crates/pine/tests/workspace_deps.rs`; the graph rule is
  *moved*, "not duplicated".
- **R3** — The `ALLOWED` table is the single source of the graph. `AGENTS.md`'s
  map stays prose; the guard's table is the truth `arch-review` cites.
- **R4** — Add `crates/guards/src/headless.rs`, scanning production source
  (`size::production_source`) of the crates `CLAUDE.md` names as headless for
  identifier-level hits of `tokio`, `async fn`, `std::thread::spawn`,
  `SystemTime::now`, `Instant::now`, `egui`, `eframe`, `HashMap`, `HashSet`.
- **R5** — Matching is at identifier level, not substring: `pine`'s
  "timeframe" strings must not be hits.
- **R6** — Every hit is a finding unless an entry in
  `crates/guards/headless-allowlist.txt` names the file and the identifier
  with a signed reason.
- **R7** — The allowlist starts honest: the two scratch-directory helpers
  (`crates/replay/src/scratch.rs`, `crates/control-local/src/scratch.rs`) are
  allowed with the reason that they are test-only helpers where `SystemTime`
  seeds a unique directory name and never a result.
- **R8** — Each `Instant::now()` in `orderflow` is read and dispositioned as
  (a) inverted or (b) allowed with a reason; the PR body lists all seven with
  the letter each got. Settled by **D1**.
- **R9** — Each finding names the `CLAUDE.md` line it enforces — `CLAUDE.md:
  Architecture, headless` or `CLAUDE.md: Architecture, dependency direction`
  — so the agent that trips it reads the sentence, not the whole file.
- **R10** — `--report` gains two lines: `headless.findings` and `graph.edges`.
- **R11** — `CLAUDE.md`'s headless bullet names the guard in the same
  sentence; `AGENTS.md`'s verification section loses nothing.
- **R12** — `cargo test -p quantick-guards` covers every check the six `pine`
  tests covered, by a test per rule over fixture manifests.
- **R13** — A reverse edge introduced in a scratch copy, and a
  `SystemTime::now()` introduced in `crates/sim/src`, each produce exactly one
  finding naming the file, the identifier and the `CLAUDE.md` rule.
- **R14** — `guards` still builds with an empty `[dependencies]` table and
  still runs in about a second.
- **R15** — `cargo test -p quantick-pine` stays green with one test file
  fewer.
- **R16** — The finding count is zero from the first commit; it is not
  ratcheted.
- **R17** — Out of scope, held to: no new edges and no change to any crate's
  dependencies; the headless scan does not reach `feed`, the `feed-*` venue
  crates, `backtest`, `mcp` or `app`.
- **R18** *(purpose)* — "A red must mean the change is wrong": the two
  invariants an agent is most likely to break by accident become one-second
  checks. This is the ask that judges the others.

## Decisions taken by the trader

- **D1** — The two production `Instant::now()` reads in
  `crates/orderflow/src/engine.rs` are **allowlisted as (b)**, signed with the
  reason that they feed `last_projection_ms` / `last_live_ms`, diagnostic
  counters that never reach what the projection returns. The `orderflow`
  engine is not changed. This followed a correction to the mission brief: of
  its "seven `Instant::now()` reads", five are inside `#[cfg(test)]` modules
  (`engine.rs:1404`, a test-only `project()` shorthand; `engine.rs:2101,2175`;
  `projection.rs:4816,4891`, bench harnesses) and `project_at` already takes
  `now` from its caller, so the inversion the brief expected is already done.
- **D2** — The scan **strips comments** before matching identifiers. Every
  `egui` and `HashMap` hit in the headless crates today is prose explaining
  the very rule the guard enforces; documentation stays free to name what it
  forbids, and the allowlist stays a record of real code.

## Assumptions

- **S1** — `crates/guards/headless-allowlist.txt` reuses the `path value`
  shape `ratchet.rs` already parses (`#` comments, one entry per line), rather
  than inventing a second data format. Conventional in this crate; reversible
  in one edit.
- **S2** — The headless crate list in `headless.rs` is a constant in the
  guard, checked against `CLAUDE.md`'s sentence by a test, in the same spirit
  as `graph.rs`'s `ALLOWED` — a list nobody remembered to extend is unguarded,
  which looks green.
- **S3** — `CLAUDE.md`'s edit is made byte-neutral or absorbed within its
  existing context ceiling by rewording, rather than by raising the ceiling
  and lowering another file's. *Wanted to ask*; if the ceiling cannot absorb
  it, the raise is signed with its reason and paid for in the same change, and
  the PR body says so.
- **S4** — `graph.rs` and `headless.rs` join `GUARDS` in `lib.rs` as two
  entries with `ratchet: None`, since neither has a number to lower.
  **R16** forbids ratcheting the finding count.
- **S5** — `check_file` for both guards answers for a single path so the
  edit-time hook can call them, matching every existing guard.
- **S6** — The `feed-*` rule (`feeds_depend_on_the_domain_crates_only`)
  travels with the graph guard even though the feed crates are exempt from the
  *headless* rule; it is a dependency-direction check, not a headless one.

## Acceptance criteria

- [ ] **A1** — `crates/guards/src/graph.rs` exists and performs all six checks
      the `pine` test performed: feeds depend on domain crates only, domain
      crates never depend upwards, every crate is covered by a rule,
      `CLAUDE.md` lists every crate, third-party versions only in the root
      manifest, every crate inherits the workspace lints. *Evidence:* the
      `cargo test -p quantick-guards` test-name list, beside the six `pine`
      test names taken from `origin/main`.
      → `.claude/evidence/graph-and-headless-tests.log`. *(R1, R3, R12)*
- [ ] **A2** — `crates/pine/tests/workspace_deps.rs` is deleted and
      `cargo test -p quantick-pine` is green. *Evidence:* the deletion in
      `git diff --stat origin/main...HEAD`, and the pine test run's summary
      line. → `.claude/evidence/pine-after.log`. *(R2, R15)*
- [ ] **A3** — `crates/guards/src/headless.rs` scans the `CLAUDE.md` headless
      crates' production source for the nine identifiers, at identifier level
      and with comments stripped, and reaches none of `feed`, `feed-*`,
      `backtest`, `mcp` or `app`. *Evidence:* the guard's own unit tests,
      including one asserting `pine`'s "timeframe" strings are not hits and one
      asserting a commented `egui` is not a hit.
      → `.claude/evidence/graph-and-headless-tests.log`. *(R4, R5, R17, D2)*
- [ ] **A4** — `crates/guards/headless-allowlist.txt` holds one signed entry
      per remaining `(file, identifier)` pair and no others: the two scratch
      helpers and the two `orderflow` diagnostic stopwatches, which share a
      file and an identifier and so share one entry — three lines for four
      sites. Keyed that way rather than by line number, which would go stale
      on every edit above it. An entry naming
      a file or identifier that no longer hits is itself a finding.
      *Evidence:* the file, and a test that a stale entry is reported.
      → the file plus `.claude/evidence/graph-and-headless-tests.log`. *(R6, R7, R8, D1)*
- [ ] **A5** — Every finding from either guard ends with the `CLAUDE.md` line
      it enforces — `CLAUDE.md: Architecture, headless` or `CLAUDE.md:
      Architecture, dependency direction`. *Evidence:* a test asserting the
      suffix on a finding from each guard. → `.claude/evidence/graph-and-headless-tests.log`.
      *(R9)*
- [ ] **A6** — Introducing a reverse edge (`engine` depending on `pine`) and a
      `SystemTime::now()` in `crates/sim/src` each produce exactly one finding
      naming the file, the identifier and the rule, then are reverted.
      *Evidence:* both command outputs verbatim, plus the `git status` proving
      the revert. → `.claude/evidence/injected-breakage.log`. *(R13)*
- [ ] **A7** — `cargo run -p quantick-guards -- --report` prints
      `headless.findings` and `graph.edges`. *Evidence:* the report output.
      → `.claude/evidence/report.log`. *(R10)*
- [ ] **A8** — `CLAUDE.md`'s headless bullet names the guard in the same
      sentence and `AGENTS.md`'s verification section loses nothing.
      *Evidence:* the diff of both files. → the PR body. *(R11)*
- [ ] **A9** — The whole repository is clean under both new guards from the
      first commit that adds them: `cargo run -p quantick-guards` exits 0, with
      no ratchet and no recorded finding count. *Evidence:* the exit code and
      the absence of any baseline file for either guard.
      → `.claude/evidence/report.log`. *(R16, R18)*
- [ ] **A10** — No crate's dependency set changed. *Evidence:* the manifest
      half of `git diff origin/main...HEAD`, showing no dependency edit.
      → the PR body. *(R17)*
- [ ] **G1** — `crates/guards/Cargo.toml`'s `[dependencies]` table is still
      empty and `cargo test -p quantick-guards` still runs in about a second.
      *Evidence:* the manifest and the timed run.
      → `.claude/evidence/guards-timing.log`. *(R14)*
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`, each run on its own.
      *Evidence:* the four exit codes. → `.claude/evidence/four-checks.log`.
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate. *Evidence:* the classification, in the PR body.
- [ ] **G4** — Every artifact in English, per `CLAUDE.md`'s language rule.
      *Evidence:* `arch-review` dimension 8 and `cargo test -p quantick-guards`
      (the `language` guard). → the review verdict.
- [ ] **G5** — `arch-review` run over the final branch with every Blocker and
      Should-fix resolved or deferred in the PR body. *Evidence:* the verdict.
      → the PR body.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the tier beside the four verification boxes.

## Not applicable, and why

- **Hot path** — the guards run as a build-time command, never per trade, per
  depth event or per frame. No `APP_HEALTH_SUMMARY` run is owed. `G3` still
  declares the classification.
- **User-visible surface** — nothing renders. No `ui-harness` hook,
  `visual-qa` pass or `trader-ux-review` is owed.
- **New capability (`new-extension`)** — a guard module is not a capability
  that docks against a port; `GUARDS` in `lib.rs` *is* the registry, and the
  two new modules dock as a file plus one registration line each, which is the
  recipe's outcome already.
- **Engine / determinism** — no engine code changes (**D1**). The guards are
  still written test-first, being pure functions over fixture text.
- **Docs-only waiver** — not claimed: this change ships Rust and a data file,
  so `arch-review`'s full shape pass applies.

## The request as received

Quoted verbatim, unedited and untranslated, because the ledger above must be
auditable against the exact words that produced it — the marked, attributed
quotation `CLAUDE.md`'s language rule exempts.

> medium fix/guards-graph-and-headless — two CLAUDE.md invariants are still
> enforced by review or by a test that costs a pine build: the one-way crate
> graph lives in crates/pine/tests/workspace_deps.rs, and "everything below app
> is headless — no UI, no network, no async, no wall clock" has no check at
> all, which is how the orderflow extraction carried seven Instant::now() reads
> into a crate the rule names. Move the graph test into quantick-guards as
> graph.rs, add headless.rs that scans the headless crates for async runtimes,
> clock reads, UI identifiers and HashMap in production source against a signed
> allowlist, and make each finding name the CLAUDE.md line it enforces. Read
> C:\src\mission-guards-graph-and-headless.md in full before anything else and
> build the request ledger from it.

The brief the invocation points at, `C:\src\mission-guards-graph-and-headless.md`,
is the ledger's other source; it is not committed here because it lives outside
the repository. Its scope list, acceptance criteria and out-of-scope list are
carried into `R1`–`R18` above without loss, and the two claims of its evidence
table that measurement contradicted are recorded in **D1**.
