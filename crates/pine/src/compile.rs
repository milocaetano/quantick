//! Compile passes: arena AST → [`CompiledScript`], everything the
//! interpreter needs with **zero string lookups at eval time**.
//!
//! The §3.4 passes, all running even when earlier ones erred so a script
//! with three problems reports three:
//!
//! 1. **Declaration collection** — the `indicator()` header, the fixed
//!    top-level `plot*` registry, `input.*` extraction (const args only).
//! 2. **Name resolution** — every name to a numbered slot (global or
//!    function-local), unknown names get a did-you-mean note.
//! 3. **Call-site numbering** — every stateful kernel call gets a
//!    [`CallSiteId`]; stateful calls inside loop bodies are refused
//!    (`PINE_STATEFUL_IN_LOOP`), recursion is refused (`PINE_RECURSION`).
//! 4. **Const folding + length check** — windowed kernel lengths must fold
//!    to a positive integer (`PINE_SERIES_LENGTH`), through input defaults.
//! 5. **`max_bars_back` inference** — constant history offsets are measured,
//!    dynamic ones capped by [`MAX_BARS_BACK_CAP`], and every `History` node
//!    gets its own implicit series id.
//! 6. **Unsupported-construct scan** — the §3.1 reject list, each family
//!    with its honest reason.
//!
//! Lookup results live in dense per-node tables (indexed by `NodeId`), which
//! is what "slot resolution" means here — the interpreter never touches a
//! map or a string on the per-bar path.

use std::collections::BTreeSet;

use quantick_indicators::{InputSpec, PlotId, PlotSpec, PlotStyle, Rgba8, SourceId};

use crate::ast::{Arg, Ast, BinOp, NodeId, NodeKind, UnOp};
use crate::builtins::{Builtin, did_you_mean, rejection_of};
use crate::error::{ErrorCode, PineError, Span};
use crate::lexer;
use crate::parser;

/// Reads deeper than this many bars yield `na` when the offset is dynamic
/// (constant offsets size storage exactly; the cap bounds the dynamic case).
pub const MAX_BARS_BACK_CAP: usize = 500;

/// Default plot stroke width when a `plot()` gives none.
const DEFAULT_PLOT_WIDTH: f32 = 1.5;
/// Default plot color when a `plot()` gives none (egui-agnostic amber).
const DEFAULT_PLOT_COLOR: Rgba8 = Rgba8::opaque(255, 179, 0);

/// Identity of one stateful kernel call site (§2.5 locked decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallSiteId(pub u32);

/// What a resolved name/member/callee means at eval time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Resolution {
    /// Not a reference (literals, operators, …).
    #[default]
    None,
    /// A builtin variable or function.
    Builtin(Builtin),
    /// A global (script-level) variable slot.
    Global(u32),
    /// A function-local slot (param or body declaration), frame-indexed.
    Local(u32),
    /// A user function, by index into [`CompiledScript::functions`].
    Function(u32),
    /// An enum-like member constant (`plot.style_line`, `shape.circle`, …),
    /// resolved to its tag for the interpreter.
    EnumConst(&'static str),
    /// A color constant (`color.red`, …), resolved to its packed RGBA so the
    /// per-bar path never re-derives it from names.
    ColorConst(u32),
}

/// One user function, resolved.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// The function's name (diagnostics only; calls are index-resolved).
    pub name: String,
    /// Number of parameters (the first locals of the frame).
    pub param_count: usize,
    /// Total frame slots (params + body declarations).
    pub local_slots: usize,
    /// The body node.
    pub body: NodeId,
}

/// A compiled script: the AST plus every dense eval-time table.
pub struct CompiledScript {
    /// The arena.
    pub ast: Ast,
    /// The `Script` root.
    pub root: NodeId,
    /// Display title from `indicator(...)` (file stem when absent).
    pub title: String,
    /// Optional short title.
    pub short_title: Option<String>,
    /// Overlay flag from `indicator(overlay=…)`.
    pub overlay: bool,
    /// Declared inputs, in extraction order.
    pub inputs: Vec<InputSpec>,
    /// The fixed plot registry, in top-level declaration order.
    pub plots: Vec<PlotSpec>,
    /// Load-time warnings (ignored args, inert `alertcondition`, missing
    /// version directive) — surfaced in the UI, never fatal.
    pub warnings: Vec<PineError>,
    /// Per-node resolution (dense, indexed by `NodeId`).
    pub resolutions: Vec<Resolution>,
    /// Per-node call-site id for stateful kernel calls.
    pub call_sites: Vec<Option<CallSiteId>>,
    /// Per-node plot index for top-level `plot`/`plotshape`/`plotchar`
    /// calls.
    pub plot_of_call: Vec<Option<usize>>,
    /// Per-node input index for `input.*` calls.
    pub input_of_call: Vec<Option<usize>>,
    /// Per-node implicit series id for `History` nodes (expression history).
    pub history_series: Vec<Option<u32>>,
    /// User functions.
    pub functions: Vec<FunctionInfo>,
    /// Number of global slots.
    pub global_slots: usize,
    /// Total stateful call sites.
    pub call_site_count: usize,
    /// Total implicit history series.
    pub history_series_count: usize,
    /// Deepest history the script needs (constant offsets measured, dynamic
    /// capped).
    pub max_bars_back: usize,
}

/// Compile source text end to end (lex → parse → passes).
///
/// # Errors
///
/// Every error from every stage, collected; the script loads only if this
/// returns `Ok`.
pub fn compile(source: &str, fallback_title: &str) -> Result<CompiledScript, Vec<PineError>> {
    let lexed = lexer::lex(source)?;
    let (ast, root) = parser::parse(&lexed.tokens)?;

    let mut compiler = Compiler::new(ast, root, fallback_title);
    if let Some((version, span)) = lexed.version {
        if version != 5 {
            compiler.errors.push(PineError::new(
                ErrorCode::PineVersion,
                span,
                format!("only Pine v5 scripts are supported, found version {version}"),
            ));
        }
    } else {
        compiler.warnings.push(PineError::new(
            ErrorCode::PineVersion,
            Span::at(0),
            "no //@version directive; assuming version 5",
        ));
    }

    compiler.collect_reassigned_names();
    compiler.collect_declarations();
    compiler.resolve_and_scan();
    compiler.register_pending_plots();
    if compiler.errors.is_empty() {
        Ok(compiler.finish())
    } else {
        Err(compiler.errors)
    }
}

/// A constant value the folder can produce at load time.
#[derive(Debug, Clone, PartialEq)]
enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Color(u32),
    Na,
    /// `plot.style_line`, `shape.circle`, `location.abovebar`, …
    Enum(&'static str),
}

impl Const {
    fn as_f64(&self) -> Option<f64> {
        match self {
            Const::Int(v) => Some(*v as f64),
            Const::Float(v) => Some(*v),
            _ => None,
        }
    }

    fn as_positive_len(&self) -> Option<usize> {
        match self {
            Const::Int(v) if *v >= 1 => usize::try_from(*v).ok(),
            _ => None,
        }
    }
}

/// Member constants of the `color`/`plot`/`shape`/`location` namespaces.
/// Colors fold to values; the rest fold to enum tags the interpreter maps.
fn member_constant(namespace: &str, field: &str) -> Option<Const> {
    if namespace == "color" {
        let rgb = match field {
            "red" => 0xF236_45FF_u32,
            "green" => 0x0899_81FF,
            "blue" => 0x2962_FFFF,
            "orange" => 0xFF98_00FF,
            "aqua" => 0x00BC_D4FF,
            "yellow" => 0xFDD8_35FF,
            "purple" => 0x9C27_B0FF,
            "lime" => 0x00E6_76FF,
            "white" => 0xFFFF_FFFF,
            "black" => 0x0000_00FF,
            "gray" | "grey" => 0x7878_80FF,
            "silver" => 0xB2B5_BEFF,
            "maroon" => 0x8000_00FF,
            "navy" => 0x0000_80FF,
            "olive" => 0x8080_00FF,
            "teal" => 0x0089_7BFF,
            "fuchsia" => 0xE040_FBFF,
            _ => return None,
        };
        return Some(Const::Color(rgb));
    }
    let tag = match (namespace, field) {
        // format.* is accepted so `indicator(format=...)` resolves; the
        // header then warns that it is ignored.
        ("format", "price") => "format_price",
        ("format", "volume") => "format_volume",
        ("format", "percent") => "format_percent",
        ("format", "inherit") => "format_inherit",
        ("plot", "style_line") => "style_line",
        ("plot", "style_stepline") => "style_stepline",
        ("plot", "style_histogram") => "style_histogram",
        ("plot", "style_columns") => "style_columns",
        ("plot", "style_circles") => "style_circles",
        ("plot", "style_cross") => "style_cross",
        ("plot", "style_area") => "style_area",
        ("shape", "triangleup") => "triangleup",
        ("shape", "triangledown") => "triangledown",
        ("shape", "circle") => "circle",
        ("shape", "labelup") => "labelup",
        ("shape", "labeldown") => "labeldown",
        ("shape", "cross") => "cross",
        ("location", "abovebar") => "abovebar",
        ("location", "belowbar") => "belowbar",
        ("location", "absolute") => "absolute",
        _ => return None,
    };
    Some(Const::Enum(tag))
}

fn style_of_tag(tag: &str) -> PlotStyle {
    match tag {
        "style_stepline" => PlotStyle::StepLine,
        "style_histogram" => PlotStyle::Histogram,
        "style_columns" => PlotStyle::Columns,
        "style_circles" => PlotStyle::Circles,
        "style_cross" => PlotStyle::Cross,
        "style_area" => PlotStyle::Area,
        _ => PlotStyle::Line,
    }
}

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

struct Compiler {
    ast: Ast,
    root: NodeId,
    errors: Vec<PineError>,
    warnings: Vec<PineError>,
    title: String,
    short_title: Option<String>,
    overlay: bool,
    inputs: Vec<InputSpec>,
    plots: Vec<PlotSpec>,
    resolutions: Vec<Resolution>,
    call_sites: Vec<Option<CallSiteId>>,
    plot_of_call: Vec<Option<usize>>,
    input_of_call: Vec<Option<usize>>,
    history_series: Vec<Option<u32>>,
    functions: Vec<FunctionInfo>,
    function_names: Vec<String>,
    global_slots: u32,
    call_site_count: u32,
    history_series_count: u32,
    max_bars_back: usize,
    /// Global declaration RHS by slot, for const folding through names.
    global_init: Vec<Option<NodeId>>,
    /// Slots reassigned anywhere (`:=`): never const-foldable.
    reassigned: Vec<bool>,
    /// Every name that appears on the left of a `:=`, collected from the
    /// whole arena before the walk. Filling `reassigned` during the walk made
    /// the answer depend on textual order: a `:=` *after* the use folded the
    /// value anyway, so `var len = 9` / `ta.sma(close, len)` / `len := len+1`
    /// compiled clean and pinned bar 0's window forever.
    reassigned_names: BTreeSet<String>,
    /// Plot calls found in pass 1, registered after name resolution.
    pending_plots: Vec<(NodeId, Vec<Arg>, Option<&'static str>)>,
}

impl Compiler {
    fn new(ast: Ast, root: NodeId, fallback_title: &str) -> Self {
        let n = ast.len();
        Self {
            ast,
            root,
            errors: Vec::new(),
            warnings: Vec::new(),
            title: fallback_title.to_owned(),
            short_title: None,
            overlay: false,
            inputs: Vec::new(),
            plots: Vec::new(),
            pending_plots: Vec::new(),
            resolutions: vec![Resolution::None; n],
            call_sites: vec![None; n],
            plot_of_call: vec![None; n],
            input_of_call: vec![None; n],
            history_series: vec![None; n],
            functions: Vec::new(),
            function_names: Vec::new(),
            global_slots: 0,
            call_site_count: 0,
            history_series_count: 0,
            max_bars_back: 1,
            global_init: Vec::new(),
            reassigned: Vec::new(),
            reassigned_names: BTreeSet::new(),
        }
    }

    fn finish(self) -> CompiledScript {
        CompiledScript {
            ast: self.ast,
            root: self.root,
            title: self.title,
            short_title: self.short_title,
            overlay: self.overlay,
            inputs: self.inputs,
            plots: self.plots,
            warnings: self.warnings,
            resolutions: self.resolutions,
            call_sites: self.call_sites,
            plot_of_call: self.plot_of_call,
            input_of_call: self.input_of_call,
            history_series: self.history_series,
            functions: self.functions,
            global_slots: self.global_slots as usize,
            call_site_count: self.call_site_count as usize,
            history_series_count: self.history_series_count as usize,
            max_bars_back: self.max_bars_back,
        }
    }

    // ---- helpers over the arena ------------------------------------------

    fn kind(&self, id: NodeId) -> &NodeKind {
        &self.ast.node(id).kind
    }

    fn span(&self, id: NodeId) -> Span {
        self.ast.node(id).span
    }

    /// The dotted name of a callee: `Name` or `Name.field` (one level —
    /// deeper chains are object method calls, resolved at eval).
    fn callee_name(&self, callee: NodeId) -> Option<String> {
        match self.kind(callee) {
            NodeKind::Name(name) => Some(name.clone()),
            NodeKind::Member { object, field } => match self.kind(*object) {
                NodeKind::Name(namespace) => Some(format!("{namespace}.{field}")),
                _ => None,
            },
            _ => None,
        }
    }

    fn arg<'a>(&self, args: &'a [Arg], position: usize, name: &str) -> Option<&'a Arg> {
        args.iter()
            .find(|a| a.name.as_deref() == Some(name))
            .or_else(|| args.iter().filter(|a| a.name.is_none()).nth(position))
    }

    // ---- pass 1: declaration collection ----------------------------------

    /// Scan the whole arena for `:=` targets before anything folds.
    fn collect_reassigned_names(&mut self) {
        self.reassigned_names = self
            .ast
            .all()
            .filter_map(|node| match &node.kind {
                NodeKind::Reassign { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
    }

    fn collect_declarations(&mut self) {
        let statements = match self.kind(self.root) {
            NodeKind::Script { statements } => statements.clone(),
            _ => return,
        };
        for statement in statements {
            let expr = match self.kind(statement) {
                NodeKind::ExprStmt { expr } => *expr,
                // Inputs live on declaration RHSes; collected during
                // resolution's const walk instead (they need slot context).
                _ => continue,
            };
            let NodeKind::Call { callee, args } = self.kind(expr).clone() else {
                continue;
            };
            let Some(name) = self.callee_name(callee) else {
                continue;
            };
            match Builtin::lookup(&name) {
                Some(Builtin::Indicator) => self.indicator_header(expr, &args),
                // Registered after resolution: `plot(close, color=c)` can
                // only fold `c` once names resolve, and folding it here
                // silently substituted the default for every argument that
                // came from a variable. Queued in textual order, which is
                // the plot order the whole system indexes by.
                Some(Builtin::Plot) => self.pending_plots.push((expr, args.clone(), None)),
                Some(Builtin::PlotShape) => {
                    self.pending_plots.push((expr, args.clone(), Some("shape")));
                }
                Some(Builtin::PlotChar) => {
                    self.pending_plots.push((expr, args.clone(), Some("char")));
                }
                Some(Builtin::AlertCondition) => {
                    let span = self.span(expr);
                    self.warnings.push(PineError::new(
                        ErrorCode::PineUnsupported,
                        span,
                        "alertcondition() is accepted but inert (no alerts here)",
                    ));
                }
                _ => {}
            }
        }
    }

    fn indicator_header(&mut self, call: NodeId, args: &[Arg]) {
        const KNOWN: &[&str] = &["title", "shorttitle", "overlay", "precision"];
        if let Some(arg) = self.arg(args, 0, "title")
            && let Some(Const::Str(title)) = self.fold(arg.value)
        {
            self.title = title;
        }
        if let Some(arg) = self.arg(args, 1, "shorttitle")
            && let Some(Const::Str(short)) = self.fold(arg.value)
        {
            self.short_title = Some(short);
        }
        if let Some(arg) = args.iter().find(|a| a.name.as_deref() == Some("overlay"))
            && let Some(Const::Bool(overlay)) = self.fold(arg.value)
        {
            self.overlay = overlay;
        }
        for arg in args {
            if let Some(name) = arg.name.as_deref()
                && !KNOWN.contains(&name)
            {
                self.warnings.push(PineError::new(
                    ErrorCode::PineUnsupported,
                    arg.span,
                    format!("indicator(... {name}=...) is accepted but ignored"),
                ));
            }
        }
        // `precision` parses and is ignored, with the honest warning.
        if let Some(arg) = args.iter().find(|a| a.name.as_deref() == Some("precision")) {
            self.warnings.push(PineError::new(
                ErrorCode::PineUnsupported,
                arg.span,
                "indicator(precision=...) is accepted but ignored",
            ));
        }
        let _ = call;
    }

    /// Register every queued plot, now that names resolve.
    fn register_pending_plots(&mut self) {
        for (call, args, marker) in std::mem::take(&mut self.pending_plots) {
            self.plot_call(call, &args, marker);
        }
    }

    /// Say so when an argument the author supplied could not be folded.
    ///
    /// A plot carries one constant look, so `color = up ? green : red` (or
    /// any value that is only known per bar) cannot be honoured. Substituting
    /// the default in silence is the failure mode this warning exists to
    /// prevent — the chart would simply not be what the script says.
    fn warn_unfoldable_plot_arg(&mut self, args: &[Arg], position: usize, name: &str) {
        let Some(arg) = self.arg(args, position, name) else {
            return;
        };
        if self.fold(arg.value).is_some() {
            return;
        }
        let span = self.span(arg.value);
        self.warnings.push(PineError::new(
            ErrorCode::PineUnsupported,
            span,
            format!(
                "`{name}` must be constant at load time; this one is not, so the                  default is used (per-bar colors and widths land with the                  dynamic-color milestone)"
            ),
        ));
    }

    fn plot_call(&mut self, call: NodeId, args: &[Arg], marker: Option<&str>) {
        let index = self.plots.len();
        for (position, name) in [(1, "title"), (2, "color"), (3, "linewidth"), (4, "style")] {
            if marker.is_some() && position == 4 {
                continue; // markers ignore `style`; nothing to warn about
            }
            self.warn_unfoldable_plot_arg(args, position, name);
        }
        let title = self
            .arg(args, 1, "title")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Str(s)) => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| format!("plot {index}"));
        let style = match marker {
            // plotshape/plotchar render as markers; the style tag rides the
            // spec so M3 can refine shapes without changing the registry.
            Some(_) => PlotStyle::Circles,
            None => self
                .arg(args, 4, "style")
                .and_then(|a| match self.fold(a.value) {
                    Some(Const::Enum(tag)) => Some(style_of_tag(tag)),
                    _ => None,
                })
                .unwrap_or(PlotStyle::Line),
        };
        let base_color = self
            .arg(args, 2, "color")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Color(rgba)) => Some(Rgba8::from_u32(rgba)),
                _ => None,
            })
            .unwrap_or(DEFAULT_PLOT_COLOR);
        let width = self
            .arg(args, 3, "linewidth")
            .and_then(|a| self.fold(a.value)?.as_f64())
            .map_or(DEFAULT_PLOT_WIDTH, |w| w as f32);
        self.plots.push(PlotSpec {
            id: PlotId::new(index),
            title,
            style,
            base_color,
            width,
            offset: 0,
        });
        self.plot_of_call[call.index()] = Some(index);
    }

    // ---- pass 4 support: const folding ------------------------------------

    fn fold(&self, id: NodeId) -> Option<Const> {
        match self.kind(id) {
            NodeKind::Int(v) => Some(Const::Int(*v)),
            NodeKind::Float(v) => Some(Const::Float(*v)),
            NodeKind::Str(v) => Some(Const::Str(v.clone())),
            NodeKind::Bool(v) => Some(Const::Bool(*v)),
            NodeKind::Color(v) => Some(Const::Color(*v)),
            NodeKind::Na => Some(Const::Na),
            NodeKind::Member { object, field } => match self.kind(*object) {
                NodeKind::Name(namespace) => member_constant(namespace, field),
                _ => None,
            },
            NodeKind::Unary {
                op: UnOp::Neg,
                operand,
            } => match self.fold(*operand)? {
                Const::Int(v) => Some(Const::Int(-v)),
                Const::Float(v) => Some(Const::Float(-v)),
                _ => None,
            },
            NodeKind::Binary { op, lhs, rhs } => {
                let l = self.fold(*lhs)?;
                let r = self.fold(*rhs)?;
                if let (Const::Int(a), Const::Int(b)) = (&l, &r) {
                    return Some(match op {
                        BinOp::Add => Const::Int(a.checked_add(*b)?),
                        BinOp::Sub => Const::Int(a.checked_sub(*b)?),
                        BinOp::Mul => Const::Int(a.checked_mul(*b)?),
                        BinOp::Div if *b != 0 && a % b == 0 => Const::Int(a / b),
                        BinOp::Div if *b != 0 => Const::Float(*a as f64 / *b as f64),
                        _ => return None,
                    });
                }
                let a = l.as_f64()?;
                let b = r.as_f64()?;
                Some(match op {
                    BinOp::Add => Const::Float(a + b),
                    BinOp::Sub => Const::Float(a - b),
                    BinOp::Mul => Const::Float(a * b),
                    // The interpreter maps division by zero to `na` as a
                    // documented divergence from IEEE; a folded expression
                    // that answered `inf` would evaluate differently from the
                    // same expression left unfolded.
                    BinOp::Div if b == 0.0 => Const::Na,
                    BinOp::Div => Const::Float(a / b),
                    _ => return None,
                })
            }
            // A name folds when it is a never-reassigned global whose
            // initialiser folds (this is how `len = input.int(9)` reaches a
            // kernel length).
            NodeKind::Name(_) => match self.resolutions[id.index()] {
                Resolution::Global(slot) => {
                    if self.reassigned.get(slot as usize).copied().unwrap_or(true) {
                        return None;
                    }
                    let init = self.global_init.get(slot as usize).copied().flatten()?;
                    self.fold(init)
                }
                Resolution::EnumConst(tag) => Some(Const::Enum(tag)),
                _ => None,
            },
            // `input.*(default, …)` folds to its default (inputs are bound
            // per instance; lengths re-checked at bind time).
            NodeKind::Call { callee, args } => {
                let name = self.callee_name(*callee)?;
                match Builtin::lookup(&name) {
                    Some(
                        Builtin::InputInt
                        | Builtin::InputFloat
                        | Builtin::InputBool
                        | Builtin::InputString,
                    ) => self.fold(self.arg(args, 0, "defval")?.value),
                    Some(Builtin::ColorNew) => {
                        let base = match self.fold(self.arg(args, 0, "color")?.value)? {
                            Const::Color(rgba) => rgba,
                            _ => return None,
                        };
                        let transparency =
                            self.fold(self.arg(args, 1, "transp")?.value)?.as_f64()?;
                        let alpha =
                            (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                        Some(Const::Color((base & 0xFFFF_FF00) | alpha))
                    }
                    Some(Builtin::ColorRgb) => {
                        let r = self.fold(self.arg(args, 0, "red")?.value)?.as_f64()? as u32;
                        let g = self.fold(self.arg(args, 1, "green")?.value)?.as_f64()? as u32;
                        let b = self.fold(self.arg(args, 2, "blue")?.value)?.as_f64()? as u32;
                        let transparency = match self.arg(args, 3, "transp") {
                            Some(arg) => self.fold(arg.value)?.as_f64()?,
                            None => 0.0,
                        };
                        let alpha =
                            (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                        Some(Const::Color(
                            ((r & 0xFF) << 24) | ((g & 0xFF) << 16) | ((b & 0xFF) << 8) | alpha,
                        ))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ---- passes 2/3/5/6: one resolved walk --------------------------------

    fn resolve_and_scan(&mut self) {
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

    fn declare_global(&mut self, name: &str, init: Option<NodeId>, scope: &mut Scope) -> u32 {
        let slot = self.global_slots;
        self.global_slots += 1;
        self.global_init.push(init);
        self.reassigned.push(self.reassigned_names.contains(name));
        scope
            .names
            .push((name.to_owned(), Resolution::Global(slot)));
        slot
    }

    fn walk_statement(&mut self, id: NodeId, scope: &mut Scope, ctx: &mut WalkCtx) {
        match self.kind(id).clone() {
            NodeKind::Declare { name, value, .. } => {
                self.walk_expr(value, scope, ctx);
                if let Some(frame) = ctx.frame.as_mut() {
                    let slot = frame.next_slot;
                    frame.next_slot += 1;
                    scope.names.push((name, Resolution::Local(slot)));
                    self.resolutions[id.index()] = Resolution::Local(slot);
                    return;
                }
                let slot = self.declare_global(&name, Some(value), scope);
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
                    self.declare_global(name, None, scope);
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
                    let slot = self.declare_global(&var, None, scope);
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
                    if matches!(namespace.as_str(), "ta" | "math" | "color" | "input") {
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
                    self.unknown_name(&name, self.span(id));
                    return;
                }
                // Method-style call on an expression (`myline.set_xy(...)`)
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
            self.check_kernel_length(call, builtin, args);
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
            .and_then(|c| c.as_positive_len())
            .is_none()
        {
            self.errors.push(PineError::new(
                ErrorCode::PineSeriesLength,
                arg.span,
                "a kernel length must fold to a positive integer at load time \
                 (a literal, an input, or arithmetic over them)",
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
