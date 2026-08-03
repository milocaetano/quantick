//! The tree-walking interpreter.
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
    BoxObj, Ctx, IndicatorBar, InputValue, LabelObj, LabelStyle, LineObj, MarkerLocation, ObjectId,
    ObjectStore, Rgba8, SourceId, fmath, ta,
};

use crate::ast::{BinOp, NodeId, NodeKind, UnOp, VarMode};
use crate::builtins::Builtin;
use crate::compile::{CompiledScript, Resolution};
use crate::error::{ErrorCode, PineError, Span};

/// Iteration budget per `for`/`while` per bar — a runaway loop errors the
/// indicator instead of freezing the worker (§3.5).
pub const LOOP_BUDGET: u32 = 10_000;

/// Default draw-object stroke/border/label color (a readable blue).
const DEFAULT_OBJECT_COLOR: Rgba8 = Rgba8::opaque(41, 98, 255);
/// Default box fill: the border color at ~85% transparency.
const DEFAULT_BOX_FILL: Rgba8 = Rgba8::new(41, 98, 255, 38);
/// Default label text color.
const DEFAULT_LABEL_TEXT: Rgba8 = Rgba8::opaque(255, 255, 255);
/// Default line width in points.
const DEFAULT_LINE_WIDTH: f32 = 1.0;

/// A runtime value. `na` for numbers is NaN inside `Num`; the distinct `Na`
/// covers non-numeric contexts.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number (NaN = na).
    Num(f64),
    /// A boolean.
    Bool(bool),
    /// A packed RGBA color.
    Color(u32),
    /// A string (thin Rc: fat pointers would push Value past 16 bytes,
    /// and strings never ride the hot numeric path).
    Str(Rc<String>),
    /// A function's multi-return (thin Rc, same reasoning).
    Tuple(Rc<Vec<Value>>),
    /// A draw-object handle (`line.new` & co.).
    Obj(ObjKind, u32),
    /// `na` outside numeric context.
    Na,
}

/// Which store a [`Value::Obj`] handle points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjKind {
    /// A `line` handle.
    Line,
    /// A `box` handle.
    Box,
    /// A `label` handle.
    Label,
}

impl Value {
    /// Numeric view: numbers pass through, `na` is NaN, anything else is a
    /// type error at the caller.
    fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(v) => Some(*v),
            Value::Na => Some(f64::NAN),
            _ => None,
        }
    }

    /// Condition view: bools pass, `na` is false (Pine's rule).
    fn as_cond(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Na => Some(false),
            Value::Num(v) if v.is_nan() => Some(false),
            _ => None,
        }
    }

    fn is_na(&self) -> bool {
        matches!(self, Value::Na) || matches!(self, Value::Num(v) if v.is_nan())
    }
}

/// One kernel instance behind a `(call site, path)` key.
#[derive(Clone)]
pub(crate) enum Kernel {
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
            objects,
        } = snapshot;
        for (slot, (value, done)) in globals.into_iter().zip(var_done).enumerate() {
            if slot_modes.get(slot) != Some(&VarMode::Varip) {
                self.globals[slot] = value;
                self.var_done[slot] = done;
            }
        }
        self.kernels = kernels;
        // Not a plain assignment: `restore_from` keeps the object id counter
        // moving forward, so a `varip` handle that survives this rollback
        // cannot collide with an id the commit run is about to allocate.
        self.objects.restore_from(objects);
        self.staged_histories.clear();
    }

    /// Commit the forming bar: flush staged history values into their rings.
    pub(crate) fn commit_bar(&mut self, max_bars_back: usize) {
        for ((series, path), value) in std::mem::take(&mut self.staged_histories) {
            let ring = self.histories.entry((series, path)).or_default();
            ring.values.push_back(value);
            if ring.values.len() > max_bars_back {
                ring.values.pop_front();
            }
        }
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
    objects: ObjectStore,
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
    /// True on commit runs (`barstate.isconfirmed`).
    pub is_commit: bool,

    /// Current interned call path (0 = top level).
    pub path: u32,
    /// Current frame base in `locals`.
    frame_base: usize,
}

impl<'a> Eval<'a> {
    pub(crate) fn new(
        script: &'a CompiledScript,
        inputs: &'a [InputValue],
        bar: &'a IndicatorBar,
        ctx: &'a Ctx<'a>,
        state: &'a mut ScriptState,
        row: &'a mut [f64],
        is_commit: bool,
    ) -> Self {
        Self {
            script,
            inputs,
            bar,
            ctx,
            state,
            row,
            is_commit,
            path: 0,
            frame_base: 0,
        }
    }

    /// Run the whole script top to bottom for this bar.
    pub(crate) fn run(&mut self) -> Result<(), PineError> {
        let NodeKind::Script { statements } = &self.script.ast.node(self.script.root).kind else {
            return Ok(());
        };
        for &statement in statements {
            self.statement(statement)?;
        }
        Ok(())
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

    fn kind(&self, id: NodeId) -> &'a NodeKind {
        &self.script.ast.node(id).kind
    }

    fn span(&self, id: NodeId) -> Span {
        self.script.ast.node(id).span
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

    // ---- statements ------------------------------------------------------

    fn statement(&mut self, id: NodeId) -> Result<Value, PineError> {
        match self.kind(id) {
            NodeKind::Declare { mode, value, .. } => {
                let resolution = self.script.resolutions[id.index()];
                match resolution {
                    Resolution::Global(slot) => {
                        let slot = slot as usize;
                        // var/varip initialisers run once; later bars keep
                        // the carried value (Pine's rule).
                        if *mode != VarMode::Plain && self.state.var_done[slot] {
                            return Ok(self.state.globals[slot].clone());
                        }
                        let v = self.eval(*value)?;
                        self.state.globals[slot] = v.clone();
                        if *mode != VarMode::Plain {
                            self.state.var_done[slot] = true;
                        }
                        Ok(v)
                    }
                    Resolution::Local(slot) => {
                        let v = self.eval(*value)?;
                        let index = self.frame_base + slot as usize;
                        self.state.locals[index] = v.clone();
                        Ok(v)
                    }
                    _ => Ok(Value::Na),
                }
            }
            NodeKind::TupleDeclare { names, value } => {
                let v = self.eval(*value)?;
                let Value::Tuple(items) = &v else {
                    return Err(self.type_err(id, "a tuple", &v));
                };
                if items.len() != names.len() {
                    return Err(self.err(
                        id,
                        ErrorCode::PineType,
                        format!("expected {} tuple values, got {}", names.len(), items.len()),
                    ));
                }
                match self.script.resolutions[id.index()] {
                    Resolution::Global(first) => {
                        for (offset, item) in items.iter().enumerate() {
                            self.state.globals[first as usize + offset] = item.clone();
                        }
                    }
                    Resolution::Local(first) => {
                        for (offset, item) in items.iter().enumerate() {
                            let index = self.frame_base + first as usize + offset;
                            self.state.locals[index] = item.clone();
                        }
                    }
                    _ => {}
                }
                Ok(v)
            }
            NodeKind::Reassign { value, .. } => {
                let v = self.eval(*value)?;
                match self.script.resolutions[id.index()] {
                    Resolution::Global(slot) => self.state.globals[slot as usize] = v.clone(),
                    Resolution::Local(slot) => {
                        let index = self.frame_base + slot as usize;
                        self.state.locals[index] = v.clone();
                    }
                    _ => {}
                }
                Ok(v)
            }
            // Definitions execute nothing by themselves.
            NodeKind::FunctionDef { .. } => Ok(Value::Na),
            NodeKind::ExprStmt { expr } => self.eval(*expr),
            _ => self.eval(id),
        }
    }

    // ---- expressions -----------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn eval(&mut self, id: NodeId) -> Result<Value, PineError> {
        match self.kind(id) {
            NodeKind::Int(v) => Ok(Value::Num(*v as f64)),
            NodeKind::Float(v) => Ok(Value::Num(*v)),
            NodeKind::Str(v) => Ok(Value::Str(Rc::new(v.clone()))),
            NodeKind::Bool(v) => Ok(Value::Bool(*v)),
            NodeKind::Color(v) => Ok(Value::Color(*v)),
            NodeKind::Na => Ok(Value::Na),
            NodeKind::Tuple { items } => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(*item)?);
                }
                Ok(Value::Tuple(Rc::new(values)))
            }
            NodeKind::Name(_) => self.name_value(id),
            NodeKind::Member { .. } => match self.script.resolutions[id.index()] {
                Resolution::Builtin(builtin) => self.builtin_variable(id, builtin),
                Resolution::ColorConst(rgba) => Ok(Value::Color(rgba)),
                Resolution::EnumConst(tag) => Ok(Value::Str(Rc::new(tag.to_owned()))),
                _ => Err(self.err(
                    id,
                    ErrorCode::PineType,
                    "object members are not available yet (draw objects land later)",
                )),
            },
            NodeKind::Unary { op, operand } => match op {
                UnOp::Neg => {
                    let v = self.num(*operand)?;
                    Ok(Value::Num(-v))
                }
                UnOp::Not => {
                    let v = self.cond(*operand)?;
                    Ok(Value::Bool(!v))
                }
            },
            NodeKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs),
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                if self.cond(*condition)? {
                    self.eval(*if_true)
                } else {
                    self.eval(*if_false)
                }
            }
            NodeKind::If {
                condition,
                then_block,
                otherwise,
            } => {
                if self.cond(*condition)? {
                    self.eval(*then_block)
                } else if let Some(otherwise) = otherwise {
                    self.eval(*otherwise)
                } else {
                    Ok(Value::Na)
                }
            }
            NodeKind::Block { statements } => {
                let mut last = Value::Na;
                for statement in statements {
                    last = self.statement(*statement)?;
                }
                Ok(last)
            }
            NodeKind::For {
                from, to, by, body, ..
            } => {
                let from = self.num(*from)?;
                let to = self.num(*to)?;
                let step = match by {
                    Some(by) => self.num(*by)?,
                    None => 1.0,
                };
                if !from.is_finite() || !to.is_finite() || step == 0.0 || !step.is_finite() {
                    return Ok(Value::Na);
                }
                let mut i = from;
                let mut last = Value::Na;
                let mut budget = LOOP_BUDGET;
                let ascending = step > 0.0;
                while (ascending && i <= to) || (!ascending && i >= to) {
                    if budget == 0 {
                        return Err(self.err(
                            id,
                            ErrorCode::PineLoopBudget,
                            format!("loop exceeded {LOOP_BUDGET} iterations in one bar"),
                        ));
                    }
                    budget -= 1;
                    self.write_resolved(id, Value::Num(i));
                    last = self.eval(*body)?;
                    i += step;
                }
                Ok(last)
            }
            NodeKind::While { condition, body } => {
                let mut last = Value::Na;
                let mut budget = LOOP_BUDGET;
                while self.cond(*condition)? {
                    if budget == 0 {
                        return Err(self.err(
                            id,
                            ErrorCode::PineLoopBudget,
                            format!("loop exceeded {LOOP_BUDGET} iterations in one bar"),
                        ));
                    }
                    budget -= 1;
                    last = self.eval(*body)?;
                }
                Ok(last)
            }
            NodeKind::History { subject, offset } => self.history(id, *subject, *offset),
            NodeKind::Call { callee, args } => match self.script.resolutions[callee.index()] {
                Resolution::Builtin(builtin) => self.builtin_call(id, builtin, args),
                Resolution::Function(index) => self.user_call(id, index, args),
                _ => {
                    // A method call: the callee is a member expression whose
                    // receiver evaluates to an object handle.
                    if let NodeKind::Member { object, field } = self.kind(*callee) {
                        let receiver = self.eval(*object)?;
                        let Value::Obj(kind, raw) = receiver else {
                            return Err(self.type_err(*callee, "an object handle", &receiver));
                        };
                        return self.method_call(id, kind, ObjectId(raw), field, args);
                    }
                    Err(self.err(id, ErrorCode::PineType, "only object handles have methods"))
                }
            },
            // Statement kinds never reach eval directly.
            other => Err(self.err(
                id,
                ErrorCode::PineType,
                format!("cannot evaluate {other:?} as an expression"),
            )),
        }
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

    fn name_value(&mut self, id: NodeId) -> Result<Value, PineError> {
        match self.script.resolutions[id.index()] {
            Resolution::Global(slot) => Ok(self.state.globals[slot as usize].clone()),
            Resolution::Local(slot) => {
                Ok(self.state.locals[self.frame_base + slot as usize].clone())
            }
            Resolution::Builtin(builtin) => self.builtin_variable(id, builtin),
            Resolution::Function(_) => Err(self.err(
                id,
                ErrorCode::PineType,
                "a function needs arguments to be called",
            )),
            Resolution::ColorConst(rgba) => Ok(Value::Color(rgba)),
            Resolution::EnumConst(tag) => Ok(Value::Str(Rc::new(tag.to_owned()))),
            Resolution::None => Err(self.err(id, ErrorCode::PineUnknownName, "unresolved name")),
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
            Builtin::Time => bar.open_time as f64,
            Builtin::TimeClose => bar.close_time as f64,
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
        self.state.staged_histories.insert(key, current.clone());
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
            .unwrap_or(Value::Na);
        Ok(value)
    }

    fn binary(&mut self, op: BinOp, lhs: NodeId, rhs: NodeId) -> Result<Value, PineError> {
        match op {
            BinOp::And => {
                // Short-circuit; na counts false (Pine).
                if !self.cond(lhs)? {
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(self.cond(rhs)?))
            }
            BinOp::Or => {
                if self.cond(lhs)? {
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(self.cond(rhs)?))
            }
            BinOp::Eq | BinOp::Ne => {
                let l = self.eval(lhs)?;
                let r = self.eval(rhs)?;
                let equal = match (&l, &r) {
                    // NaN == anything is false (IEEE), and na == na is
                    // therefore false too — Pine agrees.
                    (Value::Num(a), Value::Num(b)) => a == b,
                    (Value::Bool(a), Value::Bool(b)) => a == b,
                    (Value::Color(a), Value::Color(b)) => a == b,
                    (Value::Str(a), Value::Str(b)) => a == b,
                    _ => false,
                };
                Ok(Value::Bool(if op == BinOp::Eq { equal } else { !equal }))
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let l = self.num(lhs)?;
                let r = self.num(rhs)?;
                let result = match op {
                    BinOp::Lt => l < r,
                    BinOp::Le => l <= r,
                    BinOp::Gt => l > r,
                    _ => l >= r,
                };
                Ok(Value::Bool(result))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let l = self.num(lhs)?;
                let r = self.num(rhs)?;
                let result = match op {
                    BinOp::Add => l + r,
                    BinOp::Sub => l - r,
                    BinOp::Mul => l * r,
                    // Division by zero is na, not ±inf — a chart drawing an
                    // infinite line is a lie (documented divergence
                    // from IEEE, agreement with Pine).
                    BinOp::Div => {
                        if r == 0.0 {
                            f64::NAN
                        } else {
                            l / r
                        }
                    }
                    _ => {
                        if r == 0.0 {
                            f64::NAN
                        } else {
                            l % r
                        }
                    }
                };
                Ok(Value::Num(result))
            }
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
        for (slot, value) in argv.into_iter().enumerate() {
            self.state.locals[self.frame_base + slot] = value;
        }
        self.path = path;
        let result = self.eval(body);
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
        // Output calls first: they stage into the plot row.
        match builtin {
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
            // fill() is fully resolved at load time; at eval it is a no-op.
            // Accepted-but-inert output calls still evaluate their first
            // argument (consistency: kernels inside advance every bar).
            Builtin::Fill
            | Builtin::Bgcolor
            | Builtin::Barcolor
            | Builtin::AlertCondition
            | Builtin::Indicator => {
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
            return self.kernel_call(id, builtin, site.0, args);
        }
        self.pure_call(id, builtin, args)
    }

    fn arg_num(&mut self, args: &[crate::ast::Arg], position: usize) -> Result<f64, PineError> {
        match args.get(position) {
            Some(arg) => self.num(arg.value),
            None => Ok(f64::NAN),
        }
    }

    fn kernel_call(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        site: u32,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        let key = (site, self.path);
        // Bool-family kernels take their own paths below.
        match builtin {
            Builtin::TaCrossover | Builtin::TaCrossunder | Builtin::TaCross => {
                let a = self.arg_num(args, 0)?;
                let b = self.arg_num(args, 1)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| match builtin {
                        Builtin::TaCrossover => Kernel::CrossOver(ta::CrossOver::new()),
                        Builtin::TaCrossunder => Kernel::CrossUnder(ta::CrossUnder::new()),
                        _ => Kernel::Cross(ta::Cross::new()),
                    });
                let fired = match kernel {
                    Kernel::CrossOver(k) => k.push(a, b),
                    Kernel::CrossUnder(k) => k.push(a, b),
                    Kernel::Cross(k) => k.push(a, b),
                    _ => unreachable!("cross site holds a cross kernel"),
                };
                return Ok(Value::Bool(fired));
            }
            Builtin::TaBarsSince => {
                let condition = match args.first() {
                    Some(arg) => self.cond(arg.value)?,
                    None => false,
                };
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::BarsSince(ta::BarsSince::new()));
                let Kernel::BarsSince(k) = kernel else {
                    unreachable!("barssince site holds a barssince kernel");
                };
                return Ok(Value::Num(k.push(condition)));
            }
            Builtin::TaValueWhen => {
                let condition = match args.first() {
                    Some(arg) => self.cond(arg.value)?,
                    None => false,
                };
                let source = self.arg_num(args, 1)?;
                let occurrence = self.arg_num(args, 2)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let occurrence = if occurrence.is_nan() {
                    0
                } else {
                    occurrence.max(0.0) as usize
                };
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::ValueWhen(ta::ValueWhen::new(occurrence)));
                let Kernel::ValueWhen(k) = kernel else {
                    unreachable!("valuewhen site holds a valuewhen kernel");
                };
                return Ok(Value::Num(k.push(condition, source)));
            }
            Builtin::Fixnan => {
                let x = self.arg_num(args, 0)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Fixnan(f64::NAN));
                let Kernel::Fixnan(last) = kernel else {
                    unreachable!("fixnan site holds a fixnan kernel");
                };
                if !x.is_nan() {
                    *last = x;
                }
                return Ok(Value::Num(*last));
            }
            _ => {}
        }

        // Numeric kernels: gather inputs, then a positive-int length where
        // the kernel takes one (re-validated here because inputs bind per
        // instance).
        let length = |v: f64, id: NodeId, this: &Self| -> Result<usize, PineError> {
            // The upper bound matters as much as the lower one: a kernel
            // allocates its ring up front, and a failed allocation aborts the
            // process instead of unwinding into this indicator's error state.
            // Inputs are bound per instance, so the load-time check in
            // `compile` is not the last word — this is.
            let in_range = v.is_finite()
                && v >= 1.0
                && v.fract() == 0.0
                && v <= crate::compile::MAX_KERNEL_LENGTH as f64;
            if in_range {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                Ok(v as usize)
            } else {
                Err(this.err(
                    id,
                    ErrorCode::PineSeriesLength,
                    format!(
                        "kernel length must be a positive integer no greater than {}, got {v}",
                        crate::compile::MAX_KERNEL_LENGTH
                    ),
                ))
            }
        };
        let value = match builtin {
            Builtin::TaTr => {
                let handle_na = match args.first() {
                    Some(arg) => self.cond(arg.value)?,
                    None => false,
                };
                let (high, low, close) = (self.bar.high, self.bar.low, self.bar.close);
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Tr(ta::Tr::new(handle_na)));
                let Kernel::Tr(k) = kernel else {
                    unreachable!("tr site holds a tr kernel");
                };
                k.push(high, low, close)
            }
            Builtin::TaAtr => {
                let len = length(self.arg_num(args, 0)?, id, self)?;
                self.pin_kernel_length(key, len, id)?;
                let (high, low, close) = (self.bar.high, self.bar.low, self.bar.close);
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Atr(ta::Atr::new(len)));
                let Kernel::Atr(k) = kernel else {
                    unreachable!("atr site holds an atr kernel");
                };
                k.push(high, low, close)
            }
            Builtin::TaCum => {
                let x = self.arg_num(args, 0)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Cum(ta::Cum::new()));
                let Kernel::Cum(k) = kernel else {
                    unreachable!("cum site holds a cum kernel");
                };
                k.push(x)
            }
            Builtin::TaChange | Builtin::TaMom => {
                let x = self.arg_num(args, 0)?;
                let n = match args.get(1) {
                    Some(arg) => length(self.num(arg.value)?, id, self)?,
                    None => 1,
                };
                self.pin_kernel_length(key, n, id)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Change(ta::Change::new(n)));
                let Kernel::Change(k) = kernel else {
                    unreachable!("change site holds a change kernel");
                };
                k.push(x)
            }
            Builtin::TaVwma => {
                let src = self.arg_num(args, 0)?;
                let len = length(self.arg_num(args, 1)?, id, self)?;
                self.pin_kernel_length(key, len, id)?;
                let volume = self.bar.volume();
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Vwma(ta::Vwma::new(len)));
                let Kernel::Vwma(k) = kernel else {
                    unreachable!("vwma site holds a vwma kernel");
                };
                k.push(src, volume)
            }
            Builtin::TaPivotHigh | Builtin::TaPivotLow => {
                let src = self.arg_num(args, 0)?;
                let left = length(self.arg_num(args, 1)?, id, self)?;
                let right = length(self.arg_num(args, 2)?, id, self)?;
                self.pin_kernel_length(key, left + right, id)?;
                let is_high = builtin == Builtin::TaPivotHigh;
                let kernel = self.state.kernels.entry(key).or_insert_with(|| {
                    if is_high {
                        Kernel::PivotHigh(ta::PivotHigh::new(left, right))
                    } else {
                        Kernel::PivotLow(ta::PivotLow::new(left, right))
                    }
                });
                match kernel {
                    Kernel::PivotHigh(k) => k.push(src),
                    Kernel::PivotLow(k) => k.push(src),
                    _ => unreachable!("pivot site holds a pivot kernel"),
                }
            }
            // The (src, len) family.
            _ => {
                let src = self.arg_num(args, 0)?;
                let len = length(self.arg_num(args, 1)?, id, self)?;
                self.pin_kernel_length(key, len, id)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| match builtin {
                        Builtin::TaSma => Kernel::Sma(ta::Sma::new(len)),
                        Builtin::TaEma => Kernel::Ema(ta::Ema::new(len)),
                        Builtin::TaRma => Kernel::Rma(ta::Rma::new(len)),
                        Builtin::TaWma => Kernel::Wma(ta::Wma::new(len)),
                        Builtin::TaRsi => Kernel::Rsi(ta::Rsi::new(len)),
                        Builtin::TaStdev => Kernel::Stdev(ta::Stdev::new(len)),
                        Builtin::TaHighest => Kernel::Highest(ta::Highest::new(len)),
                        Builtin::TaLowest => Kernel::Lowest(ta::Lowest::new(len)),
                        Builtin::TaHighestBars => Kernel::HighestBars(ta::HighestBars::new(len)),
                        Builtin::TaLowestBars => Kernel::LowestBars(ta::LowestBars::new(len)),
                        Builtin::MathSum => Kernel::WindowSum(ta::WindowSum::new(len)),
                        other => unreachable!("{other:?} is not a (src, len) kernel"),
                    });
                match kernel {
                    Kernel::Sma(k) => k.push(src),
                    Kernel::Ema(k) => k.push(src),
                    Kernel::Rma(k) => k.push(src),
                    Kernel::Wma(k) => k.push(src),
                    Kernel::Rsi(k) => k.push(src),
                    Kernel::Stdev(k) => k.push(src),
                    Kernel::Highest(k) => k.push(src),
                    Kernel::Lowest(k) => k.push(src),
                    Kernel::HighestBars(k) => k.push(src),
                    Kernel::LowestBars(k) => k.push(src),
                    Kernel::WindowSum(k) => k.push(src),
                    _ => unreachable!("(src, len) site holds a (src, len) kernel"),
                }
            }
        };
        Ok(Value::Num(value))
    }

    fn pure_call(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        let value = match builtin {
            Builtin::NaCall => {
                let v = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                return Ok(Value::Bool(v.is_na()));
            }
            Builtin::Nz => {
                let v = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                if v.is_na() {
                    return match args.get(1) {
                        Some(replacement) => self.eval(replacement.value),
                        None => Ok(Value::Num(0.0)),
                    };
                }
                return Ok(v);
            }
            Builtin::ColorNew => {
                let base = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                let Value::Color(rgba) = base else {
                    return Err(self.type_err(id, "a color", &base));
                };
                let transparency = self.arg_num(args, 1)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let alpha = (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                return Ok(Value::Color((rgba & 0xFFFF_FF00) | alpha));
            }
            Builtin::ColorRgb => {
                let r = self.arg_num(args, 0)?;
                let g = self.arg_num(args, 1)?;
                let b = self.arg_num(args, 2)?;
                let transparency = match args.get(3) {
                    Some(arg) => self.num(arg.value)?,
                    None => 0.0,
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let pack = |v: f64| (v.clamp(0.0, 255.0)) as u32;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let alpha = (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                return Ok(Value::Color(
                    (pack(r) << 24) | (pack(g) << 16) | (pack(b) << 8) | alpha,
                ));
            }
            Builtin::MathAbs => self.arg_num(args, 0)?.abs(),
            Builtin::MathFloor => self.arg_num(args, 0)?.floor(),
            Builtin::MathCeil => self.arg_num(args, 0)?.ceil(),
            Builtin::MathRound => self.arg_num(args, 0)?.round(),
            Builtin::MathSign => {
                let v = self.arg_num(args, 0)?;
                if v.is_nan() {
                    f64::NAN
                } else if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }
            Builtin::MathSqrt => self.arg_num(args, 0)?.sqrt(),
            Builtin::MathMax | Builtin::MathMin | Builtin::MathAvg => {
                // Streaming, so the per-bar path allocates nothing — these
                // appear inside plotted expressions. The identities are the
                // infinities, not `f64::MIN`/`MAX`: those are the extreme
                // *finite* values, so `math.max(math.log(0))` answered
                // -1.797e308 where -inf is the true maximum of its arguments.
                let mut max = f64::NEG_INFINITY;
                let mut min = f64::INFINITY;
                let mut sum = 0.0;
                let mut count = 0usize;
                let mut saw_nan = false;
                for arg in args {
                    let v = self.num(arg.value)?;
                    if v.is_nan() {
                        saw_nan = true;
                    }
                    max = max.max(v);
                    min = min.min(v);
                    sum += v;
                    count += 1;
                }
                if count == 0 || saw_nan {
                    f64::NAN
                } else {
                    match builtin {
                        Builtin::MathMax => max,
                        Builtin::MathMin => min,
                        _ => sum / count as f64,
                    }
                }
            }
            Builtin::MathPow => fmath::pow(self.arg_num(args, 0)?, self.arg_num(args, 1)?),
            Builtin::MathExp => fmath::exp(self.arg_num(args, 0)?),
            Builtin::MathLog => fmath::ln(self.arg_num(args, 0)?),
            Builtin::MathLog10 => fmath::log10(self.arg_num(args, 0)?),
            other => {
                return Err(self.err(
                    id,
                    ErrorCode::PineType,
                    format!("`{other:?}` cannot be called here"),
                ));
            }
        };
        Ok(Value::Num(value))
    }

    // ---- draw objects ------------------------------------------------

    /// A named-or-positional argument, mirroring the compile-time helper.
    fn call_arg<'b>(
        args: &'b [crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Option<&'b crate::ast::Arg> {
        args.iter()
            .find(|a| a.name.as_deref() == Some(name))
            .or_else(|| args.iter().filter(|a| a.name.is_none()).nth(position))
    }

    fn arg_bar_index(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Result<i64, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => {
                let v = self.num(arg.value)?;
                #[allow(clippy::cast_possible_truncation)]
                Ok(if v.is_finite() {
                    v.round() as i64
                } else {
                    // `label.new(na, price)` has no bar to sit on. Zero would
                    // anchor it to the very first bar — a real label drawn
                    // out of a missing value, while the price side of the
                    // same call answers NaN and the renderer honestly skips
                    // it. `OFF_CHART_BAR` is negative, which the renderer's
                    // existing `x < 0` guards already drop.
                    quantick_indicators::OFF_CHART_BAR
                })
            }
            None => Ok(self.ctx.bar_index as i64),
        }
    }

    fn arg_price(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Result<f64, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => self.num(arg.value),
            None => Ok(f64::NAN),
        }
    }

    fn arg_color(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
        default: Rgba8,
    ) -> Result<Rgba8, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => {
                let v = self.eval(arg.value)?;
                match v {
                    Value::Color(rgba) => Ok(Rgba8::from_u32(rgba)),
                    Value::Na => Ok(default),
                    other => Err(self.type_err(arg.value, "a color", &other)),
                }
            }
            None => Ok(default),
        }
    }

    fn arg_text(
        &mut self,
        args: &[crate::ast::Arg],
        position: usize,
        name: &str,
    ) -> Result<String, PineError> {
        match Self::call_arg(args, position, name) {
            Some(arg) => {
                let v = self.eval(arg.value)?;
                match v {
                    Value::Str(s) => Ok(s.as_ref().clone()),
                    Value::Na => Ok(String::new()),
                    other => Err(self.type_err(arg.value, "a string", &other)),
                }
            }
            None => Ok(String::new()),
        }
    }

    fn line_new(&mut self, args: &[crate::ast::Arg]) -> Result<Value, PineError> {
        let x1 = self.arg_bar_index(args, 0, "x1")?;
        let y1 = self.arg_price(args, 1, "y1")?;
        let x2 = self.arg_bar_index(args, 2, "x2")?;
        let y2 = self.arg_price(args, 3, "y2")?;
        let color = self.arg_color(args, 4, "color", DEFAULT_OBJECT_COLOR)?;
        let width = match Self::call_arg(args, 5, "width") {
            Some(arg) => {
                #[allow(clippy::cast_possible_truncation)]
                let w = self.num(arg.value)? as f32;
                w
            }
            None => DEFAULT_LINE_WIDTH,
        };
        let handle = self.state.objects.new_line(LineObj {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        });
        Ok(Value::Obj(ObjKind::Line, handle.0))
    }

    fn box_new(&mut self, args: &[crate::ast::Arg]) -> Result<Value, PineError> {
        let left = self.arg_bar_index(args, 0, "left")?;
        let top = self.arg_price(args, 1, "top")?;
        let right = self.arg_bar_index(args, 2, "right")?;
        let bottom = self.arg_price(args, 3, "bottom")?;
        let border_color = self.arg_color(args, 4, "border_color", DEFAULT_OBJECT_COLOR)?;
        let bg_color = self.arg_color(args, 5, "bgcolor", DEFAULT_BOX_FILL)?;
        let handle = self.state.objects.new_box(BoxObj {
            left,
            top,
            right,
            bottom,
            border_color,
            bg_color,
            border_width: DEFAULT_LINE_WIDTH,
        });
        Ok(Value::Obj(ObjKind::Box, handle.0))
    }

    fn label_new(&mut self, args: &[crate::ast::Arg]) -> Result<Value, PineError> {
        let x = self.arg_bar_index(args, 0, "x")?;
        let y = self.arg_price(args, 1, "y")?;
        let text = self.arg_text(args, 2, "text")?;
        let color = self.arg_color(args, 3, "color", DEFAULT_OBJECT_COLOR)?;
        let text_color = self.arg_color(args, 4, "textcolor", DEFAULT_LABEL_TEXT)?;
        let style = match Self::call_arg(args, 5, "style") {
            Some(arg) => {
                let v = self.eval(arg.value)?;
                match v {
                    Value::Str(tag) => match tag.as_str() {
                        "style_label_up" => LabelStyle::Up,
                        "style_label_down" => LabelStyle::Down,
                        _ => LabelStyle::None,
                    },
                    _ => LabelStyle::Down,
                }
            }
            None => LabelStyle::Down,
        };
        let handle = self.state.objects.new_label(LabelObj {
            x,
            y,
            text,
            color,
            text_color,
            style,
        });
        Ok(Value::Obj(ObjKind::Label, handle.0))
    }

    /// `handle.set_*(...)` / `handle.delete()`. A stale handle (collected
    /// by the cap, or deleted) is a silent no-op — Pine's behavior, and the
    /// honest one for GC'd objects.
    #[allow(clippy::too_many_lines)]
    fn method_call(
        &mut self,
        id: NodeId,
        kind: ObjKind,
        handle: ObjectId,
        field: &str,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        match (kind, field) {
            (ObjKind::Line, "delete") => self.state.objects.delete_line(handle),
            (ObjKind::Box, "delete") => self.state.objects.delete_box(handle),
            (ObjKind::Label, "delete") => self.state.objects.delete_label(handle),
            (ObjKind::Line, "set_xy1" | "set_xy2") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                let y = self.arg_price(args, 1, "y")?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    if field == "set_xy1" {
                        line.x1 = x;
                        line.y1 = y;
                    } else {
                        line.x2 = x;
                        line.y2 = y;
                    }
                }
            }
            (ObjKind::Line, "set_x1" | "set_x2") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    if field == "set_x1" {
                        line.x1 = x;
                    } else {
                        line.x2 = x;
                    }
                }
            }
            (ObjKind::Line, "set_y1" | "set_y2") => {
                let y = self.arg_price(args, 0, "y")?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    if field == "set_y1" {
                        line.y1 = y;
                    } else {
                        line.y2 = y;
                    }
                }
            }
            (ObjKind::Line, "set_color") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_OBJECT_COLOR)?;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    line.color = color;
                }
            }
            (ObjKind::Line, "set_width") => {
                #[allow(clippy::cast_possible_truncation)]
                let width = self.arg_price(args, 0, "width")? as f32;
                if let Some(line) = self.state.objects.line_mut(handle) {
                    line.width = width;
                }
            }
            (ObjKind::Box, "set_left" | "set_right") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    if field == "set_left" {
                        object.left = x;
                    } else {
                        object.right = x;
                    }
                }
            }
            (ObjKind::Box, "set_top" | "set_bottom") => {
                let y = self.arg_price(args, 0, "y")?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    if field == "set_top" {
                        object.top = y;
                    } else {
                        object.bottom = y;
                    }
                }
            }
            (ObjKind::Box, "set_lefttop" | "set_rightbottom") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                let y = self.arg_price(args, 1, "y")?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    if field == "set_lefttop" {
                        object.left = x;
                        object.top = y;
                    } else {
                        object.right = x;
                        object.bottom = y;
                    }
                }
            }
            (ObjKind::Box, "set_bgcolor") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_BOX_FILL)?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    object.bg_color = color;
                }
            }
            (ObjKind::Box, "set_border_color") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_OBJECT_COLOR)?;
                if let Some(object) = self.state.objects.box_mut(handle) {
                    object.border_color = color;
                }
            }
            (ObjKind::Label, "set_xy") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                let y = self.arg_price(args, 1, "y")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.x = x;
                    label.y = y;
                }
            }
            (ObjKind::Label, "set_x") => {
                let x = self.arg_bar_index(args, 0, "x")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.x = x;
                }
            }
            (ObjKind::Label, "set_y") => {
                let y = self.arg_price(args, 0, "y")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.y = y;
                }
            }
            (ObjKind::Label, "set_text") => {
                let text = self.arg_text(args, 0, "text")?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.text = text;
                }
            }
            (ObjKind::Label, "set_color") => {
                let color = self.arg_color(args, 0, "color", DEFAULT_OBJECT_COLOR)?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.color = color;
                }
            }
            (ObjKind::Label, "set_textcolor") => {
                let color = self.arg_color(args, 0, "textcolor", DEFAULT_LABEL_TEXT)?;
                if let Some(label) = self.state.objects.label_mut(handle) {
                    label.text_color = color;
                }
            }
            _ => {
                return Err(self.err(
                    id,
                    ErrorCode::PineType,
                    format!("unknown method `{field}` for this object kind"),
                ));
            }
        }
        Ok(Value::Na)
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

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Obj(..) => "an object handle",
        Value::Num(_) => "a number",
        Value::Bool(_) => "a bool",
        Value::Color(_) => "a color",
        Value::Str(_) => "a string",
        Value::Tuple(_) => "a tuple",
        Value::Na => "na",
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
