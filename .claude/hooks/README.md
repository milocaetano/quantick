# Agent guardrails

Three rules in CLAUDE.md were enforceable only by an agent remembering them:
work happens in a worktree, `arch-review` runs before the PR, and
`delivery-review` grades the branch against what was actually asked for. An
instruction in markdown is advice a long session can drift away from. A hook
is a wall. These make the harness enforce all three.

Wired for Claude Code in `.claude/settings.json` and for Codex in
`.codex/hooks.json`; both call `guardrails.sh` (POSIX sh, no `jq`) and are
covered by `guardrails_test.sh`. Codex project hooks require one-time trust in
its `/hooks` screen, as required by the
[Codex hooks contract](https://learn.chatgpt.com/docs/hooks).

| Mode | Event | Acts on | Effect |
| --- | --- | --- | --- |
| `worktree-guard` | `PreToolUse` | `Edit`, `Write`, `NotebookEdit`, `apply_patch` | Denies the write when it lands in the main checkout while that checkout is on `main`. |
| `pr-gate` | `PreToolUse` | `Bash`, `exec_command` | Denies non-draft creation/readiness/merge until `arch-review-ok`, applicable `delivery-review-ok`, and `ai-review-complete` match the exact change. Ready/merge also require each marker's durable current-review PR report and zero open AI threads. Only delivery has a bounded `small` exemption; draft creation is ungated. |
| `commit-reminder` | `PostToolUse` | `Bash` | Cannot block (the commit already landed). After a `git commit` on a branch ahead of `origin/main`, says the gate is coming and names the markers that branch's tier actually owes. |
| `guard-watch` | `PostToolUse` | `Edit`, `Write`, `apply_patch` | Cannot block, and is not meant to. Runs the already-built `quantick-guards` binary over each file just written and reports what the repository guards found. Silent when nothing was found, when the binary has not been built, or when the file is outside a repository. |

## Why `guard-watch` never blocks and never builds

Campaign children use the branch-bound `mission-base` record and shared
`campaign_context.sh` commands documented in
[`docs/campaign/integration.md`](../../docs/campaign/integration.md).
That contract overrides the main-only review examples below for those tasks.
The campaign key binds target ref/tip as well as diff. Invalid context fails
closed; ready verifies the PR base/head, and merge additionally requires a
persisted grant, current base, passing CI and the explicit head-pinned command.
Main merges, auto-merge and queue shortcuts are reserved for the user.
The small-tier exemption removes only delivery review, never AI completion, thread or merge
authority checks. `campaign_context_test.sh` exercises these boundaries with
real git fixtures and fake GitHub responses through both client payloads; the
main guardrail suite invokes it in CI. Existing command-detection limitations
still apply; GitHub branch protections are the security boundary.

The other three modes are gates. This one is a courier.

The repository guards — the size, context and cycle ratchets, the language scan, the encoding
check — are the cheapest checks in the codebase and used to be the slowest to
consult, because they lived under `crates/app/tests/` and cargo built the
largest crate in the repo before it could answer. Four minutes of link for
five seconds of work, on a warm `target/`. So a crossed ceiling surfaced at
the end of a full suite run, long after the edit that caused it, and the
author paid another full run to confirm the fix. Moving the guards into
`crates/guards`, which depends on nothing, is what made the answer cheap; this
mode is what makes it arrive on time.

Three properties keep it off the critical path, and all three are load-bearing:

- **It never denies.** `PostToolUse` cannot — the edit has already landed —
  and that is the right shape. `cargo test --workspace` is still the gate.
  Moving the *news* earlier is the whole intent; moving the *gate* earlier
  would make an advisory check into a thing that stops work over its own bugs.
- **It never invokes cargo.** It runs `target/debug/quantick-guards` directly.
  Going through `cargo run` would contend for the build lock with a build the
  agent is already running, and could trigger a compile of its own behind an
  edit. No binary means no output — the guards still run in the suite, so
  nothing is lost, only delayed to where it was before.
- **It reads one file.** `--file` checks that path against the baseline
  instead of walking the repo: about 40ms, against the few seconds a full scan
  costs. That is the difference between a hook you leave on and one you
  disable.

The relative path handed to the binary comes from `git rev-parse
--show-prefix`, not from subtracting the toplevel out of the absolute path.
Under Git Bash the payload spells a path the way the host writes it and
`--show-toplevel` answers the way git does, so `/tmp/x/src/a.rs` and
`C:/Users/.../Temp/x` describe the same tree and share no prefix. The
subtraction produced no match, which reads exactly like a clean file — the
failure mode this repo's guards exist to prevent, reproduced in the hook that
reports them.

## Where the gate sits, now that the chain has two phases

Phase one makes the branch work and ends at a **draft** PR. That PR is where
`ai-review` posts its findings, one resolvable thread each, so the gate cannot
sit in front of it: requiring the reviews before the draft exists would demand
the reviews before the review that informs them. A draft ships nothing — it
cannot be merged — so nothing is lost by letting it open.

Phase two closes those threads, and the gate moved to where work actually
leaves the branch: `gh pr ready` and `gh pr merge`. Both want what
`gh pr create` always wanted — the applicable review projections, for the exact
diff being shipped — plus their durable current-review PR reports and
independently zero unresolved AI threads.

The tier is read, never required, exactly as before.

### Recording AI completion

The canonical [AI-review skill](../skills/ai-review/SKILL.md#record-completion)
calls `review_report.sh`, the only documented producer. It verifies stable
worktree/branch/HEAD/status/base/key and PR identity, publishes and reads back
the durable report, then records the private `ai-review-complete` projection.
A bare restamp has no matching receipt and fails. A completed review with
findings may record completion, but the separate unresolved-thread check below
still denies readiness/merge.

The consumer is `pr-gate` in `guardrails.sh`: after the existing create return,
it passes the current branch and shared review key to `require_marker`, then
checks threads. The AI record is exactly one LF-terminated line,
`<branch> <shared-review-key>`. Legacy architecture/delivery records keep their
raw-key format and existing comparator. AI additionally binds its branch:
switching branches can leave a raw diff byte-identical, so the key alone cannot
satisfy that boundary. Detached heads, missing/malformed/stale records and a
record in another worktree cannot supply completion. Campaign keys already
bind base ref/tip; no second key algorithm is introduced.

Zero findings alone never proves completion. The report records full HEAD,
explicit base/ref tip, key, scope, six verdicts and finding IDs/count. The
marker is a local cache paired with that durable receipt; this detects a bare
manual marker but does not claim cryptographic proof of review quality.
Same-branch rewords with the same key remain valid. Changed
diffs or campaign bases require the canonical follow-up review before recording
new completion. Existing command limits and main authority remain unchanged.

### Counting the open threads

The gate does not know what an `ai-review` thread is. It asks
`ai_review_threads.sh`, the same script the reviewer used to post one, which
recognises a thread by the marker it writes at the head of the first comment.
One definition, two readers. A gate with its own definition would drift from
the reviewer's, and the two failure modes are both bad: ignoring real findings,
or blocking a merge on a human's ordinary question.

`count` has a machine contract because a gate reads it — a number on stdout and
exit 0, or nothing on stdout, a reason on stderr and **exit 2**. Exit 2 means
"could not be determined", which is not the same answer as zero.

### When the count cannot be taken

`gh` missing, unauthenticated or unreachable, or a command that names no PR
number: the gate emits an **`ask`** decision naming the reason. That keeps this
file's fail-open rule — `ask` blocks nothing a human does not block, and a
guardrail that stops a session over its own network is worse than no guardrail
— while refusing to fail open *silently*. "This branch has no open findings"
and "nobody knows whether it has any" are different answers, and a gate that
prints the same nothing for both has taught its reader that silence means
clean.

### The one prefix the pinned forms allow

A campaign merge and a campaign `gh pr ready` are pinned by string equality —
the explicit PR number, `--merge` or `--squash`, `--match-head-commit` with the
reviewed HEAD, and nothing else — so that no second statement can ride on the
first's authorization. That pin wanted the command bare, and the agent shell
cannot send it bare.

Claude Code's `Bash` tool resets its working directory between calls. Every
command therefore reaches its task worktree through a leading `cd <dir> &&`,
which is why `effective_dir` reads the directory from exactly there. Both
spellings were denied, and the gate was unsatisfiable in both directions: on
2026-09-11, integrating PR #382, `cd <wt> && gh pr merge 382 --merge
--match-head-commit 01a001c7…` was denied as a compound command, while the bare
form would have been judged against the main checkout and denied as a merge to
main.

So `bare_statement` strips **one** leading `cd <dir> &&` before the comparison,
reading the directory with the pattern `effective_dir` already uses — the
directory the gate judges and the directory it strips can never disagree. Only
the prefix is forgiven. A second `cd`, a `;`, a `||`, a trailing `&& echo x`,
`--auto`, `--admin` and `--repo` all survive the strip and fail the same
equality check they failed before, each with the message it had before. A
directory containing a space is outside that pattern in both functions, so the
honest form would be denied there — the worktrees this workflow creates under
`../quantick-worktrees/` never carry one, and widening the pattern is a change
with its own review.

**A gate you cannot satisfy is a gate with a hole behind it**, and that is the
whole reason this is worth a change rather than a workaround. The `Bash`
matcher does not cover Claude Code's PowerShell tool, so an agent that finds
the honest command denied is one keystroke from a spelling nothing inspects.
Running `gh pr merge` or `gh pr ready` through that tool is prohibited: a
denial is reported to the coordinator, never routed around.

## Publishing and recording the reviews

Each review skill writes its verdict to a scratch report and calls the one
producer from the clean reviewed worktree:

```sh
cd "$WT" && sh .claude/hooks/review_report.sh publish arch-review "$PR" "$REPORT_PATH"
cd "$WT" && sh .claude/hooks/review_report.sh publish delivery-review "$PR" "$REPORT_PATH"
cd "$WT" && sh .claude/hooks/review_report.sh publish ai-review "$PR" "$REPORT_PATH"
```

The producer publishes an identity-prefixed PR comment, reads it back, and only
then records `arch-review-ok`, `delivery-review-ok`, or
`ai-review-complete`. The gate requires both the current private projection and
the matching durable receipt; a manually written file therefore fails. Receipt
lookup is bounded to the latest 100 PR comments and fails closed when absent.

They are separate files because they answer separate questions. `arch-review`
asks whether the branch is well built — shape, plus the bug pass its step 0
runs. `delivery-review` asks whether it is what was asked for, grading every
ask in the branch's goal file — `.claude/GOAL.md`, or the
`GOAL-archive-<slug>.md` it becomes, since the mandated order archives it
before either review runs — and every acceptance criterion in it. A
branch can pass either one while failing the other, so passing one is not
evidence about the other and the gate never treats it as such. The denial names
which projection is missing or stale; a generic "review is missing" would
leave an agent guessing, and a wrong guess costs a whole review.

The files live in the worktree's own git dir, so they are per-branch and never
committed. Storing a hash of the change rather than a timestamp is what makes
the gate honest: edit a tracked file after a review and that hash no longer
matches, so the gate denies and names both values. A marker that only said "a
review happened" would pass while the newest work went unreviewed.

## Final mission and ship completion

`pr-gate` protects commands, not an agent's final sentence. A PR made
non-draft in another session never crosses `gh pr ready` again. Both canonical
workflows therefore finish with the same explicit command:

```sh
sh .claude/hooks/mission_ship_gate.sh mission <pr>
sh .claude/hooks/mission_ship_gate.sh ship <pr>
```

The caller names only its audit role; both modes execute the same policy. The
command synthesizes the normal readiness check, then independently requires an
open non-draft PR matching the clean worktree's branch/head/base, a mergeable
GitHub state, at least one registered CI check and every bucket
passing, current durable reports, and an empty literal thread list. It first
compares the sole archived goal in the reviewed diff with mission's canonical
four-line `G-AI` block. It then writes report URLs and the review key into the
PR body, reads the canonical `What done means` block, publishes each clause with its
evidence, reads that report back, and only then prints
`MISSION-COMPLETION:PASS`. Green CI or `MERGEABLE` alone cannot reach it.

**Why the change and not the commit.** Keying on `rev-parse HEAD` meant a
rebase, an amend or a reword stale a review that was still perfectly valid, and
the cost of that is real: the branch that introduced this paid for five review
rounds, several of which re-graded a diff nothing had touched. Keying on the
diff is also *stricter* in the one case the sha form missed — a rebase that
lands the branch on top of upstream edits to the very files it changes now
stales the marker, which is exactly the case most deserving of a second look.
`origin/main` advancing on its own does not stale anything, because the merge
base does not move when `HEAD` does not.

When the change cannot be identified at all — no `origin/main`, unrelated
histories — the gate falls back to the commit sha, which is what it keyed on
before. That is stricter than failing open, and such a checkout is outside this
workflow anyway: every review in it measures against that same ref.

Since `arch-review`'s step 0 runs the bundled `code-review` first, its durable
report names the level and findings before the producer accepts its PASS token.
The gate can prove that current report exists; it cannot prove the judgment was
good. The same boundary applies to `delivery-review-ok`: its durable verdict
states the checklist source and what could have failed, while the gate binds
that artifact to the exact review identity.

One tier changes what this section requires: see *The `small` mission
exemption* below. Everything above holds unchanged for every other branch.

The gate proves a review was *recorded*, not that it was *good*. Nothing can
prove the latter from outside the review. What it does remove is the failure
this repo actually hits: forgetting entirely, or reviewing and then pushing
three more commits.

### The gate runs from the session checkout

Both host configurations resolve `guardrails.sh` from the git root where the
session started, not from a worktree named inside a later command. A branch
editing the script therefore tests it directly with `guardrails_test.sh`; the
hook behavior changes for sessions started from that branch or after it merges.

## Why the filtering lives in the script

Hook config supports an `if` field with permission-rule syntax to narrow a
matcher (`"if": "Bash(gh pr create:*)"`). These hooks do not use it. A matcher
that silently fails to match leaves a gate that looks armed and is not, which
is worse than no gate, and the exact syntax was not verifiable from the docs.
Each mode inspects the tool payload itself and exits immediately when the
command is not its business, so the cost on unrelated calls is one `sed` and
the behaviour is covered by tests.

### What `pr-gate` can and cannot see

`runs_command` splits a command on `&&`, `||` and `;` and anchors the match to
the start of a segment. Spellings that put the gated command somewhere else are
not detected: a pipe (`cat body.md | gh pr create --body-file -`), a newline,
an `env`/`time`/`sudo` wrapper, a `VAR=value` prefix, `bash -c '…'`, a brace
group, an absolute path.

`ship` is *not* one of them, and the distinction matters. Its step 3 spells
`gh pr create --draft --body-file -` **with a heredoc**, and step 6 spells
`gh pr ready`; both begin their segment, so both reach the matcher. Step 3 is
then exempted for being a draft, and step 6 is the one that faces the markers.
That was measured rather than assumed — an earlier draft of this paragraph
named `ship` as the pipe example, which would have told an auditor that the
repo's own standard flow evades the gate.

Both halves are load-bearing, so they move when `ship` does. Renumbering it
once left this paragraph naming a step that had stopped prescribing any
spelling, and reporting a draft creation as though it were the gated command.

That is a real gap and it is deliberately left as it was rather than deepened
here. Closing it by parsing harder was tried, over eight review rounds, and did
not converge: each round shut some spellings and opened others, and twice
produced a denial whose own remedy — telling the agent to record markers in the
main checkout's shared git dir — would have disabled the gate permanently for
every later branch. A half-parser that looks airtight is worse than a narrow
one that is written down, because only the first kind gets trusted.

Widening it is its own change with its own review. What this gate is for is the
failure the repo actually hits: forgetting the review entirely, or reviewing
and then pushing three more commits.

### What worktree-guard does not see

It guards Claude file tools and Codex `apply_patch`; for the latter it reads
every Add, Update, Delete, or Move destination header from
`tool_input.command`. A write driven from a shell — `Set-Content`, `sed -i`, a
script, or a redirect — remains unguarded because reliably deriving its targets
requires parsing the shell.

## Fail-open by design

Anything `guardrails.sh` cannot determine exits 0 and normal permissions apply:
no edit path or patch header, a path outside a repository, or a failed `git`
query. The rules it protects are also written in `CLAUDE.md`.

## Overrides

- `QUANTICK_ALLOW_MAIN_WRITES=1` in the environment before launching disables
  `worktree-guard`, for the rare deliberate edit on the main checkout.
- `.claude/GOAL.md` is always allowed because it is untracked working state.
  Archives, skills, settings, and hooks are tracked artifacts and receive the
  normal worktree protection.

- `pr-gate` has **no override**, and that is a real cost rather than a design
  boast. `runs_command` matches the gated command at the start of *any*
  `&&`/`||`/`;` segment, so a command that merely quotes the workflow is denied
  too. Measured: a `git commit -m` whose message describes the flow after a
  separator is denied, and so is an `echo … >> notes.md` that does the same.
  Only a mention with no separator in front of it gets through.

  The workaround is to keep the phrase out of the *command*: `git commit -F
  <file>` with the message in a file, or write prose through the file tools
  rather than a shell redirect. Several commits on the branch that added this
  paragraph had to be made exactly that way, including the one adding it.

  A skip file was tried and reverted. It worked, but the denial that printed
  its creation command was the same denial an agent sees when it simply has
  not run the reviews — which hands the kill switch to precisely the caller
  with a motive to use it, permanently and for the whole branch. An override
  scoped to the command that tripped it, rather than to the branch, is the
  shape worth building. It is not built here.

  The `small` tier below is **not** that override and does not reopen this
  question: it exempts delivery review, on a bound the branch has to
  meet, and it is never mentioned by a denial to a branch that did not already
  declare it. The section says why that distinction is load-bearing.

## The `small` mission exemption

`mission` declares a tier for a branch by writing **the branch's own name and
the tier** into `mission-tier`, beside the review projections in the worktree's git
dir — so it is per-branch, never committed, and discovered exactly the way the
markers are. Both fields are required, and a one-field file is refused; the
paragraph below the snippet says why:

```sh
WT=/path/to/worktree
TIER=medium                     # small | medium | high | max
cd "$WT" &&
  printf '%s %s\n' "$(git rev-parse --abbrev-ref HEAD)" "$TIER" \
    > "$(git rev-parse --absolute-git-dir)/mission-tier"
```

`TIER` is a variable and defaults to `medium` here on purpose. This section is
where both skills forward a reader looking for the mechanism, and a snippet
whose literal value is the one word that switches the gate off is a snippet
someone pastes verbatim onto a branch that never declared it.

The file holds **`<branch> <tier>`**, and the branch half is load-bearing: the
review projections go stale when the branch moves, while a bare
tier word would outlive its mission. A worktree reused for a second branch then
inherits an exemption it never asked for — measured, not imagined, on the first
version of this feature. A declaration naming another branch grants nothing,
and so does the one-field format that caused it.

The tiers are `small`, `medium`, `high` and `max`, and only `small` changes
what this gate requires: delivery review is omitted; architecture and AI remain.
Every other tier, an unrecognised word, a declaration for another branch, and
an absent file leave the full three-review gate in force — which is every
branch that existed before this file did.

That is a hole in a gate deliberately built without an override, so it is worth
being exact about why it is not the skip file that got reverted.

- **The denial never advertises it.** A branch that declared no tier gets the
  message it has always got, naming neither tiers, nor the file, nor the word
  `small`. `guardrails_test.sh` asserts that absence rather than trusting it,
  because it is the kind of property a later edit breaks by being helpful.

  **This is narrower than "the gate cannot teach its own way around itself",
  and that stronger claim would be false.** `CLAUDE.md` is loaded in every
  session and states the exemption; `arch-review`'s step 0 carries the runnable
  command that reads the tier and `ship` step 4 says to read it from that file
  rather than from prose; this very section carries a writable snippet. The
  mechanism is documented on purpose — one nobody can find is one nobody can
  audit — so what the denial buys is only that an agent which merely *forgot*
  the review is not handed the bypass at the moment it is most tempted. The
  load-bearing protection is the bound below, not the silence.
- **The word has to be true.** The exemption lapses once the branch exceeds
  `SMALL_TIER_MAX_CHANGED_LINES` changed lines — insertions plus deletions
  against `origin/main` — and past that the branch pays in full whatever the
  file says. So writing `small` dishonestly at PR time buys the exemption only
  on branches where writing it honestly would have been allowed anyway. That is
  the argument a skip file could never make, and it is the whole design.
- **It is one exemption, not all reviews.** Architecture and AI are required at every tier. A
  tier buys a shorter bug pass, never no bug pass: a small diff is not a safe
  one, and three lines is a perfectly good size for a crash.
- **It fails closed.** When the branch's size cannot be measured — no
  `origin/main`, a `git` that errors — there is no exemption. This is the one
  place in `guardrails.sh` that deliberately breaks the *Fail-open by design*
  rule stated above: everywhere else an undetermined answer costs a permission
  prompt, and here it would cost an ungraded branch. It is why the size is read
  from `git diff --numstat` under `LC_ALL=C` rather than from `--shortstat`,
  whose prose git translates — an English pattern over a localised line matches
  nothing, sums to zero, and reads as an empty diff.

What is **not** claimed: nothing stops an agent from declaring `small` on a
branch that was small and still deserved grading, and nothing here could. The
bound holds the blast radius down to what the tier's honest use already allows.
It is a limit on the damage, not a proof of good faith.

## Tests

```sh
sh .claude/hooks/guardrails_test.sh
```

Builds throwaway git repos under a temp dir, exercises all four modes
including the fail-open paths, and cleans up after itself. CI runs it as its
own step. Intact, it reports zero failures.

The check that the cases test the behaviour and not the harness: replace
`guardrails.sh` with `exit 0` and roughly half the cases go red. No exact
total is written down here on purpose — it moves with every case added, CI
compares it against nothing, and a number nobody verifies is a line that
quietly becomes false. The neutered run's total differs from the intact one
besides, which is worth knowing before it sends someone bug-hunting: a
neutered script defines no `*_MARKER_NAME` constants, so the loops that
iterate them have nothing to iterate and their cases vanish, while the "no
`MARKER_NAME` constants found" case appears in their place. Count failures,
not the denominator.

Mutations run against the suite as built, including a neutered architecture
staleness check, marker-name drift, a removed delivery check and a missing
shared producer call.

The ordering case asserts that the denial with neither review recorded names
`arch-review-ok` — the review that runs first, since a delivery review of a
branch the shape review is about to change is wasted work. Swap the two
`require_marker` calls and that case goes red.

The `pr-gate` cases move one marker at a time — arch-review satisfied and
delivery-review absent, and the reverse — because the failure worth catching
is one gate silently carrying the branch through for the other. Those cases
assert the text of the denial, not only that it denied.

What they deliberately do **not** cover is the set of command spellings the
matcher cannot see. There is no case pinning that a pipe or a newline reaches
the gate, because neither does — see the gap section above. A test asserting
otherwise existed briefly, against a parser that was reverted, and a claim in
this file outlived it by a commit; that is the failure mode this paragraph is
here to prevent, since a documented coverage claim is read as coverage.

### What the last block actually proves

It leaves the fixture repos alone and reads this repository, which is the one
place the suite is not hermetic — see the note in its header and in `ci.yml`.
Three properties, and it is worth being exact about them because an
overstatement here is a false sense of cover:

- Every marker `guardrails.sh` defines is named by **each** of this file,
  `mission` and `ship` — per file, not "somewhere among them". Checking the
  set would stay green while the instruction vanished from two of the three.
- Each review skill carries a recording command of its own, so `/arch-review`,
  `/delivery-review` and `/ai-review` record their own markers instead of leaving them to
  a caller. That asymmetry was a real bug here.
- Every marker name the prose tells an agent to **write** is one the script
  reads. This is anchored on the recording command's shape rather than on the
  marker names, because the first version grepped for the current names to
  decide which files to inspect — so a file that renamed its marker dropped
  out of the set, and the one-sided rename it exists to catch went green.

## If a hook does not fire

The command hooks need `sh` on `PATH`; on Windows that comes with Git. Check
with `sh --version`. Hook configuration is read at session start, so an edit
to `settings.json` needs a new session before it takes effect.
