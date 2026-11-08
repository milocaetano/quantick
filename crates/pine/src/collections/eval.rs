//! Collection calls adapt evaluated arguments to the arena operations.
use super::{Eval, Value};
use crate::{
    ast::{Arg, NodeId},
    collections::Op,
    error::{ErrorCode, PineError},
};
impl Eval<'_> {
    pub(super) fn collection_call(
        &mut self,
        id: NodeId,
        op: Op,
        args: &[Arg],
    ) -> Result<Value, PineError> {
        if self.semantic_run.skipped {
            self.advance_arguments(args)?;
            return Ok(Value::Na);
        }
        let values = args
            .iter()
            .map(|arg| self.eval(arg.value))
            .collect::<Result<Vec<_>, _>>()?;
        crate::collections::execute(&mut self.state.arena, self.script.version, op, &values)
            .map_err(|message| self.err(id, ErrorCode::PineType, message))
    }
}
