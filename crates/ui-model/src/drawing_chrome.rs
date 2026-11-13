//! Presentation state for the drawing chrome. Drawing rules remain in quantick-drawings.
#[path = "drawing_chrome/actions.rs"]
mod actions;
#[path = "drawing_chrome/effects.rs"]
mod effects;
#[path = "drawing_chrome/inline.rs"]
mod inline;
#[path = "drawing_chrome/inspector.rs"]
pub mod inspector;
#[path = "drawing_chrome/presets.rs"]
mod presets;
#[path = "drawing_chrome/quick_range.rs"]
pub mod quick_range;
#[path = "drawing_chrome/scenario.rs"]
pub mod scenario;
#[path = "drawing_chrome/types.rs"]
mod types;
pub use actions::{ActionInput, apply as apply_actions};
pub use effects::DrawingChromeAsk;
pub use inline::InlineEditor;
pub use presets::{PresetWrite, RecordingPresetHost};
pub use types::{DrawingEdit, InspectorActions, InspectorTab, SavedDefault};
pub type Drawing = quantick_drawings::Drawing<quantick_drawings::DrawingTool>;
/// The state the four pieces share, and the reason they are one surface.
///
/// Private to this module: the whole point of the member being one rather
/// than four is that nothing outside can reach across the subsystem.
#[derive(Default)]
pub struct Shared {
    /// Whether the floating inspector is open. Selecting a drawing no longer
    /// opens it: the context bar is what a selection raises, and the gear on
    /// that bar is the one thing that opens the full panel.
    pub open: bool,
    /// Whether the inspector is docked at the chart's side instead.
    pub pinned: bool,
    /// The selection the last automatic placement was computed for.
    pub last_selection: Option<usize>,
    /// A locked object's delete was asked for and is awaiting its answer.
    pub delete_confirm: bool,
    /// The pre-edit copy held across an in-flight edit gesture.
    pub edit_baseline: Option<DrawingEdit>,
}

#[derive(Default)]
pub struct Manager {
    pub open: bool,
    pub was_open: bool,
    pub confirm_delete_all: bool,
}
#[derive(Default)]
pub struct Core {
    pub shared: Shared,
    pub manager: Manager,
    pub inline: InlineEditor,
    pub pending_text_edit: bool,
    pub pending_open_settings: bool,
    pub clipboard: Option<Drawing>,
    pub paste_count: u32,
    pub menu_ask: Option<(u64, DrawingChromeAsk)>,
}
#[derive(Default)]
pub struct State {
    pub scenario: scenario::State,
    pub core: Core,
    pub inspector: inspector::State,
    pub quick_range: quick_range::State,
}
pub enum Intent {
    Scenario(scenario::Intent),
    Actions(ActionInput),
    Ask(DrawingChromeAsk),
    QuickRange(quick_range::Intent),
    ManagerOpen(bool),
    ManagerFrame {
        visible: bool,
    },
    ManagerAnswer {
        open: bool,
        confirm_delete_all: bool,
    },
    ConfirmDelete(bool),
    RequestTextEdit,
    CarrySelection,
    InspectorOpen(bool),
    OpenSettings,
    InspectorPinned(bool),
    InspectorTab(InspectorTab),
    InspectorPosition(Option<[f32; 2]>),
    InspectorMoved(bool),
    InspectorPrepare {
        index: usize,
        width: Option<f32>,
    },
    InspectorPrologue(Option<usize>),
    InspectorClaim(usize),
    InspectorReset(Option<[f32; 2]>),
    InspectorDrag {
        start: Option<[f32; 2]>,
        delta: [f32; 2],
    },
    InspectorPositionDirty,

    InspectorPinTouched(bool),
    InspectorPlace([f32; 2]),
    InspectorRestore(Option<[f32; 2]>),
    TakePositionDirty,
    Copy(Box<Drawing>),
    Paste,
    DropEditBaseline {
        tab: u64,
        side: quantick_chart_interaction::pane::PaneSide,
    },
    BeginInline(DrawingEdit),
    BeginPendingInline(Option<DrawingEdit>),
    ObserveInline {
        tab: u64,
        side: quantick_chart_interaction::pane::PaneSide,
        selected: Option<usize>,
        visible: bool,
        editable: bool,
    },
    EndInline,
    OpenEditGesture(DrawingEdit),
    Menu {
        pane: u64,
        ask: DrawingChromeAsk,
    },
    TakeMenu(u64),
    SelectionChanged {
        changed: bool,
    },
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = DrawingChromeAsk;
    fn update(state: &mut State, intent: Intent) -> DrawingChromeAsk {
        let mut effects = DrawingChromeAsk::default();
        match intent {
            Intent::Scenario(intent) => return scenario::update(state, intent),
            Intent::Ask(ask) => return ask,
            Intent::QuickRange(intent) => {
                return quick_range::update(&mut state.quick_range, intent);
            }
            Intent::Actions(input) => {
                return actions::apply(&mut state.core, &mut state.inspector, input);
            }
            Intent::ManagerOpen(open) => state.core.manager.open = open,
            Intent::ManagerFrame { visible } => state.core.manager.was_open = visible,
            Intent::ManagerAnswer {
                open,
                confirm_delete_all,
            } => {
                state.core.manager.open = open;
                state.core.manager.confirm_delete_all = confirm_delete_all;
            }
            Intent::ConfirmDelete(confirm) => state.core.shared.delete_confirm = confirm,
            Intent::RequestTextEdit => state.core.pending_text_edit = true,
            Intent::CarrySelection => state.core.pending_open_settings |= state.core.shared.open,
            Intent::OpenSettings => {
                state.core.shared.open = true;
                state.core.shared.last_selection = None;
            }
            Intent::InspectorOpen(open) => state.core.shared.open = open,
            Intent::InspectorPinned(pinned) => state.core.shared.pinned = pinned,
            Intent::InspectorTab(tab) => state.inspector.tab = tab,
            Intent::InspectorPosition(position) => state.inspector.pos = position,
            Intent::InspectorPrepare { index, width } => {
                state.inspector.prepared =
                    state
                        .inspector
                        .prepare_floating(&mut state.core.shared, index, width)
            }
            Intent::InspectorPrologue(index) => {
                effects.commit_edit_gesture = inspector::prologue(&mut state.core.shared, index)
            }
            Intent::InspectorClaim(index) => state.core.shared.last_selection = Some(index),
            Intent::InspectorReset(placed) => state.inspector.reset_automatic(placed),
            Intent::InspectorDrag { start, delta } => state.inspector.drag(start, delta),
            Intent::InspectorPositionDirty => state.inspector.position_dirty = true,
            Intent::InspectorMoved(moved) => state.inspector.moved = moved,
            Intent::InspectorPinTouched(touched) => state.inspector.pin_touched = touched,
            Intent::InspectorPlace(position) => state.inspector.place_by_hand(position),
            Intent::InspectorRestore(position) => state.inspector.restore_position(position),
            Intent::TakePositionDirty => {
                effects.inspector_position_dirty = state.inspector.take_position_dirty()
            }
            Intent::Copy(drawing) => {
                state.core.clipboard = Some(*drawing);
                state.core.paste_count = 0;
            }
            Intent::Paste => {
                if let Some(drawing) = state.core.clipboard.as_ref() {
                    state.core.paste_count = state.core.paste_count.saturating_add(1);
                    effects.clipboard_paste =
                        Some((Box::new(drawing.clone()), state.core.paste_count));
                }
            }
            Intent::DropEditBaseline { tab, side } => {
                if state
                    .core
                    .shared
                    .edit_baseline
                    .as_ref()
                    .is_some_and(|edit| edit.tab == tab && edit.side == side)
                {
                    state.core.shared.edit_baseline = None;
                }
            }
            Intent::BeginInline(edit) => {
                effects.inline_started =
                    state
                        .core
                        .inline
                        .begin(edit.tab, edit.side, edit.index, &edit.before)
            }
            Intent::BeginPendingInline(edit) => {
                if std::mem::take(&mut state.core.pending_text_edit)
                    && let Some(edit) = edit
                {
                    effects.inline_started =
                        state
                            .core
                            .inline
                            .begin(edit.tab, edit.side, edit.index, &edit.before);
                }
            }
            Intent::ObserveInline {
                tab,
                side,
                selected,
                visible,
                editable,
            } => {
                if state.core.inline.target().is_some_and(|target| {
                    target != (tab, side, selected.unwrap_or(usize::MAX)) || !visible || !editable
                }) {
                    effects.record_inline_edit = state.core.inline.end().map(Box::new);
                    effects.content_editing_changed = true;
                }
            }
            Intent::EndInline => effects.record_inline_edit = state.core.inline.end().map(Box::new),
            Intent::OpenEditGesture(edit) => state.core.shared.edit_baseline = Some(edit),
            Intent::Menu { pane, ask } => match &mut state.core.menu_ask {
                Some((queued, earlier)) if *queued == pane => earlier.merge(ask),
                _ => state.core.menu_ask = Some((pane, ask)),
            },
            Intent::TakeMenu(pane) => {
                if let Some((queued, ask)) = state.core.menu_ask.take()
                    && queued == pane
                {
                    return ask;
                }
            }
            Intent::SelectionChanged { changed } => {
                if changed {
                    state.core.shared.open = false;
                }
                if std::mem::take(&mut state.core.pending_open_settings) {
                    state.core.shared.open = true;
                    state.core.shared.last_selection = None;
                }
            }
        }
        effects
    }
}

#[cfg(test)]
#[path = "drawing_chrome/preset_tests.rs"]
mod preset_tests;

#[cfg(test)]
#[path = "drawing_chrome/tests.rs"]
mod tests;
