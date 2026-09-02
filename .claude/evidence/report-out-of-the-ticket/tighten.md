# A11 / R12 — `--tighten` ran last, on purpose

A parallel branch is also moving `!budget`, so the ratchet numbers were
written immediately before the push rather than early, when they would
have gone stale.

Order of the final commits:

1. the extraction (`940db62`)
2. the evidence and the archived goal file
3. **`cargo run -p quantick-guards -- --tighten`**, then the push

```
$ cargo run -q -p quantick-guards -- --tighten
tightened 1 line(s) in crates/guards/size-baseline.txt:
  crates/app/src/paper_trading.rs: 9238 -> 6382
```

`--tighten` only ever lowers a ceiling, so it could not have written the
new module's entry or the budget raise. Those two are hand-written, each
with its reason in a comment beside it:

- `crates/app/src/paper_report.rs 3281` — a new entry, signed with what
  moved and why.
- `!budget 60939 → 61364` — the +425 the seam cost, on the one line a
  reviewer watches.

```
$ cargo test -p quantick-guards
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured
```
