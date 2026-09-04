# The archaeology relocation (A9 — R12, R13)

## What moved, and nothing deleted
```
docs/control-plane/history/
capability-inventory-2026-08.md
pr2-performance.md
pr3-gateway-evidence.md
pr4-mcp-evidence.md
pr5a-events-evidence.md
pr5b-annotate-evidence.md
pr5c-evidence.md
roadmap-2026-08.md
```

Eight files. The six `pr*-evidence.md` the brief named, plus:

- `roadmap-2026-08.md` — the roadmap's sections **1 to 8**. The brief named
  sections 2-5; sections 6 to 8 came with them because they cannot stand
  without section 5 (section 6 is titled "Gates for every pull request in
  section 5") and because 7 and 8 state the delivery status of a stack that
  has since closed. Section 1's table was the document's most-read half and
  its least true — it recorded PR 5c as open and PR 6 as not started long
  after both shipped.
- `capability-inventory-2026-08.md` — the PR 0 migration baseline the
  generated inventory replaced. Kept because it records the migration target
  chosen for each `QUANTICK_*` surface, which is a decision record the
  generated file does not carry and should not.

## The banner every one of them carries
```
> **Archaeology, not current state.** This document records what was true
> when it was written and is kept for the reasoning it carries. For what
> has shipped, ask the registry — see [Precedence](../README.md#precedence).
```

## No document outside history/ states a delivery status the registry contradicts

`roadmap.md` was rewritten to state none at all — it now points at the
generated inventory for what exists, the development plan for what is
intended, and `history/` for why. The two claims the brief cited are gone
with it:
```
docs/control-plane/README.md:78:scene, PR 5b, PR 5c). It adds no decisions; when it and the plan disagree, the
docs/control-plane/roadmap.md:7:PR 5c as open and PR 6 as not started for months after both had shipped, and an
```

Both capabilities the stale table denied are in the generated inventory:
```
| `evidence.capture` | 1 | `evidence` | yes | `observe`, `observe.evidence` |
| `evidence.read` | 1 | `evidence` | yes | `observe`, `observe.evidence` |
| `layout.pane.move` | 1 | `layout` | no | `cockpit`, `cockpit.layout` |
| `trade.order.place` | 1 | `trade` | no | `trade` |
```

## Every relative link still resolves
```
broken relative links in docs/: 0
```
