---
name: arch-review
description: Architecture-first code review for quantick — checks that a change docks as a module, declares its performance impact, proves itself with tests, and hides nothing behind a magic number. Use when the user types /arch-review, asks for a code review, or asks whether a change is modular, extensible or fast enough.
---

# Architecture-first code review

A new feature should dock like a spacecraft to the ISS: a standard port, no
modification to the station. Review every change against that bar.

This skill reviews *shape* — modularity, performance, extensibility, tests,
naming. Bug hunting belongs to the bundled `code-review` skill, which this one
runs for you first: see step 0.

## Step 0 — the native code review runs first, always

Correctness outranks architecture (priority 0), so the bug pass happens before
any judgement about shape. Run the bundled `code-review` — the skill that takes
a target plus an effort level (`low`…`ultra`). A plugin command of the same
name appears prefixed, `code-review:code-review`; that one is a different thing
that comments on the PR by itself, and it is not what this step calls.

```
Skill(code-review), args: "<target> <effort>"      one string, in that order

  <target>   a PR number, a branch name, or omitted for the working diff.
             Not a revision range — `main...HEAD` is not a target it parses,
             and it re-derives its own scope from the local `main`.
  <effort>   `high` for a branch or PR, `medium` for a working diff.
             Never omit it: with no level the skill reuses whatever was typed
             last, in some other session, and this review has to name the
             level it used.
```

**Fetch before calling it.** It scopes itself against the local `main`; behind
`origin/main`, it reviews other branches' merged work as if this branch wrote
it. The scope section below covers both with one fetch.

**Expect it in the background.** The skill dispatches an agent and returns only
a name — the findings arrive later as a notification. Read for shape meanwhile,
but do not publish: the review closes only with step 0's list in hand. If it
never arrives, the header says so instead of implying a bug pass that never ran.

When the findings land:

- **Sort before promoting.** The skill returns bugs and
  reuse/simplification/efficiency cleanups in one flat list with no severity of
  its own. Wrong *behaviour* — crash, wrong output, broken determinism, race —
  becomes a **Blocker** here, listed before every shape finding, and the branch
  does not pass with one open. A cleanup is not a Blocker: it enters the
  dimension it belongs to (5 or 6) and takes this skill's severity.
- **Confirm before promoting.** `high` and above deliberately include uncertain
  findings. Item 2 of *Verify before reporting* applies to this list too: argue
  the opposite case and drop what the refutation kills. *Confirmed* means it
  survived that pass, not that the sub-agent sounded certain.
- **Cite, never restate.** A finding step 0 already reported ships as its
  `file:line` plus the severity assigned here — not re-described in new words
  as though this review found it.
- **Step 0 never publishes.** No `--fix`, no `--comment`, no `--post`, and
  never the plugin variant that posts unasked. Findings are resolved
  deliberately, and arch-review is the only thing that reports them.

`code-review` stays callable on its own. On a docs/skills change — where
`mission` waives arch-review — it *is* the review.

## Priority order

When two findings pull in opposite directions, this order decides the call.
State the order explicitly in the review when a trade-off is at stake.

0. **Correctness and determinism** — not a trade-off, a precondition. Same
   trades in, same bars out. See the non-negotiable rules in `CLAUDE.md`.
1. **Performance.** The highest-ranked quality. Never spend runtime cost to
   make code friendlier to read.
2. **Modularity and extensibility.** The next feature must dock without
   surgery on this one.
3. **Tests that prove the behaviour**, so the next feature cannot break it
   silently.
4. **Standardisation.** One way to do a thing, repo-wide.
5. **Human-friendliness.** Mandatory wherever it is free at runtime — better
   names, honest units, a comment explaining a dense algorithm. Never a reason
   to accept a slower path.

Point 5 is not the lowest because it matters least. It is last because it is
the only one that must yield when it collides with the others — and it almost
never has to, since naming and comments compile away.

## Scope the review

```sh
git fetch origin                 # first: a stale local `main` inflates every diff below
git diff main...HEAD --stat      # branch under review
git diff --stat                  # uncommitted working diff
gh pr diff <n>                   # a PR
```

Fetch before scoping, and check that `main` is not behind `origin/main` — a
worktree cut from `origin/main` while the main checkout still sits on an older
`main` makes `main...HEAD` show other branches' merged work as if this branch
wrote it. Fast-forward it (`git -C <main-checkout> pull --ff-only`) or scope
against `origin/main` instead. Both the review and step 0 use the same target.

Read the neighbouring code before judging any of it. The repo's existing
pattern is the standard; a change that invents a second way to do something
already solved is a finding, even when the new way is prettier in isolation.

## The six dimensions

### 1. The docking test — modularity and extensibility

The question to answer for every new capability:

> Could a second implementation of this be added by writing a new file and one
> registration line, without editing any existing behaviour?

If not, say exactly which file the next author would be forced to open, and
what port would have prevented it.

Look for:

- **Type switches that grow.** `match` over a closed enum, `if is_replay`,
  `if feed == "binance"` — every new variant reopens the same function. The
  repo's own answer is capability-driven behaviour: UI gates on
  `FeedCapabilities`, never on which source is playing. Follow that pattern.
- **Ports vs. concrete types.** Does a consumer depend on a trait it needs, or
  on a concrete producer it happens to have? Downcasting or matching on a
  concrete feed type downstream is a broken port.
- **Dependency direction.** One way: `app` → `pine` → `indicators` →
  `engine`, `app` also on `orderbook` / `replay` / `feed-*`, and `feed-*` →
  `engine` / `orderbook` only. Feed crates never depend on each other. A
  reverse edge is a blocker, no exceptions.
- **Forked logic.** Chart, backtest and bot share one aggregator code path.
  Any per-consumer copy of bar building is a blocker.
- **Blast radius.** Count the existing files the change modifies versus the
  files it adds. A feature that is mostly edits to existing code either found
  a missing abstraction or ignored one — decide which and say so.
- **Additive by default.** Adding a capability must change nothing until it is
  asked for: new options default to today's behaviour, and config presence
  alone never activates anything.

### 2. Performance impact — every change declares one

No change ships without an answer to: **how often does this run?**

Classify each touched path before judging it:

| Path | Rate | Bar |
| --- | --- | --- |
| Aggregator, tick ingest | per trade | zero allocation, no locks |
| Book state, depth projection | per depth update | bounded work, no full rescans |
| Renderer, per-frame view | ~60 Hz | no per-frame recompute of stable data |
| Config load, startup, panel edits | rare | clarity wins freely |

Hot-path findings to hunt:

- Allocation per event or per frame: `to_string`, `format!`, `collect()`,
  `Vec` built and dropped every tick, `clone()` of a container.
- `HashMap` where iteration order can reach the output — a determinism bug and
  a cache-locality loss at once. Prefer `BTreeMap` / `Vec`.
- Work that repeats per frame but only changes per event: recomputed
  projections, re-sorted levels, re-parsed config. Cache and invalidate.
- Locks or channel waits on the render thread; unbounded queues that grow
  under a fast tape.
- Per-element draw calls where the repo already batches into one mesh.
- Growth that is worse than the data: an O(n²) pass over book levels is fine
  on ten levels and fatal on a dense book.

Rules for reporting performance:

- Never assert "this is slow" without the call rate and the concrete cost
  (allocations per call, extra passes, added lock). A guess stated as a
  measurement is itself a finding against the reviewer.
- If the cost is real but the magnitude is unclear, say so and name the
  measurement that would settle it — the perf HUD frame time, a bench over a
  fixture, a dense-book capture.
- Cold-path micro-optimisation that costs clarity is a finding in reverse:
  call it out and ask for the readable version back.

### 3. Nothing hardcoded

Every number that a human might one day want different lives in a named
constant or in config — never inline at the point of use.

- **Named constants** at module top, `SCREAMING_SNAKE_CASE`, unit in the name
  (`_MS`, `_PX`, `_TICKS`, `_BYTES`). `const` and `static` cost nothing.
- **Config** for anything a user tunes: feeds and symbols in
  `crates/app/config/feeds.toml`, bubble looks in `config/bubbles.toml`,
  overridable by env var. Symbols, endpoints, tick sizes and thresholds are
  never literals in code.
- A magic number in a renderer or a threshold buried in a condition is a
  finding every time, including when it is "obviously" 2.0.
- Config round-trips must survive a save: a writer that drops comments or
  re-emits `0.78` as `0.7799999713897705` destroys the reason the file is
  tracked in git. Check the write path, not just the read path.

### 4. Tests that prove the change

- Every new behaviour has a test that **fails without the change**. If you
  cannot name that test, the behaviour is unproven — a blocker.
- Engine work is test-first: fixture trades, expected bars, then code.
  Determinism is guarded by golden/snapshot tests over fixed fixtures.
- **Test the port, not just the feature.** When a change adds an extension
  point, a second implementation — a fake is fine — must exercise it in a
  test. One implementer never proves a trait is a port.
- **Regression cover for what already worked.** The point of the suite is that
  the next feature cannot break this one silently. Ask what existing test
  would have caught this change if it were wrong; if none, that gap is the
  finding.
- Edge cases the domain actually produces: empty book, one-tick spread, zero
  quantity, gap in update ids, overflow on feed arithmetic (saturate, never
  panic), a session that ends mid-bar.

### 5. Standardisation

One way to do a thing. Compare against the existing repo answer for error
types, module layout, config format, naming, logging, and the shape of a
public API. A new local convention needs a stated reason or it is a finding.

### 6. Human-friendly at zero runtime cost

- Names carry intent and unit: `cluster_window_ms`, not `cw`; `visible_p99`,
  not `ref_mode`. Renaming is free — demand it.
- Complex algorithms get a comment in **English** explaining the *objective* —
  why this exists and what it is trying to achieve — not a restatement of the
  code. A dense projection or an adaptive threshold without that comment is a
  finding.
- Inferred or incomplete data is labelled as such in the code and in the UI,
  never silently patched.
- Prefer zero-cost clarity: newtypes over bare `f64`, exhaustive `match` over
  `_ =>` catch-alls, early returns over nesting. All compile away.
- No comment restating obvious code, and no dead or commented-out code.

## Verify before reporting

Reviews are judged on precision, not volume.

0. Confirm step 0 ran and its findings are in hand — if it went to the
   background, wait for the notification. A shape review published without
   them is incomplete, not "clean".
1. Open the file and read the surrounding code — most "this is missing"
   findings die here because the thing exists one function up.
2. For each surviving finding, argue the opposite case for a moment: is this
   already handled, deliberate, or out of scope for this change? Drop it if
   the refutation holds.
3. Confirm the four checks actually pass — do not take a claim on trust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

A clean change gets a short review saying it is clean and why. Never pad.

## Severity

- **Blocker** — a confirmed correctness finding from step 0; reverse dependency
  edge; forked aggregator logic; determinism broken; hot-path regression; new
  behaviour with no test; a feature that activates itself.
- **Should fix** — hardcoded value; extension point that forces edits to
  existing code; missing regression cover; unexplained complex algorithm;
  misleading name or missing unit; a second way to do a solved thing.
- **Consider** — clarity and structure improvements with no correctness,
  performance or extensibility consequence.

## Output

Open with one line for step 0: the effort level it ran at and how many findings
came back, including zero — `step 0: code-review at high, 12 findings, 3
confirmed` — or why it did not run. On the `ReportFindings` path that line is
the text accompanying the call, since the tool carries no header field. It is
the only signal that the bug pass was skipped, so it is never dropped.

Report findings with the `ReportFindings` tool when it is available, ranked
most severe first, using categories `correctness` (step 0's, promoted here),
`modularity`, `performance`, `hardcoded-values`, `test-coverage`,
`standardisation`, `readability`. Without that tool, write the same list as
markdown grouped by severity.

Each finding: `file:line`, what is wrong, why it matters *in this order of
priorities*, and the concrete fix — the trait to extract, the constant to
name, the test to add. Never a vague "consider refactoring".

Close with a verdict in four lines:

- **Correctness** — what the step 0 code review returned, and whether anything
  from it is still open.
- **Docking** — can the next feature attach without opening these files?
- **Performance** — what got faster, slower, or stayed flat, and at what rate.
- **Proof** — which test would fail if this change regressed.
