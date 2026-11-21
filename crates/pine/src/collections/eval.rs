//! Collection calls adapt evaluated arguments to the arena operations.
use crate::{
    ast::{Arg, NodeId},
    collections::Op,
    error::{ErrorCode, PineError},
};
use crate::{eval::Eval, types::Value};
impl Eval<'_> {
    pub(crate) fn collection_call(
        &mut self,
        id: NodeId,
        op: Op,
        args: &[Arg],
    ) -> Result<Value, PineError> {
        if self.semantic_run.skipped && crate::collections::mutates(op) {
            self.advance_arguments(args)?;
            return Ok(Value::Na);
        }
        let values = args
            .iter()
            .map(|arg| self.eval(arg.value))
            .collect::<Result<Vec<_>, _>>()?;
        crate::collections::execute(self.state.arena, self.script.version, op, &values)
            .map_err(|message| self.err(id, ErrorCode::PineType, message))
    }
}
