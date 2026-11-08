//! Caller-supplied local libraries and user-defined objects.
mod prepare;
mod source_scopes;
mod sources;

pub(crate) use prepare::prepare;
pub use sources::compile_with_libraries;

use crate::ast::NodeId;

#[derive(Debug, Clone)]
pub(crate) struct UserType {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub(crate) struct Field {
    pub name: String,
    pub type_name: String,
    pub default: NodeId,
    pub explicit_default: bool,
    pub varip: bool,
}

/// Resolved object operation; field and type identities are compile-time slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOp {
    /// Allocate a user object of the numbered type.
    New(u32),
    /// Read a numbered field.
    Get(u32),
    /// Assign a numbered field.
    Set(u32),
    /// Copy an object's fields into a fresh reference.
    Copy,
}

/// Resolve generic object collection constructors to immutable numeric type slots.
pub(crate) fn collection_constructor(
    name: &str,
    ast: &crate::ast::Ast,
) -> Option<crate::collections::Op> {
    use crate::collections::{Element, Op};
    let user = |name: &str| {
        ast.user_types
            .iter()
            .position(|ty| ty.name == name)
            .map(|i| Element::User(i as u32))
    };
    if let Some(name) = name.strip_prefix("array.new_") {
        return user(name).map(Op::NewArray);
    }
    let name = name.strip_prefix("map.new_")?;
    let (key, value) = name.split_once('_')?;
    let key = match key {
        "int" => Element::Int,
        "float" => Element::Float,
        "bool" => Element::Bool,
        "string" => Element::String,
        "color" => Element::Color,
        _ => return None,
    };
    Some(Op::NewMap(key, user(value)?))
}
