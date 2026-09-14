//! Passes 2, 3, 5 and 6: one resolved walk.
//!
//! Name resolution to dense slots, call-site numbering for the stateful
//! kernels, `max_bars_back` inference and the unsupported-construct scan all
//! read the same tree, so they share one traversal rather than four. The walk
//! carries a [`Scope`] for lexical shadowing and a [`WalkCtx`] for the
//! position facts a rejection needs — inside a loop, inside a function,
//! still top level.

use quantick_indicators::{InputSpec, Rgba8, SourceId};

use crate::ast::{Arg, NodeId, NodeKind, VarMode};
use crate::builtins::{Builtin, did_you_mean, rejection_of};
use crate::error::{ErrorCode, PineError, Span};

use super::fold::{Const, member_constant};
use super::{CallSiteId, Compiler, FunctionInfo, MAX_BARS_BACK_CAP, MAX_KERNEL_LENGTH, Resolution};

fn source_of_builtin(builtin: Builtin) -> Option<SourceId> {
    Some(match builtin {
        Builtin::Open => SourceId::Open,
        Builtin::High => SourceId::High,
        Builtin::Low => SourceId::Low,
        Builtin::Close => SourceId::Close,
        Builtin::Hl2 => SourceId::Hl2,
        Builtin::Hlc3 => SourceId::Hlc3,
        Builtin::Ohlc4 => SourceId::Ohlc4,
        Builtin::Hlcc4 => SourceId::Hlcc4,
        Builtin::Volume => SourceId::Volume,
        Builtin::Delta => SourceId::Delta,
        Builtin::BuyVolume => SourceId::BuyVolume,
        Builtin::SellVolume => SourceId::SellVolume,
        Builtin::TradeCount => SourceId::TradeCount,
        Builtin::Cvd => SourceId::Cvd,
        _ => return None,
    })
}

struct Scope {
    /// (name, resolution) pairs, innermost last; lexical shadowing by
    /// search from the back.
    names: Vec<(String, Resolution)>,
    /// How many names each open block introduced (for popping).
    block_marks: Vec<usize>,
}

impl Compiler {
    pub(super) fn resolve_and_scan(&mut self) {
        let statements = match self.kind(self.root) {
            NodeKind::Script { statements } => statements.clone(),
            _ => return,
        };
        let mut scope = Scope {
            names: Vec::new(),
            block_marks: Vec::new(),
        };
        // Function names first: calls may precede definitions textually?
        // No — Pine requires definition before use; a forward call is an
        // unknown name, same as TradingView.
        for statement in statements {
            self.walk_statement(statement, &mut scope, &mut WalkCtx::global());
        }
    }

    fn declare_global(
        &mut self,
        name: &str,
        init: Option<NodeId>,
        mode: VarMode,
        scope: &mut Scope,
    ) -> u32 {
        let slot = self.global_slots;
        self.global_slots += 1;
        self.slot_modes.push(mode);
        self.global_init.push(init);
        self.reassigned.push(self.reassigned_names.contains(name));
        scope
            .names
            .push((name.to_owned(), Resolution::Global(slot)));
        slot
    }

    fn walk_statement(&mut self, id: NodeId, scope: &mut Scope, ctx: &mut WalkCtx) {
        match self.kind(id).clone() {
            NodeKind::Declare { mode, name, value } => {
                self.walk_expr(value, scope, ctx);
                if let Some(frame) = ctx.frame.as_mut() {
                    let slot = frame.next_slot;
                    frame.next_slot += 1;
                    scope.names.push((name, Resolution::Local(slot)));
                    self.resolutions[id.index()] = Resolution::Local(slot);
                    return;
                }
                let slot = self.declare_global(&name, Some(value), mode, scope);
                self.resolutions[id.index()] = Resolution::Global(slot);
            }
            NodeKind::TupleDeclare { names, value } => {
                self.walk_expr(value, scope, ctx);
                // Tuple elements each get their own slot; the interpreter
                // splits the tuple value. Resolution on the statement node
                // records the FIRST slot; elements are consecutive.
                if let Some(frame) = ctx.frame.as_mut() {
                    let first = frame.next_slot;
                    for name in &names {
                        let slot = frame.next_slot;
                        frame.next_slot += 1;
                        scope.names.push((name.clone(), Resolution::Local(slot)));
                    }
                    self.resolutions[id.index()] = Resolution::Local(first);
                    return;
                }
                let first = self.global_slots;
                for name in &names {
                    self.declare_global(name, None, VarMode::Plain, scope);
                }
                self.resolutions[id.index()] = Resolution::Global(first);
            }
            NodeKind::Reassign { name, value } => {
                self.walk_expr(value, scope, ctx);
                match self.lookup_name(&name, scope, ctx) {
                    Some(resolution) => {
                        self.resolutions[id.index()] = resolution;
                        if let Resolution::Global(slot) = resolution {
                            self.reassigned[slot as usize] = true;
                        }
                    }
                    None => {
                        let span = self.span(id);
                        self.unknown_name(&name, span);
                    }
                }
            }
            NodeKind::FunctionDef { name, params, body } => {
                if ctx.frame.is_some() {
                    let span = self.span(id);
                    self.errors.push(PineError::new(
                        ErrorCode::PineSyntax,
                        span,
                        "nested function definitions are not supported",
                    ));
                    return;
                }
                let index = u32::try_from(self.functions.len()).expect("few functions");
                self.function_names.push(name.clone());
                // Two-phase: register the name first so *later* statements
                // can call it, but walk the body with recursion detection.
                self.functions.push(FunctionInfo {
                    name: name.clone(),
                    param_count: params.len(),
                    local_slots: params.len(),
                    body,
                });
                scope
                    .names
                    .push((name.clone(), Resolution::Function(index)));
                let mut frame = Frame {
                    next_slot: u32::try_from(params.len()).expect("few params"),
                };
                let mark = scope.names.len();
                for (i, param) in params.iter().enumerate() {
                    let slot = u32::try_from(i).expect("few params");
                    scope.names.push((param.clone(), Resolution::Local(slot)));
                }
                let mut inner = WalkCtx {
                    frame: Some(frame),
                    in_loop: false,
                    top_level: false,
                    function_stack: {
                        let mut stack = ctx.function_stack.clone();
                        stack.push(index);
                        stack
                    },
                };
                self.walk_expr(body, scope, &mut inner);
                frame = inner.frame.expect("frame survives the walk");
                self.functions[index as usize].local_slots = frame.next_slot as usize;
                scope.names.truncate(mark);
                self.resolutions[id.index()] = Resolution::Function(index);
            }
            NodeKind::ExprStmt { expr } => self.walk_expr(expr, scope, ctx),
            _ => self.walk_expr(id, scope, ctx),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn walk_expr(&mut self, id: NodeId, scope: &mut Scope, ctx: &mut WalkCtx) {
        match self.kind(id).clone() {
            NodeKind::Block { statements } => {
                scope.block_marks.push(scope.names.len());
                for statement in statements {
                    self.walk_statement(statement, scope, ctx);
                }
                let mark = scope.block_marks.pop().expect("pushed above");
                scope.names.truncate(mark);
            }
            NodeKind::If {
                condition,
                then_block,
                otherwise,
            } => {
                self.walk_expr(condition, scope, ctx);
                ctx.nested(|ctx| {
                    self.walk_expr(then_block, scope, ctx);
                    if let Some(otherwise) = otherwise {
                        self.walk_expr(otherwise, scope, ctx);
                    }
                });
            }
            NodeKind::For {
                var,
                from,
                to,
                by,
                body,
            } => {
                self.walk_expr(from, scope, ctx);
                self.walk_expr(to, scope, ctx);
                if let Some(by) = by {
                    self.walk_expr(by, scope, ctx);
                }
                // The loop variable is a block-scoped declaration.
                scope.block_marks.push(scope.names.len());
                let resolution = if let Some(frame) = ctx.frame.as_mut() {
                    let slot = frame.next_slot;
                    frame.next_slot += 1;
                    scope.names.push((var, Resolution::Local(slot)));
                    Resolution::Local(slot)
                } else {
                    let slot = self.declare_global(&var, None, VarMode::Plain, scope);
                    Resolution::Global(slot)
                };
                self.resolutions[id.index()] = resolution;
                let was_in_loop = ctx.in_loop;
                ctx.in_loop = true;
                ctx.nested(|ctx| self.walk_expr(body, scope, ctx));
                ctx.in_loop = was_in_loop;
                let mark = scope.block_marks.pop().expect("pushed above");
                scope.names.truncate(mark);
            }
            NodeKind::While { condition, body } => {
                self.walk_expr(condition, scope, ctx);
                let was_in_loop = ctx.in_loop;
                ctx.in_loop = true;
                ctx.nested(|ctx| self.walk_expr(body, scope, ctx));
                ctx.in_loop = was_in_loop;
            }
            NodeKind::Switch { subject, arms } => {
                if let Some(subject) = subject {
                    self.walk_expr(subject, scope, ctx);
                }
                for (condition, body) in arms {
                    if let Some(condition) = condition {
                        self.walk_expr(condition, scope, ctx);
                    }
                    self.walk_expr(body, scope, ctx);
                }
            }
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                self.walk_expr(condition, scope, ctx);
                self.walk_expr(if_true, scope, ctx);
                self.walk_expr(if_false, scope, ctx);
            }
            NodeKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs, scope, ctx);
                self.walk_expr(rhs, scope, ctx);
            }
            NodeKind::Unary { operand, .. } => self.walk_expr(operand, scope, ctx),
            NodeKind::Tuple { items } => {
                for item in items {
                    self.walk_expr(item, scope, ctx);
                }
            }
            NodeKind::History { subject, offset } => {
                self.walk_expr(subject, scope, ctx);
                self.walk_expr(offset, scope, ctx);
                let series = self.history_series_count;
                self.history_series_count += 1;
                self.history_series[id.index()] = Some(series);
                match self.fold(offset).and_then(|c| c.as_positive_len()) {
                    Some(depth) => self.max_bars_back = self.max_bars_back.max(depth + 1),
                    // Offset 0 is legal and folds to None above; only cap
                    // genuinely dynamic offsets.
                    None => {
                        if !matches!(self.fold(offset), Some(Const::Int(0))) {
                            // Raise the floor, never assign: a script with
                            // both `close[700]` and `close[i]` still needs
                            // 701 rows, and assigning made the answer depend
                            // on which read came first.
                            self.max_bars_back = self.max_bars_back.max(MAX_BARS_BACK_CAP);
                        }
                    }
                }
            }
            NodeKind::Member { object, field } => {
                if let NodeKind::Name(namespace) = self.kind(object).clone() {
                    let dotted = format!("{namespace}.{field}");
                    if let Some(builtin) = Builtin::lookup(&dotted) {
                        self.resolutions[id.index()] = Resolution::Builtin(builtin);
                        return;
                    }
                    if let Some(constant) = member_constant(&namespace, &field) {
                        self.resolutions[id.index()] = match constant {
                            Const::Enum(tag) => Resolution::EnumConst(tag),
                            Const::Color(rgba) => Resolution::ColorConst(rgba),
                            // member_constant only produces enums and colors.
                            _ => Resolution::None,
                        };
                        return;
                    }
                    if let Some((code, reason)) = rejection_of(&dotted) {
                        let span = self.span(id);
                        self.errors.push(PineError::new(code, span, reason));
                        return;
                    }
                    // An unknown member of a known namespace is an unknown
                    // name; a member of a *variable* is an object method
                    // (resolved at eval against the handle's kind).
                    if matches!(
                        namespace.as_str(),
                        "ta" | "math" | "color" | "input" | "line" | "box" | "label"
                    ) {
                        self.unknown_name(&dotted, self.span(id));
                        return;
                    }
                }
                self.walk_expr(object, scope, ctx);
            }
            NodeKind::Call { callee, args } => {
                for arg in &args {
                    self.walk_expr(arg.value, scope, ctx);
                }
                // `na(x)`: the callee lexes as the na keyword, not a name.
                if matches!(self.kind(callee), NodeKind::Na) {
                    self.resolutions[callee.index()] = Resolution::Builtin(Builtin::NaCall);
                    return;
                }
                let name = self.callee_name(callee);
                if let Some(name) = name {
                    if let Some(builtin) = Builtin::lookup(&name) {
                        self.resolutions[callee.index()] = Resolution::Builtin(builtin);
                        self.builtin_call(id, builtin, &args, ctx);
                        return;
                    }
                    if let Some((code, reason)) = rejection_of(&name) {
                        let span = self.span(id);
                        self.errors.push(PineError::new(code, span, reason));
                        return;
                    }
                    // A user function?
                    if let Some(resolution) = self.lookup_name(&name, scope, ctx) {
                        if let Resolution::Function(index) = resolution {
                            self.resolutions[callee.index()] = resolution;
                            if ctx.function_stack.contains(&index) {
                                let span = self.span(id);
                                self.errors.push(PineError::new(
                                    ErrorCode::PineRecursion,
                                    span,
                                    format!(
                                        "recursive call of `{}` is not supported",
                                        self.function_names[index as usize]
                                    ),
                                ));
                            }
                            return;
                        }
                        // Calling a non-function value.
                        let span = self.span(id);
                        self.errors.push(PineError::new(
                            ErrorCode::PineType,
                            span,
                            format!("`{name}` is not callable"),
                        ));
                        return;
                    }
                    // `handle.set_xy(...)`: the dotted name is a method
                    // call when its base resolves to a variable — dispatch
                    // happens at eval against the handle's kind.
                    if let Some((base, _)) = name.split_once('.')
                        && matches!(
                            self.lookup_name(base, scope, ctx),
                            Some(Resolution::Global(_) | Resolution::Local(_))
                        )
                    {
                        self.walk_expr(callee, scope, ctx);
                        return;
                    }
                    self.unknown_name(&name, self.span(id));
                    return;
                }
                // Method-style call on an expression (`f(x).set_xy(...)`)
                // resolves at eval; still walk the callee chain.
                self.walk_expr(callee, scope, ctx);
            }
            NodeKind::Name(name) => match self.lookup_name(&name, scope, ctx) {
                Some(resolution) => self.resolutions[id.index()] = resolution,
                None => self.unknown_name(&name, self.span(id)),
            },
            NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Str(_)
            | NodeKind::Bool(_)
            | NodeKind::Color(_)
            | NodeKind::Na => {}
            // Statement kinds reached as block members go through
            // walk_statement; reaching one here is a walker bug.
            other @ (NodeKind::Script { .. }
            | NodeKind::Declare { .. }
            | NodeKind::TupleDeclare { .. }
            | NodeKind::Reassign { .. }
            | NodeKind::FunctionDef { .. }
            | NodeKind::ExprStmt { .. }) => {
                unreachable!("statement node {other:?} reached the expression walker")
            }
        }
    }

    /// Checks specific to one builtin call: call-site numbering, loop rule,
    /// input extraction, kernel-length folding.
    fn builtin_call(&mut self, call: NodeId, builtin: Builtin, args: &[Arg], ctx: &mut WalkCtx) {
        if builtin.is_stateful_call() {
            if ctx.in_loop {
                let span = self.span(call);
                self.errors.push(PineError::new(
                    ErrorCode::PineStatefulInLoop,
                    span,
                    "stateful ta/math kernels cannot be called inside a loop body \
                     (per-iteration call sites would silently diverge); \
                     hoist the call out of the loop",
                ));
            }
            let site = CallSiteId(self.call_site_count);
            self.call_site_count += 1;
            self.call_sites[call.index()] = Some(site);
            // Inside a function body a length may be a parameter, which no
            // load-time folder can see through; the interpreter re-validates
            // every kernel length with the same error code at bar time.
            if ctx.frame.is_none() {
                self.check_kernel_length(call, builtin, args);
            }
        }
        if builtin.is_top_level_only() && !ctx.top_level {
            let span = self.span(call);
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                span,
                "this builtin may only be called at the top level of the script",
            ));
        }
        match builtin {
            Builtin::InputInt
            | Builtin::InputFloat
            | Builtin::InputBool
            | Builtin::InputColor
            | Builtin::InputString
            | Builtin::InputSource => self.input_call(call, builtin, args),
            _ => {}
        }
    }

    /// The `len` argument position of each windowed kernel; `None` = no
    /// length to check.
    fn kernel_length_arg(builtin: Builtin) -> Option<(usize, &'static str)> {
        Some(match builtin {
            Builtin::TaSma
            | Builtin::TaEma
            | Builtin::TaRma
            | Builtin::TaWma
            | Builtin::TaRsi
            | Builtin::TaStdev
            | Builtin::TaHighest
            | Builtin::TaLowest
            | Builtin::TaHighestBars
            | Builtin::TaLowestBars
            | Builtin::TaMom
            | Builtin::MathSum => (1, "length"),
            Builtin::TaVwma => (2, "length"),
            Builtin::TaAtr => (0, "length"),
            // Both sides of a pivot size the same window; checking the left
            // one reaches the same allocation the right one feeds.
            Builtin::TaPivotHigh | Builtin::TaPivotLow => (1, "leftbars"),
            _ => return None,
        })
    }

    fn check_kernel_length(&mut self, call: NodeId, builtin: Builtin, args: &[Arg]) {
        let Some((position, name)) = Self::kernel_length_arg(builtin) else {
            return;
        };
        let Some(arg) = self.arg(args, position, name) else {
            let span = self.span(call);
            self.errors.push(PineError::new(
                ErrorCode::PineArity,
                span,
                format!("this kernel requires a `{name}` argument"),
            ));
            return;
        };
        if self
            .fold(arg.value)
            .and_then(|c| c.as_kernel_len())
            .is_none()
        {
            self.errors.push(PineError::new(
                ErrorCode::PineSeriesLength,
                arg.span,
                format!(
                    "a kernel length must fold to a positive integer no greater \
                     than {MAX_KERNEL_LENGTH} at load time (a literal, an input, \
                     or arithmetic over them)"
                ),
            ));
        }
    }

    fn input_call(&mut self, call: NodeId, builtin: Builtin, args: &[Arg]) {
        let index = self.inputs.len();
        let title = self
            .arg(args, 1, "title")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Str(s)) => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| format!("input {index}"));
        let name = title.to_lowercase().replace(' ', "_");
        let Some(default_arg) = self.arg(args, 0, "defval") else {
            let span = self.span(call);
            self.errors.push(PineError::new(
                ErrorCode::PineArity,
                span,
                "input.* requires a default value",
            ));
            return;
        };
        let folded = self.fold(default_arg.value);
        let not_const = |compiler: &mut Self| {
            compiler.errors.push(PineError::new(
                ErrorCode::PineInputNotConst,
                default_arg.span,
                "input defaults must be constant at load time",
            ));
        };
        let fold_num = |arg: Option<&Arg>| arg.and_then(|a| self.fold(a.value)?.as_f64());
        let spec = match builtin {
            Builtin::InputInt => match folded {
                Some(Const::Int(default)) => InputSpec::Int {
                    name,
                    title,
                    default,
                    min: fold_num(self.arg(args, 2, "minval")).map(|v| v as i64),
                    max: fold_num(self.arg(args, 3, "maxval")).map(|v| v as i64),
                    step: fold_num(self.arg(args, 4, "step")).map(|v| v as i64),
                    options: Vec::new(),
                },
                _ => return not_const(self),
            },
            Builtin::InputFloat => match folded.as_ref().and_then(Const::as_f64) {
                Some(default) => InputSpec::Float {
                    name,
                    title,
                    default,
                    min: fold_num(self.arg(args, 2, "minval")),
                    max: fold_num(self.arg(args, 3, "maxval")),
                    step: fold_num(self.arg(args, 4, "step")),
                    options: Vec::new(),
                },
                None => return not_const(self),
            },
            Builtin::InputBool => match folded {
                Some(Const::Bool(default)) => InputSpec::Bool {
                    name,
                    title,
                    default,
                },
                _ => return not_const(self),
            },
            Builtin::InputColor => match folded {
                Some(Const::Color(rgba)) => InputSpec::Color {
                    name,
                    title,
                    default: Rgba8::from_u32(rgba),
                },
                _ => return not_const(self),
            },
            Builtin::InputString => match folded {
                Some(Const::Str(default)) => InputSpec::Str {
                    name,
                    title,
                    default,
                    options: Vec::new(),
                },
                _ => return not_const(self),
            },
            Builtin::InputSource => {
                // The default is a builtin series name, not a constant.
                let source = match self.kind(default_arg.value) {
                    NodeKind::Name(series) => Builtin::lookup(series).and_then(source_of_builtin),
                    _ => None,
                };
                match source {
                    Some(default) => InputSpec::Source {
                        name,
                        title,
                        default,
                    },
                    None => {
                        self.errors.push(PineError::new(
                            ErrorCode::PineInputNotConst,
                            default_arg.span,
                            "input.source defaults must be a series name (close, delta, …)",
                        ));
                        return;
                    }
                }
            }
            _ => unreachable!("input_call only receives input builtins"),
        };
        self.inputs.push(spec);
        self.input_of_call[call.index()] = Some(index);
    }

    fn lookup_name(&self, name: &str, scope: &Scope, _ctx: &WalkCtx) -> Option<Resolution> {
        for (candidate, resolution) in scope.names.iter().rev() {
            if candidate == name {
                return Some(*resolution);
            }
        }
        Builtin::lookup(name).map(Resolution::Builtin)
    }

    fn unknown_name(&mut self, name: &str, span: Span) {
        // A rejected *variable* — `dayofweek`, `max_bars_back` — reaches here
        // rather than the call or member arms, and "unknown name" is the
        // wrong reason: the name is known, the family is refused. Answer with
        // the family's own explanation, which is what the dialect reference
        // promises for every code in the reject list.
        if let Some((code, reason)) = rejection_of(name) {
            self.errors.push(PineError::new(code, span, reason));
            return;
        }
        let mut error = PineError::new(
            ErrorCode::PineUnknownName,
            span,
            format!("unknown name `{name}`"),
        );
        if let Some(suggestion) = did_you_mean(name) {
            error = error.with_note(format!("did you mean `{suggestion}`?"));
        }
        self.errors.push(error);
    }
}

struct Frame {
    next_slot: u32,
}

struct WalkCtx {
    frame: Option<Frame>,
    in_loop: bool,
    /// False as soon as the walk enters any nested body — an `if`/`else`
    /// arm, a loop body, a function. `frame.is_some() || in_loop` cannot
    /// stand in for it: a `plot()` inside a top-level `if` has neither, and
    /// was accepted by the check *and* skipped by the registry pass, so the
    /// script silently drew nothing.
    top_level: bool,
    function_stack: Vec<u32>,
}

impl WalkCtx {
    fn global() -> Self {
        Self {
            frame: None,
            in_loop: false,
            top_level: true,
            function_stack: Vec::new(),
        }
    }

    /// Walk `body` as nested code, restoring the flag afterwards.
    fn nested<T>(&mut self, walk: impl FnOnce(&mut Self) -> T) -> T {
        let was_top_level = std::mem::replace(&mut self.top_level, false);
        let out = walk(self);
        self.top_level = was_top_level;
        out
    }
}
