# Documentation index

The documents in this tree, grouped by what they are for. Design records
state their own status in their first lines. **No document here tells you what
has shipped** — for that, ask the code: the registry under
`crates/app/src/control/` is authoritative for what exists, and the two indexes
that describe it are generated from it rather than written beside it. What a
document does own is the rules the code must satisfy, and
[`control-plane/README.md`](control-plane/README.md) §Precedence draws that
line for the control plane: the contract owns wire rules, the code owns what is
registered.

New here? [`../README.md`](../README.md) is the project introduction and
[`../AGENTS.md`](../AGENTS.md) is the entry point for an AI agent — reading
the repository or driving the running application over MCP.

## The control plane and the MCP adapter

How an agent operates Quantick: the wire contract, the transport, the trust
boundary and the evidence each delivery left behind.

| Document | What it settles |
| --- | --- |
| [`control-plane/README.md`](control-plane/README.md) | The directory's own index, its precedence order and its change policy |
| [`mcp-control-plane-development-plan.md`](mcp-control-plane-development-plan.md) | Scope and delivery sequence — it owns ordering; the contract owns details |
| [`control-plane/control-contract.md`](control-plane/control-contract.md) | Identifier, schema, revision, authority, limit, tool-surface, determinism and trade-annotation rules |
| [`control-plane/adr-0001-local-transport-and-instance-discovery.md`](control-plane/adr-0001-local-transport-and-instance-discovery.md) | Why the transport is an authenticated loopback socket and how a client discovers a running instance |
| [`control-plane/observer-threat-model.md`](control-plane/observer-threat-model.md) | Assets, trust boundaries, threats and required controls for the first profile |
| [`control-plane/capability-inventory.md`](control-plane/capability-inventory.md) | Every registered capability — identifier, version, module, permissions. Generated from the registry and guarded against drift |
| [`control-plane/roadmap.md`](control-plane/roadmap.md) | Where to look for what shipped, what is planned and what the rules are — it states no delivery status itself |

The per-PR evidence records — what each delivery proved and what it carried
forward — are archaeology, kept under `control-plane/history/`: [`pr2-performance.md`](control-plane/history/pr2-performance.md),
[`pr3-gateway-evidence.md`](control-plane/history/pr3-gateway-evidence.md),
[`pr4-mcp-evidence.md`](control-plane/history/pr4-mcp-evidence.md),
[`pr5a-events-evidence.md`](control-plane/history/pr5a-events-evidence.md),
[`pr5b-annotate-evidence.md`](control-plane/history/pr5b-annotate-evidence.md) and
[`pr5c-evidence.md`](control-plane/history/pr5c-evidence.md).

The generated wire schemas live outside this tree, in
[`../schemas/control/`](../schemas/control/).

## Building the project with agents

The [architecture preparation plan](architecture/foundation.md) records the
audited ownership boundaries, [ordered refactoring tasks](architecture/preparation-tasks.md)
and [baseline evidence](architecture/baseline.md) for future extensible content,
external agents and window-independent workspaces. It is a design milestone,
not a claim that those features have shipped.

| Document | What it covers |
| --- | --- |
| [`agentic-development.md`](agentic-development.md) | The skills, the review gates and the hooks that enforce them — how work actually moves from objective to merged PR here |
| [`../CLAUDE.md`](../CLAUDE.md) | The working rules, authoritative for any agent changing this repository |
| [`../.claude/hooks/README.md`](../.claude/hooks/README.md) | The guardrail hooks — the four modes, which three are gates and which one only reports, why they fail open, and how to override them |

## Scripting and indicators

| Document | What it covers |
| --- | --- |
| [`pine-dialect.md`](pine-dialect.md) | The Quantick Pine reference: the Pine v5 subset, the order-flow builtins, what is deliberately absent |
| [`indicator-system-plan.md`](indicator-system-plan.md) | The indicator runtime's design record (implemented, M1–M5) |

## Chart and order-flow design

| Document | What it covers |
| --- | --- |
| [`footprint-design.md`](footprint-design.md) | Footprint / candle tape reading, distilled from a survey of ATAS, Sierra Chart, Bookmap, exocharts and Quantower |
| [`order-flow-and-cross-venue-execution-ideas.md`](order-flow-and-cross-venue-execution-ideas.md) | Research note on cross-venue execution — not an approved implementation plan |

## User experience

| Document | What it covers |
| --- | --- |
| [`ux/ui-design-model.md`](ux/ui-design-model.md) | The shell: status bar, toolbar, icon set, right dock, tool rail (phases 1–5 implemented) |
| [`ux/paper-trading.md`](ux/paper-trading.md) | The paper-trading surface: drag-to-create brackets, the trades ledger |
| [`ux/strategy-anchors.md`](ux/strategy-anchors.md) | The semi-automatic operation and the division of labour between trader and machine |
| [`drawing-toolbar-ux.md`](drawing-toolbar-ux.md) | The drawing toolbar and inspector redesign (specification) |
| [`ux/drawing-tools-2026-08.md`](ux/drawing-tools-2026-08.md) | The design review that reshaped the drawing rail into a price-action toolbox |
| [`ux/drawing-tools-ux-spec.html`](ux/drawing-tools-ux-spec.html) | The detailed interaction target for user-authored chart objects: toolbox, non-modal inspector, selection, lock/visibility/delete semantics, keyboard grammar and the Fibonacci level editor. Pre-dates the English rule and is allowlisted by the language guard |
| [`ux/img/`](ux/img/) | The shell diagrams `ui-design-model.md` refers to as part of the spec, not decoration |
| [`ux/ux-audit-2026-08.md`](ux/ux-audit-2026-08.md) | The August 2026 audit of the whole `crates/app` surface — six reviewers, one report per area in [`ux/ux-audit-2026-08/`](ux/ux-audit-2026-08/) |
