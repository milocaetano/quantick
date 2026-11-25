//! Persisted workspace identities; labels and tab-local copies are not owners.
use super::WorkspaceBook;
use crate::{layout_document::LayoutId, market_settings::DrawingScope};
use std::collections::BTreeSet;

impl WorkspaceBook {
    pub(super) fn settle_workspace_ids(&mut self) {
        self.next_workspace_id = self.next_workspace_id.max(
            self.workspace_ids
                .values()
                .copied()
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        self.workspace_ids
            .retain(|name, _| self.recipes.contains_key(name));
        let mut seen = BTreeSet::new();
        let names: Vec<String> = self.names().into_iter().map(str::to_owned).collect();
        for name in names {
            let valid = self
                .workspace_ids
                .get(&name)
                .is_some_and(|id| *id != 0 && seen.insert(*id));
            if !valid {
                let id = self.next_workspace_id.max(1);
                self.next_workspace_id = id.saturating_add(1);
                self.workspace_ids.insert(name, id);
                seen.insert(id);
            }
        }
    }

    #[must_use]
    pub fn workspace_id(&self, name: &str) -> Option<u64> {
        self.workspace_ids.get(self.resolve(name)?).copied()
    }

    /// Allocator reservation that must reach disk before new drawing owners.
    #[must_use]
    pub fn workspace_id_highwater(&self) -> u64 {
        self.next_workspace_id
    }

    #[must_use]
    pub fn drawing_scope(&self, tab: u64, layout: LayoutId) -> Option<DrawingScope> {
        let name = &self.tabs.get(&tab)?.workspace;
        Some(DrawingScope {
            workspace: self.workspace_id(name)?,
            layout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_book::parse_book;

    #[test]
    fn an_arriving_tab_has_no_drawing_scope_until_adopted() {
        let mut book = WorkspaceBook::default();
        assert_eq!(book.drawing_scope(7, LayoutId(3)), None);
        book.open_tab(7);
        assert_eq!(
            book.drawing_scope(7, LayoutId(3)).unwrap().workspace,
            book.workspace_id("Default").unwrap()
        );
    }

    #[test]
    fn identities_follow_renames_restart_and_delete_undo_without_reuse() {
        let mut book = WorkspaceBook::default();
        book.open_tab(7);
        book.save_as(7, "One").unwrap();
        let id = book.workspace_id("One").unwrap();
        book.rename("One", "Two").unwrap();
        assert_eq!(book.workspace_id("Two"), Some(id));
        let mut book = parse_book(&toml::to_string(&book).unwrap()).unwrap();
        assert_eq!(book.workspace_id("Two"), Some(id));
        book.delete_at("Two", 0).unwrap();
        book.undo_delete("Two", 1).unwrap();
        assert_eq!(book.workspace_id("Two"), Some(id));
        book.delete("Two").unwrap();
        book.open_tab(7);
        book.save_as(7, "Two").unwrap();
        assert_ne!(book.workspace_id("Two"), Some(id));
    }

    #[test]
    fn revert_and_its_undo_keep_ownership_through_a_rename_without_saving_the_copy() {
        use crate::workspace_recipe::WorkspaceRecipe;

        let mut book = WorkspaceBook::default();
        book.open_tab(7);
        let saved = WorkspaceRecipe {
            split_fraction: Some(0.5),
            ..Default::default()
        };
        book.observe(7, &saved).unwrap();
        book.save_as(7, "Scalp").unwrap();
        let scope = book.drawing_scope(7, LayoutId(3)).unwrap();
        let mut opened = saved.clone();
        opened.flow_bars = Some("tick:500".to_owned());
        book.settle_tab(7, &opened);
        let mut edit = opened.clone();
        edit.flow_bars = Some("tick:2000".to_owned());
        edit.split_fraction = Some(0.7);
        book.observe(7, &edit).unwrap();

        assert!(book.revert(7).unwrap().changed);
        assert_eq!(book.tab_copy(7), Some(&opened));
        book.rename("Scalp", "Renamed").unwrap();
        assert_eq!(book.drawing_scope(7, scope.layout), Some(scope));
        assert!(book.undo_revert(7).unwrap().changed);
        assert_eq!(book.tab_copy(7), Some(&edit));
        assert_eq!(book.drawing_scope(7, scope.layout), Some(scope));
        let reopened = parse_book(&toml::to_string(&book).unwrap()).unwrap();
        assert_eq!(reopened.workspace_id("Renamed"), Some(scope.workspace));
        assert_eq!(reopened.recipe("Renamed"), Some(&saved));
        assert!(!reopened.autosave());
    }
}
