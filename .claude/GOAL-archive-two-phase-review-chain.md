# Mission — split the review chain into two phases, carried by the PR

Phase one makes the branch work and ends at a draft PR. `ai-review` posts its
shape findings as resolvable GitHub review threads on that PR. Phase two closes
them one at a time, allowed to redesign. Nothing merges with a thread open.

Why it matters: the chain today interleaves "make it work" with "make it
right", so a fix commit may only patch inside a design it is not allowed to
change. PR #306 ran 28 commits — "round 17 of the review chain", "the fifteenth
bug pass" — against a three-round budget nothing enforces, passed all three
reviews green, and still returned 5 of 6 `ai-review` dimensions WEAK. Its four
coordination booleans were authored in rounds 12, 12, 12 and 15: the late rounds
did not fix the design, they hung flags on it, because a round may not redesign.

**Tier:** `high`. Four separate deliverables across four subsystems (a skill's
charter, a hook gate, a Rust CLI mode, the repository's own instructions); it
changes the gates and the reviewer that judge it; and it spends against a
context budget with 753 bytes of headroom. Nothing here is `small` or `medium`.

## Request ledger

- **R1** — `ai-review`'s charter changes from *"never edits, builds or posts"*
  so that, given a PR target, it posts each finding as its own resolvable
  GitHub review thread: one thread per finding, severity first, anchored at
  `file:line`.
- **R2** — With no PR target it keeps today's read-only report.
- **R3** — Termination rule A, binding: round one reviews the whole diff; every
  later run verifies only the OPEN threads, plus a narrow check that the fixes
  introduced no new FAIL. It may not open a new WEAK against code it already
  passed. *"Without this the finding set is unbounded and the loop cannot
  terminate."*
- **R4** — Termination rule B, binding: every thread closes one of two ways —
  the fix, or an acceptance the trader records on the thread. *"A WEAK whose
  breaking variant the reviewer cannot name is not a WEAK, it is a PASS."*
- **R5** — Phase one ends by opening a DRAFT PR, which `pr-gate` must let
  through ungated.
- **R6** — `gh pr ready` and `gh pr merge` are gated on the `arch-review-ok`
  and `delivery-review-ok` markers as today, plus zero open `ai-review` threads.
- **R7** — Both R5 and R6 are covered by cases in `guardrails_test.sh`.
- **R8** — `--blast-radius` in `quantick-guards`: read a unified diff on stdin,
  print production lines added per pre-existing file, descending, plus files
  touched, pre-existing files touched and total insertions, in the report's
  `label<TAB>value` shape.
- **R9** — `--blast-radius` spawns no git, adds no dependency, is report-only
  with no exit code of its own.
- **R10** — `CLAUDE.md`'s 671-byte review-chain bullet is rewritten to the
  two-phase model in at most 400 bytes.
- **R11** — The counted round budget goes; a stall rule replaces it — if the
  open-thread count does not fall between two runs, the branch goes to the
  trader instead of running again.
- **R12** — Net-negative against the context ratchet, or as close as these four
  changes allow; the number is stated in the PR body.
- **R13** — *Statement of purpose:* phase two is explicitly allowed to redesign,
  signatures included, and findings live as durable, addressable, countable PR
  threads rather than in a session's context — so the open-thread count is the
  convergence trend, recorded for free.
- **R14** — The plan states how the changed gates and the changed reviewer are
  tested without arming them against this branch mid-flight.

## Decisions taken by the trader

- **D1** — R1 is proven on a throwaway PR: a real draft PR on the repo from a
  scratch branch carrying a deliberate defect, `ai-review` run against it, the
  posted thread IDs and the second run's silence recorded, then the PR closed
  and the branch deleted. Real GitHub API, real resolvable threads, no lasting
  artifact.
- **D2** — The posting and thread-counting mechanics live in a tracked shell
  script under `.claude/hooks/`, outside the context ratchet's scope but inside
  review. `ai-review/SKILL.md` gains only the charter, the two termination rules
  and the line that invokes the script. The merge gate counts threads with the
  same script, so "an open `ai-review` thread" has exactly one definition.
- **D3** — The merge gate fails open and says why when the thread count cannot
  be taken (no `gh`, offline, API error): the two markers are still required,
  only the count is skipped, and the hook emits context naming the reason. This
  is `guardrails.sh`'s stated design — *"anything this script cannot determine
  exits 0"* — and a silent fail-open would be the false all-clear this
  repository files findings against. Realised as a new `ask` decision rather
  than `additionalContext`: a `PreToolUse` hook has two speaking channels,
  `deny` and `ask`, and `additionalContext` belongs to `PostToolUse`, which
  runs after the merge. `ask` blocks nothing a human does not block, so the
  decision stands as taken.
- **D4** — This branch ships under today's rules: a normal, non-draft PR gated
  on `arch-review` and `delivery-review`. The new gates and the new reviewer are
  proven by `guardrails_test.sh` and the fixture PR, and are never armed against
  this branch. That is R14's resolution.

## Assumptions

- **S1** — A "resolvable GitHub review thread" is a PR review comment created
  against a file and line (`POST /repos/{o}/{r}/pulls/{n}/comments`), which
  GitHub renders as a thread with a Resolve button; the open/resolved state is
  read back over GraphQL `reviewThreads.isResolved`. This is the only GitHub
  primitive that is both anchored at `file:line` and resolvable, so R1 and R6
  name it. Conventional mechanics, not a design choice worth a question.
- **S2** — An `ai-review` thread is identified by a fixed marker at the head of
  its first comment body. The reviewer writes it, the gate counts it, and no
  other tool writes it. Required by R6 — a gate that counted *every* unresolved
  thread would let a human's question block a merge.
- **S3** — R14's first half is already structural: `.claude/settings.json` runs
  the hooks from `${CLAUDE_PROJECT_DIR}`, the main checkout, so this worktree's
  edited `guardrails.sh` is inert against this session. `guardrails_test.sh`
  invokes the branch's own script directly, which is how the new cases are
  proven. Verified before the interrogation rather than assumed blindly.
- **S4** — Branch prefix `feat/`, slug `two-phase-review-chain`; the new script
  is `.claude/hooks/ai_review_threads.sh`; `--blast-radius` is a new mode in
  `crates/guards/src/main.rs`'s alternatives grammar with its implementation in
  its own module. Repository conventions, no question earned.
- **S5** — *Wanted to ask, decided instead:* whether `gh pr merge` gating is
  worth it when the trader usually merges in the browser, where no hook runs.
  Kept, because R6 names both commands explicitly and a CLI merge is the one
  path a gate can hold.
- **S6** — "Production lines added" in R8 uses the size guard's existing
  definition via `size::production_source`, not a second one. A second
  definition of production source is the duplicated-constant defect this
  repository files against its own code.

## Acceptance criteria

- [ ] **A1** — `ai-review` run against an open PR posts one resolvable thread
      per finding, severity first, anchored at `file:line`; a second run opens
      no new thread on unchanged code.
      *Evidence:* the fixture PR's thread IDs from run one and the empty
      delta from run two, both captured as command output.
      → PR body, *Fixture PR* section. *(R1, R3, D1)*
- [ ] **A2** — With no PR target, `ai-review` produces today's read-only report
      and posts nothing.
      *Evidence:* the SKILL.md clause, plus a no-target run showing the report
      and no `gh api` call. → PR body. *(R2)*
- [ ] **A3** — Both termination rules are stated in `ai-review/SKILL.md` as
      binding, including the WEAK-without-a-named-variant-is-a-PASS rule.
      *Evidence:* quoted section of the file. → PR body. *(R3, R4)*
- [ ] **A4** — A draft PR opens with no marker recorded.
      *Evidence:* a passing case in `guardrails_test.sh`. → the test file.
      *(R5, R7)*
- [ ] **A5** — `gh pr ready` and `gh pr merge` are denied while an `ai-review`
      thread is open, and allowed with both markers and zero open threads.
      *Evidence:* passing cases in `guardrails_test.sh`, including the
      fail-open-with-context case from D3. → the test file. *(R6, R7, D3)*
- [ ] **A6** — `git diff origin/main...HEAD | cargo run -q -p quantick-guards
      -- --blast-radius` over PR #306's diff reports `+216` production lines
      for `crates/app/src/state.rs`, plus files touched, pre-existing files
      touched and total insertions, in `label<TAB>value` shape.
      *Evidence:* the command's output over #306's diff. → PR body. *(R8)*
- [ ] **A7** — `--blast-radius` spawns no process, adds no dependency and exits
      0 regardless of what it measures.
      *Evidence:* `crates/guards/Cargo.toml` dependencies still empty, no
      `std::process` in the new module, and a test asserting the mode's exit
      code. → the guards crate. *(R9)*
- [ ] **A8** — `--blast-radius` is deterministic and test-first: fixture diff
      plus expected table committed before the implementation.
      *Evidence:* `git log` showing the test commit precedes the code commit.
      → PR body. *(R8, R9)*
- [ ] **A9** — `CLAUDE.md`'s review-chain bullet is the two-phase model in at
      most 400 bytes, with the counted round budget gone and the stall rule in
      its place.
      *Evidence:* the byte count of the replacement bullet, and the quoted
      text. → PR body. *(R10, R11)*
- [ ] **A10** — `cargo run -p quantick-guards -- --report` shows
      `ratchet.context.measured` below the same figure on `origin/main`, and
      the PR body states the delta. The request named 237,171; that figure was
      taken on a checkout three commits behind, and the same command on the
      `origin/main` this branch is cut from reads 237,465. The criterion is
      net-negative against the base actually shipped onto, which is what R12
      asks for; both numbers are stated in the PR body so the discrepancy is
      the trader's to judge, not this branch's to bury.
      *Evidence:* the report line on the branch and on `origin/main`.
      → PR body. *(R12)*
- [ ] **A11** — The PR body states how each half of the hazard was tested
      without arming it against this branch: the hooks run from the main
      checkout, and the fixture PR carries the reviewer change.
      *Evidence:* the PR body section. → PR body. *(R14, D4)*
- [ ] **A12** — No instruction text is moved into an untracked directory, no
      tenth `arch-review` dimension is added, and the six questions do not
      enter `CLAUDE.md`, `/mission` or the `/goal` condition.
      *Evidence:* the diff. → PR body. *(constraints)*

### Injected gates

- [ ] **G1** — Every artifact in English, per `CLAUDE.md`'s language rule.
      *Evidence:* `arch-review` dimension 8 and `crates/guards/src/language.rs`
      clean. → the review verdict.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace` — each run on its
      own, never chained behind a `||`.
      *Evidence:* the four exit codes. → PR body.
- [ ] **G3** — `sh .claude/hooks/guardrails_test.sh` passes.
      *Evidence:* its exit code and summary line. → PR body.
- [ ] **G4** — Performance impact declared. Every touched path classified by
      rate: `--blast-radius` is a rare, explicitly invoked CLI mode; the thread
      count is a rare, per-`gh pr ready`/`gh pr merge` network call. No
      per-trade, per-depth or per-frame path is touched.
      *Evidence:* the classification. → PR body.
- [ ] **G5** — `arch-review` run over the final branch with every Blocker and
      Should-fix resolved, or the deferral noted in the PR body.
      *Evidence:* the review verdict and the `arch-review-ok` marker.
      → PR body.
- [ ] **G6** — Blast radius of this change stated in the PR body, measured with
      the mode this change adds.
      *Evidence:* `--blast-radius` over this branch's own diff. → PR body.

### Not applicable, and why

- **Hot path evidence** (`APP_HEALTH_SUMMARY` under a dense tape): nothing here
  runs per trade, per depth update or per frame. G4 declares the rates instead.
- **`ui-harness` / `visual-qa` / `trader-ux-review`**: no UI surface is touched.
  The capabilities this change adds are named calls by construction — a shell
  script and a CLI mode — so *the second operator* is satisfied without a hook.
- **`new-extension` in full**: `--blast-radius` docks against an existing port,
  `main.rs`'s alternatives grammar, as a registration-only edit that preserves
  every existing mode's behaviour. No new registry is carved, and none is owed:
  the port already exists. Its blast radius is G6.
- **Engine / determinism territory**: `guards` is not the engine, but
  `--blast-radius` is held to the same standard — A8 requires the fixture and
  its expected table to be committed before the implementation.
- **Docs/skills waiver of shape dimensions 1–7 and 9**: not claimed. This
  change ships a shell script, a Rust module and tests alongside its prose, so
  it takes the full shape pass.

## Closing steps

- **C1** — `delivery-review` returns PASS over the branch as shipped.
- **C2** — The PR is open, non-draft, with the evidence in its body.

## The request as received

> *Quoted verbatim and in full, as an attributed quotation under `CLAUDE.md`'s
> language exemption.*

> high Split the review chain into two phases and let the PR carry the state between them: phase one makes it work, ai-review posts its shape findings as PR threads, phase two closes them, and nothing merges with a thread open.
>
> Decided by the trader this session — this is the shape, not an option to reopen:
> - The chain today interleaves "make it work" with "make it right", so a fix commit may only patch inside a design it is not allowed to change. Two phases, and phase two is explicitly allowed to redesign, signatures included.
> - Findings live as PR comment threads, not in a session's context: durable across restarts and compaction, addressable, countable, visible to the trader. The open-thread count is the convergence trend, recorded for free.
> - Phase two is worked one thread at a time by a fresh agent, so no fix is written under the weight of the previous rounds' arguments.
>
> Measured this session — do not re-derive:
> - PR #306 (feat/trades-bars-b3, merged) passed code-review, arch-review and delivery-review green; a later /ai-review returned 5 of 6 dimensions WEAK.
> - It ran 28 commits including "fix: round 17 of the review chain" and "the fifteenth bug pass", against the three-round budget CLAUDE.md states and nothing enforces.
> - DealBarBuilder's four coordination booleans were authored in rounds 12, 12, 12 and 15, and bar_opened_at in round 14; the domain concepts came in the first feature commit. Late rounds did not fix the design, they hung flags on it, because a round may not redesign.
> - crates/app/src/state.rs gained +216 production lines carrying one bar kind's series (deal_samples: Vec<DealSample>) inside the ChartState every bar kind shares. The size ratchet never fired: it rations lines per file, so a deposit into a file with headroom is free.
> - Context ratchet: measured 237,171, budget 235,924, BUDGET_HEADROOM 2,000, so it fails at 237,924 — 753 bytes spare. CLAUDE.md's review-chain bullet is exactly 671 bytes. .claude/skills/ai-review/SKILL.md is 2,753 bytes and is already counted.
> - arch-review/SKILL.md is 19,573 bytes, of which dimension 1 (modularity and extensibility) is 750 and dimension 9 (the trunk) is 690 — 7.4% of what the reviewer reads.
>
> Deliver four changes.
>
> 1. ai-review posts. Change its charter — it says "never edits, builds or posts" today — so that, given a PR target, it posts each finding as its own resolvable GitHub review thread, one thread per finding, severity first, anchored at file:line. With no PR target it keeps today's read-only report. Two binding termination rules:
>    - Round one reviews the whole diff. Every later run verifies only the OPEN threads, plus a narrow check that the fixes introduced no new FAIL. It may not open a new WEAK against code it already passed. Without this the finding set is unbounded and the loop cannot terminate.
>    - Every thread closes one of two ways: the fix, or an acceptance the trader records on the thread. A WEAK whose breaking variant the reviewer cannot name is not a WEAK, it is a PASS.
>
> 2. The gate moves to merge. Phase one ends by opening a DRAFT PR, which pr-gate must let through ungated. `gh pr ready` and `gh pr merge` are gated on the arch-review and delivery-review markers as today, plus zero open ai-review threads. Cover both in guardrails_test.sh.
>
> 3. --blast-radius in quantick-guards: read a unified diff on stdin, print production lines added per pre-existing file, descending, plus files touched, pre-existing files touched and total insertions, in the report's label<TAB>value shape. No git spawn, no new dependency, report-only, no exit code. Phase two needs to know where the deposit is, and the reviewer needs a number rather than an impression.
>
> 4. CLAUDE.md's 671-byte review-chain bullet is rewritten to the two-phase model in at most 400 bytes. The counted round budget goes: rounds were pathological because they mixed the phases, and independent threads are not iterations. What replaces it is a stall rule — if the open-thread count does not fall between two runs, the branch goes to the trader instead of running again.
>
> Constraints:
> - Net-negative against the context ratchet, or as close as these four allow; state the number in the PR body. Hooks, guards Rust and .github/ are outside the ratchet and cost nothing.
> - Do not move instruction text into an untracked directory to dodge the ratchet.
> - Do not add a tenth arch-review dimension, and do not put the six questions into CLAUDE.md, /mission or the /goal condition. Evaluated and rejected this session: the author self-grades, and /goal's evaluator reads no files and is skipped at the small tier.
> - Cut from origin/main; the main checkout is three commits behind.
>
> Out of scope, named so they are not silently absorbed:
> - Moving arch-review's and delivery-review's own markers off gh pr create. They keep worft exemption and the merge gate are new.
> - Evacuating arch-review's ~1,200 bytes of marker mechanics into a script, and rebalancing its dimensions so modularity is more than 7.4% of the file. Its own mission.
> - The type-level shape ratchet, rationing fields per struct instead of lines per file. The real fix for the headroom loophole: ~180 lines of Rust, a new baseline, ~40 signed ceilings.
> - A comment-density cap, a struct-field or boolean cap, and a guard for defaulted trait  All three measured and rejected this session.
>
> Acceptance:
> - ai-review against an open PR posts one resolvable thread per finding, and a second run new thread on unchanged code. Proven on a fixture PR.
> - A draft PR opens with no marker; gh pr ready and gh pr merge are denied while an ai-review thread is open. Proven by cases in guardrails_test.sh.
> - git diff origin/main...HEAD | cargo run -q -p quantick-guards -- --blast-radius report+216 on PR #306's diff.
> - cargo run -p quantick-guards -- --report shows ratchet.context.measured at or below today's 237,171.
> - The four verification-loop commands pass, and sh .claude/hooks/guardrails_test.sh passes.
>
> Hazard the plan must resolve: this branch changes the gates that judge it, and item 1 changes the reviewer that will review it. State how each is tested without arming it against this branch mid-flight.
