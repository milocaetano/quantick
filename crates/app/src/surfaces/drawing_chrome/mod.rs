//! Drawing chrome view and adapters for the catalog-owned headless model.
//!
//! One model/view instance serves both the pinned inspector before canvas
//! and the floating chrome after canvas. Each pass reads current host facts;
//! UI transitions use typed intents and domain mutations return host effects.
//! Subviews flush effects in their existing order, sharing one model state.
//! Geometry and painting stay here; drawing rules remain in quantick-drawings.

mod adapter;
pub(crate) use adapter::DrawingChromeExt;
pub(crate) mod context_bar;
// `demo` and `launch` are drawing-harness scenarios, gated inside their own
// files (`#![cfg]`) so each file says what it is.
pub(crate) mod demo;
pub(crate) mod inline_editor;
pub(crate) mod inspector;
pub(crate) mod launch;
pub(crate) mod manager;
pub(crate) mod object_row;
mod quick_range;
#[cfg(any(feature = "drawing-harness", test))]
pub(crate) use launch::DrawingChromeLaunch;
#[cfg(any(feature = "quick-range-harness", test))]
pub(crate) use quick_range::{QUICK_RANGE_HOOKS, QuickRangeLaunch};

#[cfg(test)]
pub(crate) use quick_range::Action as QuickRangeAction;
pub(crate) use quick_range::{
    ACTION_COUNT as QUICK_RANGE_ACTION_COUNT, ActionUi as QuickRangeActionUi,
    Control as QuickRangeControl, Owner as QuickRangeOwner, QuickRange,
};

use eframe::egui;
use quantick_ui_model::drawing_chrome::Intent;

use crate::bands::BandLabel;
use crate::drawings::{Drawing, PresetHost};
use crate::pane::PaneSide;
use crate::toolrail::ToolboxDock;

/// Initial position of the selected-drawing inspector, before
/// [`inspector::placement`] has a size and a bbox to place it from.
pub(crate) const DRAWING_INSPECTOR_DEFAULT_POSITION: egui::Pos2 = egui::pos2(90.0, 120.0);
/// Inspector width bounds (UX spec: resizable between 300 and 440 px).
pub(crate) const INSPECTOR_MIN_WIDTH_PX: f32 =
    quantick_ui_model::drawing_chrome::inspector::MIN_WIDTH_PX;
/// See [`INSPECTOR_MIN_WIDTH_PX`].
const INSPECTOR_MAX_WIDTH_PX: f32 = 440.0;
/// Default inspector width for the shipped tools (the spec reserves 360 px
/// for the Fib level editor).
const INSPECTOR_DEFAULT_WIDTH_PX: f32 = 320.0;
/// Default inspector width for tools that mount a level editor tab.
const INSPECTOR_LEVELS_WIDTH_PX: f32 = 360.0;
/// Gap kept between the inspector and the selected object's bounding box.
const INSPECTOR_OBJECT_GAP_PX: f32 = quantick_ui_model::drawing_chrome::inspector::OBJECT_GAP_PX;
/// Assumed inspector height for placement before its first frame reports one.
const INSPECTOR_FALLBACK_HEIGHT_PX: f32 = 280.0;
/// Below this chart width a fresh selection opens the inspector pinned —
/// there is no floating position that would not crowd the geometry. Stops
/// applying once the user touches the pin either way.
#[cfg(test)]
pub(crate) const INSPECTOR_AUTO_PIN_CHART_WIDTH_PX: f32 =
    quantick_ui_model::drawing_chrome::inspector::AUTO_PIN_CHART_WIDTH_PX;
/// Height of the floating inspector's custom title bar.
const INSPECTOR_TITLE_HEIGHT_PX: f32 = 28.0;
/// Title-bar paint metrics: leading padding, the title column when the grip
/// glyph precedes it, and the two font sizes.
const INSPECTOR_TITLE_PAD_X_PX: f32 = 2.0;
const INSPECTOR_TITLE_TEXT_X_PX: f32 = 18.0;
const INSPECTOR_TITLE_GRIP_GLYPH_PX: f32 = 14.0;
const INSPECTOR_TITLE_TEXT_PX: f32 = 13.0;
/// Gap between the object manager and the rail edge it opens beside.
pub(crate) const DRAWING_MANAGER_GAP_PX: f32 = 12.0;
/// Where the object manager first opens: under the toolbox's home corner.
const DRAWING_MANAGER_DEFAULT_POSITION: egui::Pos2 = egui::pos2(70.0, 140.0);
/// Dragging the price field across the whole visible range takes this many
/// steps, whatever the symbol's price magnitude.
const PRICE_DRAG_STEPS: f64 = 200.0;
/// DragValue speed of bar-index coordinates, in bars per drag point.
const BAR_DRAG_SPEED: f64 = 0.25;

/// The egui id both inspector hosts and the placement rule agree on. One
/// literal, because a placement that measured a different area than the one
/// it moves would be silently wrong.
const INSPECTOR_AREA_ID: &str = "drawing_inspector";
/// Likewise for the manager, whose opening position is measured from the area
/// egui remembers.
const MANAGER_AREA_ID: &str = "drawing_manager";

pub(crate) use quantick_ui_model::drawing_chrome::{
    DrawingEdit, InspectorActions, InspectorTab, SavedDefault,
};

/// The selected object, as the chrome reads it: borrowed, never cloned on the
/// way in.
///
/// The inspector clones it — it has to, because it edits a copy — but the
/// context bar and the inline editor read a handful of fields off a selection
/// that may be a 512-anchor pencil stroke, on every frame something is
/// selected. Handing them an owned `Drawing` would put that clone on the
/// chart's idle path.
pub(crate) struct SelectedDrawing<'a> {
    pub index: usize,
    pub drawing: &'a Drawing,
}

/// One row of the object manager, built by the host while the window is open.
///
/// Owned strings, and that is the point: the row's facts come from the pane,
/// the band registry and the drawing itself, and the manager would otherwise
/// need all three to assemble them. Built only while the window is on screen,
/// like `open_markets` — a per-frame allocation for a surface nobody can see
/// is exactly the cost this port was supposed to make visible rather than
/// hide.
pub(crate) struct ManagerRow {
    pub name: String,
    pub selected: bool,
    pub locked: bool,
    pub hidden: bool,
    pub shared: bool,
    pub off_series: bool,
    pub foreign_market: bool,
    /// Where a migration merged this object from, in the list's words.
    pub origin: Option<String>,
    pub author: Option<String>,
    pub band: BandLabel,
}

/// What the drawing chrome needs from the application.
///
/// Nested inside [`super::SurfaceEnv`] rather than flattened into it: this is
/// one subsystem's slice, and eleven more fields on the shared env would move
/// the loose-field problem out one level instead of solving it.
pub(crate) struct DrawingEnv<'a> {
    pub pane_id: u64,
    /// The selected object on the pane the chrome speaks for, if any.
    pub selected: Option<SelectedDrawing<'a>>,
    /// That pane's chart rectangle, or `None` before it has been laid out.
    pub chart_area: Option<egui::Rect>,
    /// Selected indicator band's body, excluding its header. Parked chrome
    /// keeps its existing pane-relative placement instead.
    pub automatic_bar_area: Option<egui::Rect>,
    /// The *focused* pane's rectangle, which is not always the one above: a
    /// shared mark can be selected from the chart it is mirrored on. The
    /// object manager opens beside the toolbox button that opened it, and that
    /// button is on the focused pane.
    pub focused_chart_area: Option<egui::Rect>,
    /// Where the live lane begins. Every popup keeps clear of it: that strip
    /// is where the price the trader is reading is being formed.
    pub candles_right: Option<f32>,
    /// Measured footprint of the stacked flow and indicator legends.
    pub legends: Option<egui::Rect>,
    /// The visible price range, which is what makes a coordinate drag move at
    /// the same speed on a two-dollar symbol and a hundred-thousand one.
    pub auto_range: Option<(f64, f64)>,
    /// Where the selected object is painted, in screen points, already
    /// expanded by the anchor radius and through the tool's own
    /// `painted_bounds`. Projected by the host, which owns the viewport and
    /// the price scale, and only when something is selected.
    pub selected_bbox: Option<egui::Rect>,
    /// Which band the selected object is on, for the inspector's title. `None`
    /// on the price band, where a suffix on every object would be noise.
    pub selected_band: Option<String>,
    /// The **stable id** of the tab on screen, and the pane the chrome speaks
    /// for. An inline edit outlives a tab switch and must not write into
    /// whatever now sits at its index.
    pub tab: u64,
    pub side: PaneSide,
    /// Whether a drawing tool is armed. The trader is drawing, not editing:
    /// the context bar stands down so it cannot eat the click that places the
    /// next object.
    pub drawing_tool_armed: bool,
    /// Where the toolbox sits, which is the corner the manager opens beside.
    pub toolbox_dock: ToolboxDock,
    /// How many objects on this chart an assistant placed, for the one
    /// gesture that takes them all back.
    pub authored_objects: usize,
    /// Every object, as rows — empty unless the manager is open.
    pub manager_rows: &'a [ManagerRow],
    /// The saved tool defaults and named presets. Read-only here; the writes
    /// go back through [`DrawingChromeAsk::presets`].
    pub presets: &'a dyn PresetHost,
}

impl DrawingEnv<'_> {
    /// The selection's index, when there is one.
    fn selected_index(&self) -> Option<usize> {
        self.selected.as_ref().map(|selected| selected.index)
    }
}

pub(crate) use quantick_ui_model::drawing_chrome::RecordingPresetHost;

pub(crate) use quantick_ui_model::drawing_chrome::DrawingChromeAsk;

/// Toolkit mechanics owned by the catalog alongside the headless state.
#[derive(Default)]
pub(crate) struct View {
    quick_range: quick_range::View,
    inspector: inspector::View,
    bar: context_bar::ContextBarState,
    #[cfg(test)]
    manager_rects: Vec<(usize, &'static str, egui::Rect)>,
    #[cfg(any(feature = "drawing-harness", test))]
    rejected_tab: Option<String>,
}
/// Delivery stays with the adapter transaction; host operations return their effects.
pub(super) enum Session<'a> {
    Host(&'a mut quantick_ui_model::drawing_chrome::State),
    Paint(quantick_ui_model::UpdateSession<'a, quantick_ui_model::drawing_chrome::Model>),
}
impl Session<'_> {
    fn reborrow(&mut self) -> Session<'_> {
        match self {
            Self::Host(state) => Session::Host(state),
            Self::Paint(session) => Session::Paint(session.reborrow()),
        }
    }
    pub fn state(&self) -> &quantick_ui_model::drawing_chrome::State {
        match self {
            Self::Host(state) => state,
            Self::Paint(session) => session.state(),
        }
    }
    pub fn emit(&mut self, intent: quantick_ui_model::drawing_chrome::Intent) -> DrawingChromeAsk {
        use quantick_ui_model::Model;
        match self {
            Self::Host(state) => quantick_ui_model::drawing_chrome::Model::update(state, intent),
            Self::Paint(session) => {
                session.update(intent);
                DrawingChromeAsk::default()
            }
        }
    }
}
/// A temporary borrow of the catalog owner, never a second state cache.
pub(crate) struct Chrome<'a> {
    session: Session<'a>,
    view: &'a mut View,
}
impl std::ops::Deref for Chrome<'_> {
    type Target = quantick_ui_model::drawing_chrome::Core;
    fn deref(&self) -> &Self::Target {
        &self.session.state().core
    }
}
pub(crate) type DrawingChromeSurface = Owner;
pub(crate) type Owner = super::Adapter<quantick_ui_model::drawing_chrome::Model, View>;
pub(crate) fn borrow(owner: &mut Owner) -> Chrome<'_> {
    Chrome {
        session: Session::Host(&mut owner.model_state),
        view: &mut owner.view,
    }
}
impl Chrome<'_> {
    fn state(&self) -> &quantick_ui_model::drawing_chrome::State {
        self.session.state()
    }
    fn emit(&mut self, intent: quantick_ui_model::drawing_chrome::Intent) -> DrawingChromeAsk {
        self.session.emit(intent)
    }
    pub fn quick_range(&mut self) -> quick_range::QuickRange<'_> {
        quick_range::QuickRange::new(self.session.reborrow(), &mut self.view.quick_range)
    }
}
impl super::View<quantick_ui_model::drawing_chrome::Model> for View {
    fn paint_borrowed(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<
            '_,
            quantick_ui_model::drawing_chrome::Model,
        >,
        target: &mut super::ViewContext<'_>,
        slot: quantick_ui_model::Slot,
        env: Option<&mut super::BorrowedSurfaceEnv<'_>>,
    ) -> super::SurfaceResponse {
        if slot == quantick_ui_model::Slot::PinnedInspector
            && let Some(super::BorrowedSurfaceEnv::Drawings(env)) = env
        {
            self.paint_pass(session.reborrow(), target.context(), env, None, false);
        }
        super::SurfaceResponse::default()
    }
    fn id(&self) -> &'static str {
        "drawing_chrome"
    }
    fn paint(
        &mut self,
        _: &quantick_ui_model::drawing_chrome::State,
        _: &mut super::ViewContext<'_>,
        _: Option<&super::SurfaceEnv<'_>>,
    ) -> Option<quantick_ui_model::drawing_chrome::Intent> {
        None
    }
}
pub(crate) fn effects(effects: DrawingChromeAsk) -> super::SurfaceResponse {
    if effects == DrawingChromeAsk::default() {
        return super::SurfaceResponse::default();
    }
    super::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::DrawingChrome(effects)],
        ..Default::default()
    }
}
impl Chrome<'_> {
    /// The view owner selects its rendering pass; the window supplies facts only.
    pub fn draw_pass(
        &mut self,
        ctx: &egui::Context,
        env: &DrawingEnv<'_>,
        current_owner: Option<QuickRangeOwner>,
        floating: bool,
    ) -> DrawingChromeAsk {
        if floating {
            self.draw_floating(ctx, env, current_owner)
        } else {
            self.draw_pinned_panel(ctx, env)
        }
    }
    // ---- What the host reads --------------------------------------------

    /// Whether anything here is on screen or about to be, so the host knows
    /// whether the env slices only an open surface reads are worth building.
    ///
    /// The manager alone, because it is the only piece whose slice costs an
    /// allocation and whose visibility does not follow the selection: the
    /// rest read borrowed fields the host has in hand either way.
    pub fn manager_open(&self) -> bool {
        self.manager.open
    }

    /// The same answer with the tab and pane that make it unambiguous: an
    /// index alone names a different object on every pane.
    pub fn content_editing_target(&self) -> Option<(u64, PaneSide, usize)> {
        self.inline.target()
    }

    // ---- What the host commands -----------------------------------------

    /// Replace the internal clipboard with this drawing snapshot.
    pub fn copy_drawing(&mut self, drawing: &Drawing) {
        self.emit(Intent::Copy(Box::new(drawing.clone())));
    }

    /// Return the copied snapshot and its next one-based paste count.
    ///
    /// The count makes repeated pastes advance instead of stacking at the
    /// same offset. Cloning is paid only for the explicit paste gesture.
    pub fn next_drawing_paste(&mut self) -> Option<(Drawing, u32)> {
        self.emit(Intent::Paste)
            .clipboard_paste
            .map(|(drawing, count)| (*drawing, count))
    }

    /// Open the object manager, or shut it.
    pub fn set_manager_open(&mut self, open: bool) {
        self.emit(Intent::ManagerOpen(open));
    }

    /// Carry an open inspector across a selection the host is about to make.
    ///
    /// One method rather than the line copied into each hook, because the
    /// omission is silent: the demo that forgets it produces a screenshot
    /// that looks merely uninteresting, and that is how three of these hooks
    /// came to disagree about it.
    #[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
    pub fn carry_across_selection(&mut self) {
        self.emit(Intent::CarrySelection);
    }

    /// Ask for the caret on the next frame — the placement that just happened
    /// wants its note typed.
    pub fn request_text_edit(&mut self) {
        self.emit(Intent::RequestTextEdit);
    }

    /// The host placed the note the `QUANTICK_TEXT_NOTE` hook asked for.
    ///
    /// The ask is repeated every frame until this lands, because at launch
    /// there is no chart to place against and no bar to place at: the hook
    /// waits for a laid-out pane rather than firing once into an empty one
    /// and photographing nothing.
    #[cfg(any(feature = "drawing-harness", test))]
    pub fn note_text_note_placed(&mut self) {
        self.emit(Intent::Scenario(
            quantick_ui_model::drawing_chrome::scenario::Intent::NotePlaced(true),
        ));
    }

    /// Drop an undo baseline that describes this pane, rather than record it.
    ///
    /// Its object is about to be replaced by another set's, and an entry
    /// recorded against that would undo something the trader never did.
    pub fn drop_edit_baseline_on(&mut self, tab: u64, side: PaneSide) {
        self.emit(Intent::DropEditBaseline { tab, side });
    }

    /// Put the caret in a note now. The host owns the store, so it hands the
    /// object over; refused for one that holds no words or is locked, because
    /// an editor that opened and then dropped every keystroke would be worse
    /// than none.
    #[cfg(any(feature = "drawing-harness", test))]
    pub fn begin_inline_text_edit(
        &mut self,
        tab: u64,
        side: PaneSide,
        index: usize,
        drawing: &Drawing,
    ) -> bool {
        self.emit(Intent::BeginInline(DrawingEdit {
            tab,
            side,
            index,
            before: drawing.clone(),
        }))
        .inline_started
    }

    /// Commit the inline edit against its actual pane and synchronize its render suppression.
    /// Both ordinary close and layout-swap shells use this same drawing operation.
    pub(crate) fn commit_inline_text(
        &mut self,
        tabs: &mut crate::app::arrangement_host::ArrangementHost,
    ) {
        if let Some(edit) = self.end_inline_text_edit()
            && let Some(tab) = tabs.by_id_mut(edit.tab)
        {
            tab.pane_mut(edit.side)
                .drawings
                .record_edit_of(edit.index, edit.before);
        }
        self.sync_content_editing(tabs);
    }
    pub(crate) fn sync_content_editing(
        &self,
        tabs: &mut crate::app::arrangement_host::ArrangementHost,
    ) {
        let editing = self.content_editing_target();
        for (tab_id, tab) in tabs.iter_with_ids_mut() {
            let target = editing
                .filter(|(id, _, _)| *id == tab_id)
                .map(|(_, side, index)| (side, index));
            tab.set_content_editing(target);
        }
    }

    /// Close the editor, if one is open, and return its undo record.
    pub fn end_inline_text_edit(&mut self) -> Option<DrawingEdit> {
        self.emit(Intent::EndInline)
            .record_inline_edit
            .map(|edit| *edit)
    }

    /// Restore the position a workspace file remembered.
    pub fn restore_inspector_position(&mut self, remembered: Option<[f32; 2]>) {
        self.emit(Intent::InspectorRestore(remembered));
        self.view.inspector.max_width = None;
    }

    /// Draw the docked inspector.
    ///
    /// A second entry point rather than a branch inside [`Self::draw_floating`],
    /// because a `SidePanel` has to be declared *before* the central canvas —
    /// the canvas pays its width, and a panel declared after it would overlay
    /// the chart instead of docking beside it.
    pub fn draw_pinned_panel(
        &mut self,
        ctx: &egui::Context,
        env: &DrawingEnv<'_>,
    ) -> DrawingChromeAsk {
        let ask = inspector::draw_pinned(self, ctx, env);
        self.emit(Intent::Ask(ask))
    }

    /// Draw the four floating pieces, in the one order that is correct.
    ///
    /// Order matters *here* and nowhere else, which is part of why they are
    /// one member. The context bar runs first because it owns the selection
    /// reset that clears [`Shared::open`], and the inspector's placement rule
    /// reads what that reset leaves behind — the reverse order would place the
    /// panel against the previous selection for a frame.
    ///
    /// Between orders egui still decides what covers what: the bar and the
    /// editor name `Foreground`, the windows are ordinary `Middle` areas
    /// created on the frame they open. Nothing here relies on being called
    /// last.
    pub fn draw_floating(
        &mut self,
        ctx: &egui::Context,
        env: &DrawingEnv<'_>,
        current_owner: Option<QuickRangeOwner>,
    ) -> DrawingChromeAsk {
        self.quick_range().reconcile_tab(env.tab);
        if self.quick_range().owner().is_some() {
            self.quick_range().reconcile(current_owner);
        }
        self.quick_range().note_selection(
            env.pane_id,
            env.selected.as_ref().map(|selected| selected.drawing.id.0),
        );
        let mut ask = match self.quick_range().reconcile_tab(env.tab) {
            Some(true) => quick_range::draw(&mut self.quick_range(), ctx, env),
            // A temporary ruler is already the active chart selection. Its
            // own action arrives on release; unrelated object chrome stands
            // down for the held gesture.
            Some(false) => DrawingChromeAsk::default(),
            None => context_bar::draw(self, ctx, env),
        };
        // Flush each subview before the next so first-valued effects keep their order
        // in both synchronous host calls and immediate session delivery.
        ask = self.emit(Intent::Ask(ask));
        let inline = inline_editor::draw(self, ctx, env);
        ask.merge(self.emit(Intent::Ask(inline)));
        let inspector = inspector::draw_floating(self, ctx, env);
        ask.merge(self.emit(Intent::Ask(inspector)));
        let manager = manager::draw(self, ctx, env);
        ask.merge(self.emit(Intent::Ask(manager)));
        ask.merge(self.emit(Intent::TakeMenu(env.pane_id)));
        ask
    }

    /// Queue the chart menu's ask for pane `pane`, merged with any earlier
    /// one for the same pane that has not been applied yet.
    pub(crate) fn ask_from_menu(&mut self, pane: u64, ask: DrawingChromeAsk) {
        self.emit(Intent::Menu { pane, ask });
    }

    /// Raise or drop that question.
    pub fn set_delete_confirm(&mut self, confirming: bool) {
        self.emit(Intent::ConfirmDelete(confirming));
    }

    /// Whether the parked inspector position owes the workspace file a write,
    /// taken so it is owed once.
    pub fn take_inspector_position_dirty(&mut self) -> bool {
        self.emit(Intent::TakePositionDirty)
            .inspector_position_dirty
    }
}

#[cfg(test)]
impl Chrome<'_> {
    /// Open the inspector, or shut it.
    pub fn set_inspector_open(&mut self, open: bool) {
        self.emit(Intent::InspectorOpen(open));
    }

    /// Park the inspector where a hand put it. The hook that stands in for one
    /// reaches [`inspector::Inspector::place_by_hand`] through the opt-in
    /// owner launch scenario.
    pub fn place_inspector_by_hand(&mut self, position: egui::Pos2) {
        self.emit(Intent::InspectorPlace([position.x, position.y]));
        self.view.inspector.max_width = None;
    }

    /// The rest is what a test needs to say about state that has no other
    /// door: the pin it is about to click, the position it is about to check,
    /// the flag a capture hook would otherwise have to be launched to set.
    pub fn set_inspector_pinned(&mut self, pinned: bool) {
        self.emit(Intent::InspectorPinned(pinned));
    }

    pub fn set_inspector_tab(&mut self, tab: InspectorTab) {
        self.emit(Intent::InspectorTab(tab));
    }

    pub fn set_inspector_pos(&mut self, position: Option<egui::Pos2>) {
        self.emit(Intent::InspectorPosition(position.map(|p| [p.x, p.y])));
    }

    pub fn set_inspector_moved(&mut self, moved: bool) {
        self.emit(Intent::InspectorMoved(moved));
    }

    pub fn set_inspector_pin_touched(&mut self, touched: bool) {
        self.emit(Intent::InspectorPinTouched(touched));
    }

    pub fn set_pending_text_note(&mut self, pending: bool) {
        self.emit(Intent::Scenario(
            quantick_ui_model::drawing_chrome::scenario::Intent::RequestNote(pending),
        ));
    }

    /// Forget the rectangle the bar was last drawn at, so a test can prove a
    /// frame drew it again rather than read a stale one.
    pub fn forget_context_bar_rect(&mut self) {
        self.view.bar.last_drawn_rect = None;
    }

    /// Open an edit gesture by hand, so a test can prove that one which
    /// outlives its selection is committed rather than dropped.
    pub fn open_edit_gesture(&mut self, tab: u64, side: PaneSide, index: usize, before: Drawing) {
        self.emit(Intent::OpenEditGesture(DrawingEdit {
            tab,
            side,
            index,
            before,
        }));
    }
}

/// Apply what a frame of chrome asked for.
///
/// One implementation for three hosts — the context bar, the floating
/// inspector and the docked panel — so lock, delete, hide, the pin and the
/// undo coalescing cannot drift between them. What this surface owns it
/// changes here; what the host owns leaves as an ask.
fn apply_actions(
    chrome: &mut Chrome<'_>,
    ctx: &egui::Context,
    actions: InspectorActions,
    index: usize,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let before = (actions.edited && chrome.shared.edit_baseline.is_none())
        .then(|| {
            env.selected
                .as_ref()
                .map(|selected| Box::new(selected.drawing.clone()))
        })
        .flatten();
    chrome.emit(Intent::Actions(
        quantick_ui_model::drawing_chrome::ActionInput {
            actions,
            index,
            tab: env.tab,
            side: env.side,
            before,
            gesture_settled: ctx.input(|input| !input.pointer.any_down())
                && ctx.memory(|memory| memory.focused().is_none()),
        },
    ))
}

/// Read the five `QUANTICK_*` capture hooks this surface owns.
///
/// Called from the constructor, where every launch hook is read — **not**
/// from [`super::View::apply_env_hook`], which the registry applies on the first
/// *drawn* frame. That is too late for two of these: the demo appliers run
/// earlier in that same frame, and `QUANTICK_DRAWING_INSPECTOR=1` is a state
/// they read (`carry_across_selection` asks whether the panel is open before
/// it places an object and moves the selection). A hook another hook depends
/// on has to be in place before any frame, and photographing a chart with no
/// inspector because the order slipped is exactly the silent capture failure
/// this file's own comments warn about.
///
/// Here rather than in `app.rs` because the fields are here: a hook that
/// stops matching its field stops compiling.
#[cfg(any(feature = "quick-range-harness", feature = "drawing-harness", test))]
pub(crate) fn apply_launch_hooks(owner: &mut Owner) {
    let mut chrome = borrow(owner);
    #[cfg(any(feature = "quick-range-harness", test))]
    chrome.quick_range().apply_launch();
    #[cfg(any(feature = "drawing-harness", test))]
    chrome.apply_launch();
}

/// Clamp a window of `size` at `position` into `chart`, top-left biased when
/// the window is larger than the pane.
pub(crate) fn clamp_into_chart(
    position: egui::Pos2,
    size: egui::Vec2,
    chart: egui::Rect,
) -> egui::Pos2 {
    let max_x = (chart.right() - size.x).max(chart.left());
    let max_y = (chart.bottom() - size.y).max(chart.top());
    egui::pos2(
        position.x.clamp(chart.left(), max_x),
        position.y.clamp(chart.top(), max_y),
    )
}

#[cfg(any(feature = "drawing-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_AVWAP_DEMO",
    "QUANTICK_DRAWINGS_DEMO",
    "QUANTICK_DRAWINGS_DEMO_RECUT",
    "QUANTICK_DRAWINGS_DEMO_SELECT",
    "QUANTICK_DRAWINGS_DEMO_SHARED",
    "QUANTICK_DRAWING_CONSTRAIN",
    "QUANTICK_DRAWING_DRAFT",
    "QUANTICK_FRVP_DEMO",
    "QUANTICK_FRVP_DEMO_SELECT",
    "QUANTICK_CONTEXT_BAR_POS",
    "QUANTICK_DRAWINGS_MANAGER",
    "QUANTICK_DRAWING_INSPECTOR",
    "QUANTICK_DRAWING_INSPECTOR_POS",
    "QUANTICK_DRAWING_INSPECTOR_TAB",
    "QUANTICK_TEXT_NOTE"
];

#[cfg(test)]
pub(crate) fn new_owner() -> Owner {
    Owner::new(View::default(), effects).in_slots(&[
        quantick_ui_model::Slot::PinnedInspector,
        quantick_ui_model::Slot::DrawingChrome,
    ])
}

impl View {
    pub(crate) fn paint_pass(
        &mut self,
        mut session: quantick_ui_model::UpdateSession<'_, quantick_ui_model::drawing_chrome::Model>,
        ctx: &egui::Context,
        env: &DrawingEnv<'_>,
        current_owner: Option<QuickRangeOwner>,
        floating: bool,
    ) {
        let mut chrome = Chrome {
            session: Session::Paint(session.reborrow()),
            view: self,
        };
        chrome.draw_pass(ctx, env, current_owner, floating);
    }
}

#[cfg(test)]
pub(super) mod tests;
