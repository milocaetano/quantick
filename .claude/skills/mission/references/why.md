# Why the mission flow is shaped this way

Background, not procedure. `SKILL.md` carries every rule and every command;
this file carries what each rule was bought with, for a reader deciding whether
to change one. Nothing here is operative — if a step is stated in both files,
that is a defect in this one.

## The failure the whole flow is shaped around

A request carrying eight asks becomes six criteria, and the two that fell out
of the paraphrase are invisible from that moment on. Then the same agent that
wrote the criteria ticks its own boxes, and the trader finds the gap by using
the thing.

Every step closes one part of that: the ledger makes a dropped ask visible, the
interrogation makes a wrong reading expensive early instead of late, the
checklist format makes a criterion gradeable by someone else, and
`delivery-review` is that someone else.

## Why `small` is the default

The default was the trader's, and it was moved there after the first branch to
use this mechanism spent five review rounds on a docs change. A gate that costs
more than the work it guards is a gate people route around, and a fast path
nobody selects is a fast path that does not exist. `medium` and above are what
you type when the change earns them.

## Why the bare tier word is accepted despite the misparse

The bare form misreads an objective that genuinely opens with one of the four
words: `/mission small fonts are unreadable on the axis`, `/mission high CPU on
the heatmap`. For three of the four tiers a misparse costs nothing anyone
notices. For `small` it costs the interrogation, most of the gate table and
`delivery-review` — a skipped gate, from a typo-shaped ambiguity.

Two things hold it, and neither pretends to be a parser. Step 1's echo names
*what the tier drops* rather than merely the word it read, so the expensive
misparse is the one that announces itself loudest to the person reading the
first turn. And the flagged form is there for exactly the objective a bare word
would guess wrong on. This is a residual the design accepts openly rather than
one it claims to have closed.

## Why tiers exist at all

Until the tier table existed every mission charged the same: a one-line fix
paid for an interrogation round, a full gate table, a `high`-effort bug pass
and a fresh-context conformance review. The predictable result is that the flow
got skipped rather than scaled, and a skipped flow protects nothing.

The whole ladder then moved down a notch after the trader measured what it
cost: three `xhigh` bug passes and a full conformance review on one docs
branch, for work that used to ship at roughly four-fifths the quality in a
fraction of the time. The reply to that is not to delete the gates, it is to
stop charging `high` prices for `small` work.

## Why the goal file is written into the worktree, not the checkout

The ordering used to be the other way round. A `GOAL.md` written into the main
checkout is not on the branch, so the archive step has no source to rename
there and stages a commit onto `main` if run from the main checkout — and
`delivery-review`, which looks for the checklist *on the branch*, returns NOT
GRADEABLE. The stranded `GOAL-archive-*.md` files sitting untracked in the main
checkout are what that ordering left behind.

The file keeps its name: dozens of archives already use it, and renaming the
record would buy nothing.

## Why the tier is recorded with its branch name

The two review markers hold a sha, so they go stale the moment the branch
moves. A bare tier word would outlive the mission that wrote it, and the next
branch checked out in that worktree would inherit an exemption it never asked
for and ship ungraded. That was measured on the first version of this feature,
not imagined — which is why `guardrails.sh` refuses a declaration naming any
other branch, and refuses the one-field format outright rather than guessing.

## Why the archive commit comes before the reviews

Getting it backwards is a trap with a pleasant-looking exit. Archive *after*
recording the markers and that commit moves `HEAD`, both markers go stale,
`pr-gate` denies — and the cheapest way out is to re-stamp both markers without
re-running either review, which silently destroys the one property the
sha-based marker exists to give. Nothing would catch that; the gate would still
say two reviews passed.

## Why closing steps are not criteria

They cannot be graded when the grading happens. `delivery-review` reads the
checklist and grades every `A` and `G` against the shipped branch — but its own
verdict does not exist while it is being written, and `pr-gate` will not let
the PR open until that verdict is recorded. Written as criteria, those lines
come back UNPROVEN on every mission, the fix loop stalls on gaps no edit can
close, and the gate escalates to the trader every single time. A gate
that always fails teaches everyone to ignore it, which costs more than not
having it.
