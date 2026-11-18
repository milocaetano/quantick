# Building quantick with agents

Most of this repository is written by AI agents working under a human
maintainer. That is a claim worth being precise about, because "built with AI"
usually means an autocomplete was on. Here it means something narrower and
checkable: the workflow that turns an objective into a merged pull request is
itself committed to the repository, as skills the agent loads, gates it has to
pass, and hooks that refuse the work when it skips one.

This document describes that workflow. It is not a rule for contributors —
[`CONTRIBUTING.md`](../CONTRIBUTING.md) is — and it changes nothing about what
the code has to prove. It exists because the process is one of the more
interesting parts of the project, and it was previously legible only by
reading `.claude/`.

## The problem it solves

An instruction written in markdown is advice. A long agent session drifts away
from advice — not maliciously, just by attention decay: it is forty tool calls
deep, the objective has narrowed to the bug in front of it, and the rule about
worktrees was in a file it read an hour ago.

So the rules that matter most are not written as advice. They are written as
walls the harness enforces, and everything else is written as a *skill*: a
loadable procedure with its own acceptance criteria, invoked by name.

## The skills

Each canonical workflow is a directory under `.claude/skills/` with a
`SKILL.md`. Claude Code invokes it as `/name`. Codex discovers the thin adapter
with the same name under `.agents/skills/` and invokes it as `$name`; the
adapter reads the canonical workflow and the shared host mappings, so the two
agents do not own competing copies. The agent loads
one when the task matches its description, or when the maintainer types its
name as a slash command.

The adapter location and `$name` spelling follow the
[Codex skill discovery contract](https://developers.openai.com/codex/skills).

| Skill | What it owns |
| --- | --- |
| `new-extension` | The build-time form of `CLAUDE.md`'s docking rule: it designs a feature as a new file plus one registration line from the start, instead of asking afterwards whether it could have been. |
| `quantick-score` | The project baseline. It assigns evidence-based scores to sustainable engineering, scalability, agentic development and AI-operated product behavior; unlike `ai-review`, it measures a repository revision rather than a diff. |
| `outside-score` | The dissenting baseline. It scores architecture, software engineering and agent change cost the way an outside architect would, from measured rows (`tools/outside_score/measure.py`) and without reading `quantick-score` first, then reports where the two rubrics disagree. |
| `trader-ux-review` | The harness's screenshots, judged by trader personas against order-flow heuristics: does this cost attention, clicks or trust at a moment the market is moving? |
| `ui-harness` | The contract that makes visual work possible — every user-visible surface reachable from a fresh launch by environment hooks alone, zero clicks — and the autonomous QA pass built on it: it drives every affected surface, **asks the live control plane what the application believes is on screen**, captures a state matrix and reads the images against a defect checklist. It absorbed `visual-qa`. |
| `issue` | Turns an idea into a well-formed issue with scope, acceptance criteria, labels and board placement — or redirects it to Discussions when there is no concrete deliverable yet. `/issue start <N>` picks one up: reads it, names the branch from its labels, moves the board card, and hands the work to `/implement`. It absorbed `new-task`. |
| `implement` | The default path today. It writes a plan of about 200 tokens and a `/goal` line, and does nothing else. The goal then builds, takes one independent review at a level set by risk, gets a test agent for anything user-visible, and delivers a ready PR with green CI. It replaced `mission`, `ship` and the pre-PR reviews; see [The lean path](#the-lean-path) and [Retired skills](#retired-skills-2026-11-17). |
| `feature` | `implement` for work too large for one PR. It splits the work into the fewest child goals, cutting only at a crate or port boundary, fixes their shared design up front, and judges the combined result as one PR. |

`ui-harness` deserves the emphasis. It is the same rule as the product's
fourth design principle — *operable without a hand* — applied to the
development loop. The control plane exists so an agent can operate the
application; the harness hooks exist so an agent can *test* it. A capability
reachable only by mouse fails both.

## The gates

Three things stand between a change and `main`, and none of them is the
agent's own judgement that it is finished.

**The four-check verification loop.** `cargo fmt --all -- --check`, then
`cargo clippy --workspace --all-targets`, then `cargo build --workspace`, then
`cargo test --workspace`. CI runs the same four on every PR and on every push
to `main`, plus five steps the workspace cannot see: the guardrails' own
test script, `ruff check --select F` over the Python under `tools/mt5/` and
`bridge/mt5/`, the session exporter's tests, the MT5 bridge's paging
tests — whose own comment records that without that step a revert ships
green — and `cargo deny check bans licenses`. A PR with red CI is never
merged.

The clippy line carries no `-D warnings`, and used to. The levels moved into
`[workspace.lints]` in the root `Cargo.toml`, which every crate inherits, and
the move is worth understanding because the failure it fixed is the one this
whole document is about. A flag on a command line is only in force when
somebody types that command — so `cargo clippy -p <crate>`, the narrow fast
form a session actually runs between edits, was checking at a laxer level than
the gate it eventually had to pass. The difference arrived as a red CI on code
that had been clean locally, and the session then spent itself looking for the
cause in its own diff, where it was not. In the table the level applies to
every cargo invocation, `cargo check` included, so a warning surfaces at the
first edit that causes it rather than at the gate.

`rust-toolchain.toml` pins the compiler for the same reason, one layer down.
CI used to install whatever `stable` meant on the morning the job ran, which
made a toolchain release indistinguishable from a regression until somebody
read the log closely enough to notice the version had moved. Both changes buy
the same property: a red means the change is wrong, so an agent can trust the
signal instead of first proving the repository innocent.

**One independent code-review over `git diff origin/<target>...HEAD`.** It
runs at a level the change's risk sets, its valid findings are fixed before the
PR opens, and a deferred finding is named in the PR body, so the deferral is a
decision on the record rather than an omission. The range names the remote
deliberately: `git fetch` moves `origin/main` and leaves the local `main` ref
behind, so `main...HEAD` in a worktree shows other branches' merged work as
though this branch wrote it.

**The checks the work actually earns.** `implement`'s rules decide which apply:
a change a trader touches mid-session gets `trader-ux-review`; anything user-
visible or agent-facing gets a test agent through `ui-harness` or the control
plane; a docs-only change gets neither, but never skips the English check or
the review. Local checks follow the changed inputs under
[the delivery contract](workflow/delivery.md); full final-head CI remains.

## Two phases, and why the rounds had to go

This section is history: it describes the review chain of the retired
`mission` skill, and the rules it produced that `ai-review` still follows.

The chain used to run one loop: review, fix, review again, and a budget of
three rounds over the whole thing. It did not hold. PR #306 shipped after 28
commits, among them "fix: round 17 of the review chain" and "the fifteenth bug
pass" — and a later `ai-review` still returned five of its six dimensions WEAK.
Every gate the repository owns had passed it.

Two things were wrong, and they are the two halves of the fix.

**A round could not redesign.** Making it work and making it right ran in one
loop, so a fix commit was always a patch *inside* a design it had no licence to
change. `DealBarBuilder`'s four coordination booleans were authored in rounds
12, 12, 12 and 15, and `bar_opened_at` in round 14, while the domain concepts
they coordinate came in the first feature commit. The late rounds did not
improve that design; they hung flags on it, which is the only move a round has.
So the work is two phases now, and phase two is explicitly allowed to change
signatures. That is not licence to redesign at will — it is the licence a
finding needs when the honest fix is structural and the alternative is a
fifth boolean.

**A round was the wrong unit to count.** Three rounds was a real rule that
nothing enforced, and 17 rounds happened anyway. Worse, counting rounds treats
independent findings as iterations of one thing: closing six unrelated threads
is not six attempts at anything. So the count moved to the findings themselves,
which now live as GitHub review threads — durable across a restart or a
compaction, addressable by id, anchored at `file:line`, resolvable, and visible
to the trader without an agent in the room. The number of open ones is the
convergence trend, recorded for free by the act of reviewing.

The first stall rule compared total open findings. Campaign #330 exposed its
false positive: #339 closed an omission, then a new reviewer split unchanged
source more finely and found two others. The implementation gates passed, but
the count forced a human handoff over documentary mapping. Current rules track
finding identities, separate new discoveries from failed repairs, and retain
finite budgets. The delivery contract owns those decisions.

**The loop still needs a stop, and two rules give it one.** Round one reviews
the whole diff; every later run verifies only the open threads plus a narrow
check that the fixes introduced no new FAIL, and may not open a new WEAK
against code it already passed. Without that, a reviewer re-reading the same
file can always find one more thing, the finding set is unbounded and no amount
of fixing ever empties it. And a thread closes exactly two ways — the fix, or
an acceptance the trader records on it — so a finding nobody will act on has a
door out that is not "argue with it again next run". The corollary is the
sharpest of the rules: a WEAK whose breaking variant the reviewer cannot name
is not a WEAK, it is a PASS. A worry is not a finding.

## The lean path

Two phases fixed the loop, but the ceremony stayed: a tiered mission, three
independent pre-PR reviews and a delivery grade for every change. That was
sized for a funded team. quantick is one maintainer paying for their own
tokens, and the measurement below showed where the tokens went: almost all of
them were spent after the PR had opened.

`implement` (#592) is the result, and it is now how ordinary work lands:

- **Planning is a separate, tiny step.** `/implement` reads only what it needs
  and writes a plan of about 200 tokens: outcome, base SHA, paths, acceptance
  and checks. It never builds. The maintainer starts the build with the
  `/goal` line it prints, so the build runs against a written condition
  instead of a conversation.
- **One review, sized by risk.** Hooks, guards, trading, concurrency, public
  contracts and diffs over 600 lines get `high`. A local change of 150 lines
  or fewer with no contract change gets `low`. Everything else gets `medium`.
  There are at most two rounds; anything left over goes into the PR as a
  follow-up rather than starting a third round.
- **A test agent stands in for the user.** For any user-visible or
  agent-facing change, an agent with no chat context drives the built app
  through `ui-harness` or the control plane, and its failure blocks.
- **Where the code lands is the default, not a gate.** New state and decisions
  go to a headless crate outside `crates/app`. The app keeps a field, one-line
  calls and painting. The guards above still decide what merges.

The walls did not move: worktrees, the review marker, the guards and green CI
are still enforced. What went away is the review a change was not risky
enough to need.

## Retired skills (2026-11-17)

`mission`, `ship`, `arch-review`, `delivery-review` and `campaign` were
deleted, with the completion verifier (`mission_ship_gate.sh`) and the
durable-report producer (`review_report.sh`) that only they used.

- **`implement` had already replaced them.** Since #592 ordinary work landed
  through one plan, one risk-sized code-review, a test agent and green CI, and
  the trader stopped invoking the old chain. `pr-gate` had stopped asking for
  the arch, delivery and AI review markers before this change; what remained
  was a final verifier and a report producer with no caller on the default
  path.
- **Unused process still costs.** The four skills were about 70 KB of prose
  and the scripts and their tests about 1,100 lines that every agent reading
  `.claude/` had to reason around, and that each hook change had to keep green.
- **`campaign` went with them.** It ran every child through `mission` and
  `ship` and the two reviews' markers. `/feature` covers work too large for one
  PR, under the same `mission-base` record and merge grant.
- **What they enforced lives on where it decides outcomes.** Docking, *operable
  without a hand* and the English rule are in `CLAUDE.md`; size, context,
  language and evidence are guards; the code-review and full CI are the gate.
  The file names `mission-tier` and `mission-base` stay, read by
  `implement` and `feature`.

Their records stay as history in [`history/campaign/`](history/campaign/) and in
git.

## The hooks that make the gates real

Two of those rules — work in a worktree, and get a code-review before the PR
— were enforceable only by an agent remembering them. They are now walls the
harness puts up, in `.claude/hooks/guardrails.sh`.

[`.claude/hooks/README.md`](../.claude/hooks/README.md) owns the details — the
four modes, what each denies (the fourth denies nothing — it only reports),
the overrides and why they fail open — and is not repeated here. What is
worth pulling out for an outside reader is the one
design decision that makes the gate honest rather than decorative:

> `code-review-ok` holds **a hash of the change the review covered**, not a
> timestamp or boolean, and a hook writes it from the reviewer's verdict line,
> never the implementing agent.

Keying on the change rather than the commit is the second version of that
decision, and it was bought with real pain: the branch that introduced tiers
paid for five review rounds, several of which re-graded a diff nothing had
touched, because a rebase or an amend moved a sha the reviews did not care
about. It is also *stricter* in the case the sha form missed — a rebase that
lands a branch on top of upstream edits to the very files it changes now stales
the marker, which is exactly when a second look is worth most.

A marker that only recorded "a review happened" once passed while new commits
went unreviewed. The key closes that gap; it still cannot prove the review
judgment was *good*.

## The goal record

A goal's plan stays outside Git (ignored `.claude/GOAL.md` or a file beside
the worktrees). After delivery, the PR body keeps the outcome, the measured
size and the review and CI evidence. Raw goal files and execution evidence do
not accumulate in the repository:
the `evidence` guard fails a tracked goal file, evidence folder or `docs/`
dossier, because prose alone did not hold the rule.

## What this does not claim

The agents do not merge their own work; a human reviews and merges. The gates
catch classes of failure — drift, unreviewed commits, untested surfaces,
mouse-only capabilities — not all failure. And the process is only as good as
the acceptance criteria a plan writes down, which is a human judgement at
the start of every session.

The lean path is judged on use, not yet on a controlled measurement. The
maintainer reports no rework from it, and main stays green under the guards.
A paired cost comparison against the old chain is still open
([#563](https://github.com/milocaetano/quantick/issues/563)).

## Why the rules read the way they do

`CLAUDE.md` states each working rule operatively and keeps no argument. The
arguments are here, because every one of these rules was written after the
thing it forbids had already happened.

**The headless rule names the feeds as an exception because they always
were.** `CLAUDE.md` listed "the feeds" among the crates with no async and no
wall clock, and had listed them there for as long as the rule existed. They
never satisfied it and never could: a feed's whole job is a socket. The three
crates hold 190 `.await` points — 96 in `feed-binance`, 51 in `feed-mt5`, 43
in `feed-hyperliquid` — and read the clock three times, at
`feed-binance/src/depth/stream.rs:665`, `feed-mt5/src/stream.rs:2058` and
`feed-hyperliquid/src/candles.rs:414`, each stamping arrival for the latency
readout the status bar shows.

A rule with a standing exception nobody wrote down is worse than a narrower
rule, because the first agent to check it finds the rule false and cannot tell
which half is wrong — the code, or the rule. So the exception is stated, and
stated narrowly: what determinism actually depends on is that neither the
async nor the clock read *crosses the `FeedEvent` channel*. A feed emits
events carrying the venue's own timestamps, and nothing downstream is allowed
to know a clock was read. That is the property the golden tests protect, and
it is unchanged.

**Not every hook belongs in `harness.rs`, and the fifty in `app.rs` are
not all debt.**

*Almost*, because about fifty reads are still in `app.rs` and are not debt in
the same sense. Most are hooks that keep **no state at all** — they call a
setter on a tab and are finished (`QUANTICK_TAPE`, `QUANTICK_INVERTED`,
`QUANTICK_INDICATORS_AUTOSTART`), so there is no field for an owner to hold.
The rest belong to clusters that are each their own extraction — the
`QUANTICK_CONTROL_*` family, the tab and layout hooks, the replay and
workspace hooks — and they reach `self.tabs` and the control gateway, which
`harness.rs` deliberately cannot see. A hook of yours that keeps a field, and
needs nothing but its own parsed value, belongs in the owner.

That surface rule used to live at the bottom of the registry, which is now a
61KB data file this skill tells you to `grep` rather than read. An authoring
rule nobody reads is one the size guard enforces by failing you instead, so it
belongs here, beside the instruction it is the exception to.

**The edit loop is not the gate.** `cargo check` appeared nowhere in this
repository's documentation, so the four-check gate was doubling as the loop
between edits. An agent with no name for the fast path defaults to the slow
one, on every edit, for the length of a goal. A one-package `cargo check`
skips codegen and linking and answers on `quantick-app` in well under a
minute, where a workspace build takes many.

**The guard binary has to be armed on purpose.** A fresh worktree has no
`target/`, so `target/debug/quantick-guards` does not exist, so the
`PostToolUse` hook reports nothing — not "clean", *nothing* — for as long as
that stays true. This was found switched off across every worktree in one
checkout at once, which is the state `git worktree add` leaves behind by
default rather than an accident someone had to cause. The crate has no
dependencies, so arming it costs seconds, and it is the difference between a
crossed ceiling reported at the edit that caused it and one reported after the
code is written.

**The size ratchet exists because the review measures the leaf.**
The docking rule asks whether a new capability can dock as a new file
plus one registration line, and recent features answered yes honestly. `app.rs`
still grew from 108 lines to over 36,000 in the five weeks after it was
created, monotonically, never once shrinking. Nothing asks *where the
registration lines accumulate*: they accumulate in `QuantickApp` and its
constructor, because a capability with no port is docked by hand — a field, an
init, a draw call, a hotkey — and every one of those four edits lands in the
same file. fmt, clippy, build and the whole suite stay green throughout.

It counts production lines only because a guard counting total lines would
fire on a well-tested change and teach the author to write fewer tests, which
is worse than the disease. It has teeth below a ceiling as well as above one
because unclaimed slack is only headroom for the next feature to refill
silently, which is how the debt was run up the first time. And the total is
capped on top of the per-file ceilings because one branch raised `app.rs` from
9,775 to 9,890 with a comment explaining why, extracted nothing in return, and
every check stayed green — as it should have. Eighteen entries each raised
"for this branch" read as eighteen reasonable decisions and one lost trunk,
and no per-file rule can see that, because the question is about the sum.

**`guards` has an empty manifest by design.** These guards read files and count
lines, so living under `crates/app/tests/` made cargo build the largest crate
in the repo before the cheapest question in it could be answered — four minutes
of link for five seconds of work. A ratchet is meant to fire while you work,
and one that expensive to consult gets consulted late, which is when its
finding costs most to act on.

## Reasons moved out of the always-loaded files

The instruction diet that halved `CLAUDE.md`, `AGENTS.md` and the skills kept
every operative rule where it was and moved the arguments here.

**Why `AGENTS.md` is only the map.** It used to repeat `CLAUDE.md`'s design
rules and verification loop "so an agent reading only this file does not
violate one", and to carry a full guide to driving the app over MCP. A second
copy of a rule drifts, and the MCP guide belongs to the adapter it describes:
it now lives in [`crates/mcp/README.md`](../crates/mcp/README.md). The control
plane exists because of *operable without a hand*; the `trader` profile exists
in the contract so the day fills stop being simulated, nothing about authority
has to be re-decided in a hurry. The map names `replay::test_support` because a
published module the map does not mention cannot be told from a leak.

**Why reviews diff against `origin/main`.** On the branch that added the rule,
`main...HEAD` credited 26 files from someone else's PR to the branch under
review, because `git fetch` moves `origin/main` and leaves the local ref behind.

**Why a surface docks through the `Surface` port.** The old shape — a field, a
constructor line, a `draw_frame` call and a menu hotkey on `QuantickApp` — took
it to 130 fields and a 1,022-line constructor before the size guard stopped it.

**Why the hook registry is grepped, not read.** At 69 KB, loading it whole to
answer "what turns the heatmap on" was the single largest token cost in the
agentic flow, paid on every capture. No one file owns the hooks — `harness.rs`
held 24 of 126 when this was written — so the registry's *Declared in* column
is the answer. The agent target dir lived on `F:` until that drive disappeared;
`C:` runs into single-digit gigabytes with a few worktrees, hence the free-space
check.

**Where the QA pass's precedents come from.** The drawing inspector is opaque to
the pointer and placed so it never covers the action — the occlusion rule's
model. The live-tail-compact proposal was rejected because it froze the live
region, which is why the motion check insists it keeps moving.

**Why `visual-qa` and `new-task` were folded in.** Both overlapped a sibling's
trigger: `visual-qa` launched and captured the app exactly as `ui-harness`
taught, and `new-task` duplicated the worktree recipe `CLAUDE.md` owns while
sharing the board with `issue`. One skill per trigger means one place
a rule can drift.
