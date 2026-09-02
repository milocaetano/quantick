# Step 0 — invoking the bundled code review

Read before invoking `code-review`. `SKILL.md` carries the invocation and the
rules that always apply; this file carries how the effort level is chosen,
proven and defended, and what the evidence for that is.

## Why the effort level goes first

**The order is load-bearing, not stylistic, and `SKILL.md` taught it backwards
until 2026-09-01.** The bundled skill reads the level from the **first token
only**. A first token that is not a level is not an error it reports: the level
silently becomes "not given", *and the whole argument string — effort word
included — becomes the target*.

So the `"<target> <effort>"` form documented here for months lost both halves
at once. The review fell back to the level cached in `~/.claude.json` from some
other session, and it went looking for a target named `my-branch medium`. That
second half is what the *Check the scope it comes back with* warning had been
catching without ever naming its cause.

Measured on this repository, not inferred:
`.claude/evidence/arch-review-effort-level/reproduction.md` records the CLI's
own published argument hint, the cached level actually sitting on this machine,
and two live invocations differing only in token order.

## Why the tier ladder sits one notch low

The tier is the trader's own statement of how much this change is worth
reviewing. The trader measured three `xhigh` passes on a single docs branch and
called the slowness not worth it, so the ladder was moved down rather than the
gate removed: `low` for `small` and `medium`, `medium` for `high`, `high` for
`max`.

`.claude/hooks/README.md` owns the tier file's format and the rules the hook
applies to it; do not re-derive them, because a third statement of a format is
a third thing to keep true. All this step needs from it: no file, or a tier the
hook would not honour for this branch, means **no tier** — take the defaults
rather than guessing at a middle level.

## Why naming the level is not enough

The old rule stopped at the header, and a header is written by the same agent
that got the invocation wrong: on PR #274 it faithfully recorded `xhigh` on a
branch whose tier had bought `medium`, and the record changed nothing.

Proof is two things together, and neither alone is enough:

- **By construction** — the level went in as the first token, so the parser
  took it as explicit and never consulted the cached one.
- **By the absence of a notice** — when the bundled skill falls back to the
  cached level it *says so*, in a line of the shape "No effort level given —
  reusing `<level>`, the level the user typed last time". Read the returned
  report for that line before reading it for findings. It is the one signal
  this repository gets, and a report carrying it is a failed invocation
  whatever else it found.

`code-review` is bundled — it does not live in `.claude/skills/`, so this
repository cannot make it state the level it ran at. The header therefore
claims exactly that much and no more: the level requested, that it was passed
as the first token, and that no reuse notice came back — or, when the report is
silent in a way that settles nothing, that the level is **unverified**, which
is a thing to write down rather than a thing to round up to a pass.

## Why the re-run is asymmetric

If the notice says the pass ran **below** the level the tier bought, re-invoke
once at the tier's level: a shallower pass has not answered the question the
tier asked.

If it ran **above** — the `xhigh`-for-`medium` case, and the likelier one —
**accept it and do not re-run.** A deeper pass has already answered; a second
would spend the very budget this rule exists to protect. Record the overspend
instead, in the header and in the PR body beside the deferred findings, so the
cost is visible and arguable rather than absorbed in silence.

One retry, never two. If a second invocation still comes back reused, that is a
finding to report, not a third attempt.

## Why the scope check exists

When the target does not pin a range the skill derives one, so it can end up
reviewing another branch's merged work (local `main` behind `origin/main`) or
nothing at all (a pushed branch whose upstream already contains every commit).
Findings over files this branch never touched, or a suspiciously empty pass,
mean re-invoking with an explicit target — not a clean bill of health.
