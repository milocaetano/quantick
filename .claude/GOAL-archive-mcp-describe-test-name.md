# Mission: name the refused-describe test for what it asserts

Rename the `quantick-mcp` test `a_refused_describe_leaves_the_first_version_rather_than_its_error`
to `a_refused_describe_is_returned_and_nothing_is_invoked`, because since #451
a refused `control.describe` returns its own error and invokes nothing, which is
the opposite of what the old name says. A test name that contradicts its
assertion misleads every reader who greps for the behaviour.

**Tier:** `small`. One identifier in one test module, no behaviour change, no
question that is the trader's. `delivery-review` is exempt within the small-tier
diff ceiling (2 of 300 changed lines for the code commit).

Campaign fix for the #449 architecture follow-up Should-fix
(https://github.com/milocaetano/quantick/pull/449#issuecomment-5655740272),
campaign #367 (https://github.com/milocaetano/quantick/issues/367); base
`campaign/lean-a-plus`.

## Request ledger

- **R1** — the test at `crates/mcp/src/tools.rs:1336` is named
  `a_refused_describe_is_returned_and_nothing_is_invoked`. *("rename the test")*
- **R2** — its doc comment changes only if it mentions the old name. *("Adjust
  its doc comment only if it mentions the old name")*
- **R3** — existing `.claude/GOAL-archive-*.md` files citing the old name stay
  untouched, and nothing else is in scope. *("they are history"; "Nothing else
  is in scope")*

## Assumptions

- **S1** — the doc comment does not mention the old name (read at
  `tools.rs:1330-1334`), so it stays as is. Safe: R2 makes the change conditional.
- **S2** — the rename is not engine or determinism territory, so no test-first
  split applies. Safe: no fixture or production code changes.

## Acceptance criteria

- [x] **A1** — `git grep a_refused_describe_leaves_the_first_version_rather_than_its_error`
      outside `.claude/GOAL-archive-*.md` returns nothing, and
      `cargo test -p quantick-mcp` runs
      `tools::tests::a_refused_describe_is_returned_and_nothing_is_invoked ... ok`.
      *Evidence:* the test log line and the grep. → PR body. *(R1)*
- [x] **A2** — the diff against `origin/campaign/lean-a-plus` touches only the
      `fn` line in `crates/mcp/src/tools.rs` plus this archive; no archive that
      existed before is modified. *Evidence:* `git diff --stat`. → PR body.
      *(R2, R3)*
- [x] **G1** — every artifact in English. *Evidence:* `arch-review`
      dimension 8 and `cargo test -p quantick-guards`. → arch-review report.
- [x] **G2** — four checks green: `cargo fmt --all -- --check`,
      `cargo clippy -p quantick-mcp --all-targets`, `cargo test -p quantick-mcp`,
      `cargo test -p quantick-guards` locally; the full workspace in CI.
      *Evidence:* exit codes; CI checks at the head. → PR body.
- [x] **G3** — performance impact declared: none; the only touched path is a
      test function name (rate: never at runtime). → PR body.
- [ ] **G4** — `arch-review` run with every Blocker/Should-fix resolved.
      *Evidence:* the published report. → PR comment.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence destinations: the `ai-review` report comment on the PR, the
`ai_review_threads.sh list` output, and the `ai-review-complete` projection for
the campaign review key.

## Closing steps

- **C1** — the PR is open, non-draft, CI green at its head.
- **C2** — `mission_ship_gate.sh ship <pr>` prints PASS.

## Not applicable

- Hot path, user-visible, new capability, trader action: the diff renames a
  test only.
- Engine / determinism: no engine code.
- `delivery-review`: `small` tier, within the diff ceiling.

## Verbatim request

> Tiny campaign fix, mission tier **small**, through this repo's `/mission`
> workflow: resolve the one remaining Should-fix of the consolidated PR #449's
> architecture follow-up (https://github.com/milocaetano/quantick/pull/449#issuecomment-5655740272),
> campaign #367 (https://github.com/milocaetano/quantick/issues/367).
>
> **Change:** rename the test `a_refused_describe_leaves_the_first_version_rather_than_its_error`
> at `crates/mcp/src/tools.rs:1336` to `a_refused_describe_is_returned_and_nothing_is_invoked`.
> It now asserts the opposite of its old name, since #451 made a refused
> `control.describe` return its own error and invoke nothing. Adjust its doc
> comment only if it mentions the old name. Leave existing
> `.claude/GOAL-archive-*.md` files that cite the old name untouched; they are
> history. `git grep` confirms no other non-archive reference. Nothing else is
> in scope.
>
> — the campaign coordinator, relaying the trader's campaign #367
