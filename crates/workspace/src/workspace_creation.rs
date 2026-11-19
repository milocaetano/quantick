//! The complete, validated starting point of a new workspace.
use super::{WorkspaceBook, WorkspaceRefusal};
use crate::canvas::{CanvasState, presets::timeframes_with};
use crate::workspace_colour::WorkspaceColour;
use crate::workspace_recipe::{ContextPaneRecipe, DeclaredLayout, WorkspaceRecipe};

pub const DEFAULT_TIMEFRAME_MS: u64 = 300_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceCreation {
    pub timeframe_ms: u64,
    pub charts: usize,
}

impl Default for WorkspaceCreation {
    fn default() -> Self {
        Self {
            timeframe_ms: DEFAULT_TIMEFRAME_MS,
            charts: 1,
        }
    }
}

impl WorkspaceCreation {
    /// Validate before any book, layout, or tab is changed.
    pub fn recipe(self) -> Result<WorkspaceRecipe, WorkspaceRefusal> {
        let preset = timeframes_with(self.charts).ok_or(WorkspaceRefusal::CreationInvalid)?;
        if self.timeframe_ms == 0 || self.timeframe_ms > i64::MAX as u64 {
            return Err(WorkspaceRefusal::CreationInvalid);
        }
        let bars = Some(format!("time:{}", self.timeframe_ms));
        Ok(WorkspaceRecipe {
            layout: DeclaredLayout::Time,
            canvas: Some(CanvasState::from_preset(preset)),
            context_panes: vec![
                ContextPaneRecipe {
                    bars: bars.clone(),
                    ..ContextPaneRecipe::default()
                };
                self.charts
            ],
            flow_bars: bars,
            candle_styles: Some(Vec::new()),
            ..WorkspaceRecipe::default()
        })
    }
}

impl WorkspaceBook {
    /// Create with the initial one-chart, five-minute choices.
    pub fn create(
        &mut self,
        tab: u64,
        name: &str,
    ) -> Result<super::CreateOutcome, WorkspaceRefusal> {
        self.create_with(tab, name, None, WorkspaceCreation::default())
    }

    /// Replace this tab's view while parking held edits for its next visit.
    pub fn create_with(
        &mut self,
        tab: u64,
        name: &str,
        colour: Option<WorkspaceColour>,
        creation: WorkspaceCreation,
    ) -> Result<super::CreateOutcome, WorkspaceRefusal> {
        let old = self.state(tab)?.clone();
        let name = self.create_recipe(name, colour, creation)?;
        if self.is_modified(&old) {
            self.parked
                .entry(tab)
                .or_default()
                .insert(old.workspace, old.copy);
        }
        let copy = self.recipe(&name).expect("created above").clone();
        self.tabs.insert(tab, super::TabWorkspace::on(&name, copy));
        self.unsynced.remove(&tab);
        Ok(super::CreateOutcome {
            workspace: name,
            dropped_edits: false,
        })
    }

    /// Resolve and validate the name and colour before the host observes live edits.
    pub fn creation_name(
        &self,
        name: &str,
        colour: Option<WorkspaceColour>,
    ) -> Result<String, WorkspaceRefusal> {
        let name = if name.trim().is_empty() {
            self.suggested_name("Workspace", self.recipes.len() + 1)
        } else {
            name.to_owned()
        };
        let name = self.validate_new_name(&name)?;
        if let Some(colour) = colour {
            self.colour_for_new(&name, colour)?;
        }
        Ok(name)
    }

    /// Add a clean recipe independently of any existing tab's held edits.
    pub fn create_recipe(
        &mut self,
        name: &str,
        colour: Option<WorkspaceColour>,
        creation: WorkspaceCreation,
    ) -> Result<String, WorkspaceRefusal> {
        let recipe = creation.recipe()?;
        let name = self.creation_name(name, colour)?;
        self.recipes.insert(name.clone(), recipe);
        if let Some(colour) = colour {
            self.colours.insert(name.clone(), colour);
            if let WorkspaceColour::Custom(rgb) = colour {
                self.last_custom = Some(rgb);
            }
        }
        self.settle_colours();
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creating_parks_edits_and_revisiting_restores_them_without_saving_over_the_recipe() {
        let mut book = WorkspaceBook::default();
        book.open_tab(1);
        book.save_as(1, "Work").unwrap();
        let saved = book.recipe("Work").unwrap().clone();
        let mut edited = saved.clone();
        edited.layers.insert("volume".to_owned(), false);
        book.observe(1, &edited).unwrap();
        let created = book
            .create_with(
                1,
                "",
                None,
                WorkspaceCreation {
                    charts: 3,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!created.dropped_edits);
        assert_eq!(book.recipe("Work"), Some(&saved));
        assert_eq!(book.tab_copy(1).unwrap().context_panes.len(), 3);
        let mut saved_tab: super::super::SavedTab = toml::from_str(
            "feed='test'
symbol='ABC'
layout='time'
flow_bars='tick:50'
time_bars='time:1m'",
        )
        .unwrap();
        book.stamp_saved_tab(1, &mut saved_tab);
        let mut book = super::super::parse_book(&toml::to_string(&book).unwrap()).unwrap();
        let saved_tab = toml::from_str(&toml::to_string(&saved_tab).unwrap()).unwrap();
        let name = book.restore_saved_workspace(1, &saved_tab).unwrap();
        book.restore_tab(1, &name, saved_tab.workspace_copy);
        let copy = book.tab_copy(1).unwrap();
        assert_eq!(copy.context_panes.len(), 3);
        assert_eq!(copy.candle_styles.as_deref(), Some([].as_slice()));
        let pristine = WorkspaceRecipe::default();
        assert!(pristine.is_factory());
        assert!(pristine.clone().with_unshown_of(&pristine).is_factory());
        assert_eq!(
            pristine.with_unshown_of(copy).candle_styles,
            Some(Vec::new())
        );
        book.set(1, "Work").unwrap();
        assert_eq!(book.tab_copy(1), Some(&edited));
        assert!(book.tab_state(1).unwrap().modified);
        assert!(book.autosave_winners(&[1], 1).is_empty());
    }

    #[test]
    fn counts_and_intervals_are_validated_before_creating_anything() {
        let mut book = WorkspaceBook::default();
        for charts in [1, 3, 8] {
            let name = book
                .create_recipe(
                    "",
                    None,
                    WorkspaceCreation {
                        charts,
                        timeframe_ms: 60_000,
                    },
                )
                .unwrap();
            let recipe = book.recipe(&name).unwrap();
            assert_eq!(recipe.canvas_state().panes().len(), charts);
            assert_eq!(recipe.context_panes.len(), charts);
            assert!(recipe.context_panes.iter().all(
                |pane| pane.indicators.is_empty() && pane.bars.as_deref() == Some("time:60000")
            ));
        }
        let before = book.clone();
        for creation in [
            WorkspaceCreation {
                charts: 0,
                ..Default::default()
            },
            WorkspaceCreation {
                charts: 9,
                ..Default::default()
            },
            WorkspaceCreation {
                timeframe_ms: 0,
                ..Default::default()
            },
        ] {
            assert_eq!(
                book.create_recipe("", None, creation),
                Err(WorkspaceRefusal::CreationInvalid)
            );
            assert_eq!(book, before);
        }
    }
}
