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

An objective such as `/mission small fonts are unreadable` can be misparsed
as a tier, skipping gates. The echo exposes what that tier drops, and the
flagged form disambiguates the objective. The bare form retains this known
ambiguity; the workflow does not claim to eliminate it.

## Why tiers exist at all

Uniform interrogation and review costs encouraged skipping the workflow on
small changes. The tier ladder was lowered after a documentation branch paid
for three `xhigh` bug passes and a full conformance review. Tiers scale that
cost while retaining the gates appropriate to the work.

## Why the goal file is written into the worktree, not the checkout

A goal written in the main checkout is absent from the task branch: archiving
can commit to main, and delivery review returns NOT GRADEABLE. Earlier ordering
left stranded archives there. The established filename remains compatible
with existing archives.

## Why the tier is recorded with its branch name

The first implementation let a later branch inherit a bare tier exemption.
Binding the declaration to its branch prevents that measured failure;
`guardrails.sh` rejects other branches and the old one-field format.

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
