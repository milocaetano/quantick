---
name: ai-review
description: Review a diff the way an AI engineer would - modularity, decoupling, MCP and agent readiness, agent testability and legibility, extensibility, scalability. Use when the user types /ai-review, or asks whether code is modular, decoupled, agent-ready, extensible or scalable. Reports findings; never edits.
---

# AI engineer review

Target: `git diff origin/main...HEAD` by default, or the path, branch or PR
number given as argument. Read the code, not its description.

Answer six questions. Each gets one verdict - PASS, WEAK or FAIL - and
`file:line` evidence. No praise, no restating the diff.

1. **Is it modular?** One responsibility per unit. A new capability lands as
   a new file plus one registration line, not edits across the trunk.
2. **Is it decoupled?** Dependencies point one way, no reverse edge, no module
   cycle, no hidden shared state. A change here does not force a change there.
3. **Is it ready for MCP and other AI use?** Every behaviour is reachable as a
   named call with typed input and a readable result, so MCP, a bot or a
   script drives it without the UI.
4. **Can agents test it and understand it?** Deterministic and headless -
   fixture in, expected out. Names and doc comments state intent, so an agent
   finds and tests the unit without reading the whole crate.
5. **Is it extensible?** The next variant - feed, bar type, indicator, tool -
   is added, not patched in. No `match` every new case must grow.
6. **Does it scale?** Cost grows with the data, not with the feature count.
   No quadratic pass over trades or bars, no per-frame allocation, no
   unbounded growth.

## Report

```
## AI review - <target>
1. Modular:      PASS|WEAK|FAIL - evidence
2. Decoupled:    ...
3. AI-ready:     ...
4. Agent-tested: ...
5. Extensible:   ...
6. Scalable:     ...
Top fix: the one change that raises the most verdicts
```

Findings only. The human decides; do not apply fixes.
