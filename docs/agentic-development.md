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
| `mission` | The orchestrator. Captures the session objective in English, classifies it, and derives the acceptance criteria — including which of the gates below are part of *done* for this kind of work, so the maintainer never has to list them. It also takes a **tier** — `small` (the default), `medium`, `high`, `max` — which scales all of that to the size of the change, down to how hard the bug pass looks and whether the conformance review runs at all. One session, one mission. |
| `new-task` | Starts work from a GitHub issue: reads it, branches from updated `main` with the right prefix, moves the board card. |
| `new-extension` | The build-time twin of the review question below. `arch-review` asks after the fact whether a feature could have been a new file plus one registration line; this skill designs it that way from the start. |
| `arch-review` | The pre-PR review. Step 0 runs a correctness pass; then it grades *shape* — does the change dock like a module, does it declare its performance impact, do its tests stay out of the shipped binary, is it drivable without a mouse, does it hide anything behind a magic number, is it English throughout. |
| `delivery-review` | The other pre-PR review, and the one that asks a different question: not *is this well built* but **is this what was asked for**. It grades every ask in the mission's request ledger and every acceptance criterion — DELIVERED, PARTIAL, MISSING or UNPROVEN — from a fresh-context subagent that never sees the implementing session's account of its own work. |
| `visual-qa` | Autonomous visual QA. Drives every affected surface through the harness hooks, **asks the live control plane what the application believes is on screen**, captures a state matrix, and reads the images against a defect checklist. |
| `trader-ux-review` | The same screenshots, judged by trader personas against order-flow heuristics: does this cost attention, clicks or trust at a moment the market is moving? |
| `ui-harness` | The contract that makes the two above possible: every user-visible surface must be reachable from a fresh launch by environment hooks alone, zero clicks. A new panel that cannot be opened without a mouse is an incomplete panel. |
| `issue` | Turns an idea into a well-formed issue with scope, acceptance criteria, labels and board placement — or redirects it to Discussions when there is no concrete deliverable yet. |
| `ship` | The delivery: the four-check loop, the commit, the push, the PR with `Closes #N`, and CI watched until green. |

`ui-harness` deserves the emphasis. It is the same rule as the product's
fourth design principle — *operable without a hand* — applied to the
development loop. The control plane exists so an agent can operate the
application; the harness hooks exist so an agent can *test* it. A capability
reachable only by mouse fails both.

## The gates

Four things stand between a change and `main`, and none of them is the
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

**`arch-review` over `git diff origin/main...HEAD`.** Every Blocker and
Should-fix finding is resolved before the PR opens. A finding deliberately
deferred is named in the PR body, so the deferral is a decision on the record
rather than an omission. The range names the remote deliberately: `git fetch`
moves `origin/main` and leaves the local `main` ref behind, so `main...HEAD` in
a worktree shows other branches' merged work as though this branch wrote it.

**`delivery-review` over the branch as shipped.** `arch-review` takes the
change as given and grades how well it is made; it never opens the request and
checks that all of it arrived. That is this gate's only question. It reads the
mission's goal file — the request quoted verbatim, the ledger of asks derived
from it, the acceptance criteria — and grades each one, from a subagent that
receives artifacts rather than the implementing session's story. It passes only
when nothing is MISSING, PARTIAL or UNPROVEN, and a gap ships only as a
deferral the trader approved.

This is the one gate a mission can buy its way out of, and only at the `small`
tier — the one-line fix, where a ledger has nothing to grade and the review
costs more than the change. The exemption is bounded rather than trusted: it
lapses the moment the branch grows past a ceiling measured against
`origin/main`, so the word has to be true of the branch that ships and not
merely of the one that was planned. The hook never mentions the exemption to a
branch that did not declare it, which is what keeps the gate from teaching its
own way around itself.

**The review gates the work actually earns.** `mission` decides which apply:
a change a trader touches mid-session gets `trader-ux-review`; anything
visual gets `visual-qa`; a docs-only change gets neither, but never skips the
English check or the correctness pass. The tier decides how hard the ones that
do apply look — and nothing, at any tier, skips the four checks or the bug
pass. A cheap review is a real one done briefly; it is never an absent one.

## Two phases, and why the rounds had to go

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

What replaces the budget is a stall rule: if the open set does not shrink
between two runs, the branch goes to the trader. That is the same judgement the
old budget was trying to buy — findings shrinking is convergence, findings flat
or climbing into the last run's code is a design problem — with the arbitrary
number taken out.

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

## The hooks that make the gates real

Three of those rules — work in a worktree, run arch-review before the PR, and
grade the branch against what was asked — were enforceable only by an agent
remembering them. They are now walls the
harness puts up, in `.claude/hooks/guardrails.sh`.

[`.claude/hooks/README.md`](../.claude/hooks/README.md) owns the details — the
four modes, what each denies (the fourth denies nothing — it only reports),
the overrides and why they fail open — and is not repeated here. What is
worth pulling out for an outside reader is the one
design decision that makes the gate honest rather than decorative:

> Each marker `pr-gate` reads holds **a hash of the change the review
> covered** — `git diff origin/main...HEAD` — not a timestamp, not a boolean,
> and not the sha of whichever commit happened to carry it. Edit a tracked file
> after reviewing and the hash no longer matches, so the gate denies and names
> both values. There are two markers, one per review, because a branch that
> passed one has not passed the other.

Keying on the change rather than the commit is the second version of that
decision, and it was bought with real pain: the branch that introduced tiers
paid for five review rounds, several of which re-graded a diff nothing had
touched, because a rebase or an amend moved a sha the reviews did not care
about. It is also *stricter* in the case the sha form missed — a rebase that
lands a branch on top of upstream edits to the very files it changes now stales
the marker, which is exactly when a second look is worth most.

A marker that only recorded "a review happened" would pass while the newest
three commits went unreviewed — which is the failure this repository
actually hit. It still only proves a review was *recorded*, not that it was
*good*; nothing outside the review can prove the latter, and the hooks README
says so rather than implying otherwise.

## The mission archive

Each completed objective leaves its mission file behind as
`.claude/GOAL-archive-<slug>.md` — 45 of them at the time of writing. They are
not changelogs. A mission file records what the objective *was*, the decisions
taken with the trader and on what date, and the acceptance criteria the work
had to meet, all written before the code existed.

That makes them the design record git history cannot reconstruct: a commit
shows what changed, and the archived mission shows what question the change
was answering, and which alternatives were rejected in a conversation that
would otherwise have evaporated. When a later reader asks why the strategy's
audible alarm deliberately fires before the strategy could ever place an
order, the answer is in the mission file, in the trader's own reasoning.

## What this does not claim

The agents do not merge their own work; a human reviews and merges. The gates
catch classes of failure — drift, unreviewed commits, untested surfaces,
mouse-only capabilities — not all failure. And the process is only as good as
the acceptance criteria a mission writes down, which is a human judgement at
the start of every session.

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
one, on every edit, for the length of a mission. A one-package `cargo check`
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
`arch-review` dimension 1 asks whether a new capability can dock as a new file
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

**The review budget covers the chain, not each skill.** The old arrangement had
no total at all: `arch-review`'s step 0 was bounded at two, `delivery-review`
carried a separate three, the nine-dimension shape pass had none, and since
answering any of them is a commit — which stales both markers by design and
re-runs both reviews — nothing summed them. Measured before it was written:
ordinary code branches spend about one round, meta-work on the workflow itself
averages three, and the worst branch in the last twenty spent eight.

**Model routing is a standard, not a description.** A bug pass finds real
defects partly by being a strong model, which is the exception that pays for
routing everything else down. Stated wider than its use, a rule drifts before
anyone notices, so the count is worth being exact about: `delivery-review`'s
criteria pass is the only routed call site in the repository today, and the
`haiku` tier describes no existing dispatch at all.
