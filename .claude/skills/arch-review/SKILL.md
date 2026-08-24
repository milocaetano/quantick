---
name: arch-review
description: The full pre-PR review for quantick — runs the bundled code-review for bugs first, then checks that a change docks as a module, declares its performance impact, proves itself with tests, stays drivable by an operator without a mouse, and hides nothing behind a magic number. Use when the user types /arch-review, asks for a code review or a bug pass before shipping, or asks whether a change in hand is modular, extensible, fast enough, or drivable without a mouse. Not for designing the assistant itself — this reviews a diff.
---

# Architecture-first code review

A new feature should dock like a spacecraft to the ISS: a standard port, no
modification to the station. Review every change against that bar.

This skill reviews *shape* — modularity, performance, extensibility, tests,
operability, naming. Bug hunting belongs to the bundled `code-review` skill,
which this one runs for you first: see step 0.

## Step 0 — the native code review runs first, always

Correctness outranks architecture (priority 0), so the bug pass starts before
the shape pass and the review never closes without it. Run the bundled
`code-review` — the skill that takes a target plus an effort level
(`low`…`ultra`). A plugin command of the same name appears prefixed,
`code-review:code-review`; that one posts to the PR by itself and is not what
this step calls.

```
Skill(code-review), args: "<target> <effort>"      one string, in that order

  <target>   a PR number once one exists — the least ambiguous target there
             is — otherwise a branch name, or omitted for the working diff.
             Never a revision range: `main...HEAD` is not a target it parses.
  <effort>   `high` for a branch or PR, `medium` for a working diff.
             Never omit it: with no level the skill reuses whatever was typed
             last, in some other session, and this review has to name the
             level it used.
```

**Check the scope it comes back with.** When the target does not pin a range
the skill derives one, so it can end up reviewing another branch's merged work
(local `main` behind `origin/main`) or nothing at all (a pushed branch whose
upstream already contains every commit). Findings over files this branch never
touched, or a suspiciously empty pass, mean re-invoking with an explicit
target — not a clean bill of health. Fetch first either way; see *Scope the
review*.

**Expect it in the background.** The skill dispatches an agent and returns only
a name; the findings arrive later as a notification. Read for shape meanwhile,
but publish nothing — the review closes only with step 0's list in hand. If the
notification never lands, re-invoke once; if that fails too, do the bug pass
yourself before publishing and say which it was in the header. "It never came
back" is not a reason to ship an unreviewed branch.

When the findings land:

- **Sort before promoting.** The skill returns bugs and cleanups in one flat
  list with no severity of its own. Wrong *behaviour* — crash, wrong output,
  broken determinism, race — becomes a **Blocker** here, listed before every
  shape finding, and the branch does not pass with one open. A cleanup is not
  automatically lower: file it in the dimension it belongs to and let that
  dimension decide, so an efficiency finding on a per-frame path lands in
  dimension 2 and is still a Blocker under the hot-path rule.
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

`code-review` stays callable on its own, but a branch still needs the
`arch-review-ok` marker to open a PR. So on a docs/skills change — where
`mission` waives the shape pass — run this skill anyway and report step 0's
findings through it. The bug pass is not the waived part.

## Priority order

When two findings pull in opposite directions, this order decides the call.
State the order explicitly in the review when a trade-off is at stake.

0. **Correctness, determinism and authority** — not a trade-off, a
   precondition. Same trades in, same bars out; and no operator, human or
   otherwise, reaches a market or safety action by a path the trader does not
   cross. See the non-negotiable rules in `CLAUDE.md`.
1. **Performance.** The highest-ranked quality. Never spend runtime cost to
   make code friendlier to read.
2. **Modularity and extensibility.** The next feature must dock without
   surgery on this one — and the next *operator*, a script or the embedded
   assistant, must be able to drive it without a mouse (*The second
   operator*). Its authority half sits at 0, not here.
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
git fetch origin                       # first: the ranges below read origin/main
git diff origin/main...HEAD --stat     # branch under review
git diff --stat                        # uncommitted working diff
gh pr diff <n>                         # a PR
```

The range names the remote on purpose. `git fetch` moves `origin/main` and
leaves the local `main` ref where it was, so in a worktree cut from
`origin/main` while the main checkout still sits on an older `main`,
`main...HEAD` shows other branches' merged work as if this branch wrote it —
it happened on the branch that added this line, 26 files from someone else's
PR. Fast-forwarding the main checkout (`git -C <main-checkout> pull --ff-only`)
fixes the local ref too, and is worth doing before a review either way.

Read the neighbouring code before judging any of it. The repo's existing
pattern is the standard; a change that invents a second way to do something
already solved is a finding, even when the new way is prettier in isolation.

## The seven dimensions

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
- **Opening state is config, not a literal.** Which layers, panels and
  surfaces a fresh launch draws is a product decision someone may want
  different, so it belongs in the shipped TOML under `crates/app/config/`,
  compiled in with `include_str!` the way `feeds.toml`, `bubbles.toml` and
  `chart-layers.toml` are. A `Default` impl deciding what the first frame
  shows, or a `set_*(false)` at startup, is a finding — it puts a product
  decision where nobody can change it without a build, and it splits the
  answer across a struct and a file the moment a state file exists. The test
  is not "is it a number", it is "would a human ever want this different".
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

### 7. The second operator — could an agent do this without the mouse?

quantick is going to grow an embedded assistant: something the trader talks to
mid-session — *read this chart, build me a strategy from that region, put a
trade here, lock the platform down, I am tilting*. It is not being built by
this PR, and this dimension never asks for assistant code. It asks that the
change not **close the door** on one. A capability that exists only as a
gesture is a capability the assistant can never have, and retrofitting it later
means reopening the very file under review.

The question to answer for every user-facing capability:

> Could an operator that is not holding the mouse — a script, a test, the
> future assistant — trigger this action, read back what it did, and discover
> that it exists, without a human clicking?

Three capabilities, each its own finding when it is missing:

- **Act — the action exists as a named call, not only as a gesture.** State
  mutated inside `if response.clicked() { … }` is not an action, it is a
  click. Lift the body into a named function that takes data —
  `place_drawing(tool, anchors, actor)`, `arm_strategy(preset, region, actor)`
  — and let the click call it. The actor rides in the signature, not beside
  it, because of the authorship rule below: a call that cannot say who asked
  cannot honour it. One path, never two: a separate "for the agent" entry
  point drifts from the one the trader uses and the operator ends up driving a
  ghost platform — the same reason `ui-harness` hooks call the manual toggle's
  own function instead of a parallel activation path.
- **Read — the result is legible as data, not only as pixels.** "Analyse this
  chart" is answerable only if bars, drawings, the position, the levels and the
  indicator readings can be enumerated by something that is not looking at the
  screen. A feature whose outcome lives in a widget's private fields, or exists
  only inside the paint call, is opaque — name the accessor or the snapshot
  type that is missing.
- **Discover — the capability announces itself in a registry.** A stable id in
  the *same* registry that feeds the UI, plus declared parameters wherever the
  capability takes any. Two exemplars, and they prove different halves:
  `DRAWING_TOOLS` (`crates/app/src/drawings/mod.rs`) is the id half —
  `QUANTICK_DRAWING_TOOL=<id>` reaches every tool precisely because one
  registry backs both the rail and the hook — while declaring no parameters at
  all. `IndicatorDescriptor::inputs` / `InputSpec`
  (`crates/indicators/src/input.rs`) is the parameter half: stable name,
  title, default and constraints, with the settings panel *generated* from it.
  Two findings live here — a capability that registers itself nowhere, and a
  list kept by hand beside a registry (one for the buttons, one for the agent),
  which diverges on the next PR.

**What the trader authors is data, not a rebuild.** When the new capability is
something the trader (or the assistant on their behalf) writes and varies — a
strategy, an alert, a checklist, a preset, a layout, an indicator of their own
— it belongs in a script or a config file loaded at runtime, not in an `enum`
that only grows through a build. The repo already decided this: `pine` turns a
`.pine` file into an `Indicator` with no recompilation, strategy presets live
in `quantick-strategies.toml`, looks live in `bubbles.toml`. This is **not** a
case against native code. The kernels in `indicators::native` (`ema.rs`,
`cvd.rs`, `avwap.rs`) are the shipped pattern and stay right: a kernel is
written by us, in Rust, once. The finding is narrower — a capability the
*trader* was meant to vary arriving as a compiled variant — and it names the
script or config file that would have avoided the rebuild.

**Where this layer does not go.** Performance (priority 1) outranks the
operability half of this dimension. It does not outrank the authority half
below, which is priority 0. A command is emitted when a *person acts*: a
click, a hotkey, a panel edit, one instruction to the assistant. That is the
only rate this dimension licenses, and it is explicitly **not** "once per
bar" — quantick's bars close on volume, not on a clock, so a small tick-bar
threshold under a dense tape closes them hundreds of times a second, and
`Indicator::preview` runs on the *forming* bar, so script reached from there
is on the hot path whatever the commit/preview contract says (that contract is
about rolling back staged state, not about rate). Classify with dimension 2's
table and nothing else: an interpreter, a string lookup or a dynamic dispatch
table in the aggregator, in `preview`, or in the renderer is a performance
finding first and an operability merit second.

**Authority is declared, and so is the author — at priority 0.** This
paragraph is safety, not shape: it is a precondition, and no performance win
buys it off. An exposed action states which kind it is: observation, cockpit
change, or market/safety action (send an order, lock the platform). The last
two cross the same arming and confirmation the trader crosses — a surface that
lets a non-human operator reach an order by a shorter path than the trader's
is a Blocker. And whatever acted is recorded: an order, a drawing or a preset
produced by something other than the trader's own hand is labelled as such,
under the same data-honesty rule that labels an inferred side. An object the
assistant placed that is indistinguishable from one the trader placed is a
finding.

This is the runtime twin of the `ui-harness` hook rule, not a duplicate of it:
a hook proves a surface can be *reached from a launch*, this dimension asks
whether the action can be *taken while the session runs*. One extraction
usually satisfies both — the hook and the agent call the same named function.

Naming note: the repo's `copilot.pine` is an indicator, not this assistant. Do
not reuse the name for the agent surface.

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
  behaviour with no test; a feature that activates itself; a market or safety
  action a non-human operator can reach by a shorter path than the trader's,
  or one that leaves no record of who acted.
- **Should fix** — hardcoded value; extension point that forces edits to
  existing code; missing regression cover; unexplained complex algorithm;
  misleading name or missing unit; a second way to do a solved thing; a
  capability reachable only from a click handler; state that exists only as
  pixels; a capability that registers itself nowhere, or a list kept by hand
  beside the registry; something the trader was meant to vary shipped as a
  compiled variant.
- **Consider** — clarity and structure improvements with no correctness,
  performance or extensibility consequence.

## Output

Open with one line for step 0: the effort level it ran at and how many findings
came back, including zero — `step 0: code-review at high, 12 findings, 3
confirmed` — or why it did not run. On the `ReportFindings` path that line is
the text accompanying the call, since the tool carries no header field. It is
the only signal that the bug pass was skipped, so it is never dropped — and it
goes into the PR body too, next to the deferred findings `CLAUDE.md` already
requires there. Chat scrolls away; the PR is where the next reader looks.

Report findings with the `ReportFindings` tool when it is available, ranked
most severe first, using categories `correctness` (step 0's, promoted here),
`modularity`, `performance`, `hardcoded-values`, `test-coverage`,
`standardisation`, `agent-surface`, `readability`. Without that tool, write the
same list as markdown grouped by severity.

Each finding: `file:line`, what is wrong, why it matters *in this order of
priorities*, and the concrete fix — the trait to extract, the constant to
name, the test to add. Never a vague "consider refactoring".

Close with a verdict in five lines:

- **Correctness** — what the step 0 code review returned, and whether anything
  from it is still open.
- **Docking** — can the next feature attach without opening these files?
- **Performance** — what got faster, slower, or stayed flat, and at what rate.
- **Operability** — could a script or the future assistant trigger this, read
  the result and discover it exists? Say "no surface" when the change adds no
  user-facing capability; never drop the line.
- **Proof** — which test would fail if this change regressed.
