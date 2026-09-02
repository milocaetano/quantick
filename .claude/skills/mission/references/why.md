# Why the mission flow is shaped this way

Background, not procedure. `SKILL.md` carries every rule; this file carries
what each was bought with, for a reader deciding whether to change one.

use this mechanism spent five review rounds on a docs change. The reasoning is
worth keeping: a gate that costs more than the work it guards is a gate people
route around, and a fast path nobody selects is a fast path that does not
exist. `medium` and above are what you type when the change earns them.

The bare form misreads an objective that genuinely opens with one of those
words: `/mission small fonts are unreadable on the axis`, `/mission high CPU on
the heatmap`. For three of the four tiers a misparse costs nothing anyone
notices. For `small` it costs the interrogation, most of the gate table and
`delivery-review` — a skipped gate, from a typo-shaped ambiguity.

Two things hold it, and neither pretends to be a parser. Step 1's echo **names
what the tier drops**, not merely the word it read, so the expensive misparse is
the one that announces itself loudest to the person reading the first turn. And
the flagged form is there for exactly the objective a bare word would guess
wrong on — use it when the sentence reads naturally with the tier word as an
adjective. This is a residual the design accepts openly rather than one it
## The failure this skill is shaped around

A request carrying eight asks becomes six criteria, and the two that fell out
of the paraphrase are invisible from that moment on. Then the same agent that
wrote the criteria ticks its own boxes, and the trader finds the gap by using
the thing. Every step below closes one part of that: the ledger makes a
dropped ask visible, the interrogation makes a wrong reading expensive early
instead of late, the checklist format makes a criterion gradeable by someone
else, and `delivery-review` is that someone else.

One mission does not cost what another does, and until this table existed they
all charged the same: a one-line fix paid for an interrogation round, a full
gate table, a `high`-effort bug pass and a fresh-context conformance review.
The predictable result is that the flow got skipped rather than scaled, and a
skipped flow protects nothing at all. The tier is how a mission buys less
| **8** — shape pass | only the dimensions the diff touches; **8 always** | full | full | full |
| **8** — `delivery-review` | **not run** | **completeness pass only**, inline | runs in full | runs in full |
| **9** — the `/goal` line | skipped | printed | printed | printed |

The whole ladder moved down a notch after the trader measured what it cost:
three `xhigh` bug passes and a full conformance review on one docs branch, for
work that used to ship at roughly four-fifths the quality in a fraction of the
time. The reply to that is not to delete the gates, it is to stop charging
`high` prices for `small` work — which is what the tier is for. Nothing above
`max` runs `ultra`, and nothing runs it automatically at all.

**What no tier buys.** `arch-review` runs at every one of them, the four checks

## Why the ordering rules exist

   The tier line is not bookkeeping. `delivery-review` reads this file and
   nothing else, so a branch that arrives at it having declared `small` needs
   the file to say why the exemption it took was earned — and a `small` mission
   that grew is one whose file no longer matches the diff, which is exactly the
   discrepancy a reviewer should be able to see. At `small` the file may drop
   the decisions and the not-applicable sections when both are empty, and keeps
   everything else: the ledger, the assumptions, the criteria and the verbatim
   request are what makes a goal file gradeable at all, and the tier does not
   buy an ungradeable one.

   That last section is not decoration and it is not optional. `delivery-review`
   reads `GOAL.md` and nothing else — it never sees this conversation. Without
   the original request in the file, the ledger becomes its own source of
   **Record the tier here**, in the new worktree, before the first line of
   work. It goes beside the two review markers, in that worktree's own git dir,
   so it is per-branch and never committed:

   ```sh
   WT=/path/to/worktree
   TIER=medium                 # small | medium | high | max
   cd "$WT" &&
     printf '%s %s\n' "$(git rev-parse --abbrev-ref HEAD)" "$TIER" \
       > "$(git rev-parse --absolute-git-dir)/mission-tier"
   ```
        git add ".claude/GOAL-archive-$SLUG.md" &&
        git commit -m "docs: archive the $SLUG mission"
      ```

   2. **`Skill(arch-review)`** — shape and bugs, over the final branch, at the
      effort and breadth this mission's tier sets. It records `arch-review-ok`
      itself when the review closes. Every tier runs it.
   3. **`Skill(delivery-review)`** — conformance, over the same final branch.
      It records `delivery-review-ok` itself, on PASS only. **Skipped at
