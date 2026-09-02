# Step 0 — invoking the bundled code review

Read before invoking `code-review`. `SKILL.md` carries the invocation and the rules that always apply; this file carries how the effort level is chosen, proven and defended, and what the evidence for that is.

             last, in some other session, and this review has to name the
             level it used.
  <target>   a PR number once one exists — the least ambiguous target there
             is — otherwise a branch name, or omitted for the working diff.
             Never a revision range: `main...HEAD` is not a target it parses.
```

**The order is load-bearing, not stylistic, and this file taught it backwards
until 2026-09-01.** The bundled skill reads the level from the **first token
only**. A first token that is not a level is not an error it reports: the level
silently becomes "not given", *and the whole argument string — effort word
included — becomes the target*. So the `"<target> <effort>"` form documented
here lost both halves at once. The review fell back to the level cached in
`~/.claude.json` from some other session, and it went looking for a target
named `my-branch medium`. That second half is what the *Check the scope it
comes back with* warning below had been catching for months without ever naming
its cause.

Measured on this repository, not inferred:
`.claude/evidence/arch-review-effort-level/reproduction.md` records the CLI's
own published argument hint, the cached level actually sitting on this machine,
and two live invocations differing only in token order.

**A mission's tier overrides that default**, because the tier is the trader's
own statement of how much this change is worth reviewing: **`low` for `small`
and for `medium`, `medium` for `high`, `high` for `max`** — one notch below
what the tier is named, deliberately. The trader measured three `xhigh` passes
on a single docs branch and called the slowness not worth it, so the ladder was
moved down rather than the gate removed. At `max`, say in the header that
`/code-review ultra` exists — a deep multi-agent cloud pass the trader triggers
themselves, never this skill, and never a level this step selects.

**Never re-run a level that already ran clean.** That is a rule about what to
skip *inside* one round, not a budget of its own — a level that came back clean
has already answered, and re-asking it spends tokens to be told the same thing.

The count lives in one place: `CLAUDE.md`'s *review chain has a budget*, three
rounds per branch, where a round is one sweep of **everything that owes the
branch a review** — this step, the shape pass below, then `delivery-review` —
plus the commit that answers what they all found. So this step does not carry a
number, the shape pass does not carry one either, and neither can quietly eat
the other's. A second statement of a number is a second number to keep true,
and the previous arrangement had three of them that did not add up.

**The bug pass is open judgement, so it keeps the strong model.** `code-review`
finds real defects partly by being one, and this is the exception
`CLAUDE.md`'s routing rule exists to protect. Nothing in this step is
downgraded to buy tokens.

Read the tier from **the same file `pr-gate` reads**, never from a second
statement of it. The goal file's `**Tier:**` line is for the reader; this file
is the one the gate acts on, and where the two disagree the gate is what
actually happens:

```sh
WT=/path/to/worktree
cd "$WT" && cat "$(git rev-parse --absolute-git-dir)/mission-tier"
```

`.claude/hooks/README.md` owns that file's format and the rules the hook
applies to it; do not re-derive them here, because a third statement of a
format is a third thing to keep true. All this step needs from it: no file, or
a tier the hook would not honour for this branch, means **no tier** — take the
defaults above rather than guessing at a middle level.

**Name the level in the header either way**, with where it came from, so a
short pass is never mistaken for a thorough one — and say so when this file and
the goal file's `**Tier:**` line disagree. Two surfaces disagreeing about one
branch is a finding in itself, and this review is what sees it.

**Then prove it, because naming it is what failed.** The old rule stopped at
the header, and a header is written by the same agent that got the invocation
wrong: on PR #274 it faithfully recorded `xhigh` on a branch whose tier had
bought `medium`, and the record changed nothing. Proof here is two things
together, and neither alone is enough:

- **By construction** — the level went in as the first token, so the parser
  took it as explicit and never consulted the cached one. That is what the
  block above buys.
- **By the absence of a notice** — when the bundled skill falls back to the
  cached level it *says so*, in a line of the shape "No effort level given —
  reusing `<level>`, the level the user typed last time". Read the returned
  report for that line before reading it for findings. It is the one signal
  this repository gets, and a report carrying it is a failed invocation whatever
  else it found.

**On divergence, the re-run is asymmetric and bounded to one.** If the notice
says the pass ran **below** the level the tier bought, re-invoke once,
effort-first, at the tier's level — a shallower pass has not answered the
question the tier asked. If it ran **above** — the `xhigh`-for-`medium` case,
and the likelier one — **accept it and do not re-run.** A deeper pass has
already answered; a second pass would spend the very budget this rule exists to
protect. Record the overspend instead: name it in the header and carry it into
the PR body beside the deferred findings, so the cost is visible and arguable
rather than absorbed in silence. One retry, never two — if a second invocation
still comes back reused, that is a finding to report, not a third attempt.

**Say what cannot be proven, rather than implying it was.** `code-review` is
bundled — it does not live in `.claude/skills/`, so this repository cannot make
it state the level it ran at. The level is established by construction and by
the absence of a fallback notice, never by a positive statement from the pass
itself. The header claims exactly that much and no more: the level requested,
that it was passed as the first token, and that no reuse notice came back — or,
when the report is silent in a way that settles nothing, that the level is
**unverified**, which is a thing to write down rather than a thing to round up
to a pass.

**Check the scope it comes back with.** When the target does not pin a range
the skill derives one, so it can end up reviewing another branch's merged work
(local `main` behind `origin/main`) or nothing at all (a pushed branch whose
upstream already contains every commit). Findings over files this branch never
touched, or a suspiciously empty pass, mean re-invoking with an explicit
target — not a clean bill of health. Fetch first either way; see *Scope the
review*.

**Expect it in the background.** The skill dispatches an agent and returns only
a name; the findings arrive later as a notification. Read for shape meanwhile,
but publish nothing — the review closes only with step 0's list in hand. If the
notification never lands, re-invoke once; if that fails too, do the bug pass
yourself before publishing and say which it was in the header. "It never came
back" is not a reason to ship an unreviewed branch.

When the findings land:

- **Sort before promoting.** The skill returns bugs and cleanups in one flat
  list with no severity of its own. Wrong *behaviour* — crash, wrong output,
  broken determinism, race — becomes a **Blocker** here, listed before every
  shape finding, and the branch does not pass with one open. A cleanup is not
  automatically lower: file it in the dimension it belongs to and let that
  dimension decide, so an efficiency finding on a per-frame path lands in
  dimension 2 and is still a Blocker under the hot-path rule.
- **Confirm before promoting.** `high` and above deliberately include uncertain
  findings. Item 2 of *Verify before reporting* applies to this list too: argue
  the opposite case and drop what the refutation kills. *Confirmed* means it
  survived that pass, not that the sub-agent sounded certain.
- **Cite, never restate.** A finding step 0 already reported ships as its
  `file:line` plus the severity assigned here — not re-described in new words
  as though this review found it.
- **Step 0 never publishes.** No `--fix`, no `--comment`, no `--post`, and
  never the plugin variant that posts unasked. Findings are resolved
  deliberately, and arch-review is the only thing that reports them.

`code-review` stays callable on its own, but a branch still needs the
`arch-review-ok` marker to open a PR. So on a docs/skills change — where
`mission` waives the shape pass — run this skill anyway and report step 0's
findings through it. The bug pass is not the waived part.

