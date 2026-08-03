//! The arena AST.
//!
//! Nodes live in one flat `Vec` addressed by [`NodeId`] — cache-friendly, no
//! deep `Box` chains, and trivially shareable by later passes. Every node
//! keeps its [`Span`]; every later diagnostic points through it.

use crate::error::Span;

/// Index of one node in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// The arena slot.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Declaration mode of a variable statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarMode {
    /// `x = e` — re-initialised every bar.
    Plain,
    /// `var x = e` — initialised once, persists across bars.
    Var,
    /// `varip x = e` — persists across preview runs within one bar too.
    Varip,
}

/// Binary operators, in Pine semantics (comparisons with NaN are false).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+` (numbers; string concatenation is not in v1).
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/` (division by zero yields `na`).
    Div,
    /// `%`
    Mod,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `and` (short-circuit).
    And,
    /// `or` (short-circuit).
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `not x`
    Not,
}

/// One call argument, positional or named (`color=red`).
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    /// The parameter name for a named argument.
    pub name: Option<String>,
    /// The value expression.
    pub value: NodeId,
    /// Span of the whole argument (diagnostics point at the argument, not
    /// just its value).
    pub span: Span,
}

/// What a node is. Statements and expressions share the arena; blocks are
/// expressions valued as their last statement (Pine's rule).
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// The root: every top-level statement in order.
    Script {
        /// Top-level statements.
        statements: Vec<NodeId>,
    },
    /// `x = e`, `var x = e`, `varip x = e`.
    Declare {
        /// Plain / `var` / `varip`.
        mode: VarMode,
        /// The declared name.
        name: String,
        /// The initialiser.
        value: NodeId,
    },
    /// `[a, b] = f()` — tuple destructuring.
    TupleDeclare {
        /// The declared names, left to right.
        names: Vec<String>,
        /// The tuple-valued initialiser.
        value: NodeId,
    },
    /// `x := e` — reassignment of an existing variable.
    Reassign {
        /// The assigned name.
        name: String,
        /// The new value.
        value: NodeId,
    },
    /// `f(a, b) => …` — user function definition.
    FunctionDef {
        /// The function name.
        name: String,
        /// Parameter names.
        params: Vec<String>,
        /// Body: an expression or a block.
        body: NodeId,
    },
    /// An expression evaluated for its effect (`plot(close)`).
    ExprStmt {
        /// The expression.
        expr: NodeId,
    },
    /// An indented statement sequence, valued as its last statement.
    Block {
        /// The statements, in order.
        statements: Vec<NodeId>,
    },
    /// `if cond … else …` as an expression. `else if` chains nest in
    /// `otherwise`.
    If {
        /// The condition.
        condition: NodeId,
        /// The then-block.
        then_block: NodeId,
        /// The else-block or chained `if`, when present.
        otherwise: Option<NodeId>,
    },
    /// `for i = a to b by s` with a block body; value = last iteration's
    /// body value.
    For {
        /// The loop variable.
        var: String,
        /// Start value.
        from: NodeId,
        /// End value (inclusive, Pine's rule).
        to: NodeId,
        /// Step, when written.
        by: Option<NodeId>,
        /// The body block.
        body: NodeId,
    },
    /// `while cond` with a block body.
    While {
        /// The condition.
        condition: NodeId,
        /// The body block.
        body: NodeId,
    },
    /// `cond ? a : b`.
    Ternary {
        /// The condition.
        condition: NodeId,
        /// Value when true.
        if_true: NodeId,
        /// Value when false.
        if_false: NodeId,
    },
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinOp,
        /// Left operand.
        lhs: NodeId,
        /// Right operand.
        rhs: NodeId,
    },
    /// A unary operation.
    Unary {
        /// The operator.
        op: UnOp,
        /// The operand.
        operand: NodeId,
    },
    /// A call: `callee(args…)`. The callee is a name, member or any postfix
    /// chain.
    Call {
        /// What is called.
        callee: NodeId,
        /// The arguments.
        args: Vec<Arg>,
    },
    /// The history operator `subject[offset]`.
    History {
        /// The series expression.
        subject: NodeId,
        /// How many bars back.
        offset: NodeId,
    },
    /// Member access: `ta.ema`, `myline.set_xy`.
    Member {
        /// The object or namespace expression.
        object: NodeId,
        /// The member name.
        field: String,
    },
    /// A tuple expression `[a, b]` — the multi-value return of a user
    /// function, consumed by [`NodeKind::TupleDeclare`].
    Tuple {
        /// The element expressions, left to right.
        items: Vec<NodeId>,
    },
    /// A bare name.
    Name(String),
    /// Integer literal.
    Int(i64),
    /// Float literal.
    Float(f64),
    /// String literal.
    Str(String),
    /// `true` / `false`.
    Bool(bool),
    /// `#RRGGBBAA` literal, packed RGBA.
    Color(u32),
    /// `na`.
    Na,
}

/// One arena node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// What it is.
    pub kind: NodeKind,
    /// Where it is.
    pub span: Span,
}

/// The arena.
///
/// Nodes are stored in push order, which is children-before-parents: the
/// parser pushes each statement as it finishes it and the
/// [`NodeKind::Script`] root last, so node 0 is the *first literal parsed*,
/// not the root. `parse` returns the root id explicitly — a pass that wants
/// the top of the tree takes it from there rather than assuming an index.
#[derive(Debug, Default)]
pub struct Ast {
    nodes: Vec<Node>,
}

impl Ast {
    /// An empty arena.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a node, returning its id.
    pub fn push(&mut self, kind: NodeKind, span: Span) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("scripts fit in u32 nodes"));
        self.nodes.push(Node { kind, span });
        id
    }

    /// Read one node.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Number of nodes (perf budgets talk about "AST nodes"; this is that).
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when nothing was parsed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
