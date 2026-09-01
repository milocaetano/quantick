# Agent guardrails

Three rules in CLAUDE.md were enforceable only by an agent remembering them:
work happens in a worktree, `arch-review` runs before the PR, and
`delivery-review` grades the branch against what was actually asked for. An
instruction in markdown is advice a long session can drift away from. A hook
is a wall. These make the harness enforce all three.

Wired in `.claude/settings.json`, implemented in `guardrails.sh` (POSIX sh, no
`jq`, so it runs under Git Bash on Windows and dash in CI) and covered by
`guardrails_test.sh`.

| Mode | Event | Acts on | Effect |
| --- | --- | --- | --- |
| `worktree-guard` | `PreToolUse` | `Edit`, `Write`, `NotebookEdit` | Denies the write when it lands in the main checkout while that checkout is on `main`. |
| `pr-gate` | `PreToolUse` | `Bash` | Denies `gh pr create` until **both** `arch-review-ok` and `delivery-review-ok` record the exact `HEAD` being shipped. |
| `commit-reminder` | `PostToolUse` | `Bash` | Cannot block (the commit already landed). After a `git commit` on a branch ahead of `origin/main`, says the gate is coming and names both markers. |
| `guard-watch` | `PostToolUse` | `Edit`, `Write` | Cannot block, and is not meant to. Runs the already-built `quantick-guards` binary over the file just written and reports what the repository guards found. Silent when nothing was found, when the binary has not been built, or when the file is outside a repository. |

## Why `guard-watch` never blocks and never builds

The other three modes are gates. This one is a courier.

The repository guards — the size ratchet, the language scan, the encoding
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

## Recording the two reviews

`pr-gate` reads two files in the worktree's git dir, each holding the commit
sha the review it names covered:

```sh
# after arch-review, its findings handled
WT=/path/to/worktree
cd "$WT" && git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"

# after delivery-review returns PASS
WT=/path/to/worktree
cd "$WT" && git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/delivery-review-ok"
```

They are separate files because they answer separate questions. `arch-review`
asks whether the branch is well built — shape, plus the bug pass its step 0
runs. `delivery-review` asks whether it is what was asked for, grading every
ask in the branch's goal file — `.claude/GOAL.md`, or the
`GOAL-archive-<slug>.md` it becomes, since the mandated order archives it
before either review runs — and every acceptance criterion in it. A
branch can pass either one while failing the other, so passing one is not
evidence about the other and the gate never treats it as such. The denial names
which marker is missing or stale — with two of them, "a review is missing"
would leave an agent guessing, and a wrong guess costs a whole review.

The files live in the worktree's own git dir, so they are per-branch and never
committed. Storing the sha rather than a timestamp is what makes the gate
honest: commit again after a review and that sha no longer matches, so the gate
denies and names both shas. A marker that only said "a review happened" would
pass while the newest commits went unreviewed.

Since `arch-review`'s step 0 runs the bundled `code-review` first, its marker
is meant to say both passes happened — correctness and shape. Meant to, not
proven to: the gate compares a sha and nothing more, so an agent that skips
step 0 still records a marker the gate accepts. What keeps that honest is the
review header, which names the level step 0 ran at or says it did not run. The
same hole exists for `delivery-review-ok`, and the same answer applies: its
verdict states the checklist source it graded against and what it checked that
could have failed.

The gate proves a review was *recorded*, not that it was *good*. Nothing can
prove the latter from outside the review. What it does remove is the failure
this repo actually hits: forgetting entirely, or reviewing and then pushing
three more commits.

### The gate runs from the main checkout

`.claude/settings.json` invokes the hook as
`${CLAUDE_PROJECT_DIR}/.claude/hooks/guardrails.sh`, and that variable points
at the session's project directory — the main checkout — not at whichever
worktree a command runs in. So a branch that *edits* `guardrails.sh` is still
gated by the copy on `main`: the change takes effect for sessions started after
it merges. Nothing about the gated branch is special; it just cannot test its
own gate through the hook, which is why `guardrails_test.sh` runs the script
directly.

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

`ship` step 6 is *not* one of them, and the distinction matters: it says to use
`gh pr create --body-file -` **with a heredoc**, and that spelling begins the
segment, so it does reach the gate. Both were measured rather than assumed — an
earlier draft of this paragraph named `ship` as the pipe example, which would
have told an auditor that the repo's own standard flow evades the gate.

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

It matches `Edit|Write|NotebookEdit`, so it guards writes made through the file
tools. A write driven from a shell — `Set-Content`, `sed -i`, `python` editing
a file in place, a heredoc redirect — reaches the main checkout unguarded, and
this repo's own notes make scripted edits routine. Same reasoning as above:
deciding "does this command write a file, and where" from a command string is a
larger version of the problem that would not converge for one named program.

## Fail-open by design

Anything `guardrails.sh` cannot determine exits 0 and the normal permission
flow applies: no `file_path` in the payload, a path outside a git repo, a
`git` invocation that errors. A guardrail that blocks the session over its own
bugs would be worse than no guardrail, and the rules it protects are also
written down in CLAUDE.md.

## Overrides

- `QUANTICK_ALLOW_MAIN_WRITES=1` in the environment before launching disables
  `worktree-guard`, for the rare deliberate edit on the main checkout.
- Paths under `.claude/` are always allowed on the main checkout, even while
  it sits on `main`: the goal file, its archives and the skills live there by
  design, and blocking them would break the workflow the guard exists to
  protect.

  **This is the guard's broadest hole and it is worth naming.** The exemption
  covers `.claude/hooks/guardrails.sh` and `.claude/settings.json` — the two
  files that arm every session — so a single `Edit` on the main checkout
  disarms both gates for every branch, with no worktree, no branch and no
  review. Narrowing it was tried on this branch and reverted: the carve-out
  guarded the script but not `settings.json`, and the version that guarded
  both still could not see a shell-driven write. It is documented rather than
  half-closed, which is the same call made about `runs_command` above.

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

Mutations run against the suite as it was built, each of which it catches:
neutering the arch-review staleness branch alone; swapping the order in which
the denial names the two reviews; renaming a marker constant in the script
without touching the prose; renaming it in the prose without touching the
script; swapping the two review skills' recording commands; and deleting the
delivery marker's check outright.

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
- Each review skill carries a recording command of its own, so `/arch-review`
  and `/delivery-review` each record their own marker instead of leaving it to
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
