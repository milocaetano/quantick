# Why delivery review separates its inputs

The implementing agent can satisfy every written criterion while overlooking
an outcome omitted from its own checklist. Source reconciliation addresses that
failure; independent criteria review checks the evidence for the retained asks.

The completeness judgment stays on the strong model. Checklist application
starts on a balanced model, escalating disputed lines only. Both need the full
diff: reading current files alone can mistake inherited behavior for a change.
Reviewers remain read-only and the caller checks that neither branch nor marker
changed during review.

Campaign #330 exposed another failure: fresh reviewers repeatedly atomized the
same operational instructions, so archive repairs generated more archive
repairs. A stable source map reduces reconstruction while retaining the ability
to discover an actual missing outcome. Operational requirements remain gated;
they need not become duplicate product criteria.

Total open findings also confused new discovery with a failed repair. A closed
finding remains progress even if a different omission is later found. Persistent
identities and finite repair budgets distinguish that from repeatedly failing
to fix the same behavior. The delivery contract owns the operative rules.

## Moved from `SKILL.md`

- **Why the completeness pass is never escalated.** Its failure is a false
  negative — an ask nobody noticed — which produces no line for an escalation
  to pick up; it is also the cheap pass. The criteria pass escalates per
  disputed line, so a clean branch never pays for the strong model.
- **Why the branch is bracketed.** A reviewer that finds a criterion missing,
  writes the missing line and grades it delivered returns a PASS whose
  evidence it manufactured, and nothing downstream sees it.
- **Why missing input is a loud refusal.** With no request nothing can be
  unledgered, with no ledger every ask is trivially covered: a verdict over
  empty sets satisfies every PASS clause.
- **Why an approved deferral counts as satisfied.** A deferred criterion never
  grades delivered, so every ask it discharges would grade dropped and PASS
  would be unreachable on the one route by which a gap may ship.
- **Why `## Deferral requested — NOT granted` is its own heading.** The heading
  is what gets skimmed; a subtitle correcting `## Deferred` is not enough.
- **Why `UNPROVEN` is a failure.** It is the honest answer when the outcome may
  be there and nothing on disk says so, not a softer `DELIVERED`.
- **Why this runs after `arch-review`.** It grades the branch as shipped,
  including whatever the shape review made the session change.
