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
