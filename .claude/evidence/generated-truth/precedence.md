# Precedence, before and after (A1, R2, R3)

## Before

### `docs/control-plane/README.md` §Precedence

```markdown
## Precedence

The [development plan](../mcp-control-plane-development-plan.md) owns scope and
ordering. The contract and ADRs in this directory own implementation details.
If they disagree, implementation stops until the documents are reconciled in a
reviewed change. Source code, generated schemas, and tests become authoritative
only after their corresponding implementation pull request lands.

```

### `docs/README.md`, opening paragraph

```markdown
# Documentation index

The documents in this tree, grouped by what they are for. Design records
state their own status in their first lines, and each sub-tree owns its own
precedence rule — notably
[`control-plane/README.md`](control-plane/README.md), where the plan and the
contract outrank the code until a reconciling change lands, which is the
opposite of the usual default. Read the document before assuming which wins.
```

## After

### `docs/control-plane/README.md` §Precedence

```markdown
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

```

### `docs/README.md`, opening paragraph

```markdown
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
```

## What changed, in one line each

- **Code is authoritative for what is registered.** The old rule said the
  opposite outright — "Source code, generated schemas, and tests become
  authoritative only after their corresponding implementation pull request
  lands" — and told an agent that "implementation stops until the documents
  are reconciled". An obedient agent reading the stale roadmap therefore
  halted, or set about re-implementing shipped code.
- **The contract keeps the wire rules.** The inversion is scoped, not total:
  identifier grammar, schema shape, revision semantics, the authority
  boundary and limits still belong to the contract, and code that disagrees
  with it is a bug in the code.
- **The new rule is not another promise to keep documents current by hand.**
  That was the promise the old one made and did not keep. Both indexes it
  names are generated and guarded.
- **`docs/README.md` no longer advertises the inverted default.** It said the
  control-plane tree was "the opposite of the usual default"; it now says no
  document in the tree states what has shipped.
