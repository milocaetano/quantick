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
    /// The value axis the points' prices are on: the candles', or one
    /// indicator pane's. A band the tool cannot hold
    /// ([`DrawingTool::holds_band`]) is refused, never moved: a CVD value
    /// placed as a price would be a level on the wrong axis.
    pub band: DrawingBand,
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
    /// The tool cannot hold the band asked for: a price-only tool on an
    /// indicator pane. Nothing is touched.
    BandRefused,
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
    /// Show the selected mark on every chart of its market whatever layout,
    /// or on its own layout's alone ([`Drawings::set_all_windows_at`]).
    ToggleAllWindowsSelected,
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
        DrawingIntent::ToggleAllWindowsSelected => {
            if let Some(index) = store.selected() {
                let on = store.in_all_windows(index);
                store.set_all_windows_at(index, !on);
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
        band,
        opening,
        author,
        name,
        text,
    } = placement;
    if !tool.holds_band(&band) {
        return Err(PlacementFailure::BandRefused);
    }
    // A tool that still accepts an older, longer anchor count completes at
    // its new one through the click door; the extra anchors then join the
    // object as they were given, instead of starting a second draft.
    // A count the tool does not accept never reaches the store: too many
    // would open a second draft with no opening look left to give it.
    let mut points = points;
    let required = tool.required_points();
    if required > 0 && !tool.accepts_points(points.len()) {
        return Err(PlacementFailure::Incomplete);
    }
    let legacy_tail = points.split_off(required.min(points.len()));
    let mut opening = Some(opening);
    let mut completed = false;
    for point in points {
        completed = store.place_with(tool, &band, point, |_| {
            opening.take().expect("one opening look per placement")
        });
    }
    if !completed {
        // Never leave a partial draft, which would block every later edit.
        store.cancel_draft();
        return Err(PlacementFailure::Incomplete);
    }
    let drawing = store.selected_mut().ok_or(PlacementFailure::Unreadable)?;
    drawing.points.extend(legacy_tail);
    drawing.author = author;
    // A placement that names nothing keeps the name the store gave it.
    if name.is_some() {
        drawing.name = name;
    }
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
#[path = "update_tests.rs"]
mod tests;
