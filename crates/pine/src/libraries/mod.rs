//! Caller-supplied local libraries and user-defined objects.
mod prepare;
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
