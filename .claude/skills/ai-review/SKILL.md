---
name: ai-review
description: Review a diff the way an AI engineer would - modularity, decoupling, MCP and agent readiness, agent testability and legibility, extensibility, scalability. Use when the user types /ai-review. Reports findings; never edits, builds or posts.
---

# AI engineer review

Target: `git diff origin/main...HEAD` by default, or the path, branch or PR
number given as argument. Read the code, not its description. Read-only.

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

Findings only. The human decides; do not apply fixes.
