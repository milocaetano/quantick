//! Which panes an indicator edit on one pane reaches, and what a saved set
//! puts on a fresh pane, decided over plain addresses.
//!
//! Every pane showing a layout shows its whole indicator set, addressed by
//! entry index: the slot at index 2 on one pane is the slot at index 2 on
//! every other. An edit therefore writes the layout entry once and reaches
//! the slot at the same index on every other pane of the layout. The caller
//! owns the panes and applies the returned actions; nothing here touches one.

use crate::indicator_document::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::layout_document::LayoutId;

/// One indicator edit, in the form the layout entry records.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutEdit {
    /// A new indicator of this kind at the edited index. Its inputs start
    /// empty — "the declared defaults" — until the trader commits some.
    Add(SavedKind),
    /// The entry at the edited index goes.
    Remove,
    /// The eye toggle.
    Hidden(bool),
    /// Committed inputs, never a live preview.
    Inputs(Vec<SavedInput>),
    /// The style layer, plot by plot.
    Styles(Vec<SavedPlotStyle>),
    /// The per-indicator mouse guide.
    MouseVerticalLine(bool),
}

/// One pane an edit may reach: its address, the layout it shows, and its
/// layout slots in layout order.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneSlots<P, S> {
    pub pane: P,
    pub layout: Option<LayoutId>,
    pub slots: Vec<S>,
}

/// What one other pane of the edited layout must do.
#[derive(Debug, Clone, PartialEq)]
pub enum MirrorAction<P, S> {
    /// Put an indicator of this kind on the pane: the edit was an add.
    Add { pane: P, kind: SavedKind },
    /// Apply the same edit to the pane's slot at the edited index.
    Edit { pane: P, slot: S },
}

/// Where `slot` sits in its pane's layout order, or `None` for a slot the
/// layout does not carry — an operator's, or one a validation hook added.
#[must_use]
pub fn layout_index<S: PartialEq>(slots: &[S], slot: &S) -> Option<usize> {
    slots.iter().position(|candidate| candidate == slot)
}

/// Record `edit` at `index` of a layout's entries, then return what every
/// pane on `layout` but `origin` must do: an add puts the kind on each, any
/// other edit reaches each pane's slot at `index`, and a pane with no slot
/// there is left alone.
#[must_use]
pub fn mirror<P: PartialEq, S: Copy>(
    entries: Option<&mut Vec<SavedIndicator>>,
    (layout, index): (LayoutId, usize),
    origin: &P,
    edit: LayoutEdit,
    panes: Vec<PaneSlots<P, S>>,
) -> Vec<MirrorAction<P, S>> {
    let added = match &edit {
        LayoutEdit::Add(kind) => Some(kind.clone()),
        _ => None,
    };
    if let Some(entries) = entries {
        record(entries, index, edit);
    }
    panes
        .into_iter()
        .filter(|pane| pane.layout == Some(layout) && pane.pane != *origin)
        .filter_map(|pane| match &added {
            Some(kind) => Some(MirrorAction::Add {
                pane: pane.pane,
                kind: kind.clone(),
            }),
            None => {
                let slot = *pane.slots.get(index)?;
                Some(MirrorAction::Edit {
                    pane: pane.pane,
                    slot,
                })
            }
        })
        .collect()
}

/// Write one edit into the entry at `index`. An add past the end appends;
/// an edit at an index the layout does not have writes nothing.
fn record(entries: &mut Vec<SavedIndicator>, index: usize, edit: LayoutEdit) {
    if let LayoutEdit::Add(kind) = edit {
        let entry = SavedIndicator {
            kind,
            hidden: false,
            mouse_vertical_line: false,
            inputs: Vec::new(),
            plot_styles: Vec::new(),
        };
        entries.insert(index.min(entries.len()), entry);
        return;
    }
    if matches!(edit, LayoutEdit::Remove) {
        if index < entries.len() {
            entries.remove(index);
        }
        return;
    }
    let Some(entry) = entries.get_mut(index) else {
        return;
    };
    match edit {
        LayoutEdit::Hidden(hidden) => entry.hidden = hidden,
        LayoutEdit::Inputs(inputs) => entry.inputs = inputs,
        LayoutEdit::Styles(styles) => entry.plot_styles = styles,
        LayoutEdit::MouseVerticalLine(enabled) => entry.mouse_vertical_line = enabled,
        LayoutEdit::Add(_) | LayoutEdit::Remove => {}
    }
}

/// How a saved entry's inputs bind on a fresh slot.
#[derive(Debug, Clone, PartialEq)]
pub enum SavedInputs<V> {
    /// Nothing saved: the declared defaults.
    Defaults,
    /// Every saved value read: bind them all.
    Bind(Vec<V>),
    /// A saved value could not be read, so the declared defaults stand as a
    /// whole — half a set would shift every later input onto the wrong one.
    Dropped { saved: usize, readable: usize },
}

/// What a fresh pane does to show one saved entry, in the order it does it:
/// add the kind, bind the inputs, then queue the hide, the mouse guide and the
/// style until the first rebuild gives them a view.
#[derive(Debug, Clone, PartialEq)]
pub struct Materialise<'a, V> {
    pub kind: &'a SavedKind,
    pub inputs: SavedInputs<V>,
    pub hidden: bool,
    pub mouse_vertical_line: bool,
    /// The style layer to queue; `None` for an indicator nobody styled.
    pub plot_styles: Option<&'a [SavedPlotStyle]>,
}

/// The saved set, entry by entry, as a fresh pane puts it on: `read` turns a
/// saved input into the runtime value, or `None` when it cannot.
pub fn materialise<'a, V>(
    set: &'a [SavedIndicator],
    read: impl Fn(&SavedInput) -> Option<V>,
) -> Vec<Materialise<'a, V>> {
    set.iter()
        .map(|entry| Materialise {
            kind: &entry.kind,
            inputs: saved_inputs(&entry.inputs, &read),
            hidden: entry.hidden,
            mouse_vertical_line: entry.mouse_vertical_line,
            plot_styles: (!entry.plot_styles.is_empty()).then_some(entry.plot_styles.as_slice()),
        })
        .collect()
}

/// How `inputs` bind; see [`SavedInputs`].
pub fn saved_inputs<V>(
    inputs: &[SavedInput],
    read: impl Fn(&SavedInput) -> Option<V>,
) -> SavedInputs<V> {
    let values: Vec<V> = inputs.iter().filter_map(read).collect();
    if inputs.is_empty() {
        SavedInputs::Defaults
    } else if values.len() == inputs.len() {
        SavedInputs::Bind(values)
    } else {
        SavedInputs::Dropped {
            saved: inputs.len(),
            readable: values.len(),
        }
    }
}
