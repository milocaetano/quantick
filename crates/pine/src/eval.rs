//! Builtin adapters and transactional state for the compiled register machine.
//!
//! Slot-indexed environments (globals + one flat reused frame stack), dense
//! compile tables for every reference, kernel instances keyed by
//! `(call site, call path)` — the §2.5 identity rule: each *textual* call
//! owns its state, and a user function called from two places owns two.
//! Paths are interned integers, so the per-bar path allocates nothing after
//! its first bar.
//!
//! History (`x[n]`) works on *expressions*: every `History` node owns an
//! implicit ring of its subject's committed per-bar values. Reads at `n = 0`
//! see the value being computed now; deeper reads see committed bars. A
//! subject only records on bars where its node actually evaluates — history
//! across untaken branches reads `na`, a documented divergence.
//!
//! `Value` stays ≤ 16 bytes (unit-tested): the per-bar path moves it by
//! copy, and the environments are flat `Vec<Value>`s.

use std::collections::BTreeMap;
use std::rc::Rc;

use quantick_indicators::{
    Ctx, IndicatorBar, InputValue, MarkerLocation, ObjectStore, Rgba8, SourceId, ta,
};

use crate::ast::{BinOp, NodeId, VarMode};
use crate::builtins::Builtin;
use crate::compile::{CompiledScript, Resolution};
use crate::error::{ErrorCode, PineError, Span};

#[path = "ir/loops.rs"]
mod ir_loops;
#[path = "ir/runtime.rs"]
mod ir_runtime;
mod kernels;
mod objects;
mod pure;
#[path = "semantics/runtime.rs"]
mod semantic_runtime;

/// Iteration budget per `for`/`while` per bar — a runaway loop errors the
/// indicator instead of freezing the worker (§3.5).
pub const LOOP_BUDGET: u32 = 10_000;

pub use crate::types::{ObjKind, Value};

/// One kernel instance behind a `(call site, path)` key.
#[derive(Clone)]
pub(crate) enum Kernel {
    Macd(crate::semantics::Macd),
    Sma(ta::Sma),
    Ema(ta::Ema),
    Rma(ta::Rma),
    Wma(ta::Wma),
    Vwma(ta::Vwma),
    Rsi(ta::Rsi),
    Tr(ta::Tr),
    Atr(ta::Atr),
    Stdev(ta::Stdev),
    Highest(ta::Highest),
    Lowest(ta::Lowest),
    HighestBars(ta::HighestBars),
    LowestBars(ta::LowestBars),
    Change(ta::Change),
    Cum(ta::Cum),
    CrossOver(ta::CrossOver),
    CrossUnder(ta::CrossUnder),
    Cross(ta::Cross),
    BarsSince(ta::BarsSince),
    ValueWhen(ta::ValueWhen),
    PivotHigh(ta::PivotHigh),
    PivotLow(ta::PivotLow),
    WindowSum(ta::WindowSum),
    /// `fixnan`: the last non-na value seen.
    Fixnan(f64),
}

/// One implicit expression-history ring: committed values, newest last,
/// bounded by the script's `max_bars_back`.
///
/// A `VecDeque` rather than a `Vec`: dropping the oldest value with
/// `Vec::remove(0)` shifted the whole ring on every commit once it filled —
/// up to 500 entries per history node per closed bar, and tick bars close
/// many times a second under a dense tape.
#[derive(Clone, Default)]
pub(crate) struct HistRing {
    values: std::collections::VecDeque<Value>,
}

/// Everything that persists across bars — the state a preview snapshots.
#[derive(Default)]
pub(crate) struct ScriptState {
    /// Global slot values (current bar's view).
    pub globals: Vec<Value>,
    /// Whether a `var`/`varip` slot ran its initialiser.
    pub var_done: Vec<bool>,
    /// Kernel instances by (call site, interned path).
    pub kernels: BTreeMap<(u32, u32), Kernel>,
    /// The length each kernel was instantiated with, same key. A kernel is
    /// built once and keeps its window forever, so a length that changes
    /// between bars — `f(n) => ta.sma(close, n)` called with a moving `n` —
    /// would silently keep bar zero's window while the plot title claimed
    /// otherwise. Pine refuses a series length; so do we, honestly.
    pub kernel_lengths: BTreeMap<(u32, u32), usize>,
    /// Expression histories by (history id, interned path).
    pub histories: BTreeMap<(u32, u32), HistRing>,
    /// Values staged for the forming bar's histories, flushed on commit.
    pub staged_histories: BTreeMap<(u32, u32), Value>,
    /// Retained draw objects (lines/boxes/labels, §2.6 caps).
    pub objects: ObjectStore,
    /// Path interning: (parent path, call node) → child path.
    pub path_children: BTreeMap<(u32, u32), u32>,
    /// Next fresh path id (0 = the root path).
    pub next_path: u32,
    /// The reused local-frame stack. It lives here, not in `Eval`, so its
    /// capacity survives across bars: a fresh `Vec` per bar meant any script
    /// with a user function paid a malloc/free every bar, in a walk
    /// documented as allocation-free after the first one.
    pub locals: Vec<Value>,
    pub registers: Vec<Value>,
    pub returns: Vec<usize>,
}

impl ScriptState {
    pub(crate) fn new(global_slots: usize) -> Self {
        Self {
            locals: Vec::new(),
            globals: vec![Value::Na; global_slots],
            var_done: vec![false; global_slots],
            next_path: 1,
            ..Self::default()
        }
    }

    /// The clone-small-state part of a preview snapshot (§2.3): globals,
    /// var flags, kernels and path table. Histories are committed-only, so
    /// rollback for them is just dropping the staged map.
    pub(crate) fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            globals: self.globals.clone(),
            var_done: self.var_done.clone(),
            kernels: self.kernels.clone(),
            kernel_lengths: self.kernel_lengths.clone(),
            objects: self.objects.clone(),
        }
    }

    /// Restore after a preview. `varip` slots keep their preview mutations —
    /// that is the whole point of `varip` (§2.3).
    pub(crate) fn restore(&mut self, snapshot: StateSnapshot, slot_modes: &[VarMode]) {
        let StateSnapshot {
            globals,
            var_done,
            kernels,
            kernel_lengths,
            objects,
        } = snapshot;
        for (slot, (value, done)) in globals.into_iter().zip(var_done).enumerate() {
            if slot_modes.get(slot) != Some(&VarMode::Varip) {
                self.globals[slot] = value;
                self.var_done[slot] = done;
            }
        }
        self.kernels = kernels;
        self.kernel_lengths = kernel_lengths;
        // Not a plain assignment: `restore_from` keeps the object id counter
        // moving forward, so a `varip` handle that survives this rollback
        // cannot collide with an id the commit run is about to allocate.
        self.objects.restore_from(objects);
        self.staged_histories.clear();
    }

    /// Commit the forming bar: flush staged history values into their rings.
    pub(crate) fn commit_bar(&mut self, max_bars_back: usize) {
        self.commit_histories(max_bars_back);
    }

    pub(crate) fn reset(&mut self) {
        let slots = self.globals.len();
        *self = Self::new(slots);
    }
}

/// What [`ScriptState::snapshot`] captures.
pub(crate) struct StateSnapshot {
    globals: Vec<Value>,
    var_done: Vec<bool>,
    kernels: BTreeMap<(u32, u32), Kernel>,
    kernel_lengths: BTreeMap<(u32, u32), usize>,
    objects: ObjectStore,
}

/// What one run stages for its caller: this bar's plot cells and the candle
/// paint it asked for.
///
/// The two travel together because they *are* one thing — the output of one
/// evaluation — and because the caller reads them together after a clean run:
/// a row committed without its paint, or with a stale one, would put a colour
/// on the wrong candle.
#[path = "candle_plots/eval.rs"]
mod candle_eval;

pub(crate) struct Staged<'a> {
    /// One cell per plot; pushed by the caller after a clean run.
    pub row: &'a mut [f64],
    pub candles: &'a mut [Option<quantick_indicators::CandleSample>],
    /// The paint `barcolor` asked for; `None` = this bar asked for none.
    pub paint: &'a mut Option<Rgba8>,
}

/// One run of the script over one bar (commit or preview).
pub(crate) struct Eval<'a> {
    pub script: &'a CompiledScript,
    pub inputs: &'a [InputValue],
    pub bar: &'a IndicatorBar,
    pub ctx: &'a Ctx<'a>,
    pub state: &'a mut ScriptState,
    /// One staged cell per plot; pushed by the caller after a clean run.
    pub row: &'a mut [f64],
    pub candles: &'a mut [Option<quantick_indicators::CandleSample>],
    /// The bar paint this run asked for (`barcolor`), staged like the row and
    /// read by the caller after a clean run. `None` = this bar asked for none.
    pub paint: &'a mut Option<Rgba8>,
    /// True on commit runs (`barstate.isconfirmed`).
    pub is_commit: bool,
    pub timezone: crate::calendar::CalendarTimezone,

    /// Current interned call path (0 = top level).
    pub path: u32,
    /// Current frame base in `locals`.
    frame_base: usize,
    type_context: u32,
    semantic_run: crate::semantics::Run<Kernel>,
}

impl<'a> Eval<'a> {
    pub(crate) fn new(
        script: &'a CompiledScript,
        inputs: &'a [InputValue],
        bar: &'a IndicatorBar,
        ctx: &'a Ctx<'a>,
        state: &'a mut ScriptState,
        staged: Staged<'a>,
        is_commit: bool,
    ) -> Self {
        state
            .registers
            .resize(script.program.entries.len(), Value::Na);
        Self {
            script,
            inputs,
            bar,
            ctx,
            state,
            row: staged.row,
            candles: staged.candles,
            paint: staged.paint,
            is_commit,
            timezone: crate::calendar::CalendarTimezone::default(),
            path: 0,
            frame_base: 0,
            type_context: 0,
            semantic_run: crate::semantics::Run::default(),
        }
    }

    /// Run the whole script top to bottom for this bar.
    pub(crate) fn run(&mut self) -> Result<(), PineError> {
        self.eval(self.script.root).map(|_| ())
    }

    /// The node's kind, borrowed from the *script* rather than from `self`.
    ///
    /// That lifetime is what lets `match self.kind(id) { … }` call
    /// `self.eval(..)` inside its arms. Cloning to dodge the borrow cost one
    /// allocation per evaluated node per bar — `Name(String)`, `Call(Vec<Arg>)`
    /// and friends are the common cases — against a module that promises the
    /// per-bar walk allocates nothing after its first bar.
    /// Pin a kernel's length on first use and refuse a later change.
    fn pin_kernel_length(
        &mut self,
        key: (u32, u32),
        len: usize,
        id: NodeId,
    ) -> Result<(), PineError> {
        match self.state.kernel_lengths.get(&key) {
            Some(&pinned) if pinned != len => Err(self.err(
                id,
                ErrorCode::PineSeriesLength,
                format!(
                    "this kernel was built with length {pinned} and cannot change to                      {len} mid-stream; a kernel's window is fixed when it is created"
                ),
            )),
            Some(_) => Ok(()),
            None => {
                self.state.kernel_lengths.insert(key, len);
                Ok(())
            }
        }
    }

    fn span(&self, id: NodeId) -> Span {
        self.script.program.spans[id.index()]
    }

    fn err(&self, id: NodeId, code: ErrorCode, message: impl Into<String>) -> PineError {
        PineError::new(code, self.span(id), message)
    }

    fn type_err(&self, id: NodeId, expected: &str, got: &Value) -> PineError {
        self.err(
            id,
            ErrorCode::PineType,
            format!("expected {expected}, got {}", describe(got)),
        )
    }

    fn num(&mut self, id: NodeId) -> Result<f64, PineError> {
        let value = self.eval(id)?;
        value
            .as_num()
            .ok_or_else(|| self.type_err(id, "a number", &value))
    }

    fn cond(&mut self, id: NodeId) -> Result<bool, PineError> {
        let value = self.eval(id)?;
        value
            .as_cond()
            .ok_or_else(|| self.type_err(id, "a bool condition", &value))
    }

    fn eval(&mut self, id: NodeId) -> Result<Value, PineError> {
        let base = self.state.returns.len();
        let result = self.execute(id);
        self.state.returns.truncate(base);
        result
    }

    /// Write a value through a node's resolution (the `for` loop variable).
    fn write_resolved(&mut self, id: NodeId, value: Value) {
        match self.script.resolutions[id.index()] {
            Resolution::Global(slot) => self.state.globals[slot as usize] = value,
            Resolution::Local(slot) => {
                let index = self.frame_base + slot as usize;
                self.state.locals[index] = value;
            }
            _ => {}
        }
    }

    fn builtin_variable(&mut self, id: NodeId, builtin: Builtin) -> Result<Value, PineError> {
        let bar = self.bar;
        let value = match builtin {
            Builtin::Open => bar.open,
            Builtin::High => bar.high,
            Builtin::Low => bar.low,
            Builtin::Close => bar.close,
            Builtin::Volume => bar.volume(),
            Builtin::BuyVolume => bar.buy_volume,
            Builtin::SellVolume => bar.sell_volume,
            Builtin::Delta => bar.delta(),
            Builtin::Cvd => self.ctx.cvd_now(),
            Builtin::TradeCount => bar.trade_count,
            Builtin::Hl2 => bar.hl2(),
            Builtin::Hlc3 => bar.hlc3(),
            Builtin::Ohlc4 => bar.ohlc4(),
            Builtin::Hlcc4 => bar.hlcc4(),
            Builtin::BarIndex => self.ctx.bar_index as f64,
            // The newest known bar: during any evaluation that is the bar
            // being evaluated (catch-up replays included) — documented.
            Builtin::LastBarIndex => self.ctx.bar_index as f64,
            Builtin::Time => crate::calendar::open_time(bar),
            Builtin::TimeClose => crate::calendar::close_time(bar),
            Builtin::Calendar(field) => field.at(bar.open_time, self.timezone),
            Builtin::BarstateIsConfirmed => return Ok(Value::Bool(self.is_commit)),
            Builtin::BarstateIsLast => return Ok(Value::Bool(!self.is_commit)),
            other => {
                return Err(self.err(
                    id,
                    ErrorCode::PineType,
                    format!("`{other:?}` is a function; call it with arguments"),
                ));
            }
        };
        Ok(Value::Num(value))
    }

    fn history(&mut self, id: NodeId, subject: NodeId, offset: NodeId) -> Result<Value, PineError> {
        let current = self.eval(subject)?;
        let offset_value = self.num(offset)?;
        if offset_value.is_nan() || offset_value < 0.0 {
            return Ok(Value::Na);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let back = offset_value as usize;
        let series = self.script.history_series[id.index()].expect("history node has a series");
        let key = (series, self.path);
        // Stage this bar's value (last write wins within the bar).
        if !self.semantic_run.skipped {
            self.state.staged_histories.insert(key, current.clone());
        }
        if back == 0 {
            return Ok(current);
        }
        let ring = self.state.histories.get(&key);
        let value = ring
            .and_then(|r| {
                r.values
                    .len()
                    .checked_sub(back)
                    .and_then(|i| r.values.get(i).cloned())
            })
            .unwrap_or_else(|| current.missing_like());
        Ok(value)
    }

    fn binary(&mut self, op: BinOp, lhs: NodeId, rhs: NodeId) -> Result<Value, PineError> {
        match op {
            BinOp::And => {
                // Short-circuit; na counts false (Pine).
                if !self.cond(lhs)? {
                    if !self.script.version.lazy_boolean() {
                        self.advance_skipped(rhs)?;
                    }
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(self.cond(rhs)?))
            }
            BinOp::Or => {
                if self.cond(lhs)? {
                    if !self.script.version.lazy_boolean() {
                        self.advance_skipped(rhs)?;
                    }
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(self.cond(rhs)?))
            }
            _ => unreachable!("non-logical operators execute register instructions"),
        }
    }

    fn user_call(
        &mut self,
        id: NodeId,
        index: u32,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        // Read the Copy fields; cloning the whole `FunctionInfo` — its `name`
        // included, which its own doc calls diagnostics-only — happened on
        // every user-function call on every bar. The name is taken by
        // reference on the error path alone.
        let (child_context, index, order) = self
            .script
            .types
            .dispatch
            .get(&(self.type_context, id.index()))
            .map(|(context, index, order)| (*context, *index, order.as_slice()))
            .unwrap_or((self.type_context, index, &[]));
        let function = &self.script.functions[index as usize];
        let (param_count, local_slots, body) =
            (function.param_count, function.local_slots, function.body);
        if args.len() != param_count {
            let name = self.script.functions[index as usize].name.clone();
            return Err(self.err(
                id,
                ErrorCode::PineArity,
                format!("`{name}` takes {param_count} arguments, got {}", args.len()),
            ));
        }
        // Evaluate arguments in the caller's frame and path.
        let mut argv = Vec::with_capacity(args.len());
        for arg in args {
            argv.push(self.eval(arg.value)?);
        }
        // Enter the callee: fresh frame on the reused stack, child path
        // interned from (caller path, this call node).
        let call_key = (
            self.path,
            u32::try_from(id.index()).expect("arena fits u32"),
        );
        let path = match self.state.path_children.get(&call_key) {
            Some(&child) => child,
            None => {
                let child = self.state.next_path;
                self.state.next_path += 1;
                self.state.path_children.insert(call_key, child);
                child
            }
        };
        let saved_base = self.frame_base;
        let saved_path = self.path;
        self.frame_base = self.state.locals.len();
        self.state
            .locals
            .resize(self.frame_base + local_slots, Value::Na);
        for (slot, argument) in order.iter().enumerate() {
            self.state.locals[self.frame_base + slot] = argv[*argument].clone();
        }
        self.path = path;
        let saved_context = std::mem::replace(&mut self.type_context, child_context);
        let result = self.eval(body);
        self.type_context = saved_context;
        self.state.locals.truncate(self.frame_base);
        self.frame_base = saved_base;
        self.path = saved_path;
        result
    }

    #[allow(clippy::too_many_lines)]
    fn builtin_call(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        if self.semantic_run.skipped
            && matches!(
                builtin,
                Builtin::LineNew | Builtin::BoxNew | Builtin::LabelNew | Builtin::Barcolor
            )
        {
            self.advance_arguments(args)?;
            return Ok(Value::Na);
        }
        // Output calls first: they stage into the plot row.
        match builtin {
            Builtin::PlotCandle | Builtin::PlotBar => return self.candle_plot(id, args),
            Builtin::Plot | Builtin::Hline => {
                let value = match args.first() {
                    Some(arg) => self.num(arg.value)?,
                    None => f64::NAN,
                };
                if let Some(index) = self.script.plot_of_call[id.index()] {
                    self.row[index] = value;
                }
                return Ok(Value::Na);
            }
            Builtin::PlotShape | Builtin::PlotChar => {
                let Some(index) = self.script.plot_of_call[id.index()] else {
                    return Ok(Value::Na);
                };
                let marker_value = match args.first() {
                    Some(arg) => {
                        let is_absolute = self.script.plots[index]
                            .marker
                            .as_ref()
                            .is_some_and(|m| m.location == MarkerLocation::Absolute);
                        if is_absolute {
                            // The series value is the marker's y.
                            self.num(arg.value)?
                        } else {
                            let v = self.eval(arg.value)?;
                            let fires = v.as_cond().unwrap_or(false)
                                || matches!(&v, Value::Num(n) if !n.is_nan() && *n != 0.0);
                            if fires { 1.0 } else { f64::NAN }
                        }
                    }
                    None => f64::NAN,
                };
                self.row[index] = marker_value;
                return Ok(Value::Na);
            }
            Builtin::Barcolor => {
                // Each call settles the bar outright: a colour paints it, `na`
                // leaves it unpainted, and the last call of the bar is the one
                // that counts. Not calling it at all — the `if` that did not
                // fire — leaves the candle exactly as the chart drew it.
                let value = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                *self.paint = match value {
                    Value::Color(rgba) => Some(Rgba8::from_u32(rgba)),
                    value if value.is_na() => None,
                    other => return Err(self.type_err(id, "a color", &other)),
                };
                return Ok(Value::Na);
            }
            // fill() is fully resolved at load time; at eval it is a no-op.
            // Accepted-but-inert output calls still evaluate their first
            // argument (consistency: kernels inside advance every bar).
            Builtin::Fill | Builtin::Bgcolor | Builtin::AlertCondition | Builtin::Indicator => {
                if let Some(arg) = args.first() {
                    let _ = self.eval(arg.value)?;
                }
                return Ok(Value::Na);
            }
            _ => {}
        }
        match builtin {
            Builtin::LineNew => return self.line_new(args),
            Builtin::BoxNew => return self.box_new(args),
            Builtin::LabelNew => return self.label_new(args),
            _ => {}
        }
        if let Some(input_index) = self.script.input_of_call[id.index()] {
            return Ok(input_value(&self.inputs[input_index], self.bar, self.ctx));
        }
        if let Some(site) = self.script.call_sites[id.index()] {
            return self.cached_kernel(id, builtin, site.0, args);
        }
        self.pure_call(id, builtin, args)
    }

    fn arg_num(&mut self, args: &[crate::ast::Arg], position: usize) -> Result<f64, PineError> {
        if let Some(values) = &self.semantic_run.arguments {
            return values.get(position).map_or(Ok(f64::NAN), |value| {
                value
                    .as_num()
                    .ok_or_else(|| self.type_err(args[position].value, "a number", value))
            });
        }
        match args.get(position) {
            Some(arg) => self.num(arg.value),
            None => Ok(f64::NAN),
        }
    }
}

/// An input's bound value as a runtime [`Value`]. Sources resolve against
/// the current bar (and the shared cvd).
fn input_value(input: &InputValue, bar: &IndicatorBar, ctx: &Ctx<'_>) -> Value {
    match input {
        InputValue::Int(v) => Value::Num(*v as f64),
        InputValue::Float(v) => Value::Num(*v),
        InputValue::Bool(v) => Value::Bool(*v),
        InputValue::Color(c) => Value::Color(c.to_u32()),
        InputValue::Str(s) => Value::Str(Rc::new(s.clone())),
        InputValue::Source(source) => Value::Num(source_value(*source, bar, ctx)),
    }
}

fn source_value(source: SourceId, bar: &IndicatorBar, ctx: &Ctx<'_>) -> f64 {
    match source {
        SourceId::Cvd => ctx.cvd_now(),
        other => other.value(bar, f64::NAN),
    }
}

/// `==` semantics shared by the operator and switch matching: numbers by
/// value (NaN never equals), same-kind primitives by value, everything
/// else false.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(a), Value::Num(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Color(a), Value::Color(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        _ => false,
    }
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Obj(..) => "an object handle",
        Value::Num(_) => "a number",
        Value::Bool(_) => "a bool",
        Value::Color(_) => "a color",
        Value::Str(_) => "a string",
        Value::Tuple(_) => "a tuple",
        Value::Na | Value::NaBool | Value::NaColor | Value::NaString => "na",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_stays_small() {
        assert!(
            std::mem::size_of::<Value>() <= 16,
            "Value is {} bytes; the per-bar path copies it",
            std::mem::size_of::<Value>()
        );
    }
}
