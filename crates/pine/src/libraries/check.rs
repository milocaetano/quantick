//! User-object type checking and field slot resolution.
use super::{
    check::Checker,
    model::{Base, Qualifier, Type, annotation},
};
use crate::{
    ast::{Arg, NodeId, NodeKind},
    ir::Resolution,
    libraries::ObjectOp,
};

impl Checker<'_> {
    pub(super) fn annotation(
        &self,
        words: &[String],
    ) -> Result<Option<(Base, Option<Qualifier>)>, String> {
        if let Some(name) = words.last()
            && self.ast.user_types.iter().any(|ty| &ty.name == name)
        {
            let qualifier = match words {
                [_] => None,
                [q, _] if q == "series" => Some(Qualifier::Series),
                _ => return Err("user objects require a series qualifier".into()),
            };
            return Ok(Some((Base::UserType(name.clone()), qualifier)));
        }
        annotation(words)
    }

    pub(super) fn user_constructor(
        &mut self,
        id: NodeId,
        index: u32,
        args: &[Arg],
        values: &[Type],
    ) -> Type {
        let def = self.ast.user_types[index as usize].clone();
        for ((field, value), arg) in def.fields.iter().zip(values).zip(args) {
            match self.annotation(std::slice::from_ref(&field.type_name)) {
                Ok(Some((base, _))) => {
                    if !self.accepts(&base, &value.base) {
                        self.error(
                            arg.value,
                            format!("field `{}` requires {base:?}", field.name),
                        );
                    }
                    self.coercions[arg.value.index()] = match base {
                        Base::Bool => self.version.bool_coercion(),
                        Base::Color => 2,
                        Base::String => 3,
                        _ => 0,
                    };
                }
                Err(message) => self.error(id, message),
                _ => {}
            }
        }
        Type::new(Base::UserType(def.name), Qualifier::Series)
    }

    fn field_type(&mut self, id: NodeId, object: NodeId, field: &str) -> Option<(u32, Type)> {
        let ty = self.infer(object);
        let Base::UserType(name) = ty.base else {
            if ty.base != Base::Unknown {
                self.error(id, "field access requires a user object");
            }
            return None;
        };
        let definition = self.ast.user_types.iter().find(|ty| ty.name == name)?;
        let Some((index, definition)) = definition
            .fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == field)
        else {
            self.error(id, format!("unknown field `{field}` on `{name}`"));
            return None;
        };
        match self.annotation(std::slice::from_ref(&definition.type_name)) {
            Ok(Some((base, _))) => Some((index as u32, Type::new(base, Qualifier::Series))),
            Err(message) => {
                self.error(id, message);
                None
            }
            _ => None,
        }
    }

    pub(super) fn user_field(&mut self, id: NodeId, object: NodeId, field: &str) -> Type {
        if let Some((index, ty)) = self.field_type(id, object, field) {
            self.resolutions[id.index()] = Resolution::Object(ObjectOp::Get(index));
            self.object_ops
                .insert((self.context, id.index()), ObjectOp::Get(index));
            ty
        } else {
            Type::unknown()
        }
    }

    pub(super) fn user_operation(
        &mut self,
        id: NodeId,
        callee: NodeId,
        args: &[Arg],
        values: &[Type],
    ) -> Type {
        let NodeKind::Member { object, field } = self.ast.node(callee).kind.clone() else {
            return self.method(id, callee, args, values);
        };
        if let Some(field) = field.strip_prefix("$set_") {
            if let Some((index, ty)) = self.field_type(id, object, field) {
                if values.len() != 1 || !self.accepts(&ty.base, &values[0].base) {
                    self.error(id, "field assignment changes its declared type");
                }
                self.resolutions[callee.index()] = Resolution::Object(ObjectOp::Set(index));
                self.object_ops
                    .insert((self.context, id.index()), ObjectOp::Set(index));
                return ty;
            }
            return Type::unknown();
        }
        if field == "copy" {
            let ty = self.infer(object);
            if matches!(ty.base, Base::UserType(_)) {
                if !args.is_empty() {
                    self.error(id, "object copy takes no arguments");
                }
                self.resolutions[callee.index()] = Resolution::Object(ObjectOp::Copy);
                return ty;
            }
        }
        self.method(id, callee, args, values)
    }
}
