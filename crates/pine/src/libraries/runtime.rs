//! User fields live entirely in the authoritative transactional word arena.
use super::{Eval, Value};
use crate::{
    ast::{Arg, NodeId},
    error::{ErrorCode, PineError},
    libraries::ObjectOp,
};

impl Eval<'_> {
    pub(super) fn user_object(
        &mut self,
        id: NodeId,
        op: ObjectOp,
        object: Option<NodeId>,
        args: &[Arg],
    ) -> Result<Value, PineError> {
        let op = self
            .script
            .types
            .object_ops
            .get(&(self.type_context, id.index()))
            .copied()
            .unwrap_or(op);
        if self.semantic_run.skipped && matches!(op, ObjectOp::Set(_)) {
            if let Some(object) = object {
                self.advance_skipped(object)?;
            }
            self.advance_arguments(args)?;
            return Ok(Value::Na);
        }
        let object = object.map(|node| self.eval(node)).transpose()?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg.value)?);
        }
        if let ObjectOp::New(index) = op {
            let modes = self.script.types.object_field_modes[index as usize]
                .iter()
                .map(|v| Value::Bool(*v))
                .collect();
            return self.allocate_user_object(id, &values, modes);
        }
        let Some(Value::UserObject(offset)) = object else {
            return Err(self.err(
                id,
                ErrorCode::PineType,
                "cannot access a missing user object",
            ));
        };
        let offset = offset as usize;
        match op {
            ObjectOp::Get(field) => Ok(self.state.arena.get(offset, field as usize)),
            ObjectOp::Set(field) => {
                let value = values.into_iter().next().expect("checked field assignment");
                self.state.arena.set(offset, field as usize, value.clone());
                Ok(value)
            }
            ObjectOp::Copy => {
                let values: Vec<_> = (0..self.state.arena.len(offset))
                    .map(|index| self.state.arena.get(offset, index))
                    .collect();
                let modes = self.state.arena.values(offset + 3);
                self.allocate_user_object(id, &values, modes)
            }
            ObjectOp::New(_) => unreachable!("handled allocation"),
        }
    }

    fn allocate_user_object(
        &mut self,
        id: NodeId,
        fields: &[Value],
        modes: Vec<Value>,
    ) -> Result<Value, PineError> {
        const MAX_OBJECT_WORDS: usize = 4_000_000;
        if self.state.arena.words.len() > MAX_OBJECT_WORDS {
            return Err(self.err(id, ErrorCode::PineType, "user-object arena budget exceeded"));
        }
        // Registry root 31 is reserved beside collection root 30. The handle
        // addresses the list header, while the preceding word links records.
        let record = self.state.arena.allocate(7);
        self.state.arena.words[record] = self.state.arena.words[31];
        self.state.arena.words[31] = record as u64;
        let offset = record + 1;
        self.state.arena.replace(offset + 3, modes);
        self.state.arena.resize(offset, fields.len());
        for (index, value) in fields.iter().enumerate() {
            self.state.arena.set(offset, index, value.clone());
        }
        Ok(Value::UserObject(
            u32::try_from(offset).expect("bounded arena"),
        ))
    }
}
