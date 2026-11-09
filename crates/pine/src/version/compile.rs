//! Version restrictions on already supported plot arguments.
use super::{Compiler, PendingPlot};
use crate::{
    ast::{NodeId, NodeKind},
    error::{ErrorCode, PineError},
};

impl Compiler {
    pub(super) fn validate_version_arguments(&mut self) {
        if !self.checked_types.version.two_state_bool() {
            return;
        }
        for (call, args, kind) in self.pending_plots.clone() {
            let unique: &[(usize, &str)] = match kind {
                PendingPlot::Plot => &[(4, "style")],
                PendingPlot::Shape { is_char: false } => &[(2, "style"), (3, "location")],
                PendingPlot::Shape { is_char: true } => &[(3, "location")],
                _ => &[],
            };
            for &(position, name) in unique {
                if let Some(arg) = self.arg(&args, position, name)
                    && self.version_missing(arg.value, 0)
                {
                    self.errors.push(PineError::new(
                        ErrorCode::PineType,
                        self.span(call),
                        format!("v6 unique {name} parameters cannot be na"),
                    ));
                }
            }
            if !matches!(kind, PendingPlot::Plot | PendingPlot::Hline) {
                continue;
            }
            if let Some(arg) = self.arg(&args, 3, "linewidth")
                && self
                    .fold(arg.value)
                    .and_then(|c| c.as_f64())
                    .is_some_and(|w| w < 1.0)
            {
                self.errors.push(PineError::new(
                    ErrorCode::PineType,
                    self.span(call),
                    "v6 linewidth must be at least 1",
                ));
            }
        }
    }
    fn version_missing(&self, id: NodeId, depth: usize) -> bool {
        if depth > self.ast.len() {
            return true;
        }
        let next = |id| self.version_missing(id, depth + 1);
        match self.kind(id) {
            NodeKind::Na => true,
            NodeKind::If {
                then_block,
                otherwise,
                ..
            } => next(*then_block) || otherwise.is_none_or(next),
            NodeKind::Switch { arms, .. } => {
                !arms.iter().any(|(test, _)| test.is_none())
                    || arms.iter().any(|(_, body)| next(*body))
            }
            NodeKind::Ternary {
                if_true, if_false, ..
            } => next(*if_true) || next(*if_false),
            NodeKind::Block { statements } => statements.last().is_none_or(|n| next(*n)),
            NodeKind::ExprStmt { expr } => next(*expr),
            NodeKind::Name(_) => match self.resolutions[id.index()] {
                super::Resolution::Global(slot) => {
                    self.global_init[slot as usize].is_some_and(next)
                }
                _ => false,
            },
            _ => false,
        }
    }
}
