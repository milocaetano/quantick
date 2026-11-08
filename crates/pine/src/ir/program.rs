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
    Object(NodeId, crate::libraries::ObjectOp, Option<NodeId>, Vec<Arg>),
    Enter(NodeId),
    Return(NodeId),
    Coerce(NodeId),
    CollectionCoerce(NodeId),
    Constant(NodeId, Literal),
    Read(NodeId, Resolution),
    Copy(NodeId, NodeId),
    Store(NodeId, Resolution, NodeId, VarMode),
    Initialized(NodeId, u32, usize),
    TupleStore(NodeId, Resolution, usize),
    Unary(NodeId, UnOp, NodeId),
    Binary(NodeId, BinOp, NodeId, NodeId),
    Jump(usize),
    JumpUnless(NodeId, bool, usize),
    Condition(NodeId, NodeId),
    Advance(NodeId),
    Switch(NodeId, Option<NodeId>, Vec<(Option<NodeId>, NodeId)>),
    For(NodeId, NodeId, NodeId, Option<NodeId>, NodeId),
    While(NodeId, NodeId, NodeId),
    Tuple(NodeId, Vec<NodeId>),
    History(NodeId, NodeId, NodeId),
    Builtin(NodeId, Builtin, Vec<Arg>),
    Kernel(NodeId, Builtin, u32, Vec<Arg>),
    Input(NodeId, usize),
    Plot(NodeId, Option<usize>, Option<NodeId>),
    User(NodeId, u32, Vec<Arg>),
    Method(NodeId, NodeId, String, Vec<Arg>),
    Invalid(NodeId),
}

#[derive(Clone, Default)]
pub(crate) struct Program {
    pub may_repeat: bool,
    pub constants: Vec<(NodeId, Literal)>,
    pub instructions: Vec<Instruction>,
    pub entries: Vec<usize>,
    pub spans: Vec<Span>,
    pub kernel_reach: Vec<Vec<bool>>,
}

impl Program {
    pub fn lower(script: &CompiledScript) -> Self {
        let may_repeat = (0..script.ast.len()).any(|index| {
            matches!(
                script.ast.node(script.ast.id_at(index).expect("node")).kind,
                NodeKind::For { .. } | NodeKind::While { .. }
            )
        });
        let mut program = Self {
            may_repeat,
            ..Self::default()
        };
        for index in 0..script.ast.len() {
            let id = script.ast.id_at(index).expect("existing arena slot");
            program.entries.push(program.instructions.len());
            program.spans.push(script.ast.node(id).span);
            program.lower_node(script, id);
            if script
                .types
                .coercions
                .iter()
                .any(|context| context[id.index()] != 0)
                || script
                    .types
                    .integer_divisions
                    .iter()
                    .any(|context| context[id.index()])
            {
                let instruction = if script
                    .types
                    .coercions
                    .iter()
                    .any(|context| context[id.index()] == 6)
                {
                    Instruction::CollectionCoerce(id)
                } else {
                    Instruction::Coerce(id)
                };
                program.instructions.push(instruction);
            }
            let start = program.entries[index];
            if program.instructions.len() == start + 1
                && let Instruction::Constant(node, literal) = &program.instructions[start]
            {
                program.constants.push((*node, literal.clone()));
                program.instructions.pop();
            }
            program.instructions.push(Instruction::Return(id));
        }
        program.flatten();
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

    /// Inline straight-line child entries once at compile time. Branch/call
    /// adapters still enter dedicated spans, but arithmetic never pays a runtime
    /// return-stack operation for each operand.
    fn flatten(&mut self) {
        let original = self.instructions.clone();
        let entries = self.entries.clone();
        self.instructions.clear();
        for (node, entry) in entries.iter().enumerate() {
            self.entries[node] = self.instructions.len();
            Self::inline_entry(&original, &entries, *entry, &mut self.instructions);
            if let Instruction::Return(id) =
                original[entries.get(node + 1).copied().unwrap_or(original.len()) - 1]
            {
                self.instructions.push(Instruction::Return(id));
            }
        }
    }
    fn inline_entry(
        original: &[Instruction],
        entries: &[usize],
        start: usize,
        output: &mut Vec<Instruction>,
    ) {
        let mut guards = Vec::new();
        let mut mapping = Vec::new();
        for instruction in original.iter().skip(start) {
            mapping.push(output.len());
            match instruction {
                Instruction::Return(_) => break,
                Instruction::Enter(id) => {
                    Self::inline_entry(original, entries, entries[id.index()], output)
                }
                Instruction::Initialized(_, _, target)
                | Instruction::Jump(target)
                | Instruction::JumpUnless(_, _, target) => {
                    guards.push((output.len(), *target));
                    output.push(instruction.clone());
                }
                _ => output.push(instruction.clone()),
            }
        }
        for (position, target) in guards {
            match &mut output[position] {
                Instruction::Initialized(_, _, destination)
                | Instruction::Jump(destination)
                | Instruction::JumpUnless(_, _, destination) => {
                    *destination = mapping[target - start]
                }
                _ => unreachable!("relocated branch"),
            }
        }
    }

    fn advance(script: &CompiledScript, code: &mut Vec<Instruction>, node: NodeId) {
        if (0..script.types.coercions.len()).any(|context| {
            super::kernel_reach::contains_kernel(
                script,
                node,
                u32::try_from(context).expect("type context"),
            )
        }) {
            code.push(Instruction::Advance(node));
        }
    }
    fn conditional(
        script: &CompiledScript,
        code: &mut Vec<Instruction>,
        id: NodeId,
        condition: NodeId,
        yes: NodeId,
        no: Option<NodeId>,
    ) {
        use Instruction as I;
        code.push(I::Enter(condition));
        let branch = code.len();
        code.push(I::JumpUnless(condition, true, 0));
        if let Some(no) = no {
            Self::advance(script, code, no);
        }
        code.push(I::Enter(yes));
        code.push(I::Copy(id, yes));
        let done = code.len();
        code.push(I::Jump(0));
        let fallback = code.len();
        Self::advance(script, code, yes);
        if let Some(no) = no {
            code.push(I::Enter(no));
            code.push(I::Copy(id, no));
        } else {
            code.push(I::Constant(id, Literal::Na));
        }
        let end = code.len();
        code[branch] = I::JumpUnless(condition, true, fallback);
        code[done] = I::Jump(end);
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
                }
                if let Some(statement) = statements.last() {
                    code.push(I::Copy(id, *statement));
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
            NodeKind::Member { object, .. } if matches!(resolution, Resolution::Object(_)) => {
                if let Resolution::Object(op) = resolution {
                    code.push(I::Object(id, op, Some(*object), Vec::new()));
                }
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
                code.push(I::Enter(*lhs));
                let branch = code.len();
                code.push(I::JumpUnless(*lhs, *op == BinOp::And, 0));
                code.push(I::Enter(*rhs));
                code.push(I::Condition(id, *rhs));
                let done = code.len();
                code.push(I::Jump(0));
                let fallback = code.len();
                if !script.version.lazy_boolean() {
                    Self::advance(script, code, *rhs);
                }
                code.push(I::Constant(id, Literal::Bool(*op == BinOp::Or)));
                let end = code.len();
                code[branch] = I::JumpUnless(*lhs, *op == BinOp::And, fallback);
                code[done] = I::Jump(end);
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
                Self::conditional(script, code, id, *condition, *then_block, *otherwise);
            }
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                Self::conditional(script, code, id, *condition, *if_true, Some(*if_false));
            }
            NodeKind::Switch { subject, arms } => code.push(I::Switch(id, *subject, arms.clone())),
            NodeKind::For {
                from, to, by, body, ..
            } => code.push(I::For(id, *from, *to, *by, *body)),
            NodeKind::While { condition, body } => code.push(I::While(id, *condition, *body)),
            NodeKind::Tuple { items } => code.push(I::Tuple(id, items.clone())),
            NodeKind::History { subject, offset } => code.push(I::History(id, *subject, *offset)),
            NodeKind::Call { callee, args } => match script.resolutions[callee.index()] {
                Resolution::Builtin(builtin) => {
                    if let Some(input) = script.input_of_call[id.index()] {
                        code.push(I::Input(id, input));
                    } else if matches!(builtin, Builtin::Plot | Builtin::Hline) {
                        let source = args.first().map(|arg| arg.value);
                        if let Some(source) = source {
                            code.push(I::Enter(source));
                        }
                        code.push(I::Plot(id, script.plot_of_call[id.index()], source));
                    } else if !self.may_repeat
                        && let Some(site) = script.call_sites[id.index()]
                    {
                        for arg in args {
                            code.push(I::Enter(arg.value));
                        }
                        code.push(I::Kernel(id, builtin, site.0, args.clone()));
                    } else {
                        code.push(I::Builtin(id, builtin, args.clone()));
                    }
                }
                Resolution::Function(index) => code.push(I::User(id, index, args.clone())),
                Resolution::Object(op) => {
                    let object = if matches!(op, crate::libraries::ObjectOp::New(_)) {
                        None
                    } else if let NodeKind::Member { object, .. } = &script.ast.node(*callee).kind {
                        Some(*object)
                    } else {
                        None
                    };
                    code.push(I::Object(id, op, object, args.clone()));
                }
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
