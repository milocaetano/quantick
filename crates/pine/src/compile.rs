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

use quantick_indicators::{FillSpec, InputSpec, PlotSpec};

use crate::ast::{Arg, Ast, NodeId, NodeKind, VarMode};
use crate::builtins::Builtin;
use crate::error::{ErrorCode, PineError, Span};
use crate::lexer;
use crate::parser;

mod declarations;
mod fold;
mod resolve;

/// Reads deeper than this many bars yield `na` when the offset is dynamic
/// (constant offsets size storage exactly; the cap bounds the dynamic case).
pub const MAX_BARS_BACK_CAP: usize = 500;

/// The largest window a `ta.*` kernel may declare.
///
/// A kernel allocates its ring up front, so an unbounded length is an
/// unbounded allocation: `ta.sma(close, 2000000000)` asks for 16 GB, and a
/// failed allocation *aborts* the process — it does not unwind, so the
/// promise that a runtime failure only disables its own indicator would be
/// broken by a typo. Far above any window a chart uses.
pub const MAX_KERNEL_LENGTH: usize = 100_000;

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
///
/// `Clone` so that rebinding inputs (the settings dialog's Apply) can build a
/// second instance from the same compile instead of parsing the source again
/// — a click-rate path either way, but re-parsing would also mean a script
/// edited on disk since load would silently change under Apply.
#[derive(Clone)]
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
    /// Fills between plot pairs, resolved at load time.
    pub fills: Vec<FillSpec>,
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
    /// Declaration mode per global slot (preview rollback exempts `varip`).
    pub slot_modes: Vec<VarMode>,
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
    compiler.resolve_fills();
    if compiler.errors.is_empty() {
        Ok(compiler.finish())
    } else {
        Err(compiler.errors)
    }
}

/// What a queued pass-1 call turns into once names resolve.
#[derive(Debug, Clone, Copy)]
enum PendingPlot {
    /// `plot(...)`.
    Plot,
    /// `hline(...)` — a plot column of a constant value.
    Hline,
    /// `plotshape(...)` / `plotchar(...)`.
    Shape { is_char: bool },
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
    slot_modes: Vec<VarMode>,
    call_site_count: u32,
    history_series_count: u32,
    max_bars_back: usize,
    /// Global declaration RHS by slot, for const folding through names.
    global_init: Vec<Option<NodeId>>,
    /// Slots reassigned anywhere (`:=`): never const-foldable.
    reassigned: Vec<bool>,
    /// `fill(...)` calls, resolved after name resolution (their plot-handle
    /// arguments need the global-init map).
    pending_fills: Vec<(NodeId, Vec<Arg>)>,
    fills: Vec<FillSpec>,
    /// Every name that appears on the left of a `:=`, collected from the
    /// whole arena before the walk. Filling `reassigned` during the walk made
    /// the answer depend on textual order: a `:=` *after* the use folded the
    /// value anyway, so `var len = 9` / `ta.sma(close, len)` / `len := len+1`
    /// compiled clean and pinned bar 0's window forever.
    reassigned_names: BTreeSet<String>,
    /// Plot and shape calls found in pass 1, registered after name
    /// resolution so their arguments can fold through names.
    pending_plots: Vec<(NodeId, Vec<Arg>, PendingPlot)>,
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
            slot_modes: Vec::new(),
            call_site_count: 0,
            history_series_count: 0,
            max_bars_back: 1,
            global_init: Vec::new(),
            reassigned: Vec::new(),
            pending_fills: Vec::new(),
            fills: Vec::new(),
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
            fills: self.fills,
            warnings: self.warnings,
            resolutions: self.resolutions,
            call_sites: self.call_sites,
            plot_of_call: self.plot_of_call,
            input_of_call: self.input_of_call,
            history_series: self.history_series,
            functions: self.functions,
            global_slots: self.global_slots as usize,
            slot_modes: self.slot_modes,
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
}
