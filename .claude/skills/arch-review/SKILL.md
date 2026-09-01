---
name: arch-review
description: The full pre-PR review for quantick — runs the bundled code-review for bugs first, then checks that a change docks as a module, declares its performance impact, proves itself with tests that stay out of the shipped binary, stays drivable by an operator without a mouse, hides nothing behind a magic number, and is written in English throughout. Use when the user types /arch-review, asks for a code review or a bug pass before shipping, or asks whether a change in hand is modular, extensible, fast enough, drivable without a mouse, free of hardcoded values, or correctly separated between test and production code. Not for designing the assistant itself — this reviews a diff.
---

# Architecture-first code review

A new feature should dock like a spacecraft to the ISS: a standard port, no
modification to the station. Review every change against that bar.

This skill reviews *shape* — modularity, performance, extensibility, tests,
operability, naming, hardcoded values and the language the repo is written in.
Bug hunting belongs to the bundled `code-review` skill, which this one runs for
you first: see step 0.

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

**A mission's tier overrides that default**, because the tier is the trader's
own statement of how much this change is worth reviewing: **`low` for `small`
and for `medium`, `medium` for `high`, `high` for `max`** — one notch below
what the tier is named, deliberately. The trader measured three `xhigh` passes
on a single docs branch and called the slowness not worth it, so the ladder was
moved down rather than the gate removed. At `max`, say in the header that
`/code-review ultra` exists — a deep multi-agent cloud pass the trader triggers
themselves, never this skill, and never a level this step selects.

**Never re-run a level that already ran clean.** Two passes of *this step* is
its own budget; a third is for a branch whose second found Blockers.

That budget sits inside a larger one. `CLAUDE.md`'s *review chain has a budget*
bounds the whole chain — both reviews, counted together — and owns the rules
for spending it and for deferring what will not fit. Read it there. The line
above is scoped deliberately to the bug pass and is not the total; a second
statement of a number is a second number to keep true.

**The bug pass is open judgement, so it keeps the strong model.** `code-review`
finds real defects partly by being one, and this is the exception
`CLAUDE.md`'s routing rule exists to protect. Nothing in this step is
downgraded to buy tokens.

Read the tier from **the same file `pr-gate` reads**, never from a second
statement of it. The goal file's `**Tier:**` line is for the reader; this file
is the one the gate acts on, and where the two disagree the gate is what
actually happens:

```sh
WT=/path/to/worktree
cd "$WT" && cat "$(git rev-parse --absolute-git-dir)/mission-tier"
```

`.claude/hooks/README.md` owns that file's format and the rules the hook
applies to it; do not re-derive them here, because a third statement of a
format is a third thing to keep true. All this step needs from it: no file, or
a tier the hook would not honour for this branch, means **no tier** — take the
defaults above rather than guessing at a middle level.

**Name the level in the header either way**, with where it came from, so a
short pass is never mistaken for a thorough one — and say so when this file and
the goal file's `**Tier:**` line disagree. Two surfaces disagreeing about one
branch is a finding in itself, and this review is what sees it.

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

## Record the marker when the review closes

A branch cannot open a PR until this review is recorded against the exact
change being shipped — a hash of the branch's diff, so a rebase or an amend
does not invalidate it but an edit to a tracked file does. Recording it belongs here, in the skill that knows
whether the review actually closed — not to whichever caller happened to
invoke it. Once every Blocker and Should-fix is resolved or deferred in the PR
body, and the branch has no further commits coming:

```sh
WT=/path/to/worktree
cd "$WT" &&
  git diff origin/main...HEAD |
    git hash-object --stdin > "$(git rev-parse --absolute-git-dir)/arch-review-ok"
```

The `cd` matters: both `git` calls resolve against the shell's cwd, which for
an agent session is the main checkout, not the worktree being shipped. Without
it the marker lands in the wrong git dir holding the wrong sha, and the next
`gh pr create` denies with no clue why.

If the branch gains another commit after this — a review fix, an archived goal
file — the marker is stale by design and this review runs again over the new
head before it is re-recorded. Re-stamping a marker whose review did not run
again is the one dishonest move the gate cannot detect.

## What this skill does not review

Whether the change is *what was asked for*. Every dimension below takes the
diff as given and grades how well it is made; none of them opens the request
and checks that all of it arrived. That question belongs to `delivery-review`,
which runs after this skill, over the branch as shipped, and whose marker
`pr-gate` wants alongside this one. A conformance gap noticed here — an
acceptance criterion with no code behind it — is worth a sentence in passing,
but it is graded there, not given a severity here.

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
4. **Standardisation.** One way to do a thing, and one language to say it in,
   repo-wide.
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

### The mission's tier scopes the shape pass

At the `small` tier, read only the dimensions the diff actually reaches —
**dimension 8 always**, since a foreign-language line is exactly what a hurried
change leaves behind, and step 0 always in full. This is a smaller *reading*,
never a lower bar: a dimension that applies still applies, and a finding is
still a Blocker at the severity it earns. If three or more dimensions turn out
to apply, say so in the header — the branch has outgrown its tier, and the
mission's job is to raise it rather than to have it graded cheaply.

Every other tier reads all nine. The tier is a claim about the change's size,
and this is the review that can see whether the claim held.

**Ignore the narrowing when the claim is false.** If the branch is over the
`small` ceiling in `guardrails.sh`, read all nine whatever the tier file says —
otherwise the one unverified word that bought the exemption also cuts the
budget of the pass most likely to notice it was unearned.

## The nine dimensions

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

Every literal that *configures behaviour* — a number, a threshold, a path, an
endpoint — lives in a named constant or in config, never inline at the point of
use. That it is "obviously" 2.0, or used exactly once, is not a defence: the
magic numbers that survive review are the ones nobody looked at twice.

**Scope this before hunting.** The rule is about values that tune what the code
*does*, not about every literal in the diff. Message text is not a
configuration value: `log::info!("…")`, `anyhow!("…")` and assertion strings
stay where they are read. Filing those turns a review into a wall of noise and
costs it the precision *Verify before reporting* demands.

Three tiers. Every finding names which one the value belongs in, because
"extract a constant" is the wrong fix for a value the trader was supposed to
edit:

- **A config file — anything a *user* tunes.** Feeds and symbols in
  `crates/app/config/feeds.toml`, bubble looks in `config/bubbles.toml`,
  footprint styling in `config/footprint.toml`, the layers a fresh chart
  opens with in `config/chart-layers.toml`, strategy presets in
  `quantick-strategies.toml`, each overridable by env var. Symbols, endpoints,
  tick sizes, colours and user-facing thresholds are never literals in code. A
  Rust `const` may hold the *default*, but the knob itself lives in the file: a
  `const` still costs a rebuild, and a rebuild is the one thing the trader
  cannot do. This is dimension 7's *what the trader authors is data* rule seen
  from the constants side.
- **A shared module — a value two or more places must agree on.** A bridge
  port, a protocol magic, a frame bound, a file-format version, a directory
  name written by one crate and scanned by another. Duplicating it at both ends
  is a bug with a delay fuse: it ships green and breaks the day one side
  changes. One owner, imported by the rest — and the finding is the *second*
  copy, wherever it sits. Where the value crosses a language boundary the repo
  cannot type-check (the MQL5 bridge, the Python exporter), it cannot be
  imported, so the finding is instead the missing test or doc comment pinning
  the two sides together.
- **Module top — a value one module owns.** `SCREAMING_SNAKE_CASE`, unit in
  the name (`_MS`, `_PX`, `_TICKS`, `_BYTES`), and a doc comment saying *why
  this number* rather than restating it. `const` and `static` cost nothing at
  runtime, so there is never a performance argument for leaving the literal
  inline. This is the tier the repo already uses well — compare against the
  constant blocks at the top of `crates/app/src/app.rs` before proposing
  anything else.

**Opening state is a config value too**, and its tier is always the first
one. Which layers, panels and surfaces a fresh launch draws is a product
decision someone may want different, so it belongs in the shipped TOML —
`config/chart-layers.toml` is the worked example, compiled in with
`include_str!` the way `feeds.toml` and `bubbles.toml` are. A `Default` impl
deciding what the first frame shows, or a `set_*(false)` at startup, is a
finding: it puts a product decision where nobody can change it without a
build, and it splits the answer across a struct and a file the moment a state
file exists. The test here is not "is it a number" — it is "would a human ever
want this different".

Also:

- A magic number in a renderer, a threshold buried in a condition, a sleep or
  timeout duration, a retry count, a hardcoded `C:\...` or `/tmp` path, a bare
  URL or port — each is a finding every time.
- A capacity or buffer size is a finding when the number *means* something a
  human would tune — a queue bound, a frame limit, a page size. An arbitrary
  `with_capacity` hint that only avoids a realloc is not; say so and move on.
- Exempt, and say so rather than filing them: `0`, `1` and `-1` as identity or
  step; indices into a shape the code itself fixes (`rgba[3]`); message and
  assertion text; and a literal a doc comment right there derives from a named
  constant.
- Config round-trips must survive a save: a writer that drops comments or
  re-emits `0.78` as `0.7799999713897705` destroys the reason the file is
  tracked in git. Check the write path, not just the read path.

### 4. Tests that prove the change — and stay out of the product

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

**Where a test lives.** Rust reaches the same discipline C# gets from a
separate test project, but by a different mechanism, and importing the C#
layout here would itself be the finding — there is no test project, and a
`src/` module full of `pub` helpers written for the suite is the anti-pattern,
not the goal. What the review checks is the mechanism Rust actually uses:

- **Unit tests** live in a `#[cfg(test)] mod tests` child of the module under
  test — inline at the bottom of the file, or, once that module outgrows the
  file it is buried in, in a sibling `tests.rs` pulled in with
  `#[cfg(test)] mod tests;`. The attribute is the whole point: it keeps the
  tests out of `cargo build` and out of the shipped binary, and it is what buys
  access to private items with no `[InternalsVisibleTo]` equivalent needed. A
  test module without `#[cfg(test)]` is a finding on its own.
- **Integration tests** live in `crates/<crate>/tests/*.rs` — a separate crate
  that links the library from outside and therefore can only reach the public
  API, which is precisely what makes it a contract test. That is where this
  repo already proves its contracts: `engine/tests/golden_*.rs`,
  `control/tests/*_contract.rs`, `pine/tests/*_semantics.rs`,
  `indicators/tests/fmath_guard.rs`. An integration test that needs a private
  item is either a unit test filed in the wrong folder, or the signal that the
  port under review was never made public — say which.
- **Test support an integration test needs** cannot be `#[cfg(test)]` — that
  attribute is false when the separate test crate compiles, so the helper would
  vanish exactly where it is wanted. Other Rust projects reach for a
  `test-util` cargo feature; **this repo does not, and proposing one is the
  finding, not the fix** — no crate here has a `[features]` section at all.
  The repo's answer is a *deliberately published* module, documented as part of
  what the crate is: `engine::fixture` and `engine::golden`, and
  `control::fake`, whose fake host/client ports `CLAUDE.md` names in the
  crate's own description. Per-file helpers used by one integration test go in
  `tests/common/mod.rs`, never a top-level `tests/common.rs` — cargo builds
  every top-level file in `tests/` as its own test binary, so the flat version
  compiles as a test target containing no tests.

So the line this dimension draws is not "test code in `src/` is bad". It is
**deliberate and documented, or accidental and leaking**. The findings, in
order of how much damage they do:

- **`#[cfg(test)]` that changes behaviour instead of only adding tests** — a
  branch, a shortened timeout, a stubbed clock or a skipped validation inside
  production logic. Then the thing under test is not the thing that ships and
  the suite proves nothing about the binary; a **Blocker**, and it collides
  with priority 0 besides. The fix is a seam, not a flag: pass the clock in,
  take the trait, hand the value to the constructor — the pattern `replay`
  already follows by being *told* how much time passed rather than reading a
  clock.
- **A `pub` item on a production type whose only callers are tests**, added for
  one test's convenience and documented nowhere. Either gate it `#[cfg(test)]`,
  move it inside the test module, or publish it deliberately the way
  `engine::fixture` is published — with a doc comment saying it is test support
  and who is meant to call it. The finding is the undeclared widening, not the
  existence of test support.
- **A test asserting on a private detail from the outside**, reached by
  loosening visibility for the test's benefit. The visibility change is the
  finding, not the assertion.

Before filing any of these, check the crate's `lib.rs` and `CLAUDE.md`: a
module the architecture names on purpose is not a leak, and calling one a leak
is the review inventing a second way to do a solved thing.

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

### 8. One language — the repo is written in English

**`CLAUDE.md` owns this rule** — what is in scope, and the three exemptions
where the foreign text *is* the data. Read it there; this dimension does not
restate it, because a scope list kept in two places is dimension 3's own
"second copy is the finding" applied to prose, and it drifts on the first edit.
What lives here is how to grade it.

Grade only what the diff **authors**. Lines that predate the rule are
grandfathered, and a diff that relocates, reindents or deletes one is not
writing it — a cleanup that translates an old comment must not earn a finding
for the Portuguese it is removing. The known pre-existing debt, so nobody
re-litigates it: `docs/ux/drawing-tools-ux-spec.html` (a full spec, ~46 lines),
`heatmap-design-ref/`, the tracked `.claude/GOAL-archive-*.md`, and two doc
comments in `app.rs` / `fib.rs` that quote the trader and are exempt anyway.
Translating any of them is welcome as its own change; this rule never demands
it.

Severity: a line the diff authors in another language is a **Blocker**. Not
because the line is wrong — usually it is the clearest sentence in the file.
Because the moment two languages are tolerated the boundary is never drawn
again: the next reader is locked out of half the codebase, every grep runs
twice, and a contributor who reads neither language has no way in.

**The mechanical half is a test, not a paste.**
`crates/guards/src/language.rs` runs in `cargo test --workspace` and in
CI, holds the allowlist for the debt above, and fails on a new accented run or
Portuguese keyword in `.rs`, `.pine` and `docs/`. That is the repo's own
pattern for a rule the compiler cannot see (`crates/guards/src/encoding.rs`,
`fmath_guard.rs`), and it is why this dimension does not ship a grep recipe:
one was drafted, and it silently missed every accented uppercase word (GNU
grep's `-i` does not case-fold multi-byte characters here) and every
identifier (`_` is a word character, so a snake_case hump offers `-w` no
boundary). A check that comes back clean for the wrong reason is worse than
no check at all.

So the reviewer's job in this dimension is the part the guard cannot do:

- What the guard does not scan — the **branch name, the commit messages and
  the PR title and body**, none of which appear in a file. Read them:
  `git log --format='%s%n%b' origin/main..HEAD` and
  `git rev-parse --abbrev-ref HEAD`.
- Foreign prose the guard's keyword list does not contain — a sentence built
  entirely from words it never learned, or a language it was never taught.
- Whether an exemption is honestly claimed: the string inside a fixture may be
  foreign, the comment above it may not.

Report the guard's verdict and your own separately. "`quantick-guards` language passes"
is not the same claim as "I read the prose".

### 9. The trunk — where did the registration lines land?

Dimension 1 asks whether a capability *can* dock. This one asks where the
docking went. The two are not the same question, and the gap between them is
how this repo acquired a 36,000-line file while every review passed honestly.

A change adds `pointer_compass.rs` — a real new module, a real port question
answered yes — and then adds a field to `QuantickApp`, an init to
`new_with_workspace`, a draw call to `draw_frame` and a hotkey to
`draw_menu_bar`. Four edits, one file, and dimension 1 saw only the new
module. Repeat sixty-eight times: 133 fields, a 1,149-line constructor, and a
struct that *is* the registry — implemented as a struct, so the only way to
extend it is to edit it.

Look for:

- **Growth in the trunk.** `crates/guards/src/size.rs` records a ceiling
  for every file over 1,500 production lines — every line outside a top-level
  `#[cfg(test)]` item — and fails when one grows past it. Check what it counts
  before trusting what it says: the first version of that guard stopped at the
  first `#[cfg(test)]` of any kind, which scored `control/gateway.rs` at 72
  lines of its 4,142 and left five of the largest files in the repo untracked.
  A mechanical half that is blind where the debt is largest is worse than none,
  because a review cites it and stops looking. That guard is this dimension's mechanical half, as `crates/guards/src/language.rs`
  is dimension 8's; the judgement half is yours. Raising a ceiling stays
  legitimate — it is a visible, signed line in the diff — but it is a finding
  to argue with, never a silent act. A branch that raises one without saying
  why in the comment beside it has recorded a decision rather than made one.
- **A registry that is a closed enum.** An entry that cannot join without a
  `match` arm is a type switch wearing a registry's name, and dimension 1's
  first bullet already forbids the shape. Two of the ports `new-extension`
  recommends are exactly this today: `ChartLayer` carries 21 variants across
  264 `ChartLayer::` sites in six files, `DockTab` another 64. Adding a layer
  reopens `app.rs`, `pane.rs`, `tab.rs` and `toolbar.rs`. When a change adds a
  variant, ask what a trait object in a registry would have cost instead — and
  when it adds the *second* variant of a kind, that is the moment the port was
  due.
- **Blast radius in lines, not only files.** `new-extension` §3 counts files
  added versus edited, and a change adding one file while pouring 2,000 lines
  into thirteen others passes that count looking healthy. Count the lines too.
  Mostly-edits by line is the finding dimension 1 already names, one magnitude
  louder.
- **Host or participant?** When a change puts state on the application's root
  struct, ask whether that struct has any reason to know about it — whether
  anything *else* reads the field. State a single surface owns belongs to that
  surface, not to its host. Nine of `app.rs`'s twenty-one `draw_*` surfaces
  touch one or two fields of `QuantickApp`; those are not entangled designs,
  they are modules filed in the wrong place, and saying so is cheap while they
  are still small.

The distinction this dimension turns on: a codebase becomes unmaintainable
from wiring debt long before it does from design debt, and the two look
identical in a file listing. Establish which one is in front of you before
prescribing — an extraction is mechanical and safe, a redesign is neither.

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
cargo test --workspace          # includes the quantick-guards language scan, dimension 8's half
```

A clean change gets a short review saying it is clean and why. Never pad.

## Severity

- **Blocker** — a confirmed correctness finding from step 0; reverse dependency
  edge; forked aggregator logic; determinism broken; hot-path regression; new
  behaviour with no test; a feature that activates itself; a market or safety
  action a non-human operator can reach by a shorter path than the trader's,
  or one that leaves no record of who acted; `#[cfg(test)]` that changes
  production behaviour rather than only adding tests; **any line the diff
  *authors* in a language other than English** — never one it relocates or
  deletes, and never a pre-existing line, per dimension 8.
- **Should fix** — hardcoded value, and the tier it belongs in named (config,
  shared module, module top); a user-tunable value shipped as a `const` that
  needs a rebuild to change; the same constant duplicated at both ends of a
  boundary; extension point that forces edits to existing code; missing
  regression cover; a test module without `#[cfg(test)]`; an undocumented `pub`
  item whose only callers are tests;
  unexplained complex algorithm; misleading name or missing unit; a second way
  to do a solved thing; a capability reachable only from a click handler; state
  that exists only as pixels; a capability that registers itself nowhere, or a
  list kept by hand beside the registry; something the trader was meant to vary
  shipped as a compiled variant; a new field on the application root struct for
  state only one surface reads; a new variant on a registry enum where a trait
  object would have absorbed it; a size-baseline ceiling raised with no comment
  saying why.
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
`test-layout`, `standardisation`, `agent-surface`, `accumulation`, `language`,
`readability`.
Without that tool, write the same list as markdown grouped by severity.

Each finding: `file:line`, what is wrong, why it matters *in this order of
priorities*, and the concrete fix — the trait to extract, the constant to
name, the test to add. Never a vague "consider refactoring".

**Name the commit the verdict graded, and write the verdict after that commit
exists.** The marker holds a sha and nothing else, so an undated verdict cannot
be told apart from one produced for an earlier head — and the marker will be
stamped over it without complaint. That is the one dishonest move the gate
cannot detect, and an unnamed sha is how it happens by accident rather than by
intent. It happened once on the branch that added this paragraph: the verdict
was written forty seconds *before* the commit whose marker it justified, and
only a file mtime caught it.

Close with a verdict in seven lines:

- **Correctness** — what the step 0 code review returned, and whether anything
  from it is still open.
- **Docking** — can the next feature attach without opening these files?
- **Performance** — what got faster, slower, or stayed flat, and at what rate.
- **Operability** — could a script or the future assistant trigger this, read
  the result and discover it exists? Say "no surface" when the change adds no
  user-facing capability; never drop the line.
- **Proof** — which test would fail if this change regressed, and whether it
  is a unit test (`#[cfg(test)]`, private access) or an integration test
  (`tests/`, public API only).
- **Accumulation** — did the trunk grow? Name the tracked files the diff
  moved and by how many production lines, and say whether any the size guard
  ceiling was raised and whether the comment beside it justifies the raise.
  Say "trunk flat" when nothing tracked moved; never drop the line.
- **Language** — two claims, not one: whether the language guard passed, and
  whether you read the prose, the branch name and the commit messages yourself.
  Say both rather than dropping the line — a silent language verdict is
  indistinguishable from one nobody checked.
