//! Register machine. Syntax is inspected only by the compiler.
use super::{Eval, FLAGS, GLOBALS, LOCALS, REGISTERS, RETURNS, Value, values_equal};
use crate::{
    ast::{BinOp, NodeId, UnOp, VarMode},
    error::{ErrorCode, PineError},
    ir::Resolution,
    ir::program::{Instruction, Literal},
};
use std::rc::Rc;

impl Eval<'_> {
    pub(super) fn execute(&mut self, entry: NodeId) -> Result<Value, PineError> {
        let stack_base = self.state.arena.len(RETURNS);
        let mut pc = self.script.program.entries[entry.index()];
        loop {
            let instruction = &self.script.program.instructions[pc];
            pc += 1;
            match instruction {
                Instruction::Enter(id) => {
                    self.state.arena.push_address(pc);
                    pc = self.script.program.entries[id.index()];
                }
                Instruction::Return(id) => {
                    let value = self.register(*id).coerce(
                        self.script.types.coercions[self.type_context as usize][id.index()],
                    );
                    let value = if self.script.types.integer_divisions[self.type_context as usize]
                        [id.index()]
                    {
                        match value {
                            Value::Num(n) => Value::Num(n.trunc()),
                            value => value,
                        }
                    } else {
                        value
                    };
                    self.set_register(*id, value);
                    if self.state.arena.len(RETURNS) == stack_base {
                        return Ok(self.register(entry));
                    }
                    pc = self.state.arena.pop_address();
                }
                Instruction::Initialized(id, slot, target) => {
                    if self.state.arena.get(FLAGS, *slot as usize) == Value::Bool(true) {
                        self.set_register(*id, self.state.arena.get(GLOBALS, *slot as usize));
                        pc = *target;
                    }
                }
                _ => self.execute_operation(instruction)?,
            }
        }
    }

    fn register(&self, id: NodeId) -> Value {
        self.state.arena.get(REGISTERS, id.index())
    }
    fn set_register(&mut self, id: NodeId, value: Value) {
        self.state.arena.set(REGISTERS, id.index(), value);
    }
    fn read_slot(&mut self, id: NodeId, resolution: Resolution) -> Result<Value, PineError> {
        match resolution {
            Resolution::Global(slot) => Ok(self.state.arena.get(GLOBALS, slot as usize)),
            Resolution::Local(slot) => Ok(self
                .state
                .arena
                .get(LOCALS, self.frame_base + slot as usize)),
            Resolution::Builtin(builtin) => self.builtin_variable(id, builtin),
            Resolution::ColorConst(color) => Ok(Value::Color(color)),
            Resolution::EnumConst(tag) => Ok(Value::Str(Rc::new(tag.to_owned()))),
            _ => Err(self.err(id, ErrorCode::PineType, "value has no executable slot")),
        }
    }
    fn store_slot(&mut self, resolution: Resolution, value: Value) {
        match resolution {
            Resolution::Global(slot) => self.state.arena.set(GLOBALS, slot as usize, value),
            Resolution::Local(slot) => {
                self.state
                    .arena
                    .set(LOCALS, self.frame_base + slot as usize, value)
            }
            _ => {}
        }
    }

    fn execute_operation(&mut self, instruction: &Instruction) -> Result<(), PineError> {
        use Instruction as I;
        let (id, value) = match instruction {
            I::Constant(id, literal) => (
                *id,
                match literal {
                    Literal::Num(n) => Value::Num(*n),
                    Literal::Bool(b) => Value::Bool(*b),
                    Literal::Color(c) => Value::Color(*c),
                    Literal::Str(s) => Value::Str(s.clone()),
                    Literal::Na => Value::Na,
                },
            ),
            I::Read(id, resolution) => (*id, self.read_slot(*id, *resolution)?),
            I::Copy(id, source) => (*id, self.register(*source)),
            I::Store(id, target, source, mode) => {
                let value = self.register(*source);
                self.store_slot(*target, value.clone());
                if *mode != VarMode::Plain
                    && let Resolution::Global(slot) = target
                {
                    self.state
                        .arena
                        .set(FLAGS, *slot as usize, Value::Bool(true));
                }
                (*id, value)
            }
            I::TupleStore(id, target, count) => {
                let value = self.register(*id);
                let Value::Tuple(items) = &value else {
                    return Err(self.type_err(*id, "a tuple", &value));
                };
                if items.len() != *count {
                    return Err(self.err(*id, ErrorCode::PineType, "tuple arity mismatch"));
                }
                for (offset, item) in items.iter().enumerate() {
                    let offset = u32::try_from(offset).expect("bounded tuple");
                    let slot = match target {
                        Resolution::Global(first) => Resolution::Global(first + offset),
                        Resolution::Local(first) => Resolution::Local(first + offset),
                        other => *other,
                    };
                    self.store_slot(slot, item.clone());
                }
                (*id, value)
            }
            I::Unary(id, op, operand) => {
                let value = self.register(*operand);
                let result = match op {
                    UnOp::Neg => Value::Num(
                        -value
                            .as_num()
                            .ok_or_else(|| self.type_err(*operand, "a number", &value))?,
                    ),
                    UnOp::Not => Value::Bool(
                        !value
                            .as_cond()
                            .ok_or_else(|| self.type_err(*operand, "a bool condition", &value))?,
                    ),
                };
                (*id, result)
            }
            I::Binary(id, op, lhs, rhs) => (*id, self.register_binary(*op, *lhs, *rhs)?),
            I::Logical(id, op, lhs, rhs) => (*id, self.binary(*op, *lhs, *rhs)?),
            I::Conditional(id, condition, yes, no) => {
                (*id, self.conditional(*condition, *yes, *no)?)
            }
            I::Switch(id, subject, arms) => {
                let subject = subject.map(|node| self.eval(node)).transpose()?;
                let mut value = Value::Na;
                for (condition, body) in arms {
                    let matches = match (&subject, condition) {
                        (Some(subject), Some(condition)) => {
                            values_equal(subject, &self.eval(*condition)?)
                        }
                        (None, Some(condition)) => self.cond(*condition)?,
                        (_, None) => true,
                    };
                    if matches {
                        value = self.eval(*body)?;
                        break;
                    }
                }
                (*id, value)
            }
            I::For(id, from, to, by, body) => (*id, self.run_for(*id, *from, *to, *by, *body)?),
            I::While(id, condition, body) => (*id, self.run_while(*id, *condition, *body)?),
            I::Tuple(id, items) => {
                let values = items
                    .iter()
                    .map(|item| self.eval(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                (*id, Value::Tuple(Rc::new(values)))
            }
            I::History(id, subject, offset) => (*id, self.history(*id, *subject, *offset)?),
            I::Builtin(id, builtin, args) => (*id, self.builtin_call(*id, *builtin, args)?),
            I::User(id, index, args) => (*id, self.user_call(*id, *index, args)?),
            I::Method(id, object, field, args) => {
                if self.semantic_run.skipped {
                    self.advance_skipped(*object)?;
                    self.advance_arguments(args)?;
                    (*id, Value::Na)
                } else {
                    let receiver = self.eval(*object)?;
                    let Value::Obj(kind, raw) = receiver else {
                        return Err(self.type_err(*object, "an object handle", &receiver));
                    };
                    (
                        *id,
                        self.method_call(
                            *id,
                            kind,
                            quantick_indicators::ObjectId(raw),
                            field,
                            args,
                        )?,
                    )
                }
            }
            I::Invalid(id) => {
                return Err(self.err(
                    *id,
                    ErrorCode::PineType,
                    "expression has no executable operation",
                ));
            }
            I::Enter(_) | I::Return(_) | I::Initialized(..) => unreachable!("control instruction"),
        };
        self.set_register(id, value);
        Ok(())
    }

    fn register_binary(&self, op: BinOp, lhs: NodeId, rhs: NodeId) -> Result<Value, PineError> {
        let left = self.register(lhs);
        let right = self.register(rhs);
        if matches!(op, BinOp::Eq | BinOp::Ne) {
            return Ok(Value::Bool(
                values_equal(&left, &right) == (op == BinOp::Eq),
            ));
        }
        if op == BinOp::Add {
            if let (Value::Str(a), Value::Str(b)) = (&left, &right) {
                return Ok(Value::Str(Rc::new(format!("{a}{b}"))));
            }
            if (left.is_na() || right.is_na())
                && matches!(
                    (&left, &right),
                    (Value::Str(_) | Value::NaString, _) | (_, Value::Str(_) | Value::NaString)
                )
            {
                return Ok(Value::NaString);
            }
        }
        let l = left
            .as_num()
            .ok_or_else(|| self.type_err(lhs, "a number", &left))?;
        let r = right
            .as_num()
            .ok_or_else(|| self.type_err(rhs, "a number", &right))?;
        Ok(match op {
            BinOp::Lt => Value::Bool(l < r),
            BinOp::Le => Value::Bool(l <= r),
            BinOp::Gt => Value::Bool(l > r),
            BinOp::Ge => Value::Bool(l >= r),
            BinOp::Add => Value::Num(l + r),
            BinOp::Sub => Value::Num(l - r),
            BinOp::Mul => Value::Num(l * r),
            BinOp::Div => Value::Num(if r == 0.0 { f64::NAN } else { l / r }),
            BinOp::Mod => Value::Num(if r == 0.0 { f64::NAN } else { l % r }),
            _ => unreachable!("logical and equality instructions handled separately"),
        })
    }
}
