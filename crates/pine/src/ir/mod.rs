//! Dense compiled tables, owned by child 4 after skeleton integration.
mod kernel_reach;
pub(crate) mod program;
use crate::{
    ast::{Ast, NodeId, VarMode},
    builtins::Builtin,
    error::PineError,
};
use quantick_indicators::{FillSpec, InputSpec, PlotSpec};

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

/// Specialization lookup: (parent context, call node) to child context, overload and argument order.
pub type FunctionDispatch = std::collections::BTreeMap<(u32, usize), (u32, u32, Vec<usize>)>;

/// Type-directed lowering without runtime source-name lookups.
#[derive(Clone, Default)]
pub struct TypeProgram {
    pub(crate) version: crate::version::Version,
    /// Per-context, per-node scalar conversions, checked at load.
    pub coercions: Vec<Vec<u8>>,
    /// Per-context v5 const int division nodes, truncated after evaluation.
    pub integer_divisions: Vec<Vec<bool>>,
    /// (Parent type context, call node) to (child context, resolved overload).
    pub dispatch: FunctionDispatch,
}

/// A compiled script: the AST plus every dense eval-time table.
///
/// `Clone` so that rebinding inputs (the settings dialog's Apply) can build a
/// second instance from the same compile instead of parsing the source again
/// — a click-rate path either way, but re-parsing would also mean a script
/// edited on disk since load would silently change under Apply.
#[derive(Clone)]
pub struct CompiledScript {
    pub(crate) version: crate::version::Version,
    pub(crate) program: program::Program,
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
    /// Headless OHLC outputs, in declaration order.
    pub candle_plots: Vec<quantick_indicators::CandlePlotSpec>,
    /// Fills between plot pairs, resolved at load time.
    pub fills: Vec<FillSpec>,
    /// Load-time warnings (ignored args, inert `alertcondition`, missing
    /// version directive) — surfaced in the UI, never fatal.
    pub warnings: Vec<PineError>,
    /// Per-node resolution (dense, indexed by `NodeId`).
    pub resolutions: Vec<Resolution>,
    /// Checked scalar lowering and call-context dispatch.
    pub types: TypeProgram,
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
