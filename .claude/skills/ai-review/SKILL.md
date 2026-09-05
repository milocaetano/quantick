---
name: ai-review
description: Review a diff the way an AI engineer would - modularity, decoupling, MCP and agent readiness, agent testability and legibility, extensibility, scalability. Use when the user types /ai-review. Posts each finding to a PR as its own resolvable thread; never edits or builds.
---

# AI engineer review

Target: `git diff origin/main...HEAD` by default, or the path, branch or PR
number given as argument. Read the code, not its description. Never edit or
build; a PR's threads are the one thing this writes.

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

Two rules, both binding. **Round one reviews the whole diff; every later run
takes its subject from `... list <pr>` and verifies only those open threads,
plus a narrow check that the fixes introduced no new FAIL. It may not open a
new WEAK against code it already passed.** And **a thread closes by the fix -
`... resolve <thread-id>` - or by an acceptance the trader records on it**; a
WEAK whose breaking variant you cannot name is a PASS. `CLAUDE.md`'s stall rule
owns when to stop; the reasoning is `docs/agentic-development.md`.
