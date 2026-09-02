# Where this repository's agentic flow actually spends

Measured on 2026-09-01 against `origin/main` at `4e12f8b`, in a fresh worktree.
Every number below is reproducible with the command beside it.

**Run them in a worktree checked out at that commit, not in the main checkout.**
The first draft of the crate/file table was measured in a main checkout sitting
several commits behind, and reported `crates/app` at 192,800 lines against the
193,762 the named commit actually holds. `delivery-review` caught it by re-running
the command. The 72% conclusion was unaffected, which is exactly why the error
survived a reading — a stale measurement that still supports its argument is the
kind only re-execution finds.

## 1. Review rounds: what the predecessor found, and where it was wrong

`refactor/mission-review-throughput` (#272) reported that ordinary code
branches average **~1 review-fix commit** and that the branches burning six
rounds are the ones with **zero production lines** — meta-work on the workflow
itself. Re-measured over the last 20 merged PRs, counting a "round" as a commit
whose subject names a review or a finding, and sizing each branch by lines
touched in `.rs`, `.py` and `.mq5`:

```sh
git log --merges -20 --format='%H %s' origin/main | grep 'Merge pull request' |
  while read -r h rest; do
    cnt=$(git log --oneline "$h^1..$h^2" | grep -icE 'review|finding')
    prod=$(git diff --numstat "$h^1" "$h^2" | awk '$3 ~ /\.rs$|\.py$|\.mq5$/ {s+=$1+$2} END{print s+0}')
    echo "$cnt rounds  $prod prod-lines  $rest"
  done
```

| | branches | mean rounds | worst |
| --- | --- | --- | --- |
| Zero production lines (meta-work) | 4 | **3.0** | 6 (#263) |
| Production code | 14 | **1.8** | **8 (#271)** |

**The direction holds and the headline does not.** Meta-work really is twice as
expensive per branch. But ordinary code averages 1.8 rounds rather than ~1, and
the single worst branch in the window — eight rounds, #271 — is a *production*
branch, not a meta one. "The gate is not what is slow for coding" was a fair
reading of a smaller sample; it is not safe to build on.

And #271 is **mid-sized**: 4,528 production lines, against #260's 11,987 and
#258's 11,489. That is the sharper point, because those two spent **one** round
and **two**. Round count does not track diff size in either direction — #268
took **three** rounds for 1,369 lines. Whatever drives the chain, it is not how
much code is in it.

> An earlier draft of this section called #271 "the largest production branch
> measured", which its own table above contradicts. Corrected after
> `delivery-review` caught it. The correction strengthens the finding rather
> than weakening it: a mid-sized branch burning eight rounds is worse news for
> the old arrangement than a huge one doing so.

### What actually let a chain reach eight

Nothing bounded it. Before this branch:

- `arch-review` **step 0** (the bug pass) was bounded at two passes.
- `delivery-review`'s fix loop was bounded at three rounds — *its own* three.
- The **nine-dimension shape pass**, the expensive half of `arch-review`, had
  **no bound at all**.
- Nothing summed them.

And the markers make each fix cost double. Answering either review is a commit;
a commit stales both `arch-review-ok` and `delivery-review-ok` by design; so
every fix re-runs *both* reviews. Two independent budgets over a doubling loop
with an unbounded pass in the middle is how eight rounds arrive without anyone
deciding on eight.

**Fixed by** the chain budget in `CLAUDE.md` — three rounds per branch across
both reviews together, then the remainder ships as recorded PR follow-ups, with
an open Blocker as the one thing that never defers. `arch-review` and
`delivery-review` now point at that one owner instead of carrying numbers of
their own.

## 2. Tokens: the single largest item was a data table

Bytes of every artifact an invocation reads in full:

```sh
for f in CLAUDE.md AGENTS.md .claude/hooks/README.md .claude/skills/*/SKILL.md; do
  printf '%7d  %s\n' "$(wc -c < "$f")" "$f"
done | sort -rn
```

`ui-harness/SKILL.md` was **76,459 bytes — nearly twice the next skill**, and
**61,466 of them were the hook-registry section**: 126 rows of
`| QUANTICK_… | what it reaches |`, plus the prose introducing them.

```sh
git show 4e12f8b:.claude/skills/ui-harness/SKILL.md | wc -c            # 76459
git show 4e12f8b:.claude/skills/ui-harness/SKILL.md | sed -n '21,291p' | wc -c  # 61466
git show 4e12f8b:.claude/skills/ui-harness/SKILL.md | grep -cE '^\| `QUANTICK'  # 126
```

It is *data*, consulted one row at a time by a capture run that drives one or
two surfaces, and it was loaded whole on every invocation of `ui-harness`,
`visual-qa` and `trader-ux-review`.

> Three figures in this paragraph were wrong until `delivery-review` re-ran
> them: it said 61,483 bytes (that is the extracted file, which carries a
> heading this section did not), 118 rows (the count was already 126), and
> "larger than the next two skills combined" (43,375 + 33,527 = 76,902, so it
> was smaller, by 443 bytes). Each is corrected above with the command that
> produces it. None of them changes the argument, which is why a reading did
> not catch them and a re-run did.

**Fixed by** moving it verbatim to `references/hook-registry.md`, with the skill
pointing at it and telling the reader to `grep` for the surface they need. No
row was dropped — the first split of this branch stranded five hooks in the skill,
which the review caught and this branch fixed, so the registry now holds all 126
rows including the pending ones. Reading it whole is still available for the case that
wants it — taking inventory, auditing coverage — which now costs what it always
did instead of being charged to every run.

| | before | after |
| --- | --- | --- |
| `ui-harness/SKILL.md` | 76,459 | **9,887** (−87%) |
| all ten `SKILL.md` together | 203,402 | **144,441** (−29%) |

### Be honest about the other direction

The rules this branch adds **cost** bytes, and pretending otherwise would be the
kind of accounting this repository files as a finding:

| | change |
| --- | --- |
| `delivery-review/SKILL.md` (pass split, escalation, its wiring) | +6,306 |
| `CLAUDE.md` (chain budget + model routing) | +3,028 |
| `arch-review/SKILL.md` (points at the chain budget) | +796 |
| `ship/SKILL.md` (stops carrying a second round count) | +509 |
| **spent** | **+10,639** |
| **recovered from the registry move** | −66,572 |
| **net across every always-read artifact** | −55,933 |

So the saving is one structural move, not a diet — and the rules cost nearly
twice what the first draft of this table claimed, because the review round added
four more passages. Roughly 14,000 tokens saved per invocation that touches the
harness, and nothing at all on a run that does not.

The 68,639-byte reference file is not counted as a saving anywhere above. It is
the same bytes, moved to where they are paid for on demand rather than on every
run — which is the whole claim, and inflating it into a deletion would be the
kind of accounting this section exists to avoid.

## 3. Models: every subagent was billed at open-judgement rates

`grep -rn 'model' .claude/skills/*/SKILL.md` returned **no routing at all**.
Every dispatched agent inherited the caller's model, including the largest one
in the pipeline: `delivery-review`'s reviewer, a `general-purpose` agent that
reads a diff and applies a checklist somebody else already wrote.

**Fixed by** the routing rule in `CLAUDE.md` — retrieval on `haiku`,
checklist-application on `sonnet`, open judgement on the strong model — and by
`delivery-review` naming `sonnet` on its **criteria pass**, with a second
dispatch on the strong model carrying **only** the lines the first graded as
other than `DELIVERED`. The strong pass is paid per disputed line rather than
per branch, and a clean branch never pays it.

**Two corrections the review forced, both worth recording.** The first draft
routed the whole reviewer to `sonnet`, including the **completeness pass** —
and that was backwards. `UNLEDGERED` is a *discovery* grade: its failure mode is
a false negative, an ask nobody noticed, which produces no line for any
escalation to pick up. The skill itself calls it the only failure the rest of
the pipeline is blind to by construction, so it was the one pass that could not
afford a weaker reader. It keeps the strong model and is never escalated.

The second is about this very rule's reach: `grep -rn haiku .claude/` matches
nothing but this document. No skill routes a retrieval agent, and
`delivery-review`'s criteria pass is the **only** routed call site in the
repository. The rule is written as the standard the next dispatch meets, and
`CLAUDE.md` now says so rather than implying a taxonomy the repo exercises.

**The exception is the point.** `arch-review` step 0 stays on the strong model
and now says so. `code-review` finds real defects partly by being one;
downgrading a bug pass is where quality actually falls, while downgrading the
fan-out around it costs nothing anyone can measure.

## 4. The `delivery-review` criterion #272 withdrew

#272's `A3` — start that review at its cheapest shape — was written, reverted,
and carried forward undischarged. The reason it was reverted is the important
part: grading from `branch.stat` plus the files as they stand lets a reviewer
quote a sentence that was **already on `origin/main`** and mark the criterion
`DELIVERED`. A false pass, with nothing downstream able to catch it.

This branch discharges it while keeping that hole shut, by cutting a different
thing. The reviewer keeps the **full diff** — the only input that distinguishes
what a branch did from what it inherited, and not the expensive part. What gets
cut is the model and the escalation scope, per §3. Cheaper input was the wrong
saving; a cheaper *reader* is the right one.

## 5. Modularity: what the shorter review gave up, and what now carries it

This is the section `R7` is graded on. Every reduction, and the mechanism that
absorbed it:

| Given up | What carries it now |
| --- | --- |
| Shape-pass rounds beyond the third | The chain budget defers rather than discards — the remainder ships as a PR follow-up with its severity, visible and arguable. An open Blocker never defers. |
| A strong model grading every `A`/`G` line | A `sonnet` first pass **on the criteria pass only**, with the strong model re-grading every line that is not `DELIVERED`. The completeness pass, which has no escalation that could catch its failure mode, keeps the strong model. |
| A reviewer reading the whole registry | All 126 rows still there, greppable. Nothing was deleted. |
| Reviewer attention on file growth | **The debt budget** — mechanical, ~1s, at edit time. See below. |

### The debt this repository already carries

```sh
for d in crates/*/; do echo "$(find "$d" -name '*.rs' -not -path '*/target/*' | xargs cat | wc -l) $d"; done | sort -rn
```

`crates/app` is **193,762 of 268,703 lines — 72% of the repository in one
crate**, with `app.rs` alone at 34,064 lines. The thing `CLAUDE.md` forbids has
already happened, so a cheaper review could not simply be taken on trust.

The per-file ratchet forbids *invisible* growth and permits *signed* growth.
That is the right rule for one file and no rule at all for eighteen: commit
`2dcf062`, on **#271** (`feat/mt5-session-history`), raised `app.rs` from 9,775
to 9,890 production lines with a comment explaining why, extracted nothing in
return, and every check in the repository stayed green. Eighteen entries each
raised "for this branch" read as eighteen reasonable decisions and one lost
trunk.

```sh
git log --oneline -S'crates/app/src/app.rs 9890' -- crates/guards/size-baseline.txt
git merge-base --is-ancestor 2dcf062 e398a69^2   # e398a69 is the #271 merge
```

> Attributed to #272 in two earlier drafts, and to "the branch before this one"
> in the baseline's own comment. `2dcf062` is an ancestor of both merges — #272
> branched after #271 landed — but #271 is where it was authored. This is the
> anecdote the whole `!budget` mechanism rests on, so the wrong PR number on it
> is not cosmetic.

**Fixed by** the `!budget` directive — a cap on the *sum* of every recorded
ceiling, seeded at the 61,467 lines currently signed for. Raising one ceiling
now requires lowering another in the same change; extract a surface into its own
module and both numbers fall together.

Nothing is blocked, which was the trader's specific worry. Raising the budget
line itself is still allowed and is the escape hatch **on purpose** — it is one
number, in one place, that a reviewer watches move, which a +115 buried among
eighteen entries never was. Three properties stop the hatch becoming a bypass,
each with a test: `--tighten` follows the ceilings down and never up; a missing
directive is a finding naming the uncapped total; and a stale entry keeps
spending its ceiling so a deleted file cannot finance the next raise.

Evidence, including the +115 reproduced and caught: `debt-budget.md`.

## 6. Performance impact (G3)

The only compiled change is `crates/guards`. Classified by the `arch-review`
rate table:

- `size::budget_verdict` — **rare**. Once per `check`, and once per
  `check_file`, and it is arithmetic over ~18 parsed entries with no file I/O of
  its own: the baseline was already read.
- `size::remedies` — **rare**. Two scans of the findings list, only when a guard
  has already failed.
- `check_file(BASELINE_FILE)` — **rare**, and it is a new path rather than a
  slower one: it parses the baseline and walks no files.

No per-trade, per-depth or per-frame path is touched. Measured: `cargo test -p
quantick-guards` runs its 31 unit tests in **well under a second** (0.12-0.15s
across runs; the range rather than a point, because a single timing is not a
figure anyone can reproduce), and the whole crate in about a second — unchanged, because the added work is a sum over
eighteen integers.
