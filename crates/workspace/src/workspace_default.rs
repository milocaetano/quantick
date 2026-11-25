//! Preserve legacy templates while keeping the built-in recipe immutable.
use super::{
    DEFAULT_WORKSPACE, SavedTab, TabWorkspace, Unsynced, WorkspaceBook, WorkspaceRecipe,
    workspace_colour,
};
use crate::workspace_colour::WorkspaceColour;

impl WorkspaceBook {
    pub(super) fn protect_default(&mut self) {
        let factory_colour = WorkspaceColour::Preset(workspace_colour::DEFAULT_PRESET);
        let recipe = self
            .recipes
            .get(DEFAULT_WORKSPACE)
            .cloned()
            .unwrap_or_default();
        let colour = self.colour(DEFAULT_WORKSPACE).unwrap_or(factory_colour);
        if !recipe.is_factory() || colour != factory_colour {
            let name = self.suggested_name("Default copy", 1);
            self.recipes.insert(name.clone(), recipe);
            if let Some(id) = self.workspace_ids.remove(DEFAULT_WORKSPACE) {
                self.workspace_ids.insert(name.clone(), id);
            }
            self.colours.insert(name.clone(), colour);
            // Desk and book writes are independent. Keep the redirect until an
            // old desk has been read, even across an intervening book save.
            self.renamed.insert(DEFAULT_WORKSPACE.to_owned(), name);
        }
        self.recipes
            .insert(DEFAULT_WORKSPACE.to_owned(), WorkspaceRecipe::default());
        self.colours
            .insert(DEFAULT_WORKSPACE.to_owned(), factory_colour);
    }

    /// Resolve an old desk's Default before its tabs join the protected book.
    /// Newly saved references explicitly identify the built-in template.
    pub fn restore_saved_workspace(&mut self, tab: u64, saved: &SavedTab) -> Option<String> {
        self.forget_parked_tab(tab);
        if !saved.workspace_parked.is_empty() {
            self.parked.insert(tab, saved.workspace_parked.clone());
        }
        self.restored_settled.remove(&tab);
        if let Some(settled) = &saved.workspace_settled {
            self.restored_settled.insert(tab, settled.clone());
        }
        let name = saved.workspace.as_deref()?;
        if name == DEFAULT_WORKSPACE && !saved.protected_default {
            return Some(
                self.renamed
                    .get(name)
                    .map_or(name, String::as_str)
                    .to_owned(),
            );
        }
        Some(name.to_owned())
    }

    /// A tab the desk brought back, following a rename since. A workspace the
    /// book no longer has hands it to `Default`, keeping the copy it is given,
    /// marked; the window gives what the tab shows when the desk kept none, so
    /// a lost workspace never lays `Default` over the tab. A held copy takes
    /// the settled reading its desk kept, so its bar rules read as they did.
    /// With autosave on a copy that differs did not reach its workspace
    /// before the restart: it is [`Unsynced::Outvoted`], marked and never
    /// written unasked.
    pub fn restore_tab(&mut self, tab: u64, workspace: &str, copy: Option<WorkspaceRecipe>) {
        let name = self
            .resolve(workspace)
            .unwrap_or(DEFAULT_WORKSPACE)
            .to_owned();
        let held_copy = copy.is_some();
        let mut copy = copy.unwrap_or_else(|| self.recipes.get(&name).cloned().unwrap_or_default());
        copy.adopt_legacy_bars();
        let mut state = TabWorkspace::on(&name, copy);
        let named = self
            .recipes
            .get(&name)
            .is_some_and(|recipe| !recipe.is_factory());
        let settled = self.restored_settled.remove(&tab);
        state.settled = settled.filter(|_| held_copy && named);
        self.tabs.insert(tab, state);
        self.unsynced.remove(&tab);
        if self.autosave && (self.differs(tab) || (held_copy && self.captures_inherited_bars(tab)))
        {
            self.unsynced.insert(tab, Unsynced::Outvoted);
        }
    }

    /// Capture the workspace reference and any held edits together.
    pub fn stamp_saved_tab(&self, tab: u64, saved: &mut SavedTab) {
        (saved.workspace, saved.workspace_copy) = self.saved_fields(tab);
        let settled = self.tabs.get(&tab).and_then(|state| state.settled.clone());
        saved.workspace_settled = settled.filter(|_| saved.workspace_copy.is_some());
        saved.protected_default = saved.workspace.as_deref() == Some(DEFAULT_WORKSPACE);
        saved.workspace_parked = self.parked.get(&tab).cloned().unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_book::{WorkspaceRefusal, parse_book};

    #[test]
    fn template_refuses_direct_and_indirect_writes_and_autosave() {
        let mut book = WorkspaceBook::default();
        book.open_tab(1);
        let opened = WorkspaceRecipe::new_tab_arrangement(
            crate::workspace_recipe::DeclaredLayout::Time,
            &[],
        );
        book.settle_tab(1, &opened);
        let mut edited = opened.clone();
        edited.flow_bars = Some("time:300000".to_owned());
        edited.layers.insert("candles".to_owned(), false);
        book.observe(1, &edited).unwrap();
        assert!(book.tab_state(1).unwrap().modified);
        assert!(book.unsaved_edits(1).is_some());
        assert_eq!(book.save_to(1), Err(WorkspaceRefusal::DefaultProtected));
        assert!(book.autosave_winners(&[1], 1).is_empty());
        assert!(!book.flushes(1));
        let colour = book.colour(DEFAULT_WORKSPACE).unwrap();
        assert_eq!(
            book.set_colour(DEFAULT_WORKSPACE, colour, false),
            Err(WorkspaceRefusal::DefaultProtected)
        );
        book.save_as(1, "Editable").unwrap();
        assert_eq!(
            book.set_colour("Editable", colour, true),
            Err(WorkspaceRefusal::DefaultProtected)
        );
        assert!(book.recipe(DEFAULT_WORKSPACE).unwrap().is_factory());
        assert_eq!(book.recipe("Editable"), Some(&edited));
    }

    #[test]
    fn legacy_default_is_preserved_once_and_missing_default_is_recreated() {
        let text =
            "version = 1\n[colours]\nDefault = '#3A7BD5'\n[recipes.Default]\nlayout = 'time'\n";
        let book = parse_book(text).unwrap();
        assert!(book.recipe(DEFAULT_WORKSPACE).unwrap().is_factory());
        assert_eq!(
            book.renamed.get(DEFAULT_WORKSPACE).map(String::as_str),
            Some("Default copy 1")
        );
        assert_eq!(
            book.colour("Default copy 1"),
            Some(WorkspaceColour::Custom([58, 123, 213]))
        );
        let next = parse_book(&toml::to_string(&book).unwrap()).unwrap();
        assert_eq!(book, next);
        let mut reopened = next;
        let mut saved: SavedTab = toml::from_str(
            "feed='test'
symbol='ABC'
layout='time'
flow_bars='tick:50'
time_bars='time:1m'
workspace='Default'",
        )
        .unwrap();
        assert_eq!(
            reopened.restore_saved_workspace(1, &saved).as_deref(),
            Some("Default copy 1")
        );
        reopened.open_tab(2);
        reopened.stamp_saved_tab(2, &mut saved);
        assert_eq!(
            reopened.restore_saved_workspace(2, &saved).as_deref(),
            Some(DEFAULT_WORKSPACE)
        );
        assert!(
            parse_book("version = 1")
                .unwrap()
                .recipe(DEFAULT_WORKSPACE)
                .unwrap()
                .is_factory()
        );
    }
}
