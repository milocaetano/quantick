---
name: arch-review
description: The full pre-PR shape review for quantick — bugs first via the bundled code-review, then docking, performance, tests, operability without a mouse, hardcoded values, the trunk and the English rule. Use when the user types /arch-review, asks for a code review or a bug pass before shipping, or asks whether a change is modular, fast enough, drivable by a script, or properly tested. Reviews a diff; it does not design the assistant.
---

# Architecture-first code review

A new feature should dock like a spacecraft to the ISS: a standard port, no
modification to the station. Review every change against that bar.

This skill reviews *shape*. Bug hunting belongs to the bundled `code-review`,
which step 0 runs first.

**How this file is arranged.** Every rule that decides whether a review may
close is stated here. The worked detail behind a dimension — its exemplars,
its anti-patterns, and the histories that set its bar — lives in
`references/`, one file per dimension, read on demand. **Read a dimension's
reference before writing a finding in it**, and skip it for a dimension that
comes back clean. That is what keeps a clean review cheap and a filed finding
accurate.

## Step 0 — the native code review runs first, always

Correctness outranks architecture (priority 0), so the bug pass starts before
the shape pass and the review never closes without it. Run the bundled
`code-review` — not the plugin command of the same name, `code-review:code-review`,
which posts to the PR by itself.

```
Skill(code-review), args: "<effort> <target>"      one string, effort FIRST

  <effort>   never omitted; the skill reads the level from the first token
             only, and a first token that is not a level silently reuses the
             level cached from another session and swallows the target too.
  <target>   a PR number once one exists, otherwise a branch name, or omitted
             for the working diff. Never a revision range.
```

**The level comes from the mission's tier**, one notch below what the tier is
named: `low` for `small` and `medium`, `medium` for `high`, `high` for `max`.
No tier file, or one the hook would not honour, means no tier — then `high`
for a branch or PR, `medium` for a working diff. At `max`, say in the header
that `/code-review ultra` exists; the trader triggers it, never this skill.

Read the tier from the same file `pr-gate` reads, never from the goal file's
`**Tier:**` line — and report it as a finding when the two disagree:

```sh
WT=/path/to/worktree
cd "$WT" && cat "$(git rev-parse --absolute-git-dir)/mission-tier"
```

**Never re-run a level that already ran clean.** That is about what to skip
inside one round, not a budget: the count lives in `CLAUDE.md`'s *review chain
has a budget*, and neither this step nor the shape pass carries a second one.

**The bug pass keeps the strong model.** It finds real defects partly by being
one; it is the exception `CLAUDE.md`'s routing rule exists to protect.

**Name the level in the header, and prove it** — by construction (it went in
as the first token) and by the absence of a reuse notice in the returned
report. A report carrying that notice is a failed invocation whatever else it
found. On divergence the re-run is asymmetric and bounded to one: ran
**below** the tier's level, re-invoke once; ran **above**, accept it and
record the overspend in the header and the PR body. Where nothing settles it,
write *unverified* rather than rounding up. `references/step-0.md` has the
parser's behaviour, the evidence and the failure this rule was written from.

**Expect it in the background.** The skill returns a name and the findings
arrive as a notification. Read for shape meanwhile, publish nothing. If the
notification never lands, re-invoke once; if that fails too, do the bug pass
yourself and say which it was.

**Check the scope it comes back with.** Findings over files this branch never
touched, or a suspiciously empty pass, mean re-invoking with an explicit
target — not a clean bill of health.

When the findings land:

- **Sort before promoting.** Wrong *behaviour* — crash, wrong output, broken
  determinism, race — becomes a **Blocker** here, listed before every shape
  finding. A cleanup is filed in the dimension it belongs to and graded there,
  so an efficiency finding on a per-frame path is still a Blocker under the
  hot-path rule.
- **Confirm before promoting.** `high` and above deliberately include
  uncertain findings; item 2 of *Verify before reporting* applies to them.
- **Cite, never restate.** A step 0 finding ships as its `file:line` plus the
  severity assigned here.
- **Step 0 never publishes.** No `--fix`, no `--comment`, no `--post`.

A branch still needs the `arch-review-ok` marker to open a PR, so on a
docs/skills change — where the shape pass is waived — run this skill anyway
and report step 0's findings through it. The bug pass is not the waived part.

## Record the marker when the review closes

A branch cannot open a PR until this review is recorded against the exact
change being shipped — a hash of the branch's diff, so a rebase does not
invalidate it but an edit to a tracked file does. Once every Blocker and
Should-fix is resolved or deferred in the PR body, and no further commits are
coming:

```sh
WT=/path/to/worktree
cd "$WT" &&
  git diff origin/main...HEAD |
    git hash-object --stdin > "$(git rev-parse --absolute-git-dir)/arch-review-ok"
```

The `cd` matters: both `git` calls resolve against the shell's cwd, which for
an agent session is the main checkout. Without it the marker lands in the
wrong git dir holding the wrong sha, and the next `gh pr create` denies with
no clue why.

A branch that gains another commit after this has a stale marker by design:
the review runs again over the new head before it is re-recorded. Re-stamping
a marker whose review did not run again is the one dishonest move the gate
cannot detect.

## What this skill does not review

Whether the change is *what was asked for*. That is `delivery-review`, which
runs after this skill and whose marker `pr-gate` wants alongside this one. A
conformance gap noticed here is worth a sentence in passing, never a severity.

## Priority order

When two findings pull in opposite directions, this order decides the call.
State it explicitly when a trade-off is at stake.

0. **Correctness, determinism and authority** — a precondition, not a
   trade-off. Same trades in, same bars out; and no operator, human or
   otherwise, reaches a market or safety action by a path the trader does not
   cross.
1. **Performance.** Never spend runtime cost to make code friendlier to read.
2. **Modularity and extensibility**, including the second operator's half of
   it. Its authority half sits at 0, not here.
3. **Tests that prove the behaviour.**
4. **Standardisation.** One way to do a thing, one language to say it in.
5. **Human-friendliness.** Mandatory wherever it is free at runtime, and last
   only because it is the one that must yield in a collision — which it almost
   never has to, since naming and comments compile away.

## Scope the review

```sh
git fetch origin                       # first: the ranges below read origin/main
git diff origin/main...HEAD --stat     # branch under review
git diff --stat                        # uncommitted working diff
gh pr diff <n>                         # a PR
```

The range names the remote on purpose: `main...HEAD` in a worktree shows other
branches' merged work as if this branch wrote it — 26 files from someone
else's PR, on the branch that added this line. Fast-forwarding the main
checkout (`git -C <main-checkout> pull --ff-only`) fixes the local ref too.

Read the neighbouring code before judging any of it. The repo's existing
pattern is the standard; a change that invents a second way to do something
already solved is a finding even when the new way is prettier in isolation.

### The mission's tier scopes the shape pass

At `small`, read only the dimensions the diff reaches — **dimension 8 always**,
and step 0 always in full. A smaller reading, never a lower bar. If three or
more dimensions apply, say so in the header: the branch has outgrown its tier.
Every other tier reads all nine. **Ignore the narrowing when the claim is
false** — a branch over the `small` ceiling in `guardrails.sh` gets all nine
whatever the tier file says.

## The nine dimensions

### 1. The docking test — modularity and extensibility

> Could a second implementation of this be added by writing a new file and one
> registration line, without editing any existing behaviour?

If not, name the file the next author would be forced to open, and the port
that would have prevented it. Hunt: type switches that grow (`match` over a
closed enum, `if is_replay`, `if feed == "binance"`); a consumer depending on a
concrete producer rather than the trait it needs; a reverse dependency edge
(a Blocker, no exceptions); any per-consumer copy of bar building (a Blocker);
a change that is mostly edits to existing files; a capability that activates
itself rather than defaulting to today's behaviour.

Detail: `references/docking.md`.

### 2. Performance impact — every change declares one

No change ships without an answer to **how often does this run?** Classify
every touched path — per trade, per depth update, per frame, or rare — and
judge it at that rate. Never assert "this is slow" without the rate and the
concrete cost; a guess stated as a measurement is itself a finding against the
reviewer. Cold-path micro-optimisation that costs clarity is a finding in
reverse.

Detail, with the rate table and the hot-path hunt list:
`references/performance.md`.

### 3. Nothing hardcoded

Every literal that *configures behaviour* lives in a named constant or in
config, never inline. Every finding names which of three tiers the value
belongs in — a config file (anything a *user* tunes, including what a fresh
launch draws), a shared module (a value two places must agree on), or module
top (`SCREAMING_SNAKE_CASE`, unit in the name, a doc comment saying *why this
number*) — because "extract a constant" is the wrong fix for a value the
trader was supposed to edit. Message and assertion text is not a configuration
value; filing it is noise.

Detail, with the exemptions and the config round-trip rule:
`references/hardcoded.md`.

### 4. Tests that prove the change — and stay out of the product

- Every new behaviour has a test that **fails without the change**. If you
  cannot name it, the behaviour is unproven — a Blocker.
- Engine work is test-first: fixture trades, expected bars, then code.
  Determinism is guarded by golden tests over fixed fixtures.
- **Test the port, not just the feature.** One implementer never proves a
  trait is a port; a fake is fine.
- **Regression cover for what already worked.** Ask which existing test would
  have caught this change if it were wrong; if none, that gap is the finding.
- Edge cases the domain produces: empty book, one-tick spread, zero quantity,
  gap in update ids, feed arithmetic that must saturate rather than panic, a
  session ending mid-bar.
- **`#[cfg(test)]` that changes behaviour** rather than only adding tests is a
  Blocker: the thing under test is then not the thing that ships.

The Rust layout the rest is graded against — unit tests, integration tests,
and how this repo publishes test support without a cargo feature — is
`references/tests.md`.

### 5. Standardisation

One way to do a thing. Compare against the existing repo answer for error
types, module layout, config format, naming, logging and the shape of a public
API. A new local convention needs a stated reason or it is a finding.

### 6. Human-friendly at zero runtime cost

- Names carry intent and unit: `cluster_window_ms`, not `cw`. Renaming is
  free — demand it.
- A complex algorithm gets a comment in English explaining the *objective*,
  not a restatement of the code.
- Inferred or incomplete data is labelled, never silently patched.
- Prefer zero-cost clarity: newtypes over bare `f64`, exhaustive `match` over
  `_ =>`, early returns over nesting. No comment restating obvious code, and
  no dead or commented-out code.

### 7. The second operator — could an agent do this without the mouse?

> Could an operator that is not holding the mouse — a script, a test, the
> future assistant — trigger this action, read back what it did, and discover
> that it exists, without a human clicking?

Three capabilities, each its own finding when missing: **act** (the action
exists as a named call taking data and an actor, not only as a click body),
**read** (the result is enumerable as data, not only pixels), **discover** (a
stable id in the *same* registry that feeds the UI, with declared parameters).

Two rules ride with it. **What the trader authors is data, not a rebuild** — a
capability the trader was meant to vary arriving as a compiled variant is a
finding that names the script or config file that would have avoided it.
**Authority is declared, and so is the author, at priority 0** — a market or
safety action a non-human operator reaches by a shorter path than the trader's
is a Blocker, and anything the assistant placed is labelled as such.

Performance outranks the operability half of this dimension and does not
outrank the authority half. Detail: `references/second-operator.md`.

### 8. One language — the repo is written in English

`CLAUDE.md` owns the rule and its exemptions; this dimension grades it. Grade
only what the diff **authors** — relocating, reindenting or deleting a
pre-existing foreign line is not writing it. A line the diff authors in
another language is a **Blocker**.

`crates/guards/src/language.rs` is the mechanical half and runs in `cargo test
--workspace`. The reviewer's job is what it cannot scan: the branch name, the
commit messages and the PR title and body; foreign prose its keyword list
never learned; and whether an exemption is honestly claimed. Report the
guard's verdict and your own separately.

Detail, including the known pre-existing debt: `references/language.md`.

### 9. The trunk — where did the registration lines land?

Dimension 1 asks whether a capability *can* dock; this asks where the docking
went. The gap between them is how this repo acquired a 36,000-line file while
every review passed honestly. Look for growth in the trunk (the size ratchet
is the mechanical half; a ceiling raised with no comment saying why is a
finding), a registry that is a closed enum, blast radius counted in lines and
not only files, and state put on the application root struct that only one
surface reads.

Establish which one is in front of you before prescribing: an extraction is
mechanical and safe, a redesign is neither. Detail: `references/docking.md`.

## Verify before reporting

Reviews are judged on precision, not volume.

0. Confirm step 0 ran and its findings are in hand. A shape review published
   without them is incomplete, not "clean".
1. Open the file and read the surrounding code — most "this is missing"
   findings die here because the thing exists one function up.
2. For each surviving finding, argue the opposite case: is this already
   handled, deliberate, or out of scope? Drop it if the refutation holds.
3. Confirm the four checks actually pass — do not take a claim on trust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace          # includes the guards' language and ratchet scans
```

A clean change gets a short review saying it is clean and why. Never pad.

## Severity

- **Blocker** — a confirmed correctness finding from step 0; reverse
  dependency edge; forked aggregator logic; determinism broken; hot-path
  regression; new behaviour with no test; a feature that activates itself; a
  market or safety action a non-human operator can reach by a shorter path
  than the trader's, or one that leaves no record of who acted; `#[cfg(test)]`
  that changes production behaviour; **any line the diff *authors* in a
  language other than English**.
- **Should fix** — a hardcoded value, with its tier named; a user-tunable
  value shipped as a `const`; the same constant duplicated at both ends of a
  boundary; an extension point that forces edits to existing code; missing
  regression cover; a test module without `#[cfg(test)]`; an undocumented
  `pub` item whose only callers are tests; an unexplained complex algorithm; a
  misleading name or missing unit; a second way to do a solved thing; a
  capability reachable only from a click handler; state that exists only as
  pixels; a capability that registers itself nowhere, or a hand-kept list
  beside a registry; something the trader was meant to vary shipped as a
  compiled variant; a new field on the application root struct for state only
  one surface reads; a new variant on a registry enum where a trait object
  would have absorbed it; a baseline ceiling raised with no comment saying why.
- **Consider** — clarity and structure with no correctness, performance or
  extensibility consequence.

## Output

Open with one line for step 0: the effort level, **how that level was proven**,
and how many findings came back, including zero — `step 0: code-review at high
(effort-first, no reuse notice), 12 findings, 3 confirmed` — or why it did not
run. Two other shapes exist and the header always carries exactly one: a
divergence (`at xhigh (tier bought medium; reuse notice, accepted per the
asymmetric rule)`) and an unsettled one (`at medium (effort-first; level
unverified)`). On the `ReportFindings` path that line is the text accompanying
the call, and it goes into the PR body too — chat scrolls away.

Report findings with `ReportFindings` when available, ranked most severe
first, using categories `correctness`, `modularity`, `performance`,
`hardcoded-values`, `test-coverage`, `test-layout`, `standardisation`,
`agent-surface`, `accumulation`, `language`, `readability`. Without it, the
same list as markdown grouped by severity. Each finding: `file:line`, what is
wrong, why it matters in this order of priorities, and the concrete fix — the
trait to extract, the constant to name, the test to add. Never a vague
"consider refactoring".

**Name the commit the verdict graded, and write the verdict after that commit
exists.** The marker holds a sha and nothing else, so an undated verdict
cannot be told apart from one produced for an earlier head — and it will be
stamped over without complaint.

Close with a verdict in seven lines, none of them ever dropped:

- **Correctness** — what step 0 returned, and whether anything is still open.
- **Docking** — can the next feature attach without opening these files?
- **Performance** — what got faster, slower or stayed flat, and at what rate.
- **Operability** — could a script or the future assistant trigger this, read
  the result and discover it exists? "No surface" when the change adds no
  user-facing capability.
- **Proof** — which test would fail if this change regressed, and whether it
  is a unit or an integration test.
- **Accumulation** — did the trunk grow? Name the tracked files the diff moved
  and by how much, and whether any ceiling was raised with a justifying
  comment. "Trunk flat" when nothing tracked moved.
- **Language** — two claims: whether the guard passed, and whether you read
  the prose, the branch name and the commit messages yourself.
