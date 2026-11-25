//! Drawing ownership and write preferences, independent of saved arrangements.
use super::{DrawingKey, MarketKey, MarketSettings, PaneRole};
use crate::layout_document::{LayoutBook, LayoutId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DrawingScope {
    pub workspace: u64,
    pub layout: LayoutId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DrawingPreference {
    market: MarketKey,
    scope: Option<DrawingScope>,
}

impl DrawingKey {
    #[must_use]
    pub fn in_scope(mut self, scope: DrawingScope) -> Self {
        self.scope = Some(scope);
        self
    }

    #[must_use]
    pub fn all_timeframes_key(&self) -> Self {
        Self {
            role: PaneRole::AllTimeframes,
            ..self.clone()
        }
    }

    #[must_use]
    pub fn same_scope(&self, other: &Self) -> bool {
        self.market == other.market && self.scope == other.scope
    }
}

impl MarketSettings {
    #[must_use]
    pub fn drawing_autosave(&self, key: &DrawingKey) -> bool {
        !self
            .drawing_autosave_off
            .iter()
            .any(|off| off.market == key.market && off.scope == key.scope)
    }

    pub fn set_drawing_autosave(&mut self, key: &DrawingKey, on: bool) -> bool {
        if self.drawing_autosave(key) == on {
            return false;
        }
        self.drawing_autosave_off
            .retain(|off| off.market != key.market || off.scope != key.scope);
        if !on {
            self.drawing_autosave_off.push(DrawingPreference {
                market: key.market.clone(),
                scope: key.scope,
            });
            self.drawing_autosave_off
                .sort_by(|a, b| (&a.market, a.scope).cmp(&(&b.market, b.scope)));
        }
        true
    }

    /// Preferences reach disk even when their drawings are paused. A paused
    /// scope always retains the last successful disk snapshot, including an
    /// empty snapshot after a durable clear.
    pub(crate) fn persistence_document(&self, saved: &Self) -> Self {
        let mut document = self.clone();
        document
            .drawings
            .retain(|set| self.drawing_autosave(&set.key()));
        document.drawings.extend(
            saved
                .drawings
                .iter()
                .filter(|set| !self.drawing_autosave(&set.key()))
                .cloned(),
        );
        document.drawings.sort_by_key(super::DrawingSet::key);
        document
    }

    /// Resolve only ownership proven by the supplied complete candidate set.
    /// Unresolved entries stay unscoped and round-trip untouched. Never clone
    /// a durable identity into multiple independent scopes.
    pub fn migrate_scopes(&mut self, candidates: &[DrawingKey], layouts: &LayoutBook) -> usize {
        let mut moved = Vec::new();
        let paused = &self.drawing_autosave_off;
        self.drawings.retain_mut(|set| {
            if set.scope.is_some() {
                return true;
            }
            let was_empty = set.items.is_empty();
            let key = set.key();
            set.items.retain(|item| {
                let layout = item
                    .layout
                    .or_else(|| {
                        let name = &item.origin.as_ref()?.layout;
                        layouts
                            .layouts()
                            .iter()
                            .find(|layout| &layout.name == name)
                            .map(|l| l.id)
                    })
                    .map(|id| layouts.strip_layout(id));
                if layout.is_none() && item.origin.is_some() {
                    return true;
                }
                let possible: BTreeSet<DrawingScope> = candidates
                    .iter()
                    .filter(|candidate| candidate.market == key.market)
                    .filter_map(|candidate| candidate.scope)
                    .filter(|scope| layout.is_none_or(|layout| scope.layout == layout))
                    .collect();
                if possible.len() != 1 {
                    return true;
                }
                let scope = *possible.first().expect("one candidate");
                if paused
                    .iter()
                    .any(|off| off.market == key.market && off.scope == Some(scope))
                {
                    return true;
                }
                moved.push((key.clone().in_scope(scope), item.clone()));
                false
            });
            was_empty || !set.items.is_empty()
        });
        let count = moved.len();
        for (key, item) in moved {
            self.push_renumbered(&key, item);
        }
        count
    }
}
