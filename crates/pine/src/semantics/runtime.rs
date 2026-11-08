//! Eager advancement uses a temporary variable view; conditional side effects
//! and history writes remain in the selected execution only.
use super::{Eval, Kernel, Value};
use crate::{
    ast::{Arg, NodeId, NodeKind},
    builtins::Builtin,
    error::{ErrorCode, PineError},
};

impl Eval<'_> {
    pub(super) fn advance_arguments(&mut self, args: &[Arg]) -> Result<(), PineError> {
        for arg in args {
            self.advance_skipped(arg.value)?;
        }
        Ok(())
    }

    pub(super) fn conditional(
        &mut self,
        condition: NodeId,
        yes: NodeId,
        no: Option<NodeId>,
    ) -> Result<Value, PineError> {
        let selected = self.cond(condition)?;
        if selected {
            if let Some(no) = no {
                self.advance_skipped(no)?;
            }
            self.eval(yes)
        } else {
            self.advance_skipped(yes)?;
            no.map_or(Ok(Value::Na), |no| self.eval(no))
        }
    }

    pub(super) fn advance_skipped(&mut self, node: NodeId) -> Result<(), PineError> {
        if !self.has_kernel(node) {
            return Ok(());
        }
        let globals = self.state.globals.clone();
        let locals = self.state.locals.clone();
        let done = self.state.var_done.clone();
        let skipped = self.semantic_run.skipped;
        self.semantic_run.skipped = true;
        let result = self.eval(node);
        self.semantic_run.skipped = skipped;
        self.state.globals = globals;
        self.state.locals = locals;
        self.state.var_done = done;
        result.map(|_| ())
    }

    fn has_kernel(&self, node: NodeId) -> bool {
        self.has_kernel_in(node, self.type_context)
    }
    fn has_kernel_in(&self, node: NodeId, context: u32) -> bool {
        let visit = |node| self.has_kernel_in(node, context);
        if self.script.call_sites[node.index()].is_some() {
            return true;
        }
        match self.kind(node) {
            NodeKind::Script { statements } | NodeKind::Block { statements } => {
                statements.iter().any(|n| visit(*n))
            }
            NodeKind::Declare { value, .. }
            | NodeKind::TupleDeclare { value, .. }
            | NodeKind::Reassign { value, .. } => visit(*value),
            NodeKind::ExprStmt { expr } => visit(*expr),
            NodeKind::Unary { operand, .. } => visit(*operand),
            NodeKind::Binary { lhs, rhs, .. } => visit(*lhs) || visit(*rhs),
            NodeKind::History { subject, offset } => visit(*subject) || visit(*offset),
            NodeKind::Ternary {
                condition,
                if_true,
                if_false,
            } => visit(*condition) || visit(*if_true) || visit(*if_false),
            NodeKind::If {
                condition,
                then_block,
                otherwise,
            } => visit(*condition) || visit(*then_block) || otherwise.is_some_and(visit),
            NodeKind::For {
                from, to, by, body, ..
            } => visit(*from) || visit(*to) || by.is_some_and(visit) || visit(*body),
            NodeKind::While { condition, body } => visit(*condition) || visit(*body),
            NodeKind::Switch { subject, arms } => {
                subject.is_some_and(visit)
                    || arms.iter().any(|(a, b)| a.is_some_and(visit) || visit(*b))
            }
            NodeKind::Member { object, .. } => visit(*object),
            NodeKind::Tuple { items } => items.iter().any(|n| visit(*n)),
            NodeKind::Call { callee, args } => {
                args.iter().any(|a| visit(a.value))
                    || match self.script.resolutions[callee.index()] {
                        crate::compile::Resolution::Function(i) => {
                            let (child, target) = self
                                .script
                                .types
                                .dispatch
                                .get(&(context, node.index()))
                                .map_or((context, i), |(child, target, _)| (*child, *target));
                            self.has_kernel_in(self.script.functions[target as usize].body, child)
                        }
                        _ => false,
                    }
            }
            _ => false,
        }
    }

    pub(super) fn cached_kernel(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        site: u32,
        args: &[Arg],
    ) -> Result<Value, PineError> {
        let key = (site, self.path);
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg.value)?);
        }
        if let Some((previous, value)) = self.semantic_run.values.get(&key) {
            if previous
                .iter()
                .zip(&values)
                .all(|(a, b)| a == b || (a.is_na() && b.is_na()))
            {
                return Ok(value.clone());
            }
            match &self.semantic_run.initial[&key] {
                Some(initial) => {
                    self.state.kernels.insert(key, initial.clone());
                }
                None => {
                    self.state.kernels.remove(&key);
                }
            }
        } else {
            self.semantic_run
                .initial
                .insert(key, self.state.kernels.get(&key).cloned());
        }
        self.semantic_run.arguments = Some(values.clone());
        let result = self.kernel_call(id, builtin, site, args);
        self.semantic_run.arguments = None;
        let result = result?;
        self.semantic_run
            .values
            .insert(key, (values, result.clone()));
        Ok(result)
    }

    pub(super) fn macd_kernel(
        &mut self,
        call: super::kernels::KernelCall<'_>,
    ) -> Result<Value, PineError> {
        if call.args.len() != 4 {
            return Err(self.err(
                call.id,
                ErrorCode::PineArity,
                "ta.macd requires source, fast length, slow length and signal length",
            ));
        }
        let source = self.arg_num(call.args, 0)?;
        let lengths = [
            self.arg_length(call, 1)?,
            self.arg_length(call, 2)?,
            self.arg_length(call, 3)?,
        ];
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Macd(crate::semantics::Macd::new(lengths)));
        let Kernel::Macd(kernel) = kernel else {
            unreachable!("MACD site owns MACD state");
        };
        if kernel.lengths != lengths {
            return Err(self.err(
                call.id,
                ErrorCode::PineSeriesLength,
                "MACD lengths cannot change between bars",
            ));
        }
        Ok(kernel.push(source))
    }
}

impl super::ScriptState {
    pub(super) fn commit_histories(&mut self, max_bars_back: usize) {
        for key in self.staged_histories.keys() {
            self.histories.entry(*key).or_default();
        }
        for (key, ring) in &mut self.histories {
            ring.values
                .push_back(self.staged_histories.remove(key).unwrap_or(Value::Na));
            if ring.values.len() > max_bars_back {
                ring.values.pop_front();
            }
        }
    }
}
