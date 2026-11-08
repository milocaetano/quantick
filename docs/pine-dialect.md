# Quantick Pine — dialect reference

Quantick Pine is a **Pine v5/v6 subset** for scripting indicators on quantick's
activity-sampled charts (tick / volume / dollar / imbalance bars). A
well-formed Pine v5 or v6 indicator script inside this subset runs unmodified:
drop a `*.pine` file in the indicators folder (default `./indicators`,
override with `QUANTICK_INDICATORS_DIR`) and add it from the INDICATORS
toolbar menu.

The enabling insight: Pine's x-axis is `bar_index`, not time, which maps
1:1 onto alternative bars. What is meaningless off a clock —
`request.security`, `timeframe.*`, sessions — is exactly
what the dialect cuts, and every cut is a **load-time error with a stable
code and a line number**, never a silently wrong plot.

Numerical parity with TradingView is a non-goal (it cannot even be defined
on activity-sampled bars). Our semantics are *documented* below and locked
by golden tests.

## Execution model

- **Commit run** — when a bar closes, the script executes once, top to
  bottom; every mutation persists.
- **Preview run** — while a bar forms, the script executes against the
  partial bar from a snapshot of committed state and is rolled back
  afterwards. An EMA never advances twice inside one bar. `varip` variables
  are the deliberate exception: they persist across preview runs.
- A runtime error (invalid data-dependent value, loop budget) disables that indicator —
  shown with bar index, line and code — and never touches its neighbours.

## Language

- `//@version=5` (missing → assumed 5 with a warning; other versions are
  errors).
- Declarations `=`, reassignment `:=`, `var`, `varip`, tuple destructuring
  `[a, b] = f()`. Scalar types are `int`, `float`, `bool`, `string`, and
  `color`. Annotations (`var float x = na`) are enforced at load, including
  `const < input < simple < series` qualifiers. Expressions take the strongest
  input/control-flow qualifier; assignments cannot narrow it. Incompatible
  types, annotations, argument counts and overloads report `PINE_TYPE`.
  Numeric storage remains f64; scalar identity is checked before execution.
  Integer-to-float and v5 numeric-to-bool conversions are supported, as are
  explicit `int()`, `float()`, `bool()`, `string()` and `color()` casts (the
  latter two accept their own type or `na`, not formatting conversions).
  Strings support `+`. Typed missing values retain bool/color defaults in
  `nz()`, and missing booleans are false in conditions.
- **Compatibility divergence:** an unannotated `x = na` may infer one
  unambiguous scalar/handle type from later assignments, including branches.
  Conflicting or unresolved types fail at load. This deterministic inference
  preserves the shipped indicators; strict Pine requires an explicit type.
- User functions infer each call separately, resolve typed overloads by exact
  match before numeric promotion, and lower named arguments to parameter
  order. Function parameters are at least `simple`; series arguments and
  control flow propagate through their returns. Drawing and plot handles
  remain distinct from scalar types.
- Operators: arithmetic (`+ - * / %`), comparison, `and or not` (short-
  circuit; `na` counts as false), ternary `? :`, history `[n]`.
  In v5, stateful calls in skipped `and`/`or`, ternary and `if` arms advance on
  every bar, using a temporary variable view; assignments and drawing effects
  remain conditional. This eager policy is intentional, not a claim of
  TradingView equivalence. The PineTS proxy differs on conditional cumulative
  sums; see [the measured policy](pine-parity.md#semantic-policy-measurement).
- `if` / `for … to … by` / `while` / `switch` are **expressions**; an
  indented block's value is its last statement's value. Loops are capped at
  10 000 iterations per bar (`PINE_LOOP_BUDGET`). `switch` takes both forms:
  with a subject (`switch x` + `value => body` arms, matched by `==` rules)
  and without (`switch` + `condition => body` arms); a bare `=> body` arm
  is the default.
- User functions: `f(x) => expr` or an indented block; tuples for multi-
  returns; no recursion (`PINE_RECURSION`); no nested definitions.
- Division (and `%`) by zero yields `na`, never infinity. In v5,
  `const int / const int` truncates toward zero; division with a float or
  non-const integer produces a float. This intentionally distinguishes
  `5 / 2` (2), `5.0 / 2` (2.5), and `input.int(5) / 2` (2.5).
  See TradingView's [v5-to-v6 division and boolean migration rules](https://www.tradingview.com/pine-script-docs/migration-guides/to-pine-version-6/)
  and [v5 type system](https://www.tradingview.com/pine-script-docs/v5/language/type-system/).
- Line continuation: a line ending in a binary operator or comma, or with
  unclosed brackets, continues. **Divergence:** a line ending in `=>` does
  *not* continue — it opens a multi-line function body. Tabs indent 4
  columns.

## Series variables

`open high low close volume`, `hl2 hlc3 ohlc4 hlcc4`, `bar_index`,
`last_bar_index`, `time`, `time_close` (epoch ms),
`barstate.isconfirmed` (true on commit runs), `barstate.islast` (true on
preview runs — the forming bar is the newest).

**Order-flow builtins Pine doesn't have:** `buy_volume`, `sell_volume`,
`delta` (= buy − sell), `cvd` (host-maintained running sum of delta, shared
by every indicator), `trade_count`.

## History `[n]`

`x[n]` reads the value `x` had `n` bars ago; out of range reads `na`
(or false for a v6 bool).
History works on any expression (`ta.ema(close, 9)[1]`). Constant offsets
size storage exactly; dynamic offsets are capped at 500 bars — deeper reads
yield `na`. Every known expression-history ring records one entry per committed
bar, including `na` for skipped branches or an invalid dynamic offset. Skipped
bars therefore remain gaps at their actual bar distance rather than compressing
the ring. Preview runs do not commit these entries.

## `ta.*` — stateful kernels

Each *textual call* owns its own state; a user function called from two
places owns two instances (call-path identity). Stateful calls in loops share
one textual site: each visit starts from the committed pre-bar kernel state,
and the final visit becomes this bar's state. Identical arguments reuse the
current result; varying sources replace it without advancing time again.
An unexecuted loop body advances once using its entry variable view (`for`
uses its initial index). Loop-dependent lengths remain subject to the fixed
kernel-length rule. Warmup returns `na`
until `length` inputs have been seen unless noted. NaN policy: windowed
kernels return `na` while a NaN sits in the window and recover exactly when
it leaves; recursive kernels (`ta.ema`, `ta.rma`, `ta.rsi`, `ta.atr`) skip
NaN inputs and hold their state; `ta.cum` is poisoned permanently by NaN.

| Builtin | Semantics |
|---|---|
| `ta.macd(src, fast, slow, signal)` | tuple of fast EMA minus slow EMA, its signal EMA, and their difference |
| `ta.sma(src, len)` | arithmetic mean of the last `len` |
| `ta.ema(src, len)` | α = 2/(len+1), **seed = SMA of the first `len` values** |
| `ta.rma(src, len)` | α = 1/len, seed = SMA |
| `ta.wma(src, len)` | linear weights 1..len, newest heaviest |
| `ta.vwma(src, len)` | `sma(src·volume,len)/sma(volume,len)`; zero-volume window → `na` |
| `ta.rsi(src, len)` | `100 − 100/(1 + rma(up)/rma(down))`; all-flat window → `na` |
| `ta.tr(handle_na)` / `ta.atr(len)` | true range; `atr = rma(tr(true), len)`; `ta.tr` must be called |
| `ta.stdev(src, len)` | **population** standard deviation |
| `ta.highest(src, len)` / `ta.lowest(src, len)` | window extremes |
| `ta.highestbars(src, len)` / `ta.lowestbars(src, len)` | negative offset to the extreme; ties → most recent |
| `ta.change(src[, n])` / `ta.mom(src, len)` | `src − src[n]`, default n = 1 |
| `ta.cum(src)` | running sum from the first bar, no warmup |
| `ta.crossover(a,b)` / `ta.crossunder(a,b)` / `ta.cross(a,b)` | strict crossing; NaN never signals |
| `ta.barssince(cond)` | `na` until the first occurrence |
| `ta.valuewhen(cond, src, occurrence)` | value at the occurrence-th most recent true (0-based, current bar counts) |
| `ta.pivothigh(src, l, r)` / `ta.pivotlow(src, l, r)` | pivot value on its confirming bar (`r` bars later), else `na`; strict inequality |

Kernel lengths must fold to a positive integer at load time (a literal, an
input, or arithmetic over them) — `PINE_SERIES_LENGTH` otherwise. Inside
function bodies (where a length may be a parameter) the same check runs at
bar time.

## `math.*` and value helpers

`math.sum(src, len)` (stateful window sum), `math.abs`, `math.max`,
`math.min`, `math.avg` (variadic), `math.floor`, `math.ceil`, `math.round`,
`math.sign`, `math.sqrt`; `math.pow`, `math.exp`, `math.log`, `math.log10`
route through libm for bit-exact cross-platform results.

`na(x)` (predicate), `nz(x[, repl])` (default 0), `fixnan(x)` (stateful:
carries the last non-na value).

## Colors

`#RRGGBB` / `#RRGGBBAA` literals, named constants (`color.red`,
`color.green`, `color.blue`, `color.orange`, `color.aqua`, `color.yellow`,
`color.purple`, `color.lime`, `color.white`, `color.black`, `color.gray`,
`color.silver`, `color.maroon`, `color.navy`, `color.olive`, `color.teal`,
`color.fuchsia`), `color.new(base, transp)` and
`color.rgb(r, g, b[, transp])` with transparency 0–100.

## Output

- `indicator(title, shorttitle?, overlay?)` — other named arguments are
  accepted and ignored, each with a load warning.
- `plot(series, title?, color?, linewidth?, style?)` — styles
  `plot.style_line`, `plot.style_stepline`, `plot.style_histogram`,
  `plot.style_columns`, `plot.style_circles`, `plot.style_cross`,
  `plot.style_area`. The plot set is fixed at load time (top level only);
  hide conditionally by plotting `na`. Per-bar dynamic plot colors land
  with the drawing milestone.
- `hline(value, …)` — a horizontal line, rendered through the same plot
  path.
- `plotshape(series, title?, style?, location?, color?, text?)` — marker
  columns: shapes `shape.triangleup`, `shape.triangledown`, `shape.circle`,
  `shape.labelup`, `shape.labeldown`, `shape.cross`; locations
  `location.abovebar`, `location.belowbar`, `location.absolute` (the series
  value is the y). `plotchar(series, title?, char?, …)` renders its
  character as marker text.
- `fill(p1, p2, color?)` — a band between two `plot()` results; both
  arguments must be plot handles resolvable at load time.
- `barcolor(color)` — paints the candle of the bar being evaluated. Callable
  anywhere (not top-level only), so the idiomatic form is a `barcolor` inside
  the `if` that decides. Each call settles the bar: a colour paints it, `na`
  leaves it unpainted, and the last call of the bar wins; a bar that never
  calls it keeps the chart's own colours. The forming bar is painted from its
  preview and may change colour until it closes. Where two indicators paint
  the same bar, the later one on the chart wins — the same rule as any other
  overlay drawing over an earlier one. Anything that is not a colour or `na`
  is a `PINE_TYPE` error.
- `bgcolor(...)` — accepted but inert (warns at load): the canvas behind the
  bars is not an indicator output. `alertcondition(...)` is accepted and inert
  (no alerts).
- **Draw objects** — `line.new(x1, y1, x2, y2, color?, width?)`,
  `box.new(left, top, right, bottom, border_color?, bgcolor?)`,
  `label.new(x, y, text?, color?, textcolor?, style?)` with styles
  `label.style_label_up`, `label.style_label_down`, `label.style_none`.
  X-coordinates are **bar indices only** (no `xloc.bar_time`). Handles
  support `set_*` mutators and `.delete()`; a stale handle is a no-op.
  Hard cap: 500 objects per kind per indicator — the 501st collects the
  oldest. Objects created during a preview run are transient: they render
  while the bar forms and vanish on rollback.
- `input.int`, `input.float`, `input.bool`, `input.color`, `input.string`,
  `input.source` — defaults must be constants (`PINE_INPUT_NOT_CONST`);
  `input.source` accepts any series variable including the flow series.
  Changing an input value recomputes the indicator from scratch — a script
  never observes an input changing mid-stream.

## Rejected constructs

Every rejection is a load-time error naming its reason:

| Construct | Code |
|---|---|
| `request.*` | `PINE_NO_SECURITY` — activity-sampled charts have no timeframes |
| `timeframe.*` | `PINE_NO_TIMEFRAME` |
| `strategy.*` | `PINE_NO_STRATEGY` — backtesting consumes plots through the host |
| unsupported `array.*`/`map.*`, all `matrix.* table.*` | `PINE_NO_COLLECTIONS` |
| calendar builtins (`year`, `month`, `dayofweek`, …) | `PINE_NO_CALENDAR` |
| `xloc.bar_time`, `max_bars_back()` | `PINE_UNSUPPORTED` |

## Error codes

`PINE_LEX`, `PINE_SYNTAX`, `PINE_INDENT`, `PINE_VERSION`,
`PINE_UNKNOWN_NAME` (with a did-you-mean hint), `PINE_ARITY`,
`PINE_INPUT_NOT_CONST`, `PINE_SERIES_LENGTH`, `PINE_STATEFUL_IN_LOOP`,
`PINE_RECURSION`, `PINE_NO_SECURITY`, `PINE_NO_TIMEFRAME`,
`PINE_NO_STRATEGY`, `PINE_NO_COLLECTIONS`,
`PINE_UNSUPPORTED`, `PINE_TYPE` (runtime), `PINE_LOOP_BUDGET` (runtime).

Errors render as `file.pine:line:col: message (CODE)`; loading reports
*every* problem at once, not just the first.

## Measured PineTS proxy baseline

The isolated oracle described in [pine-parity.md](pine-parity.md) pins PineTS
0.13.1 and compares fixed authored fixtures, not a verified TradingView run.
SMA, EMA, RSI, ATR, standard deviation, crossover and pivot-high match all 1,344
compared cells at `1e-8` absolute plus `1e-8` relative tolerance. All eight cases
now match 1,920/1,920 requested cells; the original baseline
report remains immutable and the current assertion includes native `ta.macd`.
App snapshots prove reproducibility only;
they are not oracle equivalence. Current timezone, typing, collection, library,
candle-output and version restrictions remain in force.

### Calendar fields

`year`, `month`, `dayofmonth`, `dayofweek`, `hour`, and `minute` read the supplied
bar open timestamp in epoch milliseconds. Sunday is 1 and Saturday is 7.
The same names accept `(timestamp)` or `(timestamp, timezone)` positional calls;
`na` timestamps yield `na`. Function timestamps must be exact integer milliseconds
within +/-9007199254740991; invalid values report `PINE_TYPE`.

`ScriptIndicator` constructors default to UTC. Hosts can use `with_timezone` with
`calendar::CalendarTimezone` parsed from `UTC` or `UTC+HH:MM` / `UTC-HH:MM`.
Hours range 00-23 and minutes 00-59. Rebinding preserves this configuration.
Function timezone arguments use the same grammar and override that configuration.
Malformed offsets and IANA names are rejected explicitly (API `TimezoneError`,
script `PINE_TYPE`). Fixed offsets have no DST transitions. No machine timezone
or clock is consulted. Dates use the proleptic Gregorian calendar, including
negative epoch timestamps. This is fixed-offset calendar support, not IANA parity.


### Headless candle and bar plots

`plotcandle(open, high, low, close, title, color, wickcolor, bordercolor=...)`
and `plotbar(open, high, low, close, title, color)` declare separate headless
OHLC columns. Positional bordercolor retains its Pine position (9); use the
named argument when omitting intervening options. Prices and colors may vary
per bar; titles must be constant. Any `na` price suppresses the entire sample.
High and low are the maximum and minimum of all four supplied prices.
Explicit `na` colors remain absent; omitted colors default to blue.

Consumers read declarations and committed samples through
`Indicator::plots().candle_specs()` / `candle_column(index)`, and forming
samples through `PreviewFrame::candles`. Reset clears rows while preserving
declarations. These columns do not enter scalar plot rendering or fill handles;
this release adds no app renderer. Calls must be at top level. `editable`,
`show_last`, and `display` are rejected explicitly in this headless subset.

## Version 6 policy

The [primary migration guide](https://www.tradingview.com/pine-script-docs/migration-guides/to-pine-version-6/)
defines the supported differences; paired original fixtures live in
`crates/pine/tests/fixtures/version`, with manually calculated goldens.

- Numeric conditions require an explicit `bool()` cast, including typed bool
  arguments and assignments. Bool values have two states: missing history,
  omitted conditional branches and `bool(na)` produce false. A bool declaration
  or assignment cannot take `na`; `na`, `nz` and `fixnan` reject bool arguments.
- Integer division always produces a fractional float. V5 const integer
  division retains truncation; nonconst integer division is fractional in both.
  The existing f64 integer precision/range limitation remains. PineTS 0.13.1
  simple-int division differs from the primary reference; existing proxy
  artifacts are unchanged and do not redefine these expectations.
- `and` and `or` skip the unselected operand, including stateful calls.
  Other conditional/loop eager advancement retains Quantick's documented
  dialect policy. This is scoped v6 support, not full execution-model parity.
- `for` reevaluates its upper bound after each iteration; the per-loop 10,000
  iteration budget still applies, including a continually growing bound.
- Direct history on literals and builtin constants is rejected; history on
  variables and calls remains supported. Duplicate named arguments are errors.
- Supported plot styles cannot take `na` or incomplete conditional values;
  constant `linewidth` must be at least 1. Plot `transp` and plot `offset` were
  never implemented; neither is added by this version work. `color.new` and
  `color.rgb` retain their separate transparency arguments.
- V6 red/teal/yellow are #F23645/#089981/#FDD835. Legacy v5 red and yellow
  already use those v6 colors and retain them to preserve shipped snapshots;
  this differs from upstream v5 #FF5252/#FFEB3B. Legacy v5 teal stays #00897B.

Calendar support is unchanged. Request, timeframe and strategy namespaces
remain rejected; version selection does not expand drawing or table support.
Collections and UDT field history are handled by their respective feature owners.


## Scalar collections (v5 and v6)

Arrays and maps use typed scalar elements (`int`, `float`, `bool`, `string`,
`color`). Explicit `array<T>` and `map<K,V>` declarations and generic constructors
and typed function parameters are accepted, including legacy `T[]` annotations.
Assignment aliases the same collection; `copy` allocates an
independent collection. `map.keys` and `map.values` allocate independent arrays in
insertion order. Overwriting a key retains its position; removing and reinserting
it appends it. Iteration never uses sorted-key order.

The following operations are implemented through both namespace calls and
receiver methods (constructors and `array.from` are namespace-only):

| Array operation | Arguments after the receiver |
| --- | --- |
| `array.size`, `array.clear`, `array.copy` | none |
| `array.get`, `array.remove` | index |
| `array.set`, `array.insert` | index, value |
| `array.push`, `array.unshift` | value |
| `array.pop`, `array.shift`, `array.first`, `array.last` | none |
| `array.reverse` | none |
| `array.concat` | second array; mutates and returns the first |
| `array.fill` | value, optional index_from and exclusive index_to |
| `array.includes`, `array.indexof`, `array.lastindexof` | value |
| `array.sum`, `array.avg`, `array.median`, `array.mode`, `array.range` | none; numeric arrays only |
| `array.min`, `array.max` | optional zero-based nth rank |
| `array.abs`, `array.standardize` | none; returns an independent numeric array |
| `array.stdev`, `array.variance` | optional biased flag (default true) |
| `array.covariance` | second numeric array, optional biased flag (default true) |
| `array.percentile_linear_interpolation`, `array.percentile_nearest_rank` | percentage in 0..100 |
| `array.percentrank` | nonnegative element index |
| `array.sort`, `array.sort_indices` | optional order.ascending (default) or order.descending |
| `array.slice` | index_from, exclusive index_to; shared parent view |
| `array.binary_search`, `array.binary_search_leftmost`, `array.binary_search_rightmost` | numeric value; requires ascending order |
| `array.join` | optional separator, default comma |
| `array.from` | positional scalar values with a common element type |

| Map operation | Arguments after the receiver |
| --- | --- |
| `map.size`, `map.clear`, `map.copy`, `map.keys`, `map.values` | none |
| `map.get`, `map.remove`, `map.contains` | key |
| `map.put` | key, value; returns the previous value or missing |
| `map.put_all` | second map; updates in that map's insertion order |

`array.new<T>(size = 0, initial_value = na)` also accepts the scalar aliases
`array.new_int`, `array.new_float`, `array.new_bool`, `array.new_string`, and
`array.new_color`. v6 missing bool elements and map results are false.
`map.new<K,V>()` creates an empty map. Internal constructor registry spellings
are listed below solely so the documentation guard can track typed lowering:

`map.new_int_int`
`map.new_int_float`
`map.new_int_bool`
`map.new_int_string`
`map.new_int_color`
`map.new_float_int`
`map.new_float_float`
`map.new_float_bool`
`map.new_float_string`
`map.new_float_color`
`map.new_bool_int`
`map.new_bool_float`
`map.new_bool_bool`
`map.new_bool_string`
`map.new_bool_color`
`map.new_string_int`
`map.new_string_float`
`map.new_string_bool`
`map.new_string_string`
`map.new_string_color`
`map.new_color_int`
`map.new_color_float`
`map.new_color_bool`
`map.new_color_string`
`map.new_color_color`


For `get`, `set`, `insert`, and `remove`, v6 negative indices count backward
from the array size before the operation. v5 rejects negative indices. Bounds,
empty removals, invalid sizes and missing map keys used in insertion report a
structured `PINE_TYPE` runtime error. Collections are capped at 100,000 elements
or 50,000 map pairs. `fill` uses nonnegative half-open bounds in both versions.

Snapshots share the authoritative contiguous word image until the first write.
Aliases retain identity through copy-on-write; preview rolls back ordinary `var`
collections, while `varip` mutations persist, including aliases held by ordinary
`var`. History retains collection identities. Deterministic tracing reclaims
unreachable collection records after commit; it does no tracing for scripts
without collections.

Sorting accepts numeric and string arrays. Empty strings and missing values sort
last ascending and first descending; equal elements retain their original order.
`sort_indices` returns an independent index array. A slice keeps its parent alive,
including through history and rollback. Updating, sorting, inserting or removing
through a slice updates its parent. Parent resizing can shrink the visible slice,
then restore it; only edits through the slice change its maximum range. Growing a
slice while its start is beyond the parent's current size reports `PINE_TYPE`.

Numeric reductions skip missing samples; covariance skips pairs containing a
missing value and requires equal array lengths. Empty/all-missing reductions
return `na`, while standardization returns an empty array. Standardization keeps
missing positions when numeric samples exist and uses population deviation.
The integer overloads of `avg`, `median`, `stdev`, `variance`, linear percentile,
and `percentrank` truncate results toward zero; integer standardization truncates
each result. The [function reference](https://www.tradingview.com/pine-script-reference/v5/)
lists int/float overloads for each of these functions and int/float-array overloads
for `standardize`; `covariance` always returns float. Numeric `join` uses eight
optional fractional digits, matching the default `str.tostring` format, and accepts
only numeric or string arrays. v6 rejects explicit `na` boolean elements.

The following edge policies are explicit dialect decisions, not verified
TradingView observations. The function reference's `array.mode` entry says to
choose the smallest value for a frequency tie or all-unique input; we follow that
specific rule despite the arrays overview saying no mode returns `na`. A rank
beyond the available numeric samples returns `na`. A zero-deviation standardized
sample returns `na` from the mathematical normalization formula. Binary left/right
searches return the first/last equal index, or the preceding/following insertion
neighbor when absent. Thus left below the minimum is -1, right above the maximum
is size; an empty array returns -1/0 respectively. Ordinary binary search returns
the first equal index or -1. Primary documentation does not settle every extreme
boundary. Read-only PineTS 0.13.1 probes (not TradingView) returned 0 for left below
the minimum and 1 for constant standardization, differing from these policies.
Those proxy observations are not acceptance goldens. Collection goldens are
independently authored calculations and use the established 1e-8 absolute plus
1e-8 relative tolerance for fractional statistics.

Nested collection elements, drawing elements and UDT elements are not implemented
in this scalar stage. Unsupported namespace operations fail at compile time;
they never silently return a substitute. `array.from` requires positional arguments.
The independent original fixtures follow the primary
[array](https://www.tradingview.com/pine-script-docs/language/arrays/) and
[map](https://www.tradingview.com/pine-script-docs/language/maps/) documentation.
## Local libraries and user objects

`libraries::compile_with_libraries(source, title, resolver)` accepts a pure
caller-supplied `Fn(&str) -> Option<String>` source resolver. Imports use exact
`local/name/version` keys and explicit aliases. No filesystem or network lookup
occurs in Pine. Missing keys, import cycles, remote keys and depth beyond 32 fail
with deterministic diagnostics. The original two-argument `compile` remains
available for scripts without imports.

`type`, typed fields, literal field defaults, `Type.new(...)`, field reads and
`:=` writes, `method` definitions and receiver calls use checked object references.
Assignment aliases references; `object.copy()` creates a shallow copy. Object
history preserves references. In v6 `object.field[1]` is rejected: use
`previous = object[1]` followed by `previous.field`, or a temporary field series.
The v5 field-history form remains supported. User-object mutable fields live in
the runtime word arena and participate in preview rollback. `varip` fields,
enums, remote libraries and nonliteral field defaults are not implemented.

Local library sources currently use the importing script's Pine version;
version mismatches are rejected explicitly. Linking is bounded to 128 sources,
32 nested imports and four million bytes of expanded source.
