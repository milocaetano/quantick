# Control plane architecture contract

This directory contains the decisions required before implementation of the
Quantick control plane can begin. The development plan describes the intended
outcome and delivery sequence; the files here make the PR 0 decisions
testable.

## Documents

- [Capability inventory](capability-inventory.md) records every production
  `QUANTICK_*` surface currently present in `crates/app/src`, its owner, and
  its migration target.
- [Control contract](control-contract.md) fixes identifier, schema, revision,
  authority, limit, tool-surface, determinism, and trade-annotation rules.
- [ADR 0001](adr-0001-local-transport-and-instance-discovery.md) selects the
  local transport and running-instance discovery mechanism.
- [Observer threat model](observer-threat-model.md) defines the assets, trust
  boundaries, threats, and required controls for the first profile.
- [PR 2 performance evidence](pr2-performance.md) records the shared-host idle
  frame comparison and the calibrated on-demand capture budget.
- [PR 3 gateway evidence](pr3-gateway-evidence.md) records the authenticated
  loopback topology, discovery controls, stable failures, frame-budget test,
  and focused verification.
- [PR 4 MCP adapter evidence](pr4-mcp-evidence.md) records the adapter's tool
  surface, instance selection, authority boundary, stdout discipline and the
  tests that prove them.
- [PR 5a events evidence](pr5a-events-evidence.md) records the event journal,
  the cursor and parked waits, the mark action and hotkey, and the durable
  control trace.
- [PR 5b annotate evidence](pr5b-annotate-evidence.md) records the annotate
  and notify tier: what the trader grants, the acceptance table, the resolve
  step the trace needed, and the blast radius.

## Precedence

The [development plan](../mcp-control-plane-development-plan.md) owns scope and
ordering. The contract and ADRs in this directory own implementation details.
If they disagree, implementation stops until the documents are reconciled in a
reviewed change. Source code, generated schemas, and tests become authoritative
only after their corresponding implementation pull request lands.

## Change policy

- Decisions are written in English and reviewed in pull requests.
- A breaking wire-contract change requires a capability version change and a
  schema snapshot diff.
- A transport or trust-boundary change requires a new ADR and an updated
  threat model.
- A new action must appear in the capability registry. The inventory in this
  directory is a migration baseline, not a second runtime registry.
