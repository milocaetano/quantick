//! Bounded control instructions over compiled entry points.
use super::{Eval, LOOP_BUDGET, Value};
use crate::{
    ast::NodeId,
    error::{ErrorCode, PineError},
};
impl Eval<'_> {
    pub(super) fn run_for(
        &mut self,
        id: NodeId,
        from: NodeId,
        to: NodeId,
        by: Option<NodeId>,
        body: NodeId,
    ) -> Result<Value, PineError> {
        let from = self.num(from)?;
        let to = self.num(to)?;
        let step = match by {
            Some(by) => self.num(by)?,
            None => 1.0,
        };
        if !from.is_finite() || !to.is_finite() || step == 0.0 || !step.is_finite() {
            return Ok(Value::Na);
        }
        self.write_resolved(id, Value::Num(from));
        self.advance_skipped(body)?;
        let mut i = from;
        let mut last = Value::Na;
        let mut budget = LOOP_BUDGET;
        let ascending = step > 0.0;
        while (ascending && i <= to) || (!ascending && i >= to) {
            if budget == 0 {
                return Err(self.err(
                    id,
                    ErrorCode::PineLoopBudget,
                    format!("loop exceeded {LOOP_BUDGET} iterations in one bar"),
                ));
            }
            budget -= 1;
            self.write_resolved(id, Value::Num(i));
            last = self.eval(body)?;
            i += step;
        }
        Ok(last)
    }
    pub(super) fn run_while(
        &mut self,
        id: NodeId,
        condition: NodeId,
        body: NodeId,
    ) -> Result<Value, PineError> {
        self.advance_skipped(body)?;
        let mut last = Value::Na;
        let mut budget = LOOP_BUDGET;
        while self.cond(condition)? {
            if budget == 0 {
                return Err(self.err(
                    id,
                    ErrorCode::PineLoopBudget,
                    format!("loop exceeded {LOOP_BUDGET} iterations in one bar"),
                ));
            }
            budget -= 1;
            last = self.eval(body)?;
        }
        Ok(last)
    }
}
