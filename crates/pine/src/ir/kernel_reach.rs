//! Compile-time reachability for eager kernel advancement.
use super::CompiledScript;
use crate::ast::{NodeId, NodeKind};

pub(super) fn contains_kernel(script: &CompiledScript, node: NodeId, context: u32) -> bool {
    let visit = |node| contains_kernel(script, node, context);
    if script.call_sites[node.index()].is_some() {
        return true;
    }
    match &script.ast.node(node).kind {
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
                || match script.resolutions[callee.index()] {
                    super::Resolution::Function(i) => {
                        let (child, target) = script
                            .types
                            .dispatch
                            .get(&(context, node.index()))
                            .map_or((context, i), |(child, target, _)| (*child, *target));
                        contains_kernel(script, script.functions[target as usize].body, child)
                    }
                    _ => false,
                }
        }
        _ => false,
    }
}
