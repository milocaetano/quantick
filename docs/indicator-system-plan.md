# Indicator System — Implementation Plan

Status: **implemented** (M1–M5, PRs #96–#110) — kept as the design record;
the code is the authority where the two disagree.
Scope: scriptable indicators (Pine-dialect) plotted on quantick's alternative-bar charts.
Audience: the implementing agent. This document locks the architecture and the
decisions; do not relitigate a "Locked decision" without flagging it to the user first.

Everything in `CLAUDE.md` applies unchanged — the four-check verification loop
per PR, test-first engine-style development, conventional commits, one-way
dependency edges. This plan only adds to it.

---

## 0. Context and goals

quantick builds tick / volume / dollar / imbalance bars from raw trades
(`crates/engine`) and shows them in an egui desktop chart (`crates/app`). Users
want to write and edit **indicators** — moving averages, CVD, zigzag tops and
bottoms, rectangles/zones — in a language that is as close to TradingView's
PineScript v5 as this domain allows, drop the script in a folder, and see it
plotted over the live chart.

The enabling insight: **Pine's x-axis is `bar_index`, not time.** Every
`plot()`, `line.new()`, `box.new()` positions by bar index, which maps 1:1 onto
alternative bars. The Pine features that are meaningless here
(`request.security`, `timeframe.*`, sessions) are exactly the ones we cut.

### Goals

- A **Pine v5 subset dialect** ("Quantick Pine", files `*.pine`) that runs
  unmodified for a large class of real-world indicator scripts: series
  semantics, `[]` history operator, `var`, `if`/`for` expressions, user
  functions, the core of `ta.*` / `math.*`, `plot*`/`hline`/`fill`, drawing
  objects (`line`, `box`, `label`), `input.*` with auto-generated settings UI.
- **Order-flow builtins Pine doesn't have**: `delta`, `buy_volume`,
  `sell_volume`, `cvd`, `trade_count` as first-class series.
- **Deterministic**: same bars in → same plot values out, bit-exact, golden
  tested. Scripted indicators must be usable later by backtest/bot consumers.
- **Never blocks a frame**: evaluation runs off the UI thread; a full
  recompute of a large history shows progress instead of freezing the chart.
- **Data honesty for scripts**: an unsupported construct is a load-time error
  with file/line/column and a stable error code — never a silently wrong plot.

### Non-goals (v1)

- Exact numerical parity with TradingView's runtime (impossible to verify on
  activity-sampled bars; our semantics are *documented*, not *cloned*).
- `request.*`, `timeframe.*`, `strategy.*` (backtesting), `array.*`,
  `matrix.*`, `map.*`, `table.*`, alerts, multi-symbol anything.
- Time-coordinate drawing (`xloc.bar_time`), sessions, calendar functions.
- A bytecode VM or JIT — the tree-walking interpreter below has a measured
  performance budget; optimize only if the benchmark says so.

---

## 1. Architecture overview

Two new crates. The existing `engine` crate is **not modified**.

```
crates/indicators   (package quantick-indicators)
    the indicator runtime: series storage, incremental ta::* kernels,
    execution contract (close/preview), plot & draw-object & input models,
    IndicatorHost (multi-indicator manager). Pure domain crate:
    no UI, no network, no async, no threads, no wall clock.
    Depends on: quantick-engine, rust_decimal, libm.

crates/pine         (package quantick-pine)
    the language frontend: lexer, parser, compile passes, tree-walking
    interpreter. Compiles a script into something that implements the
    indicators crate's Indicator trait. Pure domain crate, zero new deps
    (hand-rolled lexer/parser — no parser-generator crates).
    Depends on: quantick-indicators (re-exported types only).

crates/app
    IndicatorWorker (thread, modelled on orderflow_worker.rs), rendering
    (overlay + sub-panes), indicator manager UI, script library folder scan,
    settings UI generated from InputSpec, persistence.
    Depends on: both new crates.
```

Dependency direction stays one-way: `app` → `pine` → `indicators` → `engine`.
Never a reverse edge. `pine` must not depend on `app` or egui; `indicators`
must not depend on `pine`.

### Data flow

```
trades ──ChartState──▶ bars + partial          (exists today, app/src/state.rs)
                        │
                        ▼ (commands over a channel)
              IndicatorWorker thread
                owns IndicatorHost (indicators crate)
                  ├─ on bar close: eval each indicator once, commit state
                  ├─ on partial update: preview eval (rollback semantics)
                  └─ on rebuild/reset: replay all bars from scratch
                        │
                        ▼ (delta events back to UI)
              UI-side IndicatorView state (columns it owns)
                        │
                        ▼
              render: overlay plots on price chart, sub-panes below,
              draw objects, forming-bar values from the preview frame
```

---

## 2. Crate `indicators` (`quantick-indicators`)

Create as `crates/indicators`, added to the workspace members list.

### 2.1 Numeric model — locked decision

Indicator math is **`f64`**, converted once per bar from the engine's
`Decimal` at the crate boundary. Rationale: Pine itself is float64, indicator
math is tolerant of float rounding by nature, and `Decimal` is 10–50×
slower — hostile to the recompute budget.

Determinism is preserved by policy, not by `Decimal`:

- `+ - * /`, `sqrt`, comparisons: IEEE-754, bit-exact everywhere.
- Transcendentals (`pow`, `exp`, `ln`, `log10`): **always through the `libm`
  crate** (pure-Rust, bit-exact cross-platform), never `f64::powf` etc.
  Enforce with a clippy-visible internal wrapper module `crate::fmath` and a
  unit test that greps `src/` for forbidden calls (cheap and effective).
- No wall clock, no randomness, no `HashMap` iteration reaching output —
  same rules as `engine`, guarded the same way (golden tests, §6).

`na` is represented as `f64::NAN` inside numeric series. All kernels must
propagate NaN Pine-style (any NaN input → NaN output, except functions
documented to skip, like `fixnan`; comparisons with NaN are `false`).

### 2.2 Input type

```rust
/// The f64 projection of an engine Bar plus derived flow series, computed
/// once per bar at the boundary. Decimal → f64 via rust_decimal's to_f64.
pub struct IndicatorBar {
    pub open_time: i64,   // exposed to scripts as `time`
    pub close_time: i64,  // `time_close`
    pub open: f64, pub high: f64, pub low: f64, pub close: f64,
    pub buy_volume: f64, pub sell_volume: f64,   // volume = sum, delta = diff
    pub trade_count: f64,
}
impl From<&quantick_engine::Bar> for IndicatorBar { ... }
```

Derived builtins (`volume`, `delta`, `hl2`, `hlc3`, `ohlc4`, `hlcc4`) are
computed here, not in scripts. `cvd` is a host-maintained running sum of
`delta` (so every script sees the same value without each paying for a
`ta.cum`).

### 2.3 Execution contract — the heart of the design

Pine's model, adapted to quantick's partial bar
(`ChartState::partial()`):

- **Commit run** — when a bar *closes*, the script executes once, top to
  bottom, and every state mutation persists: series get one new committed
  row, `var` variables keep assignments, `ta` kernels advance, draw objects
  persist.
- **Preview run** — while a bar is *forming*, the script executes against the
  partial bar **starting from a snapshot of the last committed state**, and
  every mutation is discarded afterwards. This is Pine's rollback: an EMA
  never advances twice inside one bar. The preview's outputs (one row of plot
  values + transient draw objects) are kept only for rendering.

The trait both native and scripted indicators implement:

```rust
pub trait Indicator {
    /// Static description: plots declared, inputs declared, overlay flag.
    fn descriptor(&self) -> &IndicatorDescriptor;
    /// Commit run for a closed bar. Appends exactly one row to plot buffers.
    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx) -> Result<(), EvalError>;
    /// Preview run for the forming bar. Must not observably mutate state:
    /// implementations run against scratch/staged state (see §2.4).
    fn preview(&mut self, partial: &IndicatorBar, ctx: &mut Ctx) -> Result<PreviewFrame, EvalError>;
    /// Drop all state, ready for a full replay (spec switch, replay seek).
    fn reset(&mut self);
}
```

`&mut self` on `preview` is deliberate — implementations stage into their own
buffers and roll back before returning (cheaper and simpler than interior
mutability), but the *contract* is "no observable state change".

**Preview cost containment — locked decision**: preview state handling is
*truncate, don't clone* for the big data, *clone, don't diff* for the small
data:

- Series/plot columns: preview appends one staged row, reads through it,
  and truncates back to the committed length on exit. O(number of columns),
  no data copied.
- `var` slots, `ta` kernel states, draw-object store: cloned per preview.
  These are small (kernel states are a few f64s or a short deque; objects are
  capped at 500/indicator, §2.6). Budget ≤ ~50 KB per preview per indicator.

`varip` (persists across preview runs within one bar) is supported: `varip`
slots live *outside* the snapshot/rollback set. This is genuinely useful here
(intrabar aggression counters on flow charts).

### 2.4 Series storage

```rust
pub struct SeriesStore {
    columns: Vec<Column>,      // one per SeriesId, full history, Vec<f64>
    committed_len: usize,      // rows committed; preview may stage +1 row
}
```

- **Full columns, no ring buffers — locked decision.** Plots need full
  history for rendering anyway; memory at 100k bars × 30 columns ≈ 24 MB is
  acceptable. Ring-buffering non-plotted series is a *possible later
  optimization*, noted here so nobody designs for it prematurely.
- History read `x[n]` = `column[committed_or_staged_len - 1 - n]`; out of
  range → NaN (Pine semantics).
- Non-numeric series (bool, color) are stored in their own typed columns —
  keep `Column` an enum of `F64(Vec<f64>) | Bool(Vec<u8>) | Color(Vec<u32>)`.
  Do not box per-cell values.

### 2.5 `ta` kernels

Every kernel is an incremental struct: `push(x) -> f64` amortized O(1) per
bar (O(log n) tolerated only where noted), state small and `Clone`. No kernel
may re-scan its window on push — `highest`/`lowest` use a monotonic deque,
windowed sums keep a running sum + ring of the last `len` inputs.

**Call-site identity — locked decision.** As in Pine, *each textual call* of
a stateful function owns its own kernel instance. Instances are keyed by a
compile-time call-site id; inside user functions the key is the call-path
(stack of call-site ids), so `f()` called from two places yields independent
states. Stateful calls inside `for`/`while` bodies are a **compile-time
error** (`PINE_STATEFUL_IN_LOOP`) — Pine's behavior there is a footgun; we
refuse honestly instead of diverging silently.

v1 kernel list, with the exact semantics to implement and golden-test
(warmup = returns NaN until `len` inputs seen, unless noted):

| Function | Semantics |
|---|---|
| `ta.sma(src, len)` | arithmetic mean of last `len`; warmup NaN |
| `ta.ema(src, len)` | α=2/(len+1); **seed = SMA of first `len` values**; warmup NaN |
| `ta.rma(src, len)` | α=1/len; seed = SMA; warmup NaN |
| `ta.wma(src, len)` | linear weights 1..len |
| `ta.vwma(src, len)` | `sma(src*volume,len)/sma(volume,len)` |
| `ta.rsi(src, len)` | `100 − 100/(1 + rma(up,len)/rma(down,len))` |
| `ta.tr(handle_na)` / `ta.atr(len)` | true range; `atr = rma(tr(true), len)` |
| `ta.stdev(src, len)` | **population** stdev over window (document it) |
| `ta.highest/lowest(src, len)` | monotonic deque, O(1) amortized |
| `ta.highestbars/lowestbars` | negative offset to the extreme |
| `ta.change(src[, n])` | `src − src[n]`, default n=1 |
| `ta.mom(src, len)` | `change(src, len)` |
| `ta.cum(src)` | running sum from first bar (no warmup) |
| `ta.crossover/crossunder/cross` | strict Pine definition incl. NaN rules |
| `ta.barssince(cond)` | NaN until first occurrence |
| `ta.valuewhen(cond, src, occ)` | ring of last `max_bars_back` occurrences |
| `ta.pivothigh/pivotlow(src, l, r)` | value when confirmed `r` bars later, else NaN; pivot sits at `bar_index − r` |
| `math.sum(src, len)` | windowed sum (stateful, lives in `math` namespace) |

Stateless `math.*`: `abs, max, min, floor, ceil, round, sign, sqrt, avg` +
`pow, exp, log, log10` via `crate::fmath`/libm.

The seed choices for `ema`/`rma` are *our documented semantics* (defensible,
deterministic, testable) — TradingView parity is a non-goal (§0). Write this
in the dialect reference doc too.

### 2.6 Output model

```rust
pub struct IndicatorDescriptor {
    pub title: String, pub short_title: Option<String>,
    pub overlay: bool,                     // price chart vs own sub-pane
    pub plots: Vec<PlotSpec>,              // fixed at compile/registration time
    pub inputs: Vec<InputSpec>,
}
pub struct PlotSpec { pub id: PlotId, pub title: String, pub style: PlotStyle,
                      pub base_color: Rgba8, pub width: f32, pub offset: i32 }
pub enum PlotStyle { Line, StepLine, Histogram, Columns, Circles, Cross, Area }
```

- **Plot count is fixed at load time** (Pine rule: `plot*` at top level only).
  Conditional hiding = plot NaN or `color = na`. Per-bar color overrides go in
  a parallel color column only for plots that ever pass a dynamic color
  (flagged at compile time, so plain plots don't pay for a color column).
- `plotshape`/`plotchar`: subset — shapes `triangleup/triangledown/circle/
  labelup/labeldown/cross`, locations `abovebar/belowbar/absolute`.
- `hline(value, …)` renders a horizontal line in the pane; `fill(p1, p2,
  color)` fills between two plot ids; `bgcolor(color)` and `barcolor(color)`
  emit per-bar color columns (barcolor lets flow scripts repaint candles by
  delta — cheap and very on-theme).
- **Draw objects**: `line.new/box.new/label.new` with **bar-index x-coords
  only**, `.set_*` mutators, `.delete()`. Retained store per indicator with a
  hard cap of **500 objects per kind** — creating the 501st garbage-collects
  the oldest (Pine's rule, and our render-cost guarantee). Objects created
  during a preview run live in the `PreviewFrame` and are discarded on the
  next preview/commit.
- `PreviewFrame` = one row of plot values (+ optional colors) + transient
  objects + `bgcolor`/`barcolor` for the forming bar.

### 2.7 Input model

```rust
pub enum InputSpec {
    Int   { name, title, default: i64, min: Option<i64>, max: Option<i64>, step, options },
    Float { ... }, Bool { ... }, Color { ... },
    Str   { name, title, default, options: Vec<String> },
    Source{ name, title, default: SourceId },   // close, hl2, delta, …
}
```

Inputs are extracted at load time; the app builds the settings UI from this
alone (§4.4). Changing an input value = full recompute of that indicator (the
host handles it; scripts never observe a mid-stream input change).

### 2.8 `IndicatorHost`

Lives in this crate (pure, no threads — the *worker* is app-side):

- owns the ordered list of active indicators (native or scripted — it only
  sees the `Indicator` trait), each with its `InstanceId`, input values and
  per-indicator error state;
- `push_closed_bar(&Bar)`, `set_partial(Option<&Bar>)` → runs previews,
  `rebuild(&[Bar], Option<&Bar>)` → reset + replay (mirrors
  `ChartState::rebuild`), `add/remove/set_inputs`;
- converts `Bar → IndicatorBar` once per bar for all indicators, maintains
  the shared `cvd` accumulator;
- a **runtime error inside one indicator disables that indicator** (error
  state with bar index + message, shown in UI) and never poisons the others —
  `on_close` errors must not leave a half-appended row (append is the *last*
  step of a commit run).

### 2.9 Module layout

```
crates/indicators/src/
  lib.rs        // crate docs: contract, determinism rules, re-exports
  bar.rs        // IndicatorBar + From<&Bar>
  series.rs     // SeriesStore, Column, history reads, stage/truncate
  ta/mod.rs     // kernel registry; one file per family below
  ta/smooth.rs  // sma, ema, rma, wma, vwma
  ta/window.rs  // highest, lowest, *bars, stdev, sum, valuewhen, barssince
  ta/flow.rs    // cum, change, mom, rsi, tr, atr, cross*
  ta/pivot.rs   // pivothigh/pivotlow
  fmath.rs      // libm wrappers; the only transcendental call site
  output.rs     // PlotSpec, PlotBuffer, PreviewFrame, colors
  objects.rs    // line/box/label store, caps, GC
  input.rs      // InputSpec, InputValue
  indicator.rs  // Indicator trait, IndicatorDescriptor, Ctx, EvalError
  host.rs       // IndicatorHost
  native/mod.rs // native reference indicators: Ema, Cvd (M1 proves the pipe)
crates/indicators/benches/eval.rs   // plain harness=false bench like engine's
crates/indicators/tests/            // golden + unit (see §6)
```

---

## 3. Crate `pine` (`quantick-pine`)

Create as `crates/pine`. Hand-rolled lexer/parser — **zero new dependencies**
(supply-chain minimalism, full control over spans/errors, and the language is
small).

### 3.1 Dialect scope

Accepted (target: a well-formed Pine v5 indicator script in this subset runs
unmodified):

- `//@version=5` (missing → assumed 5 with a load warning; `strategy()` or
  v4 constructs → hard error).
- `indicator(title, shorttitle?, overlay?, precision?)` — other named args
  accepted-and-ignored *with a load warning* (honesty: warnings are surfaced
  in the UI, §4.4).
- Declarations `=`, reassignment `:=`, `var`, `varip`, tuple destructuring
  `[a, b] = f()`.
- Types: int, float, bool, string, color (+ `na`), object handles (line, box,
  label). Dynamic typing at runtime with compile-time arity/kind checks for
  builtins.
- Operators: arithmetic, comparison, `and or not`, ternary `? :`, history
  `[n]`.
- Control flow *as expressions*: `if/else if/else`, `for … to … by`,
  `while` (iteration caps, §3.5). `switch` may slip to M4 if tight.
- User functions `f(x) => expr` and indented-block form; no recursion
  (compile error), no closures over mutable state.
- Builtin namespaces: `ta.*`, `math.*` (§2.5), `color.*` (`new`, `rgb`,
  named constants, `#RRGGBB[AA]` literals), `input.*` (§2.7), `plot`,
  `plotshape`, `plotchar`, `hline`, `fill`, `bgcolor`, `barcolor`,
  `line/box/label` (§2.6), `nz`, `na()`, `fixnan`.
- Builtin variables: `open high low close volume`, **`buy_volume sell_volume
  delta cvd trade_count`**, `hl2 hlc3 ohlc4 hlcc4`, `bar_index`,
  `last_bar_index`, `time`, `time_close`, `barstate.isconfirmed`,
  `barstate.islast`.
- `alertcondition(...)`: parsed, inert, load warning (keeps community
  scripts loading).

Rejected with a specific error code + span (never a silent skip):
`request.*` (`PINE_NO_SECURITY` — message explains activity-sampled charts
have no timeframes), `timeframe.*`, `strategy.*`, `array.* matrix.* map.*
table.*`, `xloc.bar_time` coordinates, calendar builtins
(`year/month/dayofweek/…`), `plotcandle/plotbar`, `max_bars_back()` override
beyond the config cap.

### 3.2 Lexer

- Indentation-aware: emits `Indent`/`Dedent` tokens. Block = strictly deeper
  indentation; **line continuation** = a physical line ending while brackets
  are unclosed *or* ending in a binary operator/comma/`=>` (this accepts
  normal Pine formatting without cloning Pine's "non-multiple-of-4" rule —
  document the divergence in the dialect reference).
- Tokens carry byte spans; every later stage reports through spans.
- Number literals are f64 (int literal → int, promoted on demand); string
  escapes minimal (`\" \\ \n`); `//` comments.

### 3.3 Parser → AST

- Recursive descent, precedence climbing for expressions.
- AST nodes in a single arena (`Vec<Node>` + `NodeId` indices — cache-friendly,
  no deep `Box` chains, trivially `Clone`-free).
- Every node keeps its span.

### 3.4 Compile passes (all before first eval; all errors carry spans)

1. **Declaration collection** — `indicator()` header, top-level `plot*` calls
   (fixes the plot registry), `input.*` extraction (args must be
   const-foldable; else `PINE_INPUT_NOT_CONST`).
2. **Name resolution** — every variable/parameter resolved to a numbered
   **slot**; no string lookups at eval time. Unknown name → span error with a
   "did you mean" over builtins (cheap Levenshtein, big UX win).
3. **Call-site numbering** — every stateful builtin call gets a `CallSiteId`;
   user-function bodies get per-call-path instance keys (§2.5). Stateful call
   inside a loop body → `PINE_STATEFUL_IN_LOOP`.
4. **Const folding + length check** — `len` args of windowed kernels must
   fold to a positive int (literals, input values, arithmetic thereof); else
   `PINE_SERIES_LENGTH`.
5. **`max_bars_back` inference** — max constant `[n]` offset per slot;
   dynamic offsets capped by config (default 500) with reads beyond → NaN
   (documented).
6. **Unsupported-construct scan** — the §3.1 reject list, so *loading* a
   script tells the user everything wrong at once (collect all errors, don't
   stop at the first).

### 3.5 Interpreter

- Tree-walking over the arena, **slot-indexed environment** (`Vec<Value>`),
  zero string lookups and zero allocation in the per-bar path (pre-sized
  scratch stacks reused across bars).
- `Value`: small enum — `Num(f64)` (NaN = na), `Bool(bool)`, `Color(u32)`,
  `Str(Rc<str>)`, `Obj(ObjKind, ObjId)`, `Na`. Keep ≤ 16 bytes; unit-test
  `size_of`.
- Runtime type error / arity violation → `EvalError` with span + bar index →
  indicator enters error state (§2.8). Division by zero → NaN (Pine).
- Loop safety: `for`/`while` capped at 10 000 iterations per bar
  (`PINE_LOOP_BUDGET` runtime error) — a hang can never freeze the worker.
- User function calls: environment is one flat frame per call on a reused
  stack; recursion detected at compile time.
- `ScriptIndicator` (implements `Indicator`): owns AST + slots + kernel
  instances + object store; `on_close` = run script, commit; `preview` = §2.3
  snapshot/stage/rollback discipline.

### 3.6 Error model — AI-first

One error type, `PineError { code, span, message, notes }`, rendered two
ways: human (`file.pine:12:8: request.security is not supported on
activity-sampled charts (PINE_NO_SECURITY)`) and structured (tracing JSON
fields `schema_version, event_code, script, line, col`) — matching the
project's structured-log style (`orderflow_worker.rs`,
`feed-binance`). Stable `event_code`s make failures greppable and
AI-debuggable.

### 3.7 Module layout

```
crates/pine/src/
  lib.rs       // dialect summary, contract, re-exports
  lexer.rs     // tokens, indentation, spans
  parser.rs    // recursive descent → arena AST
  ast.rs       // Node, NodeId, spans
  compile.rs   // passes 1–6 → CompiledScript
  builtins.rs  // registry: name → (kind, arity, const-ness, callsite policy)
  eval.rs      // interpreter, Value, scratch stacks
  script.rs    // ScriptIndicator: Indicator impl, snapshot discipline
  error.rs     // PineError, codes, rendering
crates/pine/tests/       // conformance corpus + goldens (see §6)
```

Also deliverable (M2): `docs/pine-dialect.md` — the user-facing reference:
supported grammar, builtin list with *our* exact semantics (seeds, warmups,
NaN rules), divergences from TradingView, error-code catalog. Keep it in
lock-step with `builtins.rs` (a test asserts every registered builtin has a
doc entry — cheap doc-drift guard).

---

## 4. App integration

### 4.1 Threading — locked decision

Follow the `BookWorker` precedent (`app/src/orderflow_worker.rs`) exactly:
an **`IndicatorWorker`** thread owns the `IndicatorHost`. The UI never
touches host state.

UI → worker commands (unbounded std mpsc, like `BookCommand`):

```
Backfilled(Vec<Bar>)                  // initial replay
BarClosed(Bar)                        // live close
PartialUpdated(Option<Bar>)           // coalesced latest-wins in the batch loop
Rebuild(Vec<Bar>, Option<Bar>)        // set_spec / prepend_history / Reset
AddIndicator{ id, source }            // source: Native(kind) | Script(path, text)
RemoveIndicator(id) / SetInputs{ id, values } / ReloadScript{ id, text }
Flush(Sender<()>)                     // test barrier, mirrors BookCommand::Flush
```

Worker → UI **delta events** (bounded cost per event; the UI owns its own
copy of plot columns and applies deltas — same shape as the `FeedEvent`
pattern, avoids cloning full columns per bar and avoids locks in the render
path):

```
Rebuilt{ id, descriptor, columns }        // full columns moved (not cloned) once
Appended{ id, row }                       // one committed row per closed bar
Preview{ id, frame }                      // latest-wins; UI replaces previous
Objects{ id, objects }                    // full retained set (≤ 500/kind, small)
Error{ id, error: Option<…> }             // enter/leave error state
Progress{ id, done, total }               // recompute progress for the UI
```

Coalescing rule in the worker loop (mirrors `BookWorker::run`): drain the
queue into a batch; multiple `PartialUpdated` in one batch → only the newest
is evaluated. Preview cost is thereby bounded by worker loop cadence, not by
feed rate (a 50× replay can't melt it).

### 4.2 Lifecycle wiring (all in `app.rs`, small diffs)

- `FeedEvent::Backfilled` → after `state.ingest_backfill`, send
  `Backfilled(state.bars().to_vec())`.
- `ingest_live_trade` → if `state.bars().len()` grew, send `BarClosed(last)`;
  always send `PartialUpdated(state.partial().cloned())`.
- `set_spec` / `prepend_history` / `FeedEvent::Reset` →
  `Rebuild(bars, partial)`. (Replay seek already funnels through `Reset` —
  indicators inherit correct seek behavior for free.)

### 4.3 Rendering

- **Overlay indicators** draw on the price chart using the existing
  `PriceScale` + x-mapping from `chart.rs`/`viewport.rs` — indicator values
  participate in auto-scale (extend the hi/lo scan with visible overlay plot
  values).
- **Pane indicators** stack below the price chart, each with its own
  `PriceScale::auto` over its visible plot values; x-axis shared with the
  main chart. Pane height: fixed fraction v1 (e.g. 20% each, max 3 panes),
  draggable dividers later.
- Plot styles map to egui primitives: `Line/StepLine` →
  `Shape::line` polylines (batch points, one shape per plot per frame),
  `Histogram/Columns` → rects, `Circles/Cross` → point markers,
  `Area` → filled polygon + line. **NaN breaks a polyline into segments**
  (that's how warmup and conditional plots render as gaps).
- The forming bar's values come from the latest `Preview` frame and draw in
  the same pass (same x as the partial candle).
- Draw objects render after candles, before aggression bubbles (respect the
  existing paint-order contract in `candle_view.rs`).
- New modules: `app/src/indicators/mod.rs` (UI-side state: applies delta
  events, owns columns), `indicator_render.rs` (pure geometry where possible,
  mirroring the `chart.rs` vs `candle_view.rs` split), `indicator_panel.rs`
  (manager + settings UI).

### 4.4 Indicator manager UI

- A panel listing active indicators: colored status dot (ok / warning /
  error), title, eye toggle (hide without removing — render-side flag, no
  recompute), settings gear, remove.
- Settings dialog generated from `InputSpec` (int/float sliders or drag
  values with min/max/step, checkbox, color picker, text/options combo,
  source dropdown incl. flow series). Apply → `SetInputs` → recompute with
  progress.
- Error display: full `PineError` rendering (file:line:col + message + code),
  plus load warnings (ignored args, inert `alertcondition`).
- "Add indicator" browses the script library (§4.5) + native built-ins.

### 4.5 Script library

- Directory of `*.pine` files. Resolution: `indicators_dir` key in the app
  config (`AppConfig`), default `<config dir>/indicators`, created on first
  run; plus **embedded starter scripts** compiled in via `include_str!`
  (pattern: `EMBEDDED_DEFAULT` in `config.rs`) so the feature works with an
  empty folder.
- Starter pack (each doubles as a conformance fixture, §6): `ema.pine`,
  `cvd.pine` (uses `cvd` builtin), `delta_histogram.pine` (columns +
  barcolor by delta sign), `vwap_cumulative.pine` (honest name: cumulative
  from loaded history, not session-anchored — data honesty), `zigzag.pine`
  (pivots + lines + HH/LH/LL/HL labels), `range_box.pine` (rectangle over
  `highest`/`lowest` — the "zones" ask).
- Hot reload: mtime poll every ~1 s on the UI tick (no notify-crate
  dependency); changed file → `ReloadScript` → recompile + recompute; new
  compile errors show in the panel while **the last good version keeps
  running** (label it "stale — edit has errors", honesty again).

### 4.6 Persistence

Active indicator set + input values + per-plot color overrides saved to
`indicators-state.toml` next to the config (serde + toml, both already
dependencies). Load on startup, save on change (debounced). Scripts
themselves are *files* — the state file references them by path, embedded
built-ins by name. (M5; sessions before that are ephemeral.)

---

## 5. Performance engineering

Budgets (measured by benches, asserted in review, logged when exceeded):

| Path | Budget | Notes |
|---|---|---|
| Commit run, typical script (~200 AST nodes) | ≤ 50 µs | hard fail in bench review at 200 µs |
| Preview run | same + snapshot ≤ 10 µs | snapshot = small clones only (§2.3) |
| Full recompute, 10 scripts × 5 000 bars | ≤ 1.5 s worker time | non-blocking; `Progress` events; log `INDICATOR_RECOMPUTE_SLOW` above budget |
| Render, 3 000 visible points × 10 plots | ≤ 1 ms/frame | batched polylines; horizontal decimation (min/max per pixel column) only if the bench forces it — don't pre-build it |
| Live steady-state (10 bars/s, 10 scripts) | < 1% core | falls out of the 50 µs budget |

Rules that make the budgets real:

- No string lookups at eval time (slots + call-site ids, §3.4/§3.5).
- No allocation in the per-bar path: scratch stacks, pre-sized rows, reused
  buffers. A debug assertion counts allocations in `on_close` (feature-gated
  `alloc-count` dev check, or simply reviewed via bench numbers).
- Delta events UI-side: appending a row is O(plots); only `Rebuilt` moves
  bulk data, and it moves (not clones) it.
- Draw-object caps (500/kind/indicator) bound render + clone cost.
- Benches: `crates/indicators/benches/eval.rs` (kernels + native EMA over
  100k synthetic bars) and `crates/pine/benches/interp.rs` (the starter-pack
  scripts over the same bars), plain `harness = false` like
  `engine/benches/hot_path.rs`. Bench numbers go in PR descriptions
  (`arch-review` skill will ask for them).

If — and only if — the interpreter bench misses budget: first constant-fold
the AST harder, then consider a flat bytecode pass. Do not build the VM
speculatively.

---

## 6. Testing strategy

Test-first, engine-style: fixtures + expected outputs before implementation.

- **Kernel unit tests** (`indicators`): each `ta` kernel vs hand-computed
  reference values, incl. warmup, NaN propagation, deque edge cases
  (`highest` with plateaus), `valuewhen` occurrence indexing, pivot
  confirmation offsets.
- **Golden tests** (`indicators` + `pine`): reuse
  `quantick_engine::fixture` trade files → bars → run indicator → compare
  plot columns against a committed CSV (exact string equality of formatted
  f64 via `{:?}`/ryu — bit-exactness is the point). Harness mirrors
  `engine/src/golden.rs` + `engine/tests/golden_*.rs`.
- **Rollback tests**: feed trades so a bar forms across several
  `set_partial` calls → preview outputs change, committed state doesn't;
  close the bar → single commit; assert an EMA over "close, preview 3×,
  close" equals the EMA over "close, close" (the no-double-advance
  property). Same for `varip` (must *not* roll back) and preview draw
  objects (must vanish).
- **Parser/compile tests** (`pine`): a `tests/corpus/ok/*.pine` directory
  (must compile; includes the starter pack) and `tests/corpus/err/*.pine`
  (must fail with the *expected error code + line*, asserted). Multi-error
  collection asserted (a script with 3 problems reports 3).
- **Interpreter semantics tests**: history operator vs warmup, `var` vs
  plain, tuple destructuring, if/for as expressions, call-site identity
  (two `ta.ema(close, 9)` calls at different sites advance independently),
  user-function instance keys, loop budget error, div-by-zero → NaN.
- **Host/worker tests** (`app`, no egui): lifecycle mirroring the
  `ChartState` tests — backfill → live → set_spec → Reset; `Flush` barrier
  for determinism (pattern: `BookWorker::flush`); one erroring script
  doesn't affect a healthy one; delta-event stream replays to identical
  UI-side columns as a from-scratch `Rebuilt`.
- **Determinism guard**: run the whole starter pack twice over the same
  fixture in one test → identical outputs; plus the `fmath`-only grep test
  (§2.1).

---

## 7. Milestones

Each milestone = one or more PRs, each PR passing the full verification loop
(`fmt --check`, `clippy`, `build`, `test`, all `--workspace`; the lint
levels live in `[workspace.lints]`, not on the command line).
Branch names `feat/indicators-*`. Each milestone leaves `main` shippable.

### M1 — `indicators` crate + native EMA/CVD rendered (proves the whole pipe)

1. **PR: crate skeleton** — workspace member, `bar.rs`, `series.rs`,
   `indicator.rs`, `output.rs`, `input.rs`, golden harness. Tests first:
   series staging/truncate, `IndicatorBar` conversion, golden harness
   round-trip.
2. **PR: ta core** — `smooth.rs` (sma/ema/rma) + `window.rs`
   (highest/lowest/stdev/sum) + `flow.rs` (cum/change/cross*) with unit +
   golden tests; `fmath.rs` + grep guard.
3. **PR: host + native indicators** — `host.rs`, `native/` EMA + CVD,
   rollback tests, bench `eval.rs`.
4. **PR: app worker + render** — `IndicatorWorker`, delta events, UI-side
   state, overlay + one sub-pane rendering, hardcoded "add EMA / add CVD"
   entries in the UI. **This closes the roadmap item "CVD & delta visuals".**

Acceptance: EMA overlays the live chart and CVD draws in a pane, on live
Binance and on a 50× replay, forming-bar values previewing correctly; spec
switch and replay seek rebuild both; all four checks green.

### M2 — `pine` crate: language core

5. **PR: lexer + parser** — corpus tests (ok/err), spans, indentation.
6. **PR: compile passes** — slots, call-sites, const folding, input
   extraction, unsupported scan; error-code catalog; multi-error reporting.
7. **PR: interpreter + ScriptIndicator** — semantics tests, rollback via the
   M1 test harness, `plot`/`hline` only; bench `interp.rs`.
8. **PR: app "load script"** — library dir scan + embedded `ema.pine` /
   `cvd.pine` / `delta_histogram.pine`; errors in the panel;
   `docs/pine-dialect.md` first full version.

Acceptance: `ema.pine` edited on disk hot-reloads (M2 may land reload as
manual "reload" button if the mtime poll slips to M4); a script with
`request.security` shows the honest error with line number.

### M3 — drawing & pivots (the zigzag milestone)

9. **PR: objects** — `objects.rs` store/caps/GC + `line/box/label` builtins +
   preview-transient handling + render pass.
10. **PR: pivots + shape plots** — `ta.pivothigh/pivotlow`,
    `plotshape/plotchar`, `fill`, `bgcolor`, `barcolor`;
    `zigzag.pine` + `range_box.pine` land as conformance fixtures.

Acceptance: zigzag draws lines + HH/LH/HL/LL labels on a flow chart and
survives rebuild/seek; range boxes render; object caps enforced.

### M4 — inputs UI, hot reload, polish

11. **PR: settings UI** from `InputSpec` + recompute-on-apply + progress.
12. **PR: hot reload** (mtime poll, stale-version rule) + full starter pack +
    "did you mean" diagnostics + `switch` if it slipped.

### M5 — persistence + hardening

13. **PR: persistence** (`indicators-state.toml`), pane layout polish.
14. **PR: perf pass** — bench review against §5 budgets, README roadmap
    update, `CLAUDE.md` architecture section gains the two crates, dialect
    doc completeness test.

---

## 8. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Scope creep toward "full Pine" | §3.1 reject list is the contract; anything new needs a plan change, not a quiet PR |
| Preview subtly mutating state (the classic bug of this design) | the rollback property tests in §6 are written *before* `ScriptIndicator`; treat them as the spec |
| Interpreter too slow for big replays | budgets + benches from M2 day one; escalation path in §5 (fold → bytecode), decided by numbers |
| f64 cross-platform drift | libm-only transcendentals + grep guard + golden CSVs run in CI on every platform CI covers |
| Community scripts failing confusingly | multi-error reporting, "did you mean", error-code catalog in the dialect doc, warnings surfaced in the panel |
| UI thread jank from recompute | worker thread + `Progress` events; no synchronous eval anywhere in `app.rs` |
| Doc drift between dialect doc and builtins | registry-vs-doc completeness test (§3.7) |

## 9. Locked decisions (summary)

f64 + libm (not Decimal) · full columns (no ring buffers v1) ·
truncate-don't-clone series staging, clone-small-state snapshots ·
call-site kernel identity, stateful-in-loop = compile error ·
EMA/RMA seed = SMA, warmup = NaN, TV parity a non-goal ·
tree-walking interpreter with slot resolution, bytecode only if benches fail ·
worker thread + delta events (BookWorker/FeedEvent precedents) ·
500 objects/kind/indicator cap · plots fixed at top level ·
hand-rolled zero-dep lexer/parser · errors = stable codes + spans, AI-first
structured logs · engine crate untouched.
