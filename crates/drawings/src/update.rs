//! The update of Model-View-Update: one [`DrawingIntent`] folded into a
//! pane's marks, answering the [`DrawingEffect`]s only the window can carry
//! out — a toast, a strategy that rode a removed mark, a confirmation.
//!
//! The intents are the edits a trader makes to the marks themselves, already
//! translated out of the window's key and click vocabulary; what a key means
//! is the window's, what the edit does is decided here.

use crate::{DeleteOutcome, DrawingId, Drawings, ToolCatalog};

/// An edit the window asks the drawings to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingIntent {
    /// Take back the last anchor of the mark being placed; the draft goes
    /// when its last anchor does.
    StepBackDraft,
    /// Undo the last committed edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
    /// Lock the selected mark, or unlock it.
    ToggleLockSelected,
    /// Hide the selected mark, or show it.
    ToggleHideSelected,
    /// Delete the selected mark; `force` overrides its lock.
    DeleteSelected { force: bool },
}

/// What an edit asks the window to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawingEffect {
    /// Undo or redo replaced the marks wholesale: whatever rode a mark that
    /// is no longer there has to be let go.
    HistoryReplaced,
    /// The selected mark is gone, as one undoable step. `label` names it the
    /// way the trader knows it — their own name, else the tool's — because
    /// "drawing deleted" on a crowded chart leaves the undo useless.
    Deleted { id: DrawingId, label: String },
    /// The selected mark is locked: deleting it takes a confirmation.
    ConfirmDelete,
}

/// Fold `intent` into `store`, answering the effects the window runs.
pub fn update<T: ToolCatalog>(
    store: &mut Drawings<T>,
    intent: DrawingIntent,
) -> Vec<DrawingEffect> {
    match intent {
        DrawingIntent::StepBackDraft => {
            store.remove_last_draft_anchor();
            Vec::new()
        }
        DrawingIntent::Undo => {
            store.undo();
            vec![DrawingEffect::HistoryReplaced]
        }
        DrawingIntent::Redo => {
            store.redo();
            vec![DrawingEffect::HistoryReplaced]
        }
        DrawingIntent::ToggleLockSelected => {
            if let Some(index) = store.selected() {
                let locked = store.items()[index].locked;
                store.set_selected_locked(!locked);
            }
            Vec::new()
        }
        DrawingIntent::ToggleHideSelected => {
            if let Some(index) = store.selected() {
                let hidden = store.items()[index].hidden;
                store.set_selected_hidden(!hidden);
            }
            Vec::new()
        }
        DrawingIntent::DeleteSelected { force } => {
            // Read the name before the object is gone: the trader has to
            // know *what* they lost to know whether they want it back.
            let doomed = store.selected().and_then(|index| {
                let drawing = store.items().get(index)?;
                let label = drawing
                    .name
                    .clone()
                    .unwrap_or_else(|| drawing.tool.name().to_owned());
                Some((drawing.id, label))
            });
            match store.delete_selected(force) {
                DeleteOutcome::Deleted => doomed
                    .map(|(id, label)| DrawingEffect::Deleted { id, label })
                    .into_iter()
                    .collect(),
                DeleteOutcome::NeedsConfirmation => vec![DrawingEffect::ConfirmDelete],
                DeleteOutcome::NothingSelected => Vec::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChartPoint, DrawingTool};

    fn line() -> DrawingTool {
        DrawingTool::by_id("trend-line").expect("registered")
    }

    /// `count` finished trend lines, none selected.
    fn lines(count: usize) -> Drawings<DrawingTool> {
        let mut store = Drawings::default();
        for index in 0..count {
            let bar = index as f32 * 10.0;
            assert!(!store.place(line(), ChartPoint::at(bar, 100.0)));
            assert!(store.place(line(), ChartPoint::at(bar + 5.0, 110.0)));
        }
        store.select(None);
        store
    }

    #[test]
    fn deleting_a_named_mark_says_its_name_and_an_unnamed_one_its_tool() {
        let mut store = lines(2);
        store.rename_at(0, "congestion 108k");
        store.select(Some(0));
        let id = store.items()[0].id;
        assert_eq!(
            update(&mut store, DrawingIntent::DeleteSelected { force: false }),
            [DrawingEffect::Deleted {
                id,
                label: "congestion 108k".to_owned()
            }]
        );
        store.select(Some(0));
        let id = store.items()[0].id;
        assert_eq!(
            update(&mut store, DrawingIntent::DeleteSelected { force: false }),
            [DrawingEffect::Deleted {
                id,
                label: line().name().to_owned()
            }]
        );
        assert!(store.items().is_empty());
        assert_eq!(
            update(&mut store, DrawingIntent::DeleteSelected { force: false }),
            [],
            "nothing selected, nothing to say"
        );
    }

    #[test]
    fn a_locked_mark_asks_before_it_goes_and_force_takes_it() {
        let mut store = lines(1);
        store.select(Some(0));
        update(&mut store, DrawingIntent::ToggleLockSelected);
        assert!(store.items()[0].locked);
        assert_eq!(
            update(&mut store, DrawingIntent::DeleteSelected { force: false }),
            [DrawingEffect::ConfirmDelete]
        );
        assert_eq!(store.items().len(), 1, "a refused delete removes nothing");
        assert!(matches!(
            update(&mut store, DrawingIntent::DeleteSelected { force: true }).as_slice(),
            [DrawingEffect::Deleted { .. }]
        ));
    }

    #[test]
    fn undo_and_redo_replace_the_marks_and_say_so() {
        let mut store = lines(1);
        store.select(Some(0));
        update(&mut store, DrawingIntent::DeleteSelected { force: false });
        assert_eq!(
            update(&mut store, DrawingIntent::Undo),
            [DrawingEffect::HistoryReplaced]
        );
        assert_eq!(store.items().len(), 1, "undo brings the mark back");
        assert_eq!(
            update(&mut store, DrawingIntent::Redo),
            [DrawingEffect::HistoryReplaced]
        );
        assert!(store.items().is_empty(), "redo takes it again");
    }

    #[test]
    fn the_toggles_flip_the_selected_mark_and_leave_an_unselected_store_alone() {
        let mut store = lines(1);
        update(&mut store, DrawingIntent::ToggleHideSelected);
        assert!(!store.items()[0].hidden, "no selection, no edit");
        store.select(Some(0));
        update(&mut store, DrawingIntent::ToggleHideSelected);
        assert!(store.items()[0].hidden);
        update(&mut store, DrawingIntent::ToggleHideSelected);
        assert!(!store.items()[0].hidden);
    }

    #[test]
    fn stepping_back_a_draft_drops_one_anchor_then_the_draft() {
        let tool = DrawingTool::by_id("parallel-channel").expect("registered");
        let mut store = Drawings::default();
        assert!(!store.place(tool, ChartPoint::at(0.0, 100.0)));
        assert!(!store.place(tool, ChartPoint::at(5.0, 110.0)));
        update(&mut store, DrawingIntent::StepBackDraft);
        assert_eq!(store.draft().map(|draft| draft.points.len()), Some(1));
        update(&mut store, DrawingIntent::StepBackDraft);
        assert!(store.draft().is_none());
    }
}
