# Control plane roadmap

**Status:** Current. This document states no delivery status of its own.

This used to be a hand-kept ledger of which plan item was at which stage, in
which pull request. It stopped being true without anyone noticing: it recorded
PR 5c as open and PR 6 as not started for months after both had shipped, and an
agent that believed it either halted or set about re-implementing working code.
That whole ledger, sections 1 to 8 as they stood on 2026-08-26, is at
[`history/roadmap-2026-08.md`](history/roadmap-2026-08.md), where it is correct
as a record of that moment and claims nothing about today.

What replaced it is three pointers, none of which can go stale, because none of
them is a copy of something else.

| To find out… | Read |
| --- | --- |
| What the control plane can do **now** | [`capability-inventory.md`](capability-inventory.md) — generated from the registry, guarded against drift |
| What it is **meant** to do, and in what order | [the development plan](../mcp-control-plane-development-plan.md) — intent, not delivery |
| The **rules** any of it must satisfy | [`control-contract.md`](control-contract.md) and the ADRs beside it |
| Why a past decision was taken | [`history/`](history/) |

The generated inventory is the answer to "has this shipped?". A capability with
a row exists and is callable; one without a row does not exist, whatever any
document says. `cargo test -p quantick-guards` fails if that stops being true,
which is the difference between this arrangement and the one it replaced —
[Precedence](README.md#precedence) sets out why the code wins.

A reader who wants the same answer from a running instance rather than a file
can ask for it: `control.describe` returns the live registry over MCP, and
`quantick_search_capabilities` searches it.
