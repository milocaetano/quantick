# A11 / R12 — the ratchet numbers, and when each one was written

A parallel branch is also moving `!budget`, so the request asked for
`--tighten` immediately before the push rather than early, when the number
would go stale.

**The first version of this file was stale, and `delivery-review` caught
it.** It recorded a `--tighten` run producing `6382` / `3281` / `61364` and
stopped there — numbers that were true when it was written and wrong by the
time the branch shipped, because two later commits grew both files. The
reviewer's objection was exact: `--tighten` only ever *lowers* a number, so
the larger figures in the shipped baseline cannot have come from the command
this file named. They came from a hand-edit, which is legitimate and was
never disclosed here. This is the corrected record.

## What actually happened, in order

| # | step | `paper_trading.rs` | `paper_report.rs` | `!budget` |
| --- | --- | ---: | ---: | ---: |
| 1 | the extraction | 9,238 → measured 6,382 | new, 3,281 | 60,939 |
| 2 | `--tighten` wrote the fallen ceiling | **6,382** | — | — |
| 3 | hand-written: the new entry and the raise it needs, each signed | — | **3,281** | **61,364** |
| 4 | the doc-comment fix grew the module | — | 3,286 | 61,369 |
| 5 | the review fixes grew both files 14 lines | **6,396** | **3,300** | **61,397** |
| 6 | rebase onto `origin/main` (`9376ac7`), which had reworked the guards crate | unchanged | unchanged | unchanged |
| 7 | **`--tighten`, last, after every commit** | *nothing to tighten* | *nothing to tighten* | — |

Steps 3, 4 and 5 are hand-edits and are meant to be: `--tighten` cannot add
a new file's entry, and it cannot raise a budget — `tighten_never_raises_the_budget`
is one of the guard's own tests. Growth is pay-as-you-go and has to be
written by a human hand so a reviewer can argue with it, which is exactly
what the comment beside `!budget` is for.

## The final state, verified after the last commit

```
$ cargo run -q -p quantick-guards -- --tighten
nothing to tighten in the size ratchet: no tracked file has shrunk past its
slack, and the tracked total is within 500 of the !budget
nothing to tighten in the context ratchet: …

$ grep -nE '^crates/app/src/paper_(report|trading)\.rs |^!budget' crates/guards/size-baseline.txt
72:!budget 61397
120:crates/app/src/paper_report.rs 3300
122:crates/app/src/paper_trading.rs 6396
```

"Nothing to tighten" is the correct final answer, and it is the one that
matters: it says the recorded ceilings are the sizes the shipped code
actually has, with no slack left to give back.

```
$ cargo test -p quantick-guards
test result: ok. 65 passed; 0 failed        (against the reworked ratchet from PR #280)
```
