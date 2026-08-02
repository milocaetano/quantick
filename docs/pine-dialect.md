# Quantick Pine — dialect reference

Quantick Pine is a **Pine v5 subset** for scripting indicators on quantick's
activity-sampled charts (tick / volume / dollar / imbalance bars). A
well-formed Pine v5 indicator script inside this subset runs unmodified:
drop a `*.pine` file in the indicators folder (default `./indicators`,
override with `QUANTICK_INDICATORS_DIR`) and add it from the INDICATORS
toolbar menu.

The enabling insight: Pine's x-axis is `bar_index`, not time, which maps
1:1 onto alternative bars. What is meaningless off a clock —
`request.security`, `timeframe.*`, sessions, calendar math — is exactly
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
- A runtime error (type mismatch, loop budget) disables that indicator —
  shown with bar index, line and code — and never touches its neighbours.

## Language

- `//@version=5` (missing → assumed 5 with a warning; other versions are
  errors).
- Declarations `=`, reassignment `:=`, `var`, `varip`, tuple destructuring
  `[a, b] = f()`. Type annotations (`var float x = na`) are accepted and
  ignored — the dialect is dynamically typed with compile-time checks on
  builtins.
- Operators: arithmetic (`+ - * / %`), comparison, `and or not` (short-
  circuit; `na` counts as false), ternary `? :`, history `[n]`.
- `if` / `for … to … by` / `while` are **expressions**; an indented block's
  value is its last statement's value. Loops are capped at 10 000 iterations
  per bar (`PINE_LOOP_BUDGET`). `switch` is not supported yet
  (`PINE_UNSUPPORTED`).
- User functions: `f(x) => expr` or an indented block; tuples for multi-
  returns; no recursion (`PINE_RECURSION`); no nested definitions.
- Division (and `%`) by zero yields `na`, never ±infinity.
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

`x[n]` reads the value `x` had `n` bars ago; out of range reads `na`.
History works on any expression (`ta.ema(close, 9)[1]`). Constant offsets
size storage exactly; dynamic offsets are capped at 500 bars — deeper reads
yield `na`. **Divergence:** an expression's history only records on bars
where the expression actually evaluates; history across untaken `if`
branches reads `na`.

## `ta.*` — stateful kernels

Each *textual call* owns its own state; a user function called from two
places owns two instances (call-path identity). Stateful calls inside loop
bodies are a compile error (`PINE_STATEFUL_IN_LOOP`). Warmup returns `na`
until `length` inputs have been seen unless noted. NaN policy: windowed
kernels return `na` while a NaN sits in the window and recover exactly when
it leaves; recursive kernels (`ta.ema`, `ta.rma`, `ta.rsi`, `ta.atr`) skip
NaN inputs and hold their state; `ta.cum` is poisoned permanently by NaN.

| Builtin | Semantics |
|---|---|
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
- `plotshape(...)`, `plotchar(...)`, `fill(...)`, `bgcolor(...)`,
  `barcolor(...)` — accepted but inert until the shape-plot milestone (each
  warns at load). `alertcondition(...)` is accepted and inert (no alerts).
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
| `array.* matrix.* map.* table.*` | `PINE_NO_COLLECTIONS` |
| calendar builtins (`year`, `month`, `dayofweek`, …) | `PINE_NO_CALENDAR` |
| `plotcandle` / `plotbar`, `xloc.bar_time`, `max_bars_back()` | `PINE_UNSUPPORTED` |

## Error codes

`PINE_LEX`, `PINE_SYNTAX`, `PINE_INDENT`, `PINE_VERSION`,
`PINE_UNKNOWN_NAME` (with a did-you-mean hint), `PINE_ARITY`,
`PINE_INPUT_NOT_CONST`, `PINE_SERIES_LENGTH`, `PINE_STATEFUL_IN_LOOP`,
`PINE_RECURSION`, `PINE_NO_SECURITY`, `PINE_NO_TIMEFRAME`,
`PINE_NO_STRATEGY`, `PINE_NO_COLLECTIONS`, `PINE_NO_CALENDAR`,
`PINE_UNSUPPORTED`, `PINE_TYPE` (runtime), `PINE_LOOP_BUDGET` (runtime).

Errors render as `file.pine:line:col: message (CODE)`; loading reports
*every* problem at once, not just the first.
