# A6 / A7 / R7 / R8 — the numbers did not move

## The test

`the_report_numbers_are_fixed`: a fixed journal of nine closed trades in,
the whole report out as text, asserted byte for byte, across two cuts.
`All` covers the aggregation; `Week` covers the anchor-relative cutoff,
which is the arithmetic that lives in this crate rather than in
`quantick-sim`.

The dump is `{:#?}` over `PerformanceReport` rather than a hand-written
field list, so it cannot silently omit a metric, plus the `EquityWalk`
points, which the curve and the trade list's running total both read.

## Written before the move

| commit | what it did |
| --- | --- |
| `ee5fd6d` | added the golden. **No production code.** |
| `940db62` | moved the report out. |

## Byte-identical across the move

| | SHA-256 of the golden text |
| --- | --- |
| at `ee5fd6d`, in `paper_trading.rs` | `c90b6f970fd53ea2330f9dc6a2c7ad029549dc3d2f794955af5cddc8675cfe25` |
| at `HEAD`, in `paper_report.rs` | `c90b6f970fd53ea2330f9dc6a2c7ad029549dc3d2f794955af5cddc8675cfe25` |

**IDENTICAL** - 261 lines, unedited.

`git log -p` over the constant shows one commit adding it and none
changing it:

```
$ git log --oneline -S 'const GOLDEN_REPORT' -- crates/app/src/paper_trading.rs crates/app/src/paper_report.rs
940db62 refactor(app): take the performance report out of the order ticket
ee5fd6d test(app): pin the report's numbers before they move
```

The second entry is the move itself: the constant left one file and
arrived in the other in the same commit, with its text unchanged, which is
what the two hashes above prove.

## It passes

```
$ cargo test -p quantick-app the_report_numbers_are_fixed
test paper_report::tests::the_report_numbers_are_fixed ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2180 filtered out
```
