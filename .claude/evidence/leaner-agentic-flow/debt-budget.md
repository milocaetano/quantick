# The debt budget — evidence

Recorded on 2026-09-01, on `refactor/leaner-agentic-flow`.

## The failing case: the raise the previous branch actually made

Reproduced by setting `crates/app/src/app.rs` to 10005 — the same +115 that
`refactor/mission-review-throughput` signed, with nothing extracted in return.
Every per-file ceiling is still respected, so the old guard had no finding to
make; the budget is the only thing that sees it.

```
size: 1 finding(s)
  crates/guards/size-baseline.txt:63: the recorded ceilings total 61582, over the !budget of 61467 (+115) — this branch raised a ceiling without lowering another

The debt budget is the sum of every recorded ceiling — the one number that says whether this repository's largest files are getting better or worse. Individually signed raises cannot answer that: eighteen entries each raised `for this branch` read as eighteen reasonable decisions and one lost repository. So growth is pay-as-you-go. A branch needing a ceiling raised moves comparable code out of some debt file in the same change, and the total does not move; extract, and both numbers fall on their own. Raising the budget line itself stays available and is the escape hatch on purpose — it is one number, in one place, that a reviewer watches move, which a +115 buried among eighteen entries never was.

```

## The same edit, seen by the edit-time hook rather than the suite

The baseline is not a `.rs` file, so `check_file` used to ignore it — the hook
watched every source file and missed the one edit that spends the budget.

```
size: 1 finding(s)
  crates/guards/size-baseline.txt:63: the recorded ceilings total 61582, over the !budget of 61467 (+115) — this branch raised a ceiling without lowering another

The debt budget is the sum of every recorded ceiling — the one number that says whether this repository's largest files are getting better or worse. Individually signed raises cannot answer that: eighteen entries each raised `for this branch` read as eighteen reasonable decisions and one lost repository. So growth is pay-as-you-go. A branch needing a ceiling raised moves comparable code out of some debt file in the same change, and the total does not move; extract, and both numbers fall on their own. Raising the budget line itself stays available and is the escape hatch on purpose — it is one number, in one place, that a reviewer watches move, which a +115 buried among eighteen entries never was.

```

## The branch as it ships

```
$ ./target/debug/quantick-guards ; echo "exit=$?"
exit=0
```
