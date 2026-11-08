//! Normalize methods into existing user-function calls before name resolution.
use crate::{
    ast::{Arg, Ast, NodeKind},
    error::{ErrorCode, PineError},
};

pub(crate) fn prepare(ast: &mut Ast) -> Result<(), Vec<PineError>> {
    let mut errors = Vec::new();
    for definition in &ast.user_types {
        let mut fields = std::collections::BTreeSet::new();
        for field in &definition.fields {
            let span = ast.node(field.default).span;
            if !fields.insert(&field.name) {
                errors.push(PineError::new(
                    ErrorCode::PineType,
                    span,
                    "duplicate user-object field",
                ));
            }
            if !matches!(
                field.type_name.as_str(),
                "int" | "float" | "bool" | "string" | "color" | "line" | "box" | "label"
            ) && !ast.user_types.iter().any(|ty| ty.name == field.type_name)
            {
                errors.push(PineError::new(
                    ErrorCode::PineType,
                    span,
                    format!("unknown field type `{}`", field.type_name),
                ));
            }
            let literal = |node| {
                matches!(
                    ast.node(node).kind,
                    NodeKind::Int(_)
                        | NodeKind::Float(_)
                        | NodeKind::Bool(_)
                        | NodeKind::Str(_)
                        | NodeKind::Color(_)
                        | NodeKind::Na
                )
            };
            let valid = literal(field.default)
                || matches!(ast.node(field.default).kind, NodeKind::Unary { op: crate::ast::UnOp::Neg, operand } if literal(operand));
            if !valid {
                errors.push(PineError::new(
                    ErrorCode::PineType,
                    span,
                    "user-object defaults must be literals",
                ));
            }
        }
    }
    for index in 0..ast.len() {
        let id = ast.id_at(index).expect("AST node");
        let NodeKind::Call { callee, mut args } = ast.node(id).kind.clone() else {
            continue;
        };
        let NodeKind::Member { object, field } = ast.node(callee).kind.clone() else {
            continue;
        };
        if ast.methods.contains(&field) {
            args.insert(
                0,
                Arg {
                    name: None,
                    value: object,
                    span: ast.node(object).span,
                },
            );
            ast.replace(callee, NodeKind::Name(field));
            ast.replace(id, NodeKind::Call { callee, args });
        } else if field == "new"
            && let NodeKind::Name(name) = &ast.node(object).kind
            && let Some(def) = ast.user_types.iter().find(|t| &t.name == name).cloned()
        {
            let mut ordered = vec![None; def.fields.len()];
            let mut positional = 0;
            for arg in args {
                let slot = if let Some(name) = &arg.name {
                    def.fields.iter().position(|f| &f.name == name)
                } else {
                    let slot = positional;
                    positional += 1;
                    Some(slot)
                };
                if let Some(slot) =
                    slot.filter(|slot| *slot < ordered.len() && ordered[*slot].is_none())
                {
                    ordered[slot] = Some(arg);
                } else {
                    errors.push(PineError::new(
                        ErrorCode::PineType,
                        arg.span,
                        "unknown, duplicate or excess constructor argument",
                    ));
                }
            }
            let args = ordered
                .into_iter()
                .zip(def.fields)
                .map(|(arg, field)| {
                    arg.unwrap_or(Arg {
                        name: None,
                        value: field.default,
                        span: ast.node(field.default).span,
                    })
                })
                .collect();
            ast.replace(id, NodeKind::Call { callee, args });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
