//! Executable register instructions lowered once from the checked syntax arena.
use std::rc::Rc;

use crate::{
    ast::{Arg, BinOp, NodeId, NodeKind, UnOp, VarMode},
    builtins::Builtin,
    error::Span,
};

use super::{CompiledScript, Resolution};

#[derive(Clone)]
pub(crate) enum Literal {
    Num(f64),
    Bool(bool),
    Color(u32),
    Str(Rc<String>),
    Na,
}

/// Each source expression owns a result register. Calls enter instruction
/// addresses, never syntax nodes; node ids only identify registers and spans.
#[derive(Clone)]
pub(crate) enum Instruction {
    Enter(NodeId),
    Return(NodeId),
    Constant(NodeId, Literal),
    Read(NodeId, Resolution),
    Copy(NodeId, NodeId),
    Store(NodeId, Resolution, NodeId, VarMode),
    Initialized(NodeId, u32, usize),
    TupleStore(NodeId, Resolution, usize),
    Unary(NodeId, UnOp, NodeId),
    Binary(NodeId, BinOp, NodeId, NodeId),
    Logical(NodeId, BinOp, NodeId, NodeId),
    Conditional(NodeId, NodeId, NodeId, Option<NodeId>),
    Switch(NodeId, Option<NodeId>, Vec<(Option<NodeId>, NodeId)>),
    For(NodeId, NodeId, NodeId, Option<NodeId>, NodeId),
    While(NodeId, NodeId, NodeId),
    Tuple(NodeId, Vec<NodeId>),
    History(NodeId, NodeId, NodeId),
    Builtin(NodeId, Builtin, Vec<Arg>),
    User(NodeId, u32, Vec<Arg>),
    Method(NodeId, NodeId, String, Vec<Arg>),
    Invalid(NodeId),
}

#[derive(Clone, Default)]
pub(crate) struct Program {
    pub instructions: Vec<Instruction>,
    pub entries: Vec<usize>,
    pub spans: Vec<Span>,
    pub kernel_reach: Vec<Vec<bool>>,
}

impl Program {
    pub fn lower(script: &CompiledScript) -> Self {
        let mut program = Self::default();
        for index in 0..script.ast.len() {
            let id = script.ast.id_at(index).expect("existing arena slot");
            program.entries.push(program.instructions.len());
            program.spans.push(script.ast.node(id).span);
            program.lower_node(script, id);
            program.instructions.push(Instruction::Return(id));
        }
        program.kernel_reach = (0..script.types.coercions.len())
            .map(|context| {
                (0..script.ast.len())
                    .map(|index| {
                        super::kernel_reach::contains_kernel(
                            script,
                            script.ast.id_at(index).expect("arena slot"),
                            u32::try_from(context).expect("bounded type contexts"),
                        )
                    })
                    .collect()
            })
            .collect();
        program
    }

    fn lower_node(&mut self, script: &CompiledScript, id: NodeId) {
        use Instruction as I;
        let code = &mut self.instructions;
        let resolution = script.resolutions[id.index()];
        match &script.ast.node(id).kind {
            NodeKind::Script { statements } | NodeKind::Block { statements } => {
                code.push(I::Constant(id, Literal::Na));
                for &statement in statements {
                    code.push(I::Enter(statement));
                    code.push(I::Copy(id, statement));
                }
            }
            NodeKind::Declare { mode, value, .. } => {
                let guard = code.len();
                if *mode != VarMode::Plain
                    && let Resolution::Global(slot) = resolution
                {
                    code.push(I::Initialized(id, slot, 0));
                }
                code.push(I::Enter(*value));
                code.push(I::Store(id, resolution, *value, *mode));
                let end = code.len();
                if let I::Initialized(_, _, target) = &mut code[guard] {
                    *target = end;
                }
            }
            NodeKind::Reassign { value, .. } => {
                code.push(I::Enter(*value));
                code.push(I::Store(id, resolution, *value, VarMode::Plain));
            }
            NodeKind::TupleDeclare { names, value } => {
                code.push(I::Enter(*value));
                code.push(I::Copy(id, *value));
                code.push(I::TupleStore(id, resolution, names.len()));
            }
            NodeKind::ExprStmt { expr } => {
                code.push(I::Enter(*expr));
                code.push(I::Copy(id, *expr));
            }
            NodeKind::FunctionDef { .. } | NodeKind::Na => code.push(I::Constant(id, Literal::Na)),
            NodeKind::Int(value) => code.push(I::Constant(id, Literal::Num(*value as f64))),
            NodeKind::Float(value) => code.push(I::Constant(id, Literal::Num(*value))),
            NodeKind::Bool(value) => code.push(I::Constant(id, Literal::Bool(*value))),
            NodeKind::Color(value) => code.push(I::Constant(id, Literal::Color(*value))),
            NodeKind::Str(value) => {
                code.push(I::Constant(id, Literal::Str(Rc::new(value.clone()))))
            }
            NodeKind::Name(_) | NodeKind::Member { .. } => code.push(I::Read(id, resolution)),
            NodeKind::Unary { op, operand } => {
                code.push(I::Enter(*operand));
                code.push(I::Unary(id, *op, *operand));
            }
            NodeKind::Binary {
                op: op @ (BinOp::And | BinOp::Or),
                lhs,
                rhs,
            } => {
                code.push(I::Logical(id, *op, *lhs, *rhs));
            }
            NodeKind::Binary { op, lhs, rhs } => {
                code.push(I::Enter(*lhs));
                code.push(I::Enter(*rhs));
                code.push(I::Binary(id, *op, *lhs, *rhs));
            }
            NodeKind::If {
                condition,
                then_block,
                otherwise,
            } => {
                code.push(I::Conditional(id, *condition, *then_block, *otherwise));
            }
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                code.push(I::Conditional(id, *condition, *if_true, Some(*if_false)));
            }
            NodeKind::Switch { subject, arms } => code.push(I::Switch(id, *subject, arms.clone())),
            NodeKind::For {
                from, to, by, body, ..
            } => code.push(I::For(id, *from, *to, *by, *body)),
            NodeKind::While { condition, body } => code.push(I::While(id, *condition, *body)),
            NodeKind::Tuple { items } => code.push(I::Tuple(id, items.clone())),
            NodeKind::History { subject, offset } => code.push(I::History(id, *subject, *offset)),
            NodeKind::Call { callee, args } => match script.resolutions[callee.index()] {
                Resolution::Builtin(builtin) => code.push(I::Builtin(id, builtin, args.clone())),
                Resolution::Function(index) => code.push(I::User(id, index, args.clone())),
                _ => match &script.ast.node(*callee).kind {
                    NodeKind::Member { object, field } => {
                        code.push(I::Method(id, *object, field.clone(), args.clone()))
                    }
                    _ => code.push(I::Invalid(id)),
                },
            },
        }
    }
}
