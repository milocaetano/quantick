---
name: ai-review
description: Review a diff the way an AI engineer would - modularity, decoupling, MCP and agent readiness, agent testability and legibility, extensibility, scalability. Use when the user types /ai-review. Posts each finding to a PR as its own resolvable thread; never edits or builds.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# AI engineer review

Target: `git diff origin/main...HEAD` by default, or the path, branch or PR
number given as argument. Read the code, not its description. Never edit source
or build. Only PR findings/report and the private completion record may be written.

Answer six questions. Each gets one verdict and `file:line` evidence, PASS
included. FAIL: the diff breaks the rule. WEAK: it holds today but the next
variant breaks it - name that variant. PASS: cite the line that proves it,
or say why the diff cannot reach the question.

1. **Is it modular?** One responsibility per unit, and the file name says
   which. Count the pre-existing files the diff edits and name the one with
   the most edited lines: that is the blast radius.
2. **Is it decoupled?** Judge what the compiler cannot: a consumer naming a
   concrete producer where a trait bound would do; state owned by the caller
   that the unit should own; a `pub` item nothing outside the crate calls.
3. **Is it ready for MCP and other AI use?** Every behaviour the diff adds
   is a named call with a typed request, a typed result and a typed error -
   never a `String` standing in for a failure - and calling it twice is safe.
   Evidence is the call site in the registry the UI reads; a click handler
   as the only entry is FAIL.
4. **Can agents test it and understand it?** Name the test that fails
   without the diff, runnable alone with `cargo test -p <crate> <name>`,
   below `app`, with an error-path fixture; an expectation derived from the
   code under test is FAIL. Hunt: `SystemTime`, `Instant`, `HashMap`
   iteration, `rand`, env reads, thread timing.
5. **Is it extensible?** The next variant - feed, bar type, indicator, tool -
   is added, not patched in. A closed `enum` where the variants are the
   domain; a trait object where the next variant is a capability. A registry
   keyed by name appends; one keyed by position makes every branch edit the
   same line.
6. **Does it scale?** State the rate of every touched loop - per trade, per
   depth update, per frame, rare. Per-event cost is independent of session
   length: a new trade updates the open bar, never rescans history. No
   buffer without a stated cap, no burst dropped in silence.

## Report

```
## AI review - <target> @ <sha>
Identity: full HEAD SHA; branch; worktree; base ref; full base tip; review key
Scope: full diff | follow-up; finding IDs/count; unresolved/accepted/fixed
1. Modular:      PASS|WEAK|FAIL - evidence
2. Decoupled:    ...
3. AI-ready:     ...
4. Agent-tested: ...
5. Extensible:   ...
6. Scalable:     ... - rate of the touched path
Top fix: <file> - <change> - flips: <verdicts>
```

## Where the findings go

With a PR number, post every FAIL and WEAK as its own resolvable thread -
severity first, anchored at `file:line`, one per finding, body on stdin to
`sh .claude/hooks/ai_review_threads.sh post <pr> <file> <line>`. With no PR
target, print the report and post nothing. Never apply a fix either way.

**Round one reviews the whole diff.** Missing prior report, changed base or
scope, or uncertain impact requires a full current review under the delivery
contract. Otherwise a later run takes its subject from `... list <pr>` and
verifies those open threads plus a narrow check that fixes introduced no new
FAIL; it may not open a new WEAK against code it already passed.

**A thread closes by the fix -
`... resolve <thread-id>` - or by an acceptance the trader records on it**; a
WEAK whose breaking variant you cannot name is a PASS. When to stop follows
[the delivery contract](../../../docs/workflow/delivery.md), for which an open
thread id is a finding's durable identity; the reasoning is
`docs/agentic-development.md`.

## Record completion

Readiness/merge require `ai-review-complete` at every tier. Completed reviews
with published findings may record it; unresolved threads still block.
Without a PR, print the report and record nothing.

Before review, capture the worktree and all `REVIEWED_*` values using the
identity commands below; save them outside the repository. Require clean
status and matching PR head SHA/branch and base ref/tip readbacks. Resolve all
commands in that worktree and review that exact diff under the rules above.

After all six dimensions and required finding publications complete, repeat
the identity reads and compare every value and clean status. Publish the
report above with `gh pr comment PR --body-file REPORT_PATH`; retain its URL
in the dossier. Recheck PR identity after publication too. Any changed value,
failed read or incomplete publication means no marker: reconcile and rerun.

Only then write the projection. Set `WT` and all `REVIEWED_*` variables from
the saved dossier in this same shell call; variables do not survive tool calls.
Never recalculate them from changed code to force a match.

```sh
cd "$WT" &&
  test "$(git symbolic-ref --quiet --short HEAD)" = "$REVIEWED_BRANCH" &&
  test "$(git rev-parse HEAD)" = "$REVIEWED_HEAD" &&
  test "$(sh .claude/hooks/campaign_context.sh base "$WT")" = "$REVIEWED_BASE" &&
  test "$(git rev-parse "$REVIEWED_BASE")" = "$REVIEWED_BASE_TIP" &&
  test "$(sh .claude/hooks/campaign_context.sh key "$WT")" = "$REVIEWED_KEY" &&
  RECHECK_STATUS=$(git status --porcelain=v1 --untracked-files=all) &&
  test -z "$RECHECK_STATUS" &&
  printf '%s %s\n' "$REVIEWED_BRANCH" "$REVIEWED_KEY" \
    > "$(git rev-parse --absolute-git-dir)/ai-review-complete"
```

The one LF-terminated line is `<branch> <shared-review-key>`. Branch/diff or
campaign ref/tip changes stale it; same-branch rewords retain the key. Never
copy another worktree's record or restamp after fixes without review. This
projection proves recording, not quality or GitHub provenance; authority is unchanged.
