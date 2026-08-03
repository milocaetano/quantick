//! Recursive-descent parser: tokens → arena AST.
//!
//! Statement dispatch is decided by bounded lookahead (never backtracking
//! more than one saved position — used only to distinguish `f(a, b) => …`
//! function definitions from `f(a, b)` call statements). Expressions use
//! precedence climbing. On a statement error the parser records it and
//! resynchronises at the next logical line, so a script with three bad
//! statements reports three.
//!
//! `switch` is recognised and rejected here with an honest message (it may
//! land in M4, per the plan); everything else in the reject list is a
//! *compile pass* concern — the parser builds the tree, the passes judge it.

use crate::ast::{Arg, Ast, BinOp, NodeId, NodeKind, UnOp, VarMode};
use crate::error::{ErrorCode, PineError, Span};
use crate::lexer::{Tok, Token};

/// How deeply expressions may nest.
///
/// Descent costs about five stack frames per `(`, and a stack overflow
/// aborts the process — no unwind, no `PineError`, nothing for the script
/// library to render. A generated or truncated file of a hundred thousand
/// open parens is the realistic way to hit it. The limit is far above any
/// script a person writes and the check is one comparison on a path that
/// runs once per load.
const MAX_NEST_DEPTH: usize = 64;

/// Parse a lexed token stream into an AST (root = `Script`).
///
/// # Errors
///
/// Every syntax error found, each with its span; the AST is only returned
/// when the whole script parsed.
pub fn parse(tokens: &[Token]) -> Result<(Ast, NodeId), Vec<PineError>> {
    debug_assert!(
        !tokens.is_empty(),
        "a lexed stream always ends in `Eof`; `peek` reads the last token"
    );
    let mut parser = Parser {
        tokens,
        pos: 0,
        ast: Ast::new(),
        errors: Vec::new(),
        depth: 0,
    };
    let root = parser.script();
    if parser.errors.is_empty() {
        Ok((parser.ast, root))
    } else {
        Err(parser.errors)
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    ast: Ast,
    errors: Vec<PineError>,
    /// Current expression nesting, guarded by [`MAX_NEST_DEPTH`].
    depth: usize,
}

impl Parser<'_> {
    // ---- token plumbing ----------------------------------------------

    fn peek(&self) -> &Tok {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].tok
    }

    fn peek_at(&self, ahead: usize) -> &Tok {
        let i = (self.pos + ahead).min(self.tokens.len() - 1);
        &self.tokens[i].tok
    }

    fn span(&self) -> Span {
        self.tokens[self.pos.min(self.tokens.len() - 1)].span
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok, what: &str) -> bool {
        if self.eat(tok) {
            true
        } else {
            let span = self.span();
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                span,
                format!("expected {what}, found {}", describe(self.peek())),
            ));
            false
        }
    }

    /// Skip to just after the next `Newline` at the current block depth —
    /// error recovery so one bad statement doesn't drown the rest.
    fn synchronise(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Indent => {
                    depth += 1;
                    self.advance();
                }
                Tok::Dedent => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                    self.advance();
                }
                Tok::Newline if depth == 0 => {
                    self.advance();
                    // A failed header owns its indented block: swallow it
                    // whole, or every line inside cascades a bogus error.
                    if *self.peek() == Tok::Indent {
                        let mut block_depth = 0usize;
                        loop {
                            match self.peek() {
                                Tok::Eof => return,
                                Tok::Indent => {
                                    block_depth += 1;
                                    self.advance();
                                }
                                Tok::Dedent => {
                                    self.advance();
                                    block_depth -= 1;
                                    if block_depth == 0 {
                                        break;
                                    }
                                }
                                _ => {
                                    self.advance();
                                }
                            }
                        }
                    }
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    // ---- statements ---------------------------------------------------

    fn script(&mut self) -> NodeId {
        let start = self.span();
        let mut statements = Vec::new();
        while *self.peek() != Tok::Eof {
            match self.peek() {
                Tok::Newline => {
                    self.advance();
                }
                // A stray indent at top level is a layout bug worth naming.
                Tok::Indent | Tok::Dedent => {
                    let span = self.span();
                    self.errors.push(PineError::new(
                        ErrorCode::PineIndent,
                        span,
                        "unexpected indentation at top level",
                    ));
                    self.advance();
                }
                _ => {
                    let before = self.errors.len();
                    let statement = self.statement();
                    if self.errors.len() > before {
                        self.synchronise();
                    }
                    if let Some(statement) = statement {
                        statements.push(statement);
                    }
                }
            }
        }
        let end = self.span();
        self.ast.push(
            NodeKind::Script { statements },
            Span::new(start.start, end.end),
        )
    }

    fn statement(&mut self) -> Option<NodeId> {
        let start = self.span();
        match self.peek().clone() {
            Tok::KwVar | Tok::KwVarip => {
                let mode = if self.eat(&Tok::KwVar) {
                    VarMode::Var
                } else {
                    self.advance();
                    VarMode::Varip
                };
                // `var float x = …` and `var series float x = …`: the type
                // annotation is accepted and recorded nowhere — the dialect
                // is dynamically typed with compile-time checks on builtins
                // (documented divergence). Pine allows a qualifier before the
                // type, so skip every leading word that is followed by
                // another word; the last one is the variable's own name.
                while matches!(self.peek(), Tok::Ident(_))
                    && matches!(self.peek_at(1), Tok::Ident(_))
                {
                    self.advance();
                }
                let Tok::Ident(name) = self.peek().clone() else {
                    let span = self.span();
                    self.errors.push(PineError::new(
                        ErrorCode::PineSyntax,
                        span,
                        format!(
                            "expected a variable name after `{}`",
                            if mode == VarMode::Var { "var" } else { "varip" }
                        ),
                    ));
                    return None;
                };
                self.advance();
                self.expect(&Tok::Eq, "`=`");
                let (value, block_form) = self.statement_value()?;
                if !block_form {
                    self.end_of_statement();
                }
                Some(self.declare(mode, name, value, start))
            }
            Tok::LBracket => {
                if self.lbracket_starts_destructuring() {
                    self.tuple_declare(start)
                } else {
                    self.expression_statement(start)
                }
            }
            Tok::Ident(name) => {
                // `float x = …`, `simple int len = 9`, `series float x = na`:
                // skip the annotation words (see the var arm). The name is
                // the last identifier of the run, the one before `=`.
                let mut annotation_words = 0usize;
                while matches!(self.peek_at(annotation_words), Tok::Ident(_))
                    && matches!(self.peek_at(annotation_words + 1), Tok::Ident(_))
                {
                    annotation_words += 1;
                }
                if annotation_words > 0
                    && matches!(self.peek_at(annotation_words + 1), Tok::Eq)
                    && let Tok::Ident(real_name) = self.peek_at(annotation_words).clone()
                {
                    for _ in 0..=annotation_words {
                        self.advance(); // the qualifier(s), the type, the name
                    }
                    self.advance(); // `=`
                    let (value, block_form) = self.statement_value()?;
                    if !block_form {
                        self.end_of_statement();
                    }
                    return Some(self.declare(VarMode::Plain, real_name, value, start));
                }
                // Lookahead decides between: function def, declaration,
                // reassignment, or a plain expression statement.
                match self.peek_at(1) {
                    Tok::Eq => {
                        self.advance();
                        self.advance();
                        let (value, block_form) = self.statement_value()?;
                        if !block_form {
                            self.end_of_statement();
                        }
                        Some(self.declare(VarMode::Plain, name, value, start))
                    }
                    Tok::ColonEq => {
                        self.advance();
                        self.advance();
                        let (value, block_form) = self.statement_value()?;
                        if !block_form {
                            self.end_of_statement();
                        }
                        let span = self.spanned_from(start);
                        Some(self.ast.push(NodeKind::Reassign { name, value }, span))
                    }
                    Tok::LParen => {
                        if let Some(def) = self.try_function_def(&name, start) {
                            Some(def)
                        } else {
                            self.expression_statement(start)
                        }
                    }
                    _ => self.expression_statement(start),
                }
            }
            Tok::KwSwitch => {
                self.errors.push(PineError::new(
                    ErrorCode::PineUnsupported,
                    start,
                    "`switch` is not supported yet (planned for a later milestone); \
                     use if/else chains",
                ));
                None
            }
            _ => self.expression_statement(start),
        }
    }

    fn declare(&mut self, mode: VarMode, name: String, value: NodeId, start: Span) -> NodeId {
        let span = self.spanned_from(start);
        self.ast.push(NodeKind::Declare { mode, name, value }, span)
    }

    /// Whether the upcoming `[ … ]` is `[name, name] =` — a destructuring —
    /// as opposed to a tuple expression (a function's return line).
    fn lbracket_starts_destructuring(&self) -> bool {
        let mut ahead = 1; // past `[`
        loop {
            match self.peek_at(ahead) {
                Tok::Ident(_) => ahead += 1,
                _ => return false,
            }
            match self.peek_at(ahead) {
                Tok::Comma => ahead += 1,
                Tok::RBracket => return *self.peek_at(ahead + 1) == Tok::Eq,
                _ => return false,
            }
        }
    }

    fn tuple_declare(&mut self, start: Span) -> Option<NodeId> {
        self.advance(); // `[`
        let mut names = Vec::new();
        loop {
            match self.peek().clone() {
                Tok::Ident(name) => {
                    self.advance();
                    names.push(name);
                }
                _ => {
                    let span = self.span();
                    self.errors.push(PineError::new(
                        ErrorCode::PineSyntax,
                        span,
                        "expected a name inside tuple destructuring",
                    ));
                    return None;
                }
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBracket, "`]`");
        self.expect(&Tok::Eq, "`=`");
        let (value, block_form) = self.statement_value()?;
        if !block_form {
            self.end_of_statement();
        }
        let span = self.spanned_from(start);
        Some(self.ast.push(NodeKind::TupleDeclare { names, value }, span))
    }

    /// `IDENT ( idents ) => …` — tried with a saved position; a mismatch
    /// backtracks and the caller parses a call expression instead.
    fn try_function_def(&mut self, name: &str, start: Span) -> Option<NodeId> {
        let saved = self.pos;
        self.advance(); // name
        self.advance(); // `(`
        let mut params = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                let Tok::Ident(param) = self.peek().clone() else {
                    self.pos = saved;
                    return None;
                };
                self.advance();
                params.push(param);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        if !self.eat(&Tok::RParen) || !self.eat(&Tok::Arrow) {
            self.pos = saved;
            return None;
        }
        // Body: same-line expression, or an indented block.
        let body = if self.eat(&Tok::Newline) {
            self.block()?
        } else {
            let body = self.expression()?;
            self.end_of_statement();
            body
        };
        let span = self.spanned_from(start);
        Some(self.ast.push(
            NodeKind::FunctionDef {
                name: name.to_owned(),
                params,
                body,
            },
            span,
        ))
    }

    fn expression_statement(&mut self, start: Span) -> Option<NodeId> {
        let (expr, block_form) = self.statement_value()?;
        if !block_form {
            self.end_of_statement();
        }
        let span = self.spanned_from(start);
        Some(self.ast.push(NodeKind::ExprStmt { expr }, span))
    }

    /// The value position of a statement: a block-expression keyword or a
    /// plain expression. The bool is true for block forms, whose consumed
    /// `Dedent` already ended the statement (no trailing `Newline` exists).
    fn statement_value(&mut self) -> Option<(NodeId, bool)> {
        match self.peek() {
            Tok::KwIf => Some((self.if_expression()?, true)),
            Tok::KwFor => Some((self.for_expression()?, true)),
            Tok::KwWhile => Some((self.while_expression()?, true)),
            Tok::KwSwitch => {
                let span = self.span();
                self.errors.push(PineError::new(
                    ErrorCode::PineUnsupported,
                    span,
                    "`switch` is not supported yet (planned for a later milestone); \
                     use if/else chains",
                ));
                None
            }
            _ => Some((self.expression()?, false)),
        }
    }

    fn end_of_statement(&mut self) {
        match self.peek() {
            Tok::Newline => {
                self.advance();
            }
            Tok::Eof | Tok::Dedent => {}
            _ => {
                let span = self.span();
                self.errors.push(PineError::new(
                    ErrorCode::PineSyntax,
                    span,
                    format!(
                        "expected end of line, found {} (one statement per line)",
                        describe(self.peek())
                    ),
                ));
            }
        }
    }

    // ---- block expressions ---------------------------------------------

    fn block(&mut self) -> Option<NodeId> {
        let start = self.span();
        if !self.expect(&Tok::Indent, "an indented block") {
            return None;
        }
        let mut statements = Vec::new();
        loop {
            match self.peek() {
                Tok::Dedent => {
                    self.advance();
                    break;
                }
                Tok::Eof => break,
                Tok::Newline => {
                    self.advance();
                }
                _ => {
                    let before = self.errors.len();
                    let statement = self.statement();
                    if self.errors.len() > before {
                        self.synchronise();
                    }
                    if let Some(statement) = statement {
                        statements.push(statement);
                    }
                }
            }
        }
        if statements.is_empty() {
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                start,
                "a block must contain at least one statement",
            ));
            return None;
        }
        let span = self.spanned_from(start);
        Some(self.ast.push(NodeKind::Block { statements }, span))
    }

    fn if_expression(&mut self) -> Option<NodeId> {
        let start = self.span();
        self.advance(); // `if`
        let condition = self.expression()?;
        self.expect(&Tok::Newline, "a newline after the if condition");
        let then_block = self.block()?;
        let otherwise = if self.eat(&Tok::KwElse) {
            if *self.peek() == Tok::KwIf {
                Some(self.if_expression()?)
            } else {
                self.expect(&Tok::Newline, "a newline after `else`");
                Some(self.block()?)
            }
        } else {
            None
        };
        let span = self.spanned_from(start);
        Some(self.ast.push(
            NodeKind::If {
                condition,
                then_block,
                otherwise,
            },
            span,
        ))
    }

    fn for_expression(&mut self) -> Option<NodeId> {
        let start = self.span();
        self.advance(); // `for`
        let Tok::Ident(var) = self.peek().clone() else {
            let span = self.span();
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                span,
                "expected the loop variable after `for`",
            ));
            return None;
        };
        self.advance();
        self.expect(&Tok::Eq, "`=`");
        let from = self.expression()?;
        self.expect(&Tok::KwTo, "`to`");
        let to = self.expression()?;
        let by = if self.eat(&Tok::KwBy) {
            Some(self.expression()?)
        } else {
            None
        };
        self.expect(&Tok::Newline, "a newline after the for header");
        let body = self.block()?;
        let span = self.spanned_from(start);
        Some(self.ast.push(
            NodeKind::For {
                var,
                from,
                to,
                by,
                body,
            },
            span,
        ))
    }

    fn while_expression(&mut self) -> Option<NodeId> {
        let start = self.span();
        self.advance(); // `while`
        let condition = self.expression()?;
        self.expect(&Tok::Newline, "a newline after the while condition");
        let body = self.block()?;
        let span = self.spanned_from(start);
        Some(self.ast.push(NodeKind::While { condition, body }, span))
    }

    // ---- expressions -----------------------------------------------------

    fn expression(&mut self) -> Option<NodeId> {
        // The descent happens on the way *down*, so the guard belongs here:
        // by the time unbalanced input would be rejected, the stack is
        // already gone.
        if self.depth >= MAX_NEST_DEPTH {
            let span = self.span();
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                span,
                format!("expression nests deeper than {MAX_NEST_DEPTH} levels"),
            ));
            return None;
        }
        self.depth += 1;
        let node = self.ternary();
        self.depth -= 1;
        node
    }

    fn ternary(&mut self) -> Option<NodeId> {
        let start = self.span();
        let condition = self.binary(0)?;
        if !self.eat(&Tok::Question) {
            return Some(condition);
        }
        let if_true = self.ternary()?;
        self.expect(&Tok::Colon, "`:` of the ternary");
        let if_false = self.ternary()?;
        let span = self.spanned_from(start);
        Some(self.ast.push(
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            },
            span,
        ))
    }

    /// Precedence climbing over the binary operators.
    fn binary(&mut self, min_level: u8) -> Option<NodeId> {
        let start = self.span();
        let mut lhs = self.unary()?;
        loop {
            let Some((op, level)) = binop_of(self.peek()) else {
                return Some(lhs);
            };
            if level < min_level {
                return Some(lhs);
            }
            self.advance();
            let rhs = self.binary(level + 1)?;
            let span = self.spanned_from(start);
            lhs = self.ast.push(NodeKind::Binary { op, lhs, rhs }, span);
        }
    }

    fn unary(&mut self) -> Option<NodeId> {
        let start = self.span();
        if self.eat(&Tok::Minus) {
            let operand = self.unary()?;
            let span = self.spanned_from(start);
            return Some(self.ast.push(
                NodeKind::Unary {
                    op: UnOp::Neg,
                    operand,
                },
                span,
            ));
        }
        if self.eat(&Tok::KwNot) {
            let operand = self.unary()?;
            let span = self.spanned_from(start);
            return Some(self.ast.push(
                NodeKind::Unary {
                    op: UnOp::Not,
                    operand,
                },
                span,
            ));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Option<NodeId> {
        let start = self.span();
        let mut expr = self.primary()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    self.advance();
                    let args = self.arguments()?;
                    let span = self.spanned_from(start);
                    expr = self.ast.push(NodeKind::Call { callee: expr, args }, span);
                }
                Tok::LBracket => {
                    self.advance();
                    let offset = self.expression()?;
                    self.expect(&Tok::RBracket, "`]` of the history operator");
                    let span = self.spanned_from(start);
                    expr = self.ast.push(
                        NodeKind::History {
                            subject: expr,
                            offset,
                        },
                        span,
                    );
                }
                Tok::Dot => {
                    self.advance();
                    let Tok::Ident(field) = self.peek().clone() else {
                        let span = self.span();
                        self.errors.push(PineError::new(
                            ErrorCode::PineSyntax,
                            span,
                            "expected a member name after `.`",
                        ));
                        return None;
                    };
                    self.advance();
                    let span = self.spanned_from(start);
                    expr = self.ast.push(
                        NodeKind::Member {
                            object: expr,
                            field,
                        },
                        span,
                    );
                }
                _ => return Some(expr),
            }
        }
    }

    fn arguments(&mut self) -> Option<Vec<Arg>> {
        let mut args = Vec::new();
        if self.eat(&Tok::RParen) {
            return Some(args);
        }
        loop {
            let arg_start = self.span();
            // `name = value` is a named argument (but `name ==` is a
            // comparison inside a positional one).
            let name = if let (Tok::Ident(name), Tok::Eq) = (self.peek(), self.peek_at(1)) {
                let name = name.clone();
                self.advance();
                self.advance();
                Some(name)
            } else {
                None
            };
            let value = self.expression()?;
            let span = self.spanned_from(arg_start);
            args.push(Arg { name, value, span });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen, "`)` of the call");
        Some(args)
    }

    fn primary(&mut self) -> Option<NodeId> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(v) => NodeKind::Int(v),
            Tok::Float(v) => NodeKind::Float(v),
            Tok::Str(v) => NodeKind::Str(v),
            Tok::ColorLit(v) => NodeKind::Color(v),
            Tok::KwTrue => NodeKind::Bool(true),
            Tok::KwFalse => NodeKind::Bool(false),
            Tok::KwNa => NodeKind::Na,
            Tok::Ident(name) => NodeKind::Name(name),
            Tok::LParen => {
                self.advance();
                let inner = self.expression()?;
                self.expect(&Tok::RParen, "`)`");
                return Some(inner);
            }
            Tok::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if *self.peek() != Tok::RBracket {
                    loop {
                        items.push(self.expression()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBracket, "`]` of the tuple");
                let full = self.spanned_from(span);
                return Some(self.ast.push(NodeKind::Tuple { items }, full));
            }
            other => {
                self.errors.push(PineError::new(
                    ErrorCode::PineSyntax,
                    span,
                    format!("expected an expression, found {}", describe(&other)),
                ));
                return None;
            }
        };
        self.advance();
        Some(self.ast.push(kind, span))
    }

    fn spanned_from(&self, start: Span) -> Span {
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Span::new(start.start, end.end.max(start.end))
    }
}

/// Operator → (op, precedence level); higher binds tighter.
fn binop_of(tok: &Tok) -> Option<(BinOp, u8)> {
    Some(match tok {
        Tok::KwOr => (BinOp::Or, 1),
        Tok::KwAnd => (BinOp::And, 2),
        Tok::EqEq => (BinOp::Eq, 3),
        Tok::NotEq => (BinOp::Ne, 3),
        Tok::Lt => (BinOp::Lt, 3),
        Tok::Le => (BinOp::Le, 3),
        Tok::Gt => (BinOp::Gt, 3),
        Tok::Ge => (BinOp::Ge, 3),
        Tok::Plus => (BinOp::Add, 4),
        Tok::Minus => (BinOp::Sub, 4),
        Tok::Star => (BinOp::Mul, 5),
        Tok::Slash => (BinOp::Div, 5),
        Tok::Percent => (BinOp::Mod, 5),
        _ => return None,
    })
}

/// A short description of a token for error messages.
///
/// Exhaustive on purpose: a catch-all printed Rust variant names — "expected
/// an expression, found `RBracket`" — at the one moment the reader needs to
/// see what they actually typed. It also stops a new token from regressing a
/// message silently; the compiler asks for its spelling here.
fn describe(tok: &Tok) -> String {
    let spelling = match tok {
        Tok::Int(v) => return format!("`{v}`"),
        Tok::Float(v) => return format!("`{v}`"),
        Tok::Str(_) => return "a string".to_owned(),
        Tok::ColorLit(_) => return "a color literal".to_owned(),
        Tok::Ident(name) => return format!("`{name}`"),
        Tok::Newline => return "end of line".to_owned(),
        Tok::Indent => return "an indent".to_owned(),
        Tok::Dedent => return "a dedent".to_owned(),
        Tok::Eof => return "end of file".to_owned(),
        Tok::KwVar => "var",
        Tok::KwVarip => "varip",
        Tok::KwIf => "if",
        Tok::KwElse => "else",
        Tok::KwFor => "for",
        Tok::KwTo => "to",
        Tok::KwBy => "by",
        Tok::KwWhile => "while",
        Tok::KwSwitch => "switch",
        Tok::KwAnd => "and",
        Tok::KwOr => "or",
        Tok::KwNot => "not",
        Tok::KwTrue => "true",
        Tok::KwFalse => "false",
        Tok::KwNa => "na",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Star => "*",
        Tok::Slash => "/",
        Tok::Percent => "%",
        Tok::Eq => "=",
        Tok::ColonEq => ":=",
        Tok::EqEq => "==",
        Tok::NotEq => "!=",
        Tok::Lt => "<",
        Tok::Le => "<=",
        Tok::Gt => ">",
        Tok::Ge => ">=",
        Tok::Question => "?",
        Tok::Colon => ":",
        Tok::Comma => ",",
        Tok::Dot => ".",
        Tok::Arrow => "=>",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBracket => "[",
        Tok::RBracket => "]",
    };
    format!("`{spelling}`")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_ok(source: &str) -> (Ast, NodeId) {
        let out = lex(source).expect("fixture lexes");
        parse(&out.tokens).expect("fixture parses")
    }

    fn parse_err(source: &str) -> Vec<PineError> {
        let out = lex(source).expect("fixture lexes");
        parse(&out.tokens).expect_err("fixture must not parse")
    }

    fn root_statements(ast: &Ast, root: NodeId) -> Vec<NodeId> {
        match &ast.node(root).kind {
            NodeKind::Script { statements } => statements.clone(),
            other => panic!("root is not a script: {other:?}"),
        }
    }

    #[test]
    fn declarations_and_reassignment() {
        let (ast, root) = parse_ok("x = 1\nvar y = 2\nvarip z = 3\nx := x + 1\n");
        let stmts = root_statements(&ast, root);
        assert_eq!(stmts.len(), 4);
        assert!(matches!(
            &ast.node(stmts[0]).kind,
            NodeKind::Declare { mode: VarMode::Plain, name, .. } if name == "x"
        ));
        assert!(matches!(
            &ast.node(stmts[1]).kind,
            NodeKind::Declare {
                mode: VarMode::Var,
                ..
            }
        ));
        assert!(matches!(
            &ast.node(stmts[2]).kind,
            NodeKind::Declare {
                mode: VarMode::Varip,
                ..
            }
        ));
        assert!(matches!(
            &ast.node(stmts[3]).kind,
            NodeKind::Reassign { name, .. } if name == "x"
        ));
    }

    #[test]
    fn precedence_follows_pine() {
        // a + b * c > d and e  =>  ((a + (b*c)) > d) and e
        let (ast, root) = parse_ok("r = a + b * c > d and e\n");
        let stmts = root_statements(&ast, root);
        let NodeKind::Declare { value, .. } = &ast.node(stmts[0]).kind else {
            panic!("not a declare");
        };
        let NodeKind::Binary {
            op: BinOp::And,
            lhs,
            ..
        } = &ast.node(*value).kind
        else {
            panic!("top is not `and`: {:?}", ast.node(*value).kind);
        };
        let NodeKind::Binary {
            op: BinOp::Gt,
            lhs: sum,
            ..
        } = &ast.node(*lhs).kind
        else {
            panic!("lhs of and is not `>`");
        };
        let NodeKind::Binary {
            op: BinOp::Add,
            rhs: product,
            ..
        } = &ast.node(*sum).kind
        else {
            panic!("lhs of > is not `+`");
        };
        assert!(matches!(
            &ast.node(*product).kind,
            NodeKind::Binary { op: BinOp::Mul, .. }
        ));
    }

    #[test]
    fn postfix_chain_calls_history_and_members() {
        let (ast, root) = parse_ok("e = ta.ema(close, 9)[1]\n");
        let stmts = root_statements(&ast, root);
        let NodeKind::Declare { value, .. } = &ast.node(stmts[0]).kind else {
            panic!("not a declare");
        };
        let NodeKind::History { subject, .. } = &ast.node(*value).kind else {
            panic!("outermost is not history: {:?}", ast.node(*value).kind);
        };
        let NodeKind::Call { callee, args } = &ast.node(*subject).kind else {
            panic!("subject is not a call");
        };
        assert_eq!(args.len(), 2);
        let NodeKind::Member { object, field } = &ast.node(*callee).kind else {
            panic!("callee is not a member");
        };
        assert_eq!(field, "ema");
        assert!(matches!(&ast.node(*object).kind, NodeKind::Name(n) if n == "ta"));
    }

    #[test]
    fn named_arguments_parse_next_to_positional() {
        let (ast, root) = parse_ok("plot(close, color=color.red, linewidth=2)\n");
        let stmts = root_statements(&ast, root);
        let NodeKind::ExprStmt { expr } = &ast.node(stmts[0]).kind else {
            panic!("not an expression statement");
        };
        let NodeKind::Call { args, .. } = &ast.node(*expr).kind else {
            panic!("not a call");
        };
        assert_eq!(args[0].name, None);
        assert_eq!(args[1].name.as_deref(), Some("color"));
        assert_eq!(args[2].name.as_deref(), Some("linewidth"));
    }

    #[test]
    fn if_else_chains_nest() {
        let source = "\
x = if a\n    1\nelse if b\n    2\nelse\n    3\n";
        let (ast, root) = parse_ok(source);
        let stmts = root_statements(&ast, root);
        let NodeKind::Declare { value, .. } = &ast.node(stmts[0]).kind else {
            panic!("not a declare");
        };
        let NodeKind::If {
            otherwise: Some(chained),
            ..
        } = &ast.node(*value).kind
        else {
            panic!("no else branch");
        };
        assert!(
            matches!(
                &ast.node(*chained).kind,
                NodeKind::If {
                    otherwise: Some(_),
                    ..
                }
            ),
            "else-if chains into a nested if with its own else"
        );
    }

    #[test]
    fn for_and_while_carry_blocks() {
        let source = "\
s = 0.0\nfor i = 0 to 9 by 2\n    s := s + i\nwhile s > 0\n    s := s - 1\n";
        let (ast, root) = parse_ok(source);
        let stmts = root_statements(&ast, root);
        assert!(matches!(
            &ast.node(stmts[1]).kind,
            NodeKind::ExprStmt { expr } if matches!(&ast.node(*expr).kind, NodeKind::For { by: Some(_), .. })
        ));
        assert!(matches!(
            &ast.node(stmts[2]).kind,
            NodeKind::ExprStmt { expr } if matches!(&ast.node(*expr).kind, NodeKind::While { .. })
        ));
    }

    #[test]
    fn function_definitions_take_both_body_forms() {
        let source = "\
double(x) => x * 2\ntriple(x) =>\n    y = x * 3\n    y\n";
        let (ast, root) = parse_ok(source);
        let stmts = root_statements(&ast, root);
        let NodeKind::FunctionDef { name, params, body } = &ast.node(stmts[0]).kind else {
            panic!("not a function def");
        };
        assert_eq!(name, "double");
        assert_eq!(params, &["x".to_owned()]);
        assert!(matches!(&ast.node(*body).kind, NodeKind::Binary { .. }));
        let NodeKind::FunctionDef { body, .. } = &ast.node(stmts[1]).kind else {
            panic!("not a function def");
        };
        assert!(matches!(&ast.node(*body).kind, NodeKind::Block { .. }));
    }

    #[test]
    fn a_call_statement_is_not_mistaken_for_a_definition() {
        let (ast, root) = parse_ok("f(a, b)\n");
        let stmts = root_statements(&ast, root);
        assert!(matches!(
            &ast.node(stmts[0]).kind,
            NodeKind::ExprStmt { .. }
        ));
    }

    #[test]
    fn tuple_destructuring_parses() {
        let (ast, root) = parse_ok("[lo, hi] = bounds(close)\n");
        let stmts = root_statements(&ast, root);
        let NodeKind::TupleDeclare { names, .. } = &ast.node(stmts[0]).kind else {
            panic!("not a tuple declare");
        };
        assert_eq!(names, &["lo".to_owned(), "hi".to_owned()]);
    }

    #[test]
    fn ternary_is_right_associative() {
        let (ast, root) = parse_ok("x = a ? 1 : b ? 2 : 3\n");
        let stmts = root_statements(&ast, root);
        let NodeKind::Declare { value, .. } = &ast.node(stmts[0]).kind else {
            panic!("not a declare");
        };
        let NodeKind::Ternary { if_false, .. } = &ast.node(*value).kind else {
            panic!("not a ternary");
        };
        assert!(matches!(
            &ast.node(*if_false).kind,
            NodeKind::Ternary { .. }
        ));
    }

    #[test]
    fn switch_is_rejected_honestly() {
        let errors = parse_err("switch x\n    1 => 2\n");
        assert!(
            errors.iter().any(|e| e.code == ErrorCode::PineUnsupported),
            "{errors:?}"
        );
    }

    #[test]
    fn multiple_bad_statements_report_multiple_errors() {
        let errors = parse_err("x = )\ny = ]\nz = 1 +\n");
        assert!(errors.len() >= 2, "{errors:?}");
    }

    #[test]
    fn error_spans_point_at_the_problem() {
        let source = "x = 1\ny = )\n";
        let errors = parse_err(source);
        let (line, _) = errors[0].span.line_col(source);
        assert_eq!(line, 2);
    }

    #[test]
    fn a_qualified_type_annotation_is_accepted_and_ignored() {
        // Pine v5 lets a qualifier precede the type. Both forms declare the
        // variable; the annotation itself is recorded nowhere.
        for source in [
            "//@version=5
simple int len = 9
",
            "//@version=5
var series float x = na
",
            "//@version=5
float y = 1.5
",
            "//@version=5
var float z = 0.0
",
        ] {
            let out = lex(source).expect("fixture lexes");
            assert!(
                parse(&out.tokens).is_ok(),
                "a type annotation must not be mistaken for a statement: {source:?}"
            );
        }
    }

    #[test]
    fn nesting_beyond_the_limit_is_an_error_not_a_stack_overflow() {
        let source = format!(
            "//@version=5
x = {}1{}
",
            "(".repeat(MAX_NEST_DEPTH + 5),
            ")".repeat(MAX_NEST_DEPTH + 5)
        );
        let out = lex(&source).expect("parens lex");
        let errors = parse(&out.tokens).expect_err("too deep to parse");
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::PineSyntax && e.message.contains("nests deeper")),
            "{errors:?}"
        );
    }

    #[test]
    fn punctuation_is_named_as_the_author_typed_it() {
        // Never Rust variant names: the reader is looking for what they wrote.
        assert_eq!(describe(&Tok::RBracket), "`]`");
        assert_eq!(describe(&Tok::Comma), "`,`");
        assert_eq!(describe(&Tok::ColonEq), "`:=`");
        assert_eq!(describe(&Tok::KwVarip), "`varip`");
    }
}
