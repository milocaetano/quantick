//! The update of Model-View-Update: one [`DrawingIntent`] folded into a
//! pane's marks, answering the [`DrawingEffect`]s only the window can carry
//! out — a toast, a strategy that rode a removed mark, a confirmation.
//!
//! The intents are the edits made to the marks themselves, already
//! translated out of the window's key and click vocabulary or a control
//! call's wire shape; what a key or a call means is its caller's, what the
//! edit does is decided here.

use crate::{
    ChartPoint, DeleteOutcome, DrawingAuthor, DrawingBand, DrawingId, DrawingTool, Drawings,
    NewDrawing,
};

/// One finished mark an operator places whole: every anchor already
/// resolved against the pane, the look it opens with, and who placed it.
#[derive(Debug)]
pub struct Placement {
    pub tool: DrawingTool,
    pub points: Vec<ChartPoint>,
    /// The look the mark opens with, read through the same door a click
    /// reads it: one is enough, the draft asks for it once.
    pub opening: NewDrawing,
    /// `None` is the trader's own hand; anything else is shown as its
    /// author's and is the only thing [`DrawingIntent::RemoveAuthored`] takes.
    pub author: Option<DrawingAuthor>,
    pub name: Option<String>,
    /// The words of a tool that holds text; ignored by one that does not.
    pub text: Option<String>,
}

/// Why a placement was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementFailure {
    /// The tool did not finish from the anchors it was given.
    Incomplete,
    /// The tool finished but the placed mark could not be read back.
    ///
    /// The mark stays: a single placement does not retract it, as the
    /// path before the fold did not.
    Unreadable,
    /// The trader is mid-gesture: a placement would push its anchor onto
    /// their draft or replace it, so nothing is touched.
    DraftInProgress,
}

/// An edit the window asks the drawings to make.
#[derive(Debug)]
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
    /// Place one finished mark through the door a click uses.
    Place(Placement),
    /// Place every mark or none, as one undo step: when one fails, the
    /// marks, undo, redo and selection return to where the intent began.
    PlaceAll(Vec<Placement>),
    /// Remove the mark `id`, and only if an operator other than the trader
    /// placed it: work done by hand is never discarded this way.
    RemoveAuthored(DrawingId),
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
    /// A mark is placed and selected; `label` is what the object list shows.
    Placed { id: DrawingId, label: String },
    /// Placement `index` of the intent failed. A batch retracts everything it
    /// placed; a single placement that is `Unreadable` leaves its mark.
    NotPlaced {
        index: usize,
        failure: PlacementFailure,
    },
    /// An operator's mark is gone: whatever rode it has to be let go.
    Removed { id: DrawingId },
    /// The mark was drawn by the trader's hand, and stays.
    RemovalRefused { id: DrawingId },
}

impl DrawingEffect {
    /// Whether any of `effects` took marks that something may have ridden:
    /// the window sweeps whatever lost its mark.
    pub fn any_strand_riders(effects: &[Self]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Self::HistoryReplaced | Self::Removed { .. }))
    }
}

/// Fold `intent` into `store`, answering the effects the window runs.
pub fn update(store: &mut Drawings<DrawingTool>, intent: DrawingIntent) -> Vec<DrawingEffect> {
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
        DrawingIntent::DeleteSelected { force } => delete_selected(store, force),
        DrawingIntent::Place(_) | DrawingIntent::PlaceAll(_) if store.draft().is_some() => {
            vec![DrawingEffect::NotPlaced {
                index: 0,
                failure: PlacementFailure::DraftInProgress,
            }]
        }
        DrawingIntent::Place(placement) => vec![placed(0, place(store, placement))],
        DrawingIntent::PlaceAll(placements) => place_all(store, placements),
        DrawingIntent::RemoveAuthored(id) => remove_authored(store, id),
    }
}

fn delete_selected(store: &mut Drawings<DrawingTool>, force: bool) -> Vec<DrawingEffect> {
    // Read the name before the object is gone: the trader has to know
    // *what* they lost to know whether they want it back.
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

fn placed(index: usize, outcome: Result<(DrawingId, String), PlacementFailure>) -> DrawingEffect {
    match outcome {
        Ok((id, label)) => DrawingEffect::Placed { id, label },
        Err(failure) => DrawingEffect::NotPlaced { index, failure },
    }
}

/// Every placement or none, as one undo step.
fn place_all(store: &mut Drawings<DrawingTool>, placements: Vec<Placement>) -> Vec<DrawingEffect> {
    let batch = store.begin_placements();
    let mut effects = Vec::with_capacity(placements.len());
    for (index, placement) in placements.into_iter().enumerate() {
        let effect = placed(index, place(store, placement));
        if matches!(effect, DrawingEffect::NotPlaced { .. }) {
            store.retract_placements(batch);
            return vec![effect];
        }
        effects.push(effect);
    }
    store.commit_placements(batch);
    effects
}

/// Feed every anchor through the click door, then stamp who placed the mark,
/// its name and its words.
fn place(
    store: &mut Drawings<DrawingTool>,
    placement: Placement,
) -> Result<(DrawingId, String), PlacementFailure> {
    let Placement {
        tool,
        points,
        opening,
        author,
        name,
        text,
    } = placement;
    let mut opening = Some(opening);
    let mut completed = false;
    for point in points {
        completed = store.place_with(tool, &DrawingBand::Price, point, |_| {
            opening.take().expect("one opening look per placement")
        });
    }
    if !completed {
        // Never leave a partial draft, which would block every later edit.
        store.cancel_draft();
        return Err(PlacementFailure::Incomplete);
    }
    let drawing = store.selected_mut().ok_or(PlacementFailure::Unreadable)?;
    drawing.author = author;
    drawing.name = name;
    if let Some(text) = text
        && drawing.tool.holds_text()
    {
        drawing.tool.set_inline_text(drawing.payload.as_mut(), text);
    }
    let id = drawing.id;
    let index = store.selected().unwrap_or_default();
    let label = store
        .items()
        .get(index)
        .map_or_else(String::new, |drawing| drawing.display_label(index));
    Ok((id, label))
}

fn remove_authored(store: &mut Drawings<DrawingTool>, id: DrawingId) -> Vec<DrawingEffect> {
    let Some(drawing) = store.items().iter().find(|drawing| drawing.id == id) else {
        return Vec::new();
    };
    if drawing.author.is_none() {
        return vec![DrawingEffect::RemovalRefused { id }];
    }
    store.remove_by_id(id);
    vec![DrawingEffect::Removed { id }]
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
    fn placing_over_the_traders_draft_is_refused_and_leaves_it_alone() {
        let mut store = Drawings::default();
        assert!(!store.place(line(), ChartPoint::at(0.0, 100.0)));
        let placement = || {
            placement(
                line(),
                vec![ChartPoint::at(1.0, 100.0), ChartPoint::at(2.0, 101.0)],
            )
        };
        let refused = [DrawingEffect::NotPlaced {
            index: 0,
            failure: PlacementFailure::DraftInProgress,
        }];
        assert_eq!(
            update(&mut store, DrawingIntent::Place(placement())),
            refused
        );
        assert_eq!(
            update(&mut store, DrawingIntent::PlaceAll(vec![placement()])),
            refused
        );
        assert!(store.draft().is_some(), "the hand-drawn draft survives");
        assert!(store.items().is_empty());
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

    fn placement(tool: DrawingTool, points: Vec<ChartPoint>) -> Placement {
        Placement {
            tool,
            points,
            opening: crate::new_drawing_from_defaults(&crate::NullPresetHost, tool),
            author: Some(DrawingAuthor {
                actor_kind: "agent".to_owned(),
                client_name: "assistant".to_owned(),
            }),
            name: None,
            text: None,
        }
    }

    fn level(price: f64) -> Placement {
        let tool = DrawingTool::by_id("horizontal-line").expect("registered");
        placement(tool, vec![ChartPoint::at(1.5, price)])
    }

    #[test]
    fn a_placement_lands_stamped_and_selected_and_says_its_label() {
        let mut store = lines(1);
        let text = DrawingTool::by_id("text").expect("registered");
        let mut note = placement(text, vec![ChartPoint::at(2.5, 105.0)]);
        note.name = Some("absorption".to_owned());
        note.text = Some("watch the bid".to_owned());
        let effects = update(&mut store, DrawingIntent::Place(note));
        let placed = &store.items()[1];
        assert_eq!(
            effects,
            [DrawingEffect::Placed {
                id: placed.id,
                label: "absorption".to_owned()
            }]
        );
        assert_eq!(store.selected(), Some(1));
        assert_eq!(
            placed
                .author
                .as_ref()
                .map(|author| author.client_name.as_str()),
            Some("assistant")
        );
        assert_eq!(
            placed.tool.inline_text(placed.payload.as_ref()),
            Some("watch the bid")
        );
    }

    #[test]
    fn a_placement_short_of_anchors_leaves_no_draft_and_says_so() {
        let mut store = lines(1);
        let effects = update(
            &mut store,
            DrawingIntent::Place(placement(line(), vec![ChartPoint::at(1.0, 100.0)])),
        );
        assert_eq!(
            effects,
            [DrawingEffect::NotPlaced {
                index: 0,
                failure: PlacementFailure::Incomplete
            }]
        );
        assert!(
            store.draft().is_none(),
            "a partial draft would block the hand"
        );
        assert_eq!(store.items().len(), 1);
    }

    #[test]
    fn placing_all_is_one_undo_step_and_a_failure_keeps_nothing() {
        let mut store = lines(1);
        let depth = store.undo_depth();
        let effects = update(
            &mut store,
            DrawingIntent::PlaceAll(vec![level(101.0), level(99.0)]),
        );
        assert!(matches!(
            effects.as_slice(),
            [DrawingEffect::Placed { .. }, DrawingEffect::Placed { .. }]
        ));
        assert_eq!(store.undo_depth(), depth + 1, "one step for both");
        let before = store.items().to_vec();
        let short = placement(line(), vec![ChartPoint::at(1.0, 100.0)]);
        let effects = update(
            &mut store,
            DrawingIntent::PlaceAll(vec![level(98.0), short]),
        );
        assert_eq!(
            effects,
            [DrawingEffect::NotPlaced {
                index: 1,
                failure: PlacementFailure::Incomplete
            }]
        );
        assert_eq!(store.items(), &before[..], "no orphan first level");
        assert_eq!(store.undo_depth(), depth + 1, "no step brings it back");
    }

    #[test]
    fn removal_takes_an_operators_mark_and_refuses_the_traders() {
        let mut store = lines(1);
        let hand = store.items()[0].id;
        update(&mut store, DrawingIntent::Place(level(100.0)));
        let authored = store.items()[1].id;
        assert_eq!(
            update(&mut store, DrawingIntent::RemoveAuthored(hand)),
            [DrawingEffect::RemovalRefused { id: hand }]
        );
        assert_eq!(
            update(&mut store, DrawingIntent::RemoveAuthored(authored)),
            [DrawingEffect::Removed { id: authored }]
        );
        assert_eq!(store.items().len(), 1, "the hand-drawn mark stays");
        assert_eq!(
            update(&mut store, DrawingIntent::RemoveAuthored(authored)),
            []
        );
    }
}
