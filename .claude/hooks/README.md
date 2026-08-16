# Agent guardrails

Two rules in CLAUDE.md were enforceable only by an agent remembering them:
work happens in a worktree, and `arch-review` runs before the PR. An
instruction in markdown is advice a long session can drift away from. A hook
is a wall. These make the harness enforce both.

Wired in `.claude/settings.json`, implemented in `guardrails.sh` (POSIX sh, no
`jq`, so it runs under Git Bash on Windows and dash in CI) and covered by
`guardrails_test.sh`.

| Mode | Event | Acts on | Effect |
| --- | --- | --- | --- |
| `worktree-guard` | `PreToolUse` | `Edit`, `Write`, `NotebookEdit` | Denies the write when it lands in the main checkout while that checkout is on `main`. |
| `pr-gate` | `PreToolUse` | `Bash` | Denies `gh pr create` until arch-review is recorded for the exact `HEAD` being shipped. |
| `commit-reminder` | `PostToolUse` | `Bash` | Cannot block (the commit already landed). After a `git commit` on a branch ahead of `origin/main`, says the gate is coming. |

## Recording an arch-review

`pr-gate` reads `<git-dir>/arch-review-ok`, which holds the commit sha the
review covered. After running the skill and handling its findings:

```sh
git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"
```

The file lives in the worktree's own git dir, so it is per-branch and never
committed. Storing the sha rather than a timestamp is what makes the gate
honest: commit again after the review and the sha no longer matches, so the
gate denies and names both shas. A marker that only said "a review happened"
would pass while the newest commits went unreviewed.

Since the skill's step 0 runs the bundled `code-review` first, recording the
marker is meant to say both passes happened — correctness and shape. Meant to,
not proven to: the gate compares a sha and nothing more, so an agent that skips
step 0 still records a marker the gate accepts. What keeps that honest is the
review header, which names the level step 0 ran at or says it did not run.

The gate proves a review was *recorded*, not that it was *good*. Nothing can
prove the latter from outside the review. What it does remove is the failure
this repo actually hits: forgetting entirely, or reviewing and then pushing
three more commits.

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
own step. Neutering `guardrails.sh` to `exit 0` fails five of the fifteen
cases, which is the check that they test the behaviour and not the harness.

## If a hook does not fire

The command hooks need `sh` on `PATH`; on Windows that comes with Git. Check
with `sh --version`. Hook configuration is read at session start, so an edit
to `settings.json` needs a new session before it takes effect.
