# Mission — split the MT5 stream and the Python bridge under 1,500 lines

Split `crates/feed-mt5/src/stream.rs` (2,200 production lines) into owned
sibling modules, each at most 1,500 production lines, and split
`bridge/mt5/quantick_bridge.py` (1,981 lines) so that no Python file under
`bridge/mt5/` exceeds 1,500 lines, with no behaviour and no wire change.

Why it matters: the campaign's rule is one owner per file, so an agent that
opens a file to change one behaviour reads one owner rather than the whole
venue adapter. Both files are the two largest owners left in the MetaTrader
path, and one of them carries the last signed ratchet entry in `feed-mt5`.

**Tier:** `medium` — campaign child S7 of #367. The diff is large but every
move is mechanical and pinned by wire fixtures and the bridge suites; no
product decision is in play, so the full interrogation round is not needed,
but the change is far too big for `small`.

## Request ledger

- **R1** — `crates/feed-mt5/src/stream.rs` and every new sibling under
  `crates/feed-mt5/src/stream/` is at most 1,500 production lines as
  `crates/guards/src/size.rs` measures them.
- **R2** — no Python file under `bridge/mt5/` exceeds 1,500 total lines on
  disk (tests excluded from the count, not from the suite).
- **R3** — every production move is a pure move: bodies move unchanged, the
  only Rust widenings are `pub(super)` marks, and the Python functions and
  classes move unchanged.
- **R4** — no behaviour, contract or wire change. The feed's wire fixtures
  (`crates/feed-mt5/tests/fixtures/*.ndjson`) and their mapping tests are
  untouched, and no test expectation is edited.
- **R5** — the entry point file name and its CLI stay as they are, so the
  absolute `bridge_command` in `crates/app/config/feeds.toml` keeps working.
- **R6** — `crates/guards/size-baseline.txt` no longer carries an entry for
  `crates/feed-mt5/src/stream.rs`, and `!budget` falls by exactly that entry's
  ceiling (2,200) rather than rising.
- **R7** — the seams follow responsibility: for `stream.rs` the session loop
  versus the block state machines versus event mapping, with the opening-block
  arm intact and in order; for the bridge, rates paging, ticks, session export
  and transport/protocol.
- **R8** (purpose) — so that the MetaTrader path is readable one owner at a
  time, with no signed exception left behind to explain it away.
- **R9** — the file-ownership constraint: this child writes only in its own
  worktree, and only to `crates/feed-mt5/src/stream.rs`, new files under
  `crates/feed-mt5/src/stream/`, `bridge/mt5/quantick_bridge.py`, new files
  under `bridge/mt5/`, `bridge/mt5/tests/*` where an import path must follow
  the split, its own entry in `crates/guards/size-baseline.txt`, and `mod`
  lines where strictly needed. Every file outside that set is a departure that
  has to be named and justified, never taken quietly.

## Decisions

- **D1** — pure moves only. Rust bodies move unchanged; the only widenings are
  `pub(super)`. Python functions and classes move unchanged; the entry point
  keeps its name and CLI and re-exports what the outside reads off it. A move
  needing `pub` beyond `pub(super)`/`pub(crate)`, or a wire change, stops and
  is reported instead of guessed.
- **D2** — the Rust ceiling is production lines as `crates/guards/src/size.rs`
  measures them; the Python ceiling is total lines on disk per file under
  `bridge/mt5/`, tests excluded. Moving inline Rust tests to sidecars is
  allowed, never required.
- **D3** — seams as stated in R7; split further if any file still exceeds the
  ceiling.
- **D4** — proof of purity is a statement-line multiset over old versus new,
  with the command and its output in the PR body, plus a `def`/`class`
  inventory for Python; proof of behaviour is the unchanged suites, named with
  their counts.
- **D5** — performance: moving items between modules adds no work; every
  touched path is classified by rate in the PR body. No measurement required
  because no call shape changed.
- **D6** — after the moves, the `stream.rs` entry leaves the baseline and
  `!budget` falls by exactly 2,200.
- **D7** — tier `medium`: `arch-review` with `code-review` at `low`, full shape
  pass over Rust and Python, `ai-review` completion, `delivery-review`
  completeness pass inline. At most two step-0 rounds.
- **D8** — `gh pr ready` runs once, after reviews, markers and green CI, from
  the Bash tool with the single `cd <worktree> &&` prefix. Never merge, never
  touch main.

## Assumptions

- **S1** — the test-harness edits the split forces are in scope. The bridge
  suites rebind module globals (`time`, `log`, `TICKS_PER_PUMP_ROUND`) on the
  bridge module; once the session's behaviour lives in sibling modules that
  bind those names into their own globals, a single-module rebind reaches only
  one of them. `harness.patch_bridge` fans the rebind out across every loaded
  bridge module and `load_bridge` now clears every one of them between tests.
  Safe to assume rather than ask: the mission grants `bridge/mt5/tests/*`
  edits where an import path must follow the split, and no test expectation
  changes — only where the fake is installed.
- **S2** — three cross-surface agreement tests read a constant out of source
  by path: `crates/guards/tests/session_gap_agreement.rs` (four read sites)
  and two `include_str!` sites in `crates/feed-mt5/src/protocol.rs`. Those
  paths follow their constants to `crates/feed-mt5/src/stream/connection.rs`
  and `bridge/mt5/quantick_bridge_core.py`. Both tests failed loudly first,
  and the guard's own panic message asks for exactly this ("if it was renamed,
  rename it here too"), so following it is the guard working, not the guard
  being weakened. The agreements themselves — the numbers must be equal — are
  unchanged.
- **S5** — four files fall outside R9's set and are named here rather than
  taken quietly. `crates/guards/tests/session_gap_agreement.rs` and
  `crates/feed-mt5/src/protocol.rs` are S2's paths-follow-their-constants case,
  and both **failed** until they did. `bridge/mt5/README.md` gains one
  paragraph because it is the file that tells a trader how to run the bridge
  by hand, and after the split "run this one script" is no longer the whole
  truth — a README left silent about the sibling modules would be a surface
  the diff made wrong. Safe to assume rather than ask: prose only, inside the
  directory this mission otherwise owns, and no sibling child touches it.
- **S3** — `Session` is assembled from four mixins rather than kept whole,
  because no smaller cut brings the entry point under the ceiling without
  leaving a 1,300-line class in it. Method bodies are identical; only the
  class header names the mixins.
- **S4** — the orphaned `# +2: the QUANTICK_FOOTPRINT_DEBUG declaration`
  comment left above the retired entry in `size-baseline.txt` is pre-existing
  debris from an earlier campaign commit and is left exactly as found rather
  than adopted into this diff.

## Acceptance criteria

- [ ] **A1** — `crates/feed-mt5/src/stream.rs` and every file under
      `crates/feed-mt5/src/stream/` is at most 1,500 production lines, and no
      Python file under `bridge/mt5/` exceeds 1,500 lines.
      *Evidence:* `cargo test -p quantick-guards` green (the size guard has no
      finding) plus a `wc -l` table for the Python files.
      → the PR body's size table. *(R1, R2)*
- [ ] **A2** — every production move is pure.
      *Evidence:* a statement-line multiset over the old file versus the new
      family, whose only differences are `pub(super)` pairs, new module
      documentation and `mod`/`use` lines; and a `def`/`class` inventory that
      matches item for item on the Python side.
      → the PR body's purity proofs. *(R3, R7)*
- [ ] **A3** — no behaviour or wire change.
      *Evidence:* `cargo test -p quantick-feed-mt5` (112 tests) and the three
      bridge suites (56 checks) pass with unchanged expectations; the wire
      fixtures do not appear in `git diff --name-only`.
      → the PR body's local verification. *(R4)*
- [ ] **A4** — the entry point and its CLI are unchanged.
      *Evidence:* `python bridge/mt5/quantick_bridge.py --help` still prints
      the same parser, and `crates/app/config/feeds.toml` is untouched.
      → the PR body's local verification. *(R5)*
- [ ] **A5** — the baseline entry is retired and the budget fell.
      *Evidence:* the `size-baseline.txt` diff: the `stream.rs` line is gone
      and `!budget` moved 20,025 → 17,825, exactly 2,200 down.
      → the PR body's size report. *(R6)*
- [ ] **A6** — no file outside R9's set is edited without being named.
      *Evidence:* `git diff origin/campaign/lean-a-plus...HEAD --name-only`
      against R9's list, with every departure carrying an `S` line that says
      why. → this file's `S2` and `S5`, and the PR body. *(R9)*
- [ ] **G1** — every artifact English; conventional commits.
      *Evidence:* `arch-review` dimension 8 and `cargo test -p quantick-guards`
      (the language guard). → the review verdict.
- [ ] **G2** — the four checks green on the final head, run one at a time,
      plus `ruff check --select F bridge/mt5 tools/mt5`,
      `python bridge/mt5/tests/test_*.py` and
      `python tools/mt5/test_export_session.py`.
      *Evidence:* command output. → the PR body's local verification.
- [ ] **G3** — performance impact declared per touched path.
      *Evidence:* a rate table. → the PR body.
- [ ] **G4** — `arch-review` with every Blocker and Should-fix resolved or
      deferred in the PR body; `ai-review` completion recorded;
      `delivery-review` completeness pass.
      *Evidence:* review verdicts and the recorded markers. → the PR body.
- [ ] **G5** — CI green at the exact PR head.
      *Evidence:* the run URL and conclusion. → the PR body.

## Closing steps

- **C1** — `delivery-review` returns a completeness pass (tier `medium`).
- **C2** — the PR is open against `campaign/lean-a-plus` and marked ready.
- **C3** — the handoff block returns to the coordinator: issue, branch,
  worktree, PR URL, head SHA, base tip, the size table before and after, the
  review verdicts and their URLs, markers, the CI run and its conclusion,
  findings closed and open, repair batches, whether ready was accepted, any
  `human_decision`, and the coordinator's next action. A campaign child ends
  at its coordinator, not at the trader.

## Not applicable

- *Touches a hot path* — no call shape changed, so there is nothing to
  measure; the declaration under G3 is the whole obligation.
- *Touches anything user-visible* — no UI surface, no hook, no screenshot.
- *Adds a capability* — nothing docks; this retires an owner's excess.
- *Adds something a trader does* — no new action, tool, trade or lock.
- *Engine / determinism territory* — `feed-mt5` is a venue adapter, not the
  aggregator; its determinism is already pinned by the wire fixtures, which
  this change does not touch.
- *Docs/skills only* — this is a code change; the full shape pass applies.

## The request as received

> You are executing campaign child mission **S7** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for
> milocaetano/quantick: split `crates/feed-mt5/src/stream.rs` (2,200
> production lines) into owned sibling modules, each at most 1,500 production
> lines, and split the Python bridge `bridge/mt5/quantick_bridge.py` (1,981
> lines) so no Python file under `bridge/mt5/` exceeds 1,500 lines, with no
> behaviour or wire change. You are a subagent: you cannot ask the trader;
> decisions D1..Dn below answer the mission's step-3 questions. A doubt that
> would need a new decision is reported back, never guessed.
>
> [Read first, in this order]
> 1. `C:\src\quantick\CLAUDE.md` (English everywhere; the size ratchet and
>    `--tighten` under *Keeping the trunk small*; the CI runs
>    `ruff check --select F` over `tools/mt5/` and `bridge/mt5/`,
>    `python3 tools/mt5/test_export_session.py` and
>    `python3 bridge/mt5/tests/test_*.py`).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign
>    child at tier `medium`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is
>    answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`,
>    guards armed); still run `cargo check -p quantick-feed-mt5 --all-targets`
>    before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` (base
>    `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`,
>    review key from `sh .claude/hooks/campaign_context.sh key "$WT"`),
>    `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #374 (`gh issue view 374`): scope, A1..A6, gates.
> 5. How siblings did the same kind of cut and proved it: PR #388 body
>    (`gh pr view 388`), the purity proof (statement-line multiset with only
>    `pub(super)` pairs differing) and the size table. The baseline header
>    comments in `crates/guards/size-baseline.txt` explain the `stream.rs`
>    entry (the `Mt5Event::OpeningPage` arm in the block state machine).
>
> [Worktree] `C:\src\quantick-worktrees\refactor-mt5-stream-under-1500`,
> branch `refactor/mt5-stream-under-1500`, cut from
> `origin/campaign/lean-a-plus` at `793d6400`. Siblings running in parallel own
> `crates/app/src/control/*` and the tool chrome; you own only
> `crates/feed-mt5/src/stream.rs`, new files under
> `crates/feed-mt5/src/stream/`, `bridge/mt5/quantick_bridge.py` and new files
> under `bridge/mt5/`, `bridge/mt5/tests/*` only if an import path must follow
> the split, your entry in `crates/guards/size-baseline.txt`, and `mod` lines
> if strictly needed. Config `crates/app/config/feeds.toml` and the absolute
> `bridge_command` the trader uses must keep working unchanged: the entry point
> file name and its CLI stay as they are.
>
> [Decisions] D1 pure moves only … D2 the ceilings … D3 the seams … D4 the
> proofs … D5 performance … D6 the baseline and the budget … D7 tier `medium`
> … D8 `gh pr ready` once, never PowerShell for `gh pr`, never merge, never
> touch main. (Recorded above as D1–D8.)
>
> [Delivery] implement with the verification loop; the four checks one at a
> time before every commit plus the Python checks; archive `GOAL.md` (slug
> `mt5-stream-under-1500`) as the last commit before reviews; push and open a
> draft PR based on `campaign/lean-a-plus`; `/arch-review`, `/ai-review`,
> `/delivery-review`; watch CI; `gh pr ready` once; return a handoff block.
