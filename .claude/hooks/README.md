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

## Recording the two reviews

`pr-gate` reads two files in the worktree's git dir, each holding the commit
sha the review it names covered:

```sh
# after arch-review, its findings handled
git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"

# after delivery-review returns PASS
git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/delivery-review-ok"
```

They are separate files because they answer separate questions. `arch-review`
asks whether the branch is well built — shape, plus the bug pass its step 0
runs. `delivery-review` asks whether it is what was asked for, grading every
ask in `.claude/GOAL.md`'s request ledger and every acceptance criterion. A
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

## Fail-open by design

Anything `guardrails.sh` cannot determine exits 0 and the normal permission
flow applies: no `file_path` in the payload, a path outside a git repo, a
`git` invocation that errors. A guardrail that blocks the session over its own
bugs would be worse than no guardrail, and the rules it protects are also
written down in CLAUDE.md.

## Overrides

- `QUANTICK_ALLOW_MAIN_WRITES=1` in the environment before launching disables
  `worktree-guard`, for the rare deliberate edit on the main checkout.
- Paths under `.claude/` are always allowed: the goal file, its archives, the
  skills and these hooks live in the main checkout by design, and blocking
  them would break the workflow the guard exists to protect.

## Tests

```sh
sh .claude/hooks/guardrails_test.sh
```

Builds throwaway git repos under a temp dir, exercises all three modes
including the fail-open paths, and cleans up after itself. CI runs it as its
own step. Neutering `guardrails.sh` to `exit 0` fails 12 of the 30 cases, which
is the check that they test the behaviour and not the harness.

The `pr-gate` cases move one marker at a time — arch-review satisfied and
delivery-review absent, and the reverse — because the failure worth catching is
one gate silently carrying the branch through for the other. Those cases assert
the text of the denial too, not only that it denied.

The last block leaves the fixture repos alone and checks this repo instead:
that every `*_MARKER_NAME` in `guardrails.sh` is also named by this file, by
`mission` and by `ship`. A marker name is a value the script and the prose must
agree on across a boundary nothing type-checks, and renaming one side alone
would deny a branch whose review actually ran while handing back a recording
line that does not fix it. The names are read out of the script rather than
repeated in the test, so the test follows a rename instead of pinning the old
spelling.

## If a hook does not fire

The command hooks need `sh` on `PATH`; on Windows that comes with Git. Check
with `sh --version`. Hook configuration is read at session start, so an edit
to `settings.json` needs a new session before it takes effect.
