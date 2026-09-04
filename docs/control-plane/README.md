# Control plane architecture contract

The rules the Quantick control plane must satisfy, and the decisions behind
them. The development plan describes the intended outcome and delivery
sequence; the files here fix the details that outcome has to honour.

## Documents

- [Capability inventory](capability-inventory.md) lists every capability the
  application registers — identifier, version, module and required
  permissions. **Generated**; see [Precedence](#precedence) before editing it.
- [Control contract](control-contract.md) fixes identifier, schema, revision,
  authority, limit, tool-surface, determinism, and trade-annotation rules.
- [ADR 0001](adr-0001-local-transport-and-instance-discovery.md) selects the
  local transport and running-instance discovery mechanism.
- [Observer threat model](observer-threat-model.md) defines the assets, trust
  boundaries, threats, and required controls for the first profile.
- [Roadmap](roadmap.md) says where to look for what has shipped, what is
  planned, and what the rules are. It states no delivery status itself.
- [`history/`](history/) holds the archaeology: the per-pull-request evidence
  records, the August 2026 roadmap ledger, and the PR 0 migration baseline the
  generated inventory replaced. Correctly dated, never current state.

## Precedence

Two questions, two different answers. Asking the wrong one of the two is what
made this section worth rewriting.

**What is registered — the code is authoritative.** Which capabilities exist,
what each one is called, which permissions it demands, which version it
carries, which `QUANTICK_*` hooks the application reads: the registry in
`crates/app/src/control/` and the hook specs beside their own definition sites
are the answer, and a document that disagrees is stale by definition, not a
document the code owes a reconciliation to. Nothing here may be read as a
statement of what has shipped.

That is not a promise to keep the documents current by hand — the same promise
this section used to make, and did not keep. [Capability
inventory](capability-inventory.md) and the [hook
registry](../../.claude/skills/ui-harness/references/hook-registry.md) are
**generated from the code**, and `cargo test -p quantick-guards` fails when a
committed copy and the registry diverge. Do not hand-edit either file; change
the code and regenerate. `crates/control/examples/export_schemas.rs` and
`crates/control/tests/schema_snapshots.rs` are the pattern both follow, and the
reason the read contracts have never drifted.

**Wire rules — the contract is authoritative.** Identifier grammar, schema
shape, revision and cursor semantics, the authority boundary, limits,
determinism, and the trade-annotation rules belong to [control
contract](control-contract.md) and the ADRs in this directory. Code that
disagrees with the contract is a bug in the code. A breaking change is a
reviewed contract change with a capability version bump and a schema snapshot
diff, in that order — never a code change the documents catch up with later.

**Scope and ordering** belong to the [development
plan](../mcp-control-plane-development-plan.md), which states intent, not
delivery. For delivery, ask the registry.

Anything under [`history/`](history/) is archaeology: correctly dated, kept for
the reasoning it records, and never current state.

## Change policy

- Decisions are written in English and reviewed in pull requests.
- A breaking wire-contract change requires a capability version change and a
  schema snapshot diff.
- A transport or trust-boundary change requires a new ADR and an updated
  threat model.
- A new action must appear in the capability registry. The inventory in this
  directory is a migration baseline, not a second runtime registry.

## Roadmap

[Roadmap](roadmap.md) is the ledger between the plan and the code: which plan
item is at which stage and in which pull request, the merge order of the open
stack, the gaps each pull request carried forward, and the docking points and
acceptance criteria of the remaining MVP work (snapshot modules, semantic
scene, PR 5b, PR 5c). It adds no decisions; when it and the plan disagree, the
plan wins and the roadmap is corrected.
