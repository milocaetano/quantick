# Pine parity measurement

This is a **PineTS proxy baseline**, not verified TradingView equivalence.
PineTS 0.13.1 is a development-only process under `tools/pine-oracle`.
The package and lockfile pin its dependency tree. No Cargo target invokes Node,
loads PineTS, copies its implementation, or uses it as a runtime dependency.
The integration boundary is numeric CSV output from independently authored MIT
scripts. Upstream usage documentation: <https://github.com/LuxAlgo/PineTS>.

## Reproduction

From the repository root, with Node available only for regeneration:

```sh
npm ci --ignore-scripts --prefix tools/pine-oracle
npm run regenerate --prefix tools/pine-oracle
```

The second command regenerates all 24 oracle files. Run it twice and use
`git diff -- crates/pine/tests/golden/oracle` to detect byte changes.
Ordinary offline verification needs only Rust:

```sh
cargo test -p quantick-pine --test parity
```

`QUANTICK_UPDATE_PARITY=1 cargo test -p quantick-pine --test parity` deliberately
refreshes **Quantick** reports and application snapshots, never oracle values.
Review every changed report; a green baseline regression test means the measured
state stayed unchanged, not that every oracle case is implemented.

CSV format 1 has provenance on line one, plot titles in declaration order on line
two, then one row per input bar. `na` is canonical; finite values have nine decimal
places. Comparison uses `abs(actual-expected) <= 1e-8 + 1e-8*abs(expected)` and
requires `na` on both sides. Application snapshots encode finite values as exact
IEEE-754 bits and normalize NaNs. Plot descriptors, bar paints and retained draw objects are captured.
Two previews and two fresh replays exercise rollback and repeatability.

## Inputs and denominators

The three committed CSV exports contain 64 synthetic bars each, with explicit
open and close epoch milliseconds and independent buy/sell/trade counts. `time`
uses regular minutes; `tick` has ten trades per bar and irregular duration;
`volume` has twelve units per bar and irregular duration. These are authored
fixtures, not claims about an exchange recording. The flow check independently
computes expected buy volume, sell volume, delta, cumulative delta and counts.
They deliberately share OHLC sequences to isolate sampling/time differences.

Eight oracle scripts produce ten columns and 1,920 requested cells across the
three tapes. Seven cases match (1,344 cells); native `ta.macd` is unimplemented
and rejects all 576 requested cells. These rejected cells remain in the parity
denominator. See `tests/golden/oracle-report.txt` for the complete case report.
This gap prevents claiming all first oracle cases pass.

The existing compiler corpus has 13 positive scripts (13 load, 21 plot columns)
and seven deliberately invalid scripts (all seven reject). The mixed 13/20
count is not a real-world script success rate. Its diagnostic histogram counts
every diagnostic, including multiple diagnostics from one script. It cannot
justify a priority among collection/library/candle/version features because
those families are not represented in this small corpus.

Nine current app scripts have exact Quantick snapshots; zero are currently
compared directly to PineTS. `ema` and `vwap_cumulative` are candidates for later
oracle comparison; the remaining scripts use Quantick flow variables, draw
objects or host state beyond this numerical oracle's eight authored cases.
Thus app snapshot coverage is 9/9 and app oracle verification is 0/9, explicitly
separate measures. Corpus oracle verification is 0/21 columns. Snapshot tests
read shipped app source directly, so edits cannot silently leave stale copies.

Only independently authored or appropriately attributed MIT/MPL-2.0 scripts
may enter the committed corpus. `tools/pine-oracle/local-corpus/` is ignored for
other local inputs; never commit those scripts or per-script report content.
No local-corpus aggregate collector is installed in this skeleton yet.

## Ownership and preserved contracts

`compile(&str, &str) -> Result<CompiledScript, Vec<PineError>>` and
`ScriptIndicator` through `Indicator::on_close/preview` are unchanged. Existing
`compile::{CompiledScript, Resolution, FunctionInfo, CallSiteId}` and
`eval::{Value, ObjKind}` paths reexport their extracted types.

| Child | Directory | Concrete boundary in this skeleton |
| --- | --- | --- |
| 1 | `semantics/` | Reserved; evaluator statement/expression dispatch remains in the shared hub |
| 2 | `calendar/` | Supplied timestamps, six Gregorian fields, validated `CalendarTimezone` fixed offsets |
| 3 | `types/` | Owns `Value`, `ObjKind` and existing numeric/condition conversions |
| 4 | `ir/` | Owns compiled tables, `Resolution`, function and call-site identity |
| 5 | `collections/` | Existing namespace rejection delegated here |
| 6 | `libraries/` | Reserved for caller-supplied source text; no implicit I/O |
| 7 | `candle_plots/` | Existing plotcandle/plotbar rejection delegated here |
| 8 | `version/` | Existing version diagnostics, with caller-owned error/warning vectors |

After integration, these directories transfer to their named child. The current
compiled slot identifiers are numeric indices with existing `Value` payloads;
this extraction does not claim typed-slot checking,
library resolution, or candle output is implemented. All output remains the
existing deterministic headless `PlotBuffer` contract.

**Unresolved docking constraint:** `NodeKind`, `Builtin`, parser productions,
evaluator dispatch and plot lowering are closed enums/matches in shared hubs.
A no-op module registration cannot make new grammar or builtin variants usable.
An exhaustive contract for all future features would prematurely implement their
semantics. Therefore this skeleton cannot satisfy a complete shared-hub freeze.
The minimal alternative is sequential coordinator-approved hub edits for the
child introducing a variant, followed by ownership transfer of its handler;
otherwise a separately designed hub refactor must precede children 1-8. No later
child is authorized to edit a hub by this document.

## Performance scenario

`cargo bench -p quantick-pine --bench interp` retains the prior single-indicator
scenarios and adds ten instances (EMA, flow, force bar and exhaustion shapes)
on 10,000 active bars, nine preview updates and one close per bar. It reports
aggregate ns/trade and ns/indicator/update including rollback, with setup outside
the timer. The same full partial bar is repeated to measure rollback work;
this is a fixed synthetic throughput scenario, not exchange latency. No timing
assertion is placed in correctness tests. Results belong to the exact measured
commit and machine, and must be compared using the same command and workload.

Calendar acceptance uses independently calculated civil-date goldens in
`tests/golden/calendar`, not PineTS oracle output. `calendar_from_bar_time`
checks six fields and repeated preview/commit equality for negative epochs,
leap dates, and fixed-offset year/day transitions. This does not increase the
PineTS comparison denominator and does not claim IANA/DST timezone parity.
