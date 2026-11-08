//! Eager advancement uses a temporary variable view; conditional side effects
//! and history writes remain in the selected execution only.
use super::{Eval, FLAGS, GLOBALS, LOCALS, Value};
use crate::{
    ast::{Arg, NodeId},
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

    pub(super) fn advance_skipped(&mut self, node: NodeId) -> Result<(), PineError> {
        if !self.has_kernel(node) {
            return Ok(());
        }
        let globals = self.state.arena.values(GLOBALS);
        let locals = self.state.arena.values(LOCALS);
        let done = self.state.arena.values(FLAGS);
        let skipped = self.semantic_run.skipped;
        self.semantic_run.skipped = true;
        let result = self.eval(node);
        self.semantic_run.skipped = skipped;
        self.state.arena.replace(GLOBALS, globals);
        self.state.arena.replace(LOCALS, locals);
        self.state.arena.replace(FLAGS, done);
        result.map(|_| ())
    }

    fn has_kernel(&self, node: NodeId) -> bool {
        self.script.program.kernel_reach[self.type_context as usize][node.index()]
    }

    pub(super) fn cached_kernel(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        site: u32,
        args: &[Arg],
    ) -> Result<Value, PineError> {
        // An acyclic script visits each textual site/path once. Only loops
        // need the initial-state image and changing-source replay cache.
        if !self.script.program.may_repeat {
            return self.kernel_call(id, builtin, site, args);
        }
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
            self.state
                .arena
                .restore_kernel(key, self.semantic_run.initial[&key].as_deref());
        } else {
            self.semantic_run
                .initial
                .insert(key, self.state.arena.kernel_snapshot(key));
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
        self.state
            .arena
            .push_macd(call.key, lengths, source)
            .ok_or_else(|| {
                self.err(
                    call.id,
                    ErrorCode::PineSeriesLength,
                    "MACD lengths cannot change between bars",
                )
            })
    }
}
