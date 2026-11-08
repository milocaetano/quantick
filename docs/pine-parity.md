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
cargo test -p quantick-pine
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

**Approved docking policy (2026-11-08):** children implement behavior in their
owned directories. Necessary narrow shared-hub registrations are authorized in
each issue's Touch and integrated serially by the coordinator. This replaces the
skeleton's freeze blocker: closed enums and dispatch matches may connect the
owned handler without moving unrelated behavior or broadening the feature scope.

## Executable slots and rollback state

Child 4 lowers checked syntax into register instructions, explicit branch targets,
and pre-resolved global/local slots. The AST remains inspection metadata: the C41
test deletes it before running. Straight-line operands are expanded at compile
time; logical and conditional execution uses jumps, with explicit eager-kernel
advancement on skipped arms. Context-dependent coercions remain explicit
instructions. Literal registers without contextual conversions initialize once.

One `Vec<u64>` owns the mutable semantic image: globals, initialization flags,
locals and result registers, call paths, history rings and staged samples, kernel
lengths and payloads, object identifiers, revision, records and text. Offsets and
allocation bookkeeping live in that image too. Preview takes one contiguous
copy and restores it, preserving `varip` slots and the monotone object identifier
counter. Strings and tuples are encoded values, not pointers to mutable heaps.
`ObjectStore` is a regenerated immutable output projection, never transactional
authority. Native indicator APIs remain unchanged.

Packed TA kernels update caller-owned word slices directly, including rings and
monotonic queues. Their outputs match the native arithmetic bits pinned before
implementation, including NaN recovery. Acyclic scripts visit each textual
site/path once. Scripts with loops retain temporary per-run initial images and
argument/result caches so repeated visits replace that bar's result rather than
advance a kernel repeatedly. Those caches are discarded after evaluation.

`slot_ir_commit_preview_parity` covers the original hand-calculated fixture;
`slot_ir_eager_paths_history_functions` combines a stateful condition, separate
function paths, history and repeated previews. Arena tests independently check
copy-only reconstruction, reclaimed text/frame storage and bounded history.

## Performance scenario

`cargo bench -p quantick-pine --bench interp` retains the prior single-indicator
scenarios and adds ten instances (EMA, flow, force bar and exhaustion shapes)
on 10,000 active bars, nine preview updates and one close per bar. It reports
aggregate ns/trade and ns/indicator/update including rollback, with setup outside
the timer. The same full partial bar is repeated to measure rollback work;
this is a fixed synthetic throughput scenario, not exchange latency. No timing
assertion is placed in correctness tests. Results belong to the exact measured
commit and machine, and must be compared using the same command and workload.

### Child 4 paired measurements (2026-11-08)

Baseline source: `403e6c2c` (child 0). Candidate production source:
`9af1c365757f0308ee043377f18c1cf4f2d2ec56`. The candidate samples below used
the identical production inputs before commit; the only subsequent source edit
added a `cfg(test)` offset-growth regression. Benchmark inputs and command stayed
unchanged. Both targets were built before sampling with
`cargo bench -p quantick-pine --bench interp --no-run`, then each sample ran
`cargo bench -p quantick-pine --bench interp` in its isolated worktree.

Host: Intel Core i5-12400F (6 cores, 12 logical processors), Windows 11 Pro
10.0.26200, Rust 1.98.0 (`88d9e12ae`, LLVM 22.1.8), x86_64-pc-windows-msvc,
repository bench profile. Owned builds were paused. Unrelated app/feed Rust
compilations remained active; none were stopped or reprioritized. Two interleaved
ABBA groups reduce ordering bias under that changing load. A is baseline, B is
candidate. Values are aggregate microseconds per trade update across all ten
indicators; each sample contains 100,000 updates.

| Group | A first | B first | B second | A second |
| --- | ---: | ---: | ---: | ---: |
| 1 | 23.9662 | 21.1831 | 21.0920 | 20.9512 |
| 2 | 19.6575 | 18.9287 | 18.5158 | 20.1448 |

Across all four samples per source, baseline mean is **21.179925 us**, candidate
mean **19.929900 us** (5.90% lower); medians are **20.548000 us** and
**20.010350 us** (2.62% lower). Both ABBA groups independently have approximately
5.9% lower candidate means. This meets C42's no-regression budget for this
same-host workload; it does not establish exchange latency or a universal speedup.

Earlier candidates were not accepted. All earlier raw ten-indicator samples are
retained here to distinguish implementation changes from selective reporting:

| Candidate source | Baseline samples, us/update | Candidate samples, us/update | Assessment |
| --- | --- | --- | --- |
| `fc61e360` | 17.6389, 17.2857, 17.8791 | 57.0471, 69.1498, 64.7715 | Regression; optimize execution |
| `102d73ce` | 20.1458, 22.1865, 27.1099, 21.4516 | 26.8209, 27.2239, 20.5844, 21.0944 | Changing load; no acceptance |
| `4c73270a` | 23.5145, 19.4740, 19.5238, 19.0229 | 20.2606, 21.1115, 22.2414, 20.3956 | No demonstrated no-regression |

The accepted production delta completes direct offset copies for global reads,
stores and register moves. Offset caches are immutable within an evaluation;
arena growth moves its allocation but never its offsets. The offset-growth test
also reconstructs state from a word copy and checks those same addresses.

### Integrated v6 remeasurement

After integrating the v6 base `93acda40`, the final runtime source
`95bc85f8c84ec178f96b9148645e7aaef700124e` was remeasured against the same
clean child-0 baseline `403e6c2c16fdde2ddc5a8c978c562e806327aaca`. The benchmark
source, command, 100,000-update ten-indicator workload, host, toolchain and
profile above were unchanged. Both binaries were built before sampling;
owned builds were paused. Unrelated builds in three other worktrees remained
active, with process inventories captured before each sample. No process was
stopped or reprioritized. All eight samples are included below (us/update).

| Group | A first | B first | B second | A second |
| --- | ---: | ---: | ---: | ---: |
| 1 | 25.4940 | 21.7342 | 23.1456 | 23.6919 |
| 2 | 23.7147 | 21.0318 | 25.0673 | 22.7273 |

Baseline mean **23.906975 us** compares with candidate **22.744725 us**
(4.86% lower); medians are **23.703300 us** and **22.439900 us** (5.33% lower).
Both interleaved groups favor the candidate mean (8.76% and 0.74% lower).
The second group has a slower individual candidate sample, so these results
support aggregate no-regression for this workload under the recorded load,
not an improvement for every run or a universal speed claim. This fresh paired
measurement satisfies C42 for the integrated runtime; prior measurements remain
historical evidence rather than a substitute for measuring the version seams.

Calendar acceptance uses independently calculated civil-date goldens in
`tests/golden/calendar`, not PineTS oracle output. `calendar_from_bar_time`
checks six fields and repeated preview/commit equality for negative epochs,
leap dates, and fixed-offset year/day transitions. This does not increase the
PineTS comparison denominator and does not claim IANA/DST timezone parity.

## Semantic policy measurement

Child 1 preserves the original `golden/oracle-report.txt` as baseline evidence.
The current `macd_oracle_parity` assertion runs all eight cases on all three tapes:
1,920/1,920 cells match, including all 576 MACD cells. MACD independently composes
existing EMA kernels; no oracle implementation was read or translated.

The approved eager policy advances stateful calls through skipped `and`/`or`,
ternary and `if` arms with temporary variable values. Side effects remain
conditional and expression histories record `na` at skipped bar positions.
`conditional_state_and_history_parity` has an independent hand-calculated fixture
and repeated previews. This is deliberately different from the PineTS 0.13.1
black-box conditional probe: on closes 1..5, alternating `ta.cum(close)` produces
1,0,4,0,9 in the proxy, versus 1,0,6,0,15 under the approved eager policy.
Neither result is represented as verified TradingView behavior.

Independent loop probes are committed under `tests/fixtures/semantics/` and
`tests/golden/semantics/`. For `i = 0..2`, summing `ta.cum(close+i)` over closes
1..5 produces 6,18,33,51,72; summing SMA length 2 produces na,9,12,15,18.
Changing the upper bound to `bar_index % 3` produces na,3.5,10.5,4.5,9.5.
These proxy results establish one textual site's per-bar replacement semantics:
each visit starts from pre-bar state; the last visit commits, without growing
per-iteration state. Fixed kernel lengths are still enforced. An unexecuted loop
is advanced once with its entry variable view, an explicit eager-policy choice.

All nine app-script snapshots remain byte-identical after the two semantic fixes;
the list of changed app-script outputs is empty. App oracle verification remains
0/9. The original corpus and oracle baseline measurements above describe child 0,
not the current native MACD support.

## Final retained-child acceptance

C90-C93 are the offline combined acceptance entry points. Collection and mixed
object fixtures now participate in combined acceptance. Final paired performance
measurements appear below; the earlier measurements above remain historical.
The baseline remains 1,344/1,920
matching proxy cells; the current numerical oracle expects 1,920/1,920. The
positive compiler corpus is 13/13 at baseline and final, invalid rejection is
7/7 at baseline and final, mixed corpus loads are 13/20 at both, and its oracle
coverage remains 0/21 plot columns. App snapshot coverage remains 9/9 and direct
app oracle coverage remains 0/9. The child-1 semantic corrections are recorded
in the preceding semantic policy section; these denominators are
not a general real-world script success rate.

Library fixtures are independently authored hand-calculated acceptance data,
not PineTS verification. C61 covers exact local resolution, references, methods,
preview rollback, missing/cyclic/remote diagnostics and v5/v6 field history.
The mixed collection fixtures were committed before implementation and execute
through public compile/preview/commit tests. C91 compares serialized preview
outputs and committed plot/object snapshots across fresh runs of the mixed
collection graphs, including history and field-level varip progression. A
varip object reference does not make every field persistent. The initial
preview of a newly constructed object uses the documented Quantick baseline.

### Golden execution inventory

Every committed file under `crates/pine/tests/golden` is classified below.
C90 checks the closed path inventory and provenance statements, so newly added
files require an explicit assignment. `cargo test -p quantick-pine` executes
the combined and named child tests without Node.
Numeric denominators describe separate evidence kinds and must not be added
into a single oracle-parity claim.

| Files | Pine test execution or evidence role |
| --- | --- |
| `oracle/*.csv` (24 files) | C90 `combined_offline_golden_parity`: 1,920 proxy cells, absolute plus relative tolerance; all compared |
| `ir/native-kernels.csv` | C90: 3,072 native pre-IR cells through public Pine compilation, repeated preview and commit; exact finite bits and NaN identity |
| `ir/rollback.csv` | `slot_ir_commit_preview_parity`: authored transaction/history sequence, including varip preview progression |
| `types/scalars.csv`, `version/{v5,v6}.csv`, `libraries/main.csv`, `semantics/{conditional,loop}.csv` | C90 additionally compares 100 retained cells, independently of the 1,920-cell oracle denominator; original named child tests also execute these files |
| `calendar/fields.csv` | `calendar_from_bar_time`: 42 authored date-field cells, timestamp and fixed-offset fixtures |
| `candle_plots/plots.csv` | `candle_and_bar_plot_parity`: three authored OHLC/color rows for two output channels, preview/commit/reset comparisons |
| `app-*.txt` (9 files) | C92 and `oracle_baseline_and_determinism`: exact structural snapshots; direct oracle coverage remains 0/9 |
| `corpus-report.txt` | `corpus_measurement_baseline`: exact deterministic load/error/coverage report |
| `oracle-report.txt` | Historical child-0 report, retained unchanged: 1,344/1,920 matches and 576 rejected cells; current behavior is explicitly different |
| `semantics/conditional-proxy.csv` | Historical conditional proxy observation (1,0,4,0,9), intentionally different from the approved eager policy (1,0,6,0,15); not an equality golden for current semantics |
| `libraries/mixed.csv` | `mixed_object_collection_golden`: shallow aliases across object fields, arrays and maps |
| `libraries/eager.csv` | `object_eager_reads_preserve_conditional_mutations`: eager reads and conditional field writes |
| `libraries/{varip_graph,history_graph}.csv` | `mixed_graph_history_and_first_preview_varip`: mixed cycles, first-preview persistent references and slice-backed object history |
| `collections/{annotations,boundaries,defaults,eager,search,slices,sorting,statistics}.csv` | C90 and C91 execute authored collection rows through public preview/commit APIs |
| `collections/rollback.csv` | `collections_copy_on_write_rollback`: authored commit/preview state transitions |
| `collections/negative.csv` | C90 `negative_golden`: v5 runtime bounds error and v6 numeric outputs parsed from the golden |
| `libraries/defaults.csv` | `builtin_field_defaults_evaluate_at_construction`: builtin defaults, repeated previews and commits |
| `calendar/README.md`, `candle_plots/README.md`, `collections/README.md`, `libraries/README.md`, `version/provenance.md` | C90 asserts provenance statements; these documents are not executable numeric output data |

The native fixture audit exposed and corrected the VWMA registration: public
`ta.vwma(source, length)` obtains volume from the input bar, matching its evaluator
and documented contract. No oracle implementation was copied or translated.
The combined graph replay compares 136/136 authored numeric cells, separate
from the 100/100 retained scalar cells, the 3,072/3,072 exact native cells and
the 1,920/1,920 proxy cells. Preview repetitions do not inflate these counts.
Other child transaction/calendar/candle fixtures retain their own denominators.
The authored approximate comparisons share the proxy tolerance above; native
bit fixtures and application snapshots remain exact. No authored value is
represented as a new external oracle observation.

### Final paired runtime measurement

On 2026-11-08, `cargo bench -p quantick-pine --bench interp` compared pinned
baseline `403e6c2c16fdde2ddc5a8c978c562e806327aaca` (A) with final source
`7353314c3829ed54b8c6b2b370915cd4e75c5df8` (B), Pine source tree
`91b5a9fca072968d040c49b387b3ddaae811c9e6`. Both release binaries were warmed
before sampling. Environment: Windows x86_64 MSVC, Rust 1.98.0
(`88d9e12ae`, LLVM 22.1.8), Intel Core i5-12400F, six cores/twelve threads.
Each sample executed the fixed ten-indicator stack over 100,000 trade updates.
No other Cargo, rustc or interpreter benchmark process was present at the
pre/post checks for each sample. All eight samples are retained:

| Order | Revision | ns/trade, ten indicators |
| --- | --- | ---: |
| 1 | A | 15416.7 |
| 2 | B | 15267.7 |
| 3 | B | 15423.5 |
| 4 | A | 15671.9 |
| 5 | A | 15909.0 |
| 6 | B | 16283.7 |
| 7 | B | 15372.2 |
| 8 | A | 15503.1 |

Baseline mean: 15,625.175 ns/trade; final mean: 15,586.775 ns/trade
(1,558.678 ns per indicator/update), 0.246% below baseline. This is practically
flat within the observed variation, with no observed mean regression; it is
not evidence of a material speedup. The historical larger improvements above
are not substituted for this final measurement. Compilation and setup are
outside the timed region; preview rollback and retained state are included.
