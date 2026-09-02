# Why this review is shaped this way

Background, not procedure. `SKILL.md` carries every rule; this file carries
what each was bought with, for a reader deciding whether to change one.

## Why the passes split across models

Full mode used to begin at its most expensive shape and stay there. It now
splits by *which pass*, because the two passes fail in opposite directions and
one of them has no backstop at all.

**The completeness pass keeps the strong model, and is not escalated.** It is
the pass that reads the verbatim request and asks whether every ask became a
ledger line, and its grade — `UNLEDGERED` — is a *discovery*: its failure mode
is a false **negative**, an ask nobody noticed, which produces no line for any
escalation to pick up. Step 3 already calls it the most serious grade here and
the only failure the rest of the pipeline is blind to by construction. Cheapening
the one pass with nothing downstream of it is the trade this section exists to
refuse. It is also the cheap pass in tokens — two blocks of text — so there is
almost nothing to save.

**The criteria pass starts on `sonnet`, and escalates per line.** That one
applies a checklist somebody else already wrote, against a diff, citing
`file:line` — the middle kind in `CLAUDE.md`'s routing rule, and the largest
subagent in this pipeline by input size.

**What does not get cheaper, and why the last attempt to make it cheaper was
reverted.** The reviewer keeps the **full diff**. The obvious saving — hand it
`branch.stat` and let it read the files as they stand — opens a false pass with
no floor under it: a reviewer grading from the current files can quote a
sentence that was already on `origin/main` and mark the criterion `DELIVERED`,
and *nothing downstream can catch that*, because the whole gate rests on this
being the pass that did not take the work on trust. The diff is the only input
that distinguishes what this branch did from what it inherited. It is also not
the expensive part — the model is. Cut that.


## Why the reviewer is a general-purpose agent

The obvious alternative is a search-shaped agent type whose tools exclude
`Edit` and `Write` — but those types are specified to read *excerpts* rather
than whole files, and the anti-rubber-stamp rules require citing `file:line`,
reading a named test's assertions and quoting prose verbatim. A reviewer that
grades from excerpts, on the one gate whose entire value is that it did not
take the work on trust, is the wrong trade.

The capability argument for the narrower type does not survive contact either:
every type here keeps `Bash`, and `printf … >> file` writes a file as surely as
`Edit` does. So "the reviewer must not edit the branch" is prose in every case,
and prose is a rule enforced by the party it constrains. Choose for reading
depth, and enforce the rest with the branch-did-not-move check.

## Why a round that is not converging is worth naming

More rounds will not fix a design, and the two shapes look identical from
inside a fix loop. A round whose findings are smaller and fewer than the last
is converging; a round still returning Blockers, especially in code the
previous round's fix introduced, is a sign the approach is wrong rather than
incomplete.

The branch that introduced this skill took **four** rounds of `arch-review`
with the count flat at 15 and the severity climbing into newly written code,
which is exactly the shape that should have prompted the conversation instead
of a fifth round.
