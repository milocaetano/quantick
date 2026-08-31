//! The drawing chrome: everything that speaks for the *selected drawing*.
//!
//! Four pieces of chrome appear when a trader selects an object — the context
//! bar under it, the inline field for a note's words, the inspector (floating
//! or docked at the side) and the object manager listing every mark on the
//! chart. They were four `draw_*` methods on `QuantickApp` reading twenty-one
//! of its fields, and this module is where that state lives instead.
//!
//! # Why one [`Surface`], not four
//!
//! They are one subsystem, not four neighbours. Five pieces of state are
//! written by one and read by another:
//!
//! | State | Written by | Read by |
//! | --- | --- | --- |
//! | [`Shared::open`] | the context bar's gear | the inspector |
//! | [`Shared::pinned`] | the inspector's pin | the context bar, to hide a gear leading where the eye already is |
//! | [`Shared::last_selection`] | the context bar, on a new selection | the inspector's placement rule |
//! | [`Shared::delete_confirm`] | the action applier | the bar and the inspector body |
//! | [`Shared::edit_baseline`] | the action applier | the context bar, to decide whether this frame owes a clone |
//!
//! Registering four members would leave that sharing in `QuantickApp` — the
//! disease this port exists to cure — or push it through the response and
//! make [`super::Surfaces::draw_all`]'s call order load-bearing, which its own
//! documentation says it is not. One member keeps the sharing private, and
//! the order the four are drawn in is decided here, in writing, beside the
//! reason.
//!
//! So [`super::Surfaces`] grows by exactly one field, 8 to 9. The typed-struct
//! trade the registry documents is not under pressure from this change.
//!
//! # What it reads and what it asks for
//!
//! [`DrawingEnv`] is the read-only slice, nested inside [`super::SurfaceEnv`]
//! rather than flattened into it: eleven more loose fields on the shared env
//! would be the same disease one level out. Everything in it is borrowed or
//! `Copy`, and the two entries that cost an allocation —
//! [`DrawingEnv::manager_rows`] and [`DrawingEnv::selected_band`] — are built
//! by the host only while the surface that reads them is on screen, the rule
//! `open_markets` already follows.
//!
//! Writes go back through [`DrawingChromeAsk`]. The inspector **edits a copy**
//! of the selected drawing and hands it over; it never holds a `&mut` into a
//! pane. The store commands are named fields, not an enum, for the reason
//! [`super::SurfaceResponse`] is a struct: a new ask is an added field that
//! defaults to "did not ask", never a `match` arm that reopens every caller.
//!
//! # Cost
//!
//! Event-driven throughout. Nothing here runs per trade, per depth update or
//! inside a renderer. Per frame the host pays one virtual call that returns
//! after an `Option` test when nothing is selected and the manager is shut;
//! the clone of the selected drawing is paid only while the inspector is
//! actually open, which is what the trunk paid before.

pub(crate) mod context_bar;
pub(crate) mod inline_editor;
pub(crate) mod inspector;
pub(crate) mod manager;

use eframe::egui;

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::bands::BandLabel;
use crate::drawings::{Drawing, DrawingStyle, PresetHost};
use crate::pane::PaneSide;
use crate::toolrail::ToolboxDock;

/// Initial position of the selected-drawing inspector, before
/// [`inspector::placement`] has a size and a bbox to place it from.
pub(crate) const DRAWING_INSPECTOR_DEFAULT_POSITION: egui::Pos2 = egui::pos2(90.0, 120.0);
/// Inspector width bounds (UX spec: resizable between 300 and 440 px).
pub(crate) const INSPECTOR_MIN_WIDTH_PX: f32 = 300.0;
/// See [`INSPECTOR_MIN_WIDTH_PX`].
const INSPECTOR_MAX_WIDTH_PX: f32 = 440.0;
/// Default inspector width for the shipped tools (the spec reserves 360 px
/// for the Fib level editor).
const INSPECTOR_DEFAULT_WIDTH_PX: f32 = 320.0;
/// Default inspector width for tools that mount a level editor tab.
const INSPECTOR_LEVELS_WIDTH_PX: f32 = 360.0;
/// Gap kept between the inspector and the selected object's bounding box.
const INSPECTOR_OBJECT_GAP_PX: f32 = 12.0;
/// Assumed inspector height for placement before its first frame reports one.
const INSPECTOR_FALLBACK_HEIGHT_PX: f32 = 280.0;
/// Below this chart width a fresh selection opens the inspector pinned —
/// there is no floating position that would not crowd the geometry. Stops
/// applying once the user touches the pin either way.
pub(crate) const INSPECTOR_AUTO_PIN_CHART_WIDTH_PX: f32 = 1180.0;
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

/// Which inspector tab is open. Tabs exist per capability: every tool gets
/// Style and Coordinates; a tool that brings its own tab (the Fib level
/// editor) mounts it as Extra without the central code knowing its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InspectorTab {
    #[default]
    Style,
    Extra,
    Coordinates,
}

/// Which default-style button the Style tab was pressed on, so the caller can
/// say out loud that something was remembered — a silent save leaves the
/// trader wondering whether it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SavedDefault {
    OneTool,
    EveryTool,
    Forgotten,
}

impl SavedDefault {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::OneTool => "Saved - new drawings of this tool open configured like this one.",
            Self::EveryTool => "Saved - every new drawing opens with this colour, width and fill.",
            Self::Forgotten => "Reset - this tool goes back to how it opened out of the box.",
        }
    }
}

/// What the inspector body asked for this frame. The applier owns every
/// mutation, so the pinned panel and the floating window share one rule set.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct InspectorActions {
    pub toggle_hidden: bool,
    pub toggle_lock: bool,
    pub toggle_pin: bool,
    pub delete: bool,
    pub cancel_delete: bool,
    pub force_delete: bool,
    pub close: bool,
    pub edited: bool,
    /// Which default-style button was pressed, if any.
    pub saved_default: Option<SavedDefault>,
}

impl InspectorActions {
    /// Fold another frame section's requests into this one — the title bar
    /// and the body each report intent, the applier takes the union.
    pub fn merge(&mut self, other: Self) {
        self.toggle_hidden |= other.toggle_hidden;
        self.toggle_lock |= other.toggle_lock;
        self.toggle_pin |= other.toggle_pin;
        self.delete |= other.delete;
        self.cancel_delete |= other.cancel_delete;
        self.force_delete |= other.force_delete;
        self.close |= other.close;
        self.edited |= other.edited;
        self.saved_default = self.saved_default.or(other.saved_default);
    }
}

/// An inline text edit in flight: which object, on which pane of which tab,
/// and what it said before the first keystroke.
///
/// The tab and the pane, not the index alone: an index identifies nothing
/// once there are two panes, let alone two tabs. Switching tabs with an
/// editor open would otherwise record the note's undo entry against whatever
/// object happened to sit at that index on the tab now in front — swapping an
/// unrelated drawing for a copy of the note on the next Ctrl+Z.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineTextEdit {
    pub tab: u64,
    pub side: PaneSide,
    pub index: usize,
    pub before: Drawing,
}

/// The pre-edit copy held while an inspector gesture (a slider, a colour
/// wheel, a coordinate drag) is in flight, so the whole gesture commits as one
/// undo entry when pointer and keyboard let go.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InspectorEdit {
    pub tab: u64,
    pub side: PaneSide,
    pub index: usize,
    pub before: Drawing,
}

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
    pub author: Option<String>,
    pub band: BandLabel,
}

/// What the drawing chrome needs from the application.
///
/// Nested inside [`super::SurfaceEnv`] rather than flattened into it: this is
/// one subsystem's slice, and eleven more fields on the shared env would move
/// the loose-field problem out one level instead of solving it.
pub(crate) struct DrawingEnv<'a> {
    /// The selected object on the pane the chrome speaks for, if any.
    pub selected: Option<SelectedDrawing<'a>>,
    /// That pane's chart rectangle, or `None` before it has been laid out.
    pub chart_area: Option<egui::Rect>,
    /// The *focused* pane's rectangle, which is not always the one above: a
    /// shared mark can be selected from the chart it is mirrored on. The
    /// object manager opens beside the toolbox button that opened it, and that
    /// button is on the focused pane.
    pub focused_chart_area: Option<egui::Rect>,
    /// Where the live lane begins. Every popup keeps clear of it: that strip
    /// is where the price the trader is reading is being formed.
    pub lane_divider_x: Option<f32>,
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

#[cfg(test)]
impl DrawingEnv<'static> {
    /// A pane with nothing selected and no manager open, for the tests.
    ///
    /// Written the way [`super::SurfaceEnv::quiet`] is, and for the same
    /// reason: a field added here costs one line in this file rather than an
    /// edit in each of the four surfaces' test modules.
    pub fn quiet() -> Self {
        /// A preset bank a test that does not care about presets gets.
        static QUIET_PRESETS: crate::drawings::NullPresetHost = crate::drawings::NullPresetHost;
        Self {
            selected: None,
            chart_area: None,
            focused_chart_area: None,
            lane_divider_x: None,
            auto_range: None,
            selected_bbox: None,
            selected_band: None,
            tab: 0,
            side: PaneSide::Flow,
            drawing_tool_armed: false,
            toolbox_dock: ToolboxDock::Left,
            authored_objects: 0,
            manager_rows: &[],
            presets: &QUIET_PRESETS,
        }
    }
}

/// One write to the preset bank, recorded rather than performed.
///
/// The bank is host state that persists to disk on every write, so the
/// surface cannot hold a `&mut` to it any more than it can hold one to a
/// pane. [`RecordingPresetHost`] reads through the borrowed bank and pushes
/// these; the host replays them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PresetWrite {
    Save {
        tool_id: String,
        name: String,
        value: toml::Value,
    },
    Delete {
        tool_id: String,
        name: String,
    },
    SetDefaultPreset {
        tool_id: String,
        name: Option<String>,
    },
    SetDefaultStyle {
        tool_id: String,
        style: Option<DrawingStyle>,
    },
    SetDefaultConfig {
        tool_id: String,
        value: Option<toml::Value>,
    },
}

impl PresetWrite {
    /// Replay this write onto the real bank.
    ///
    /// One `match` over an enum the host cannot extend by accident: a variant
    /// added to [`PresetWrite`] without a line here does not compile, which is
    /// the property that makes recording the writes safe at all.
    pub(crate) fn apply_to(self, bank: &mut dyn PresetHost) {
        match self {
            Self::Save {
                tool_id,
                name,
                value,
            } => {
                // Overwrite: the recorder already answered the "is that name
                // taken" question against this same bank, and answering it
                // twice would refuse a save the trader was told had succeeded.
                bank.save_custom_preset(&tool_id, &name, value, true);
            }
            Self::Delete { tool_id, name } => bank.delete_custom_preset(&tool_id, &name),
            Self::SetDefaultPreset { tool_id, name } => bank.set_default_preset(&tool_id, name),
            Self::SetDefaultStyle { tool_id, style } => bank.set_default_style(&tool_id, style),
            Self::SetDefaultConfig { tool_id, value } => bank.set_default_config(&tool_id, value),
        }
    }
}

/// A [`PresetHost`] that reads the real bank and writes into a list.
///
/// The tool-owned inspector tab talks to the bank through `PresetHost`, which
/// is already a port — but its five mutating methods persist to disk, so the
/// surface records them and the host replays them on the same frame. Every
/// read is delegated, unchanged, to the bank the env borrowed.
///
/// [`PresetHost::save_custom_preset`] is the one method whose answer the
/// caller reads immediately: `false` means "that name is taken and you did
/// not say overwrite". It is answered here from the bank's own
/// [`PresetHost::load_custom_preset`] rather than from a copy of the store's
/// rule, and `recorder_refuses_a_taken_name_exactly_as_the_bank_does` pins
/// the two together.
pub(crate) struct RecordingPresetHost<'a> {
    bank: &'a dyn PresetHost,
    writes: Vec<PresetWrite>,
}

impl<'a> RecordingPresetHost<'a> {
    fn new(bank: &'a dyn PresetHost) -> Self {
        Self {
            bank,
            // Not `with_capacity`: the overwhelmingly common frame records
            // nothing, and an empty `Vec` does not allocate.
            writes: Vec::new(),
        }
    }
}

impl RecordingPresetHost<'_> {
    /// The writes this frame recorded, in the order they were made.
    fn into_writes(self) -> Vec<PresetWrite> {
        self.writes
    }
}

impl PresetHost for RecordingPresetHost<'_> {
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String> {
        self.bank.custom_preset_names(tool_id)
    }

    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value> {
        self.bank.load_custom_preset(tool_id, name)
    }

    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool {
        if !overwrite && self.bank.load_custom_preset(tool_id, name).is_some() {
            return false;
        }
        self.writes.push(PresetWrite::Save {
            tool_id: tool_id.to_owned(),
            name: name.to_owned(),
            value,
        });
        true
    }

    fn delete_custom_preset(&mut self, tool_id: &str, name: &str) {
        self.writes.push(PresetWrite::Delete {
            tool_id: tool_id.to_owned(),
            name: name.to_owned(),
        });
    }

    fn default_preset(&self, tool_id: &str) -> Option<String> {
        self.bank.default_preset(tool_id)
    }

    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>) {
        self.writes.push(PresetWrite::SetDefaultPreset {
            tool_id: tool_id.to_owned(),
            name,
        });
    }

    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle> {
        self.bank.default_style(tool_id)
    }

    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>) {
        self.writes.push(PresetWrite::SetDefaultStyle {
            tool_id: tool_id.to_owned(),
            style,
        });
    }

    fn default_config(&self, tool_id: &str) -> Option<toml::Value> {
        self.bank.default_config(tool_id)
    }

    fn set_default_config(&mut self, tool_id: &str, value: Option<toml::Value>) {
        self.writes.push(PresetWrite::SetDefaultConfig {
            tool_id: tool_id.to_owned(),
            value,
        });
    }

    fn has_default_config(&self, tool_id: &str) -> bool {
        self.bank.has_default_config(tool_id)
    }
}

/// What the drawing chrome asked the host to do.
///
/// A struct of defaults like [`super::SurfaceResponse`] itself: every field
/// means "this was not asked for" until a surface sets it, so a new request
/// never touches an existing caller. The store commands are named fields
/// rather than an enum for exactly that reason.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct DrawingChromeAsk {
    /// The inspector edited its copy of the selected drawing. Boxed: it is by
    /// far the largest thing a response can carry — a pencil stroke is 512
    /// anchors — and every other surface would pay for its size in a response
    /// returned by value on every frame.
    pub edited: Option<Box<Drawing>>,
    /// The pointer and the keyboard have let go: record this pre-edit copy as
    /// the one undo entry the whole gesture earned.
    ///
    /// The copy travels with the ask rather than being fetched back off the
    /// surface, so the host applies a response instead of interrogating the
    /// thing that produced it. Boxed for the reason [`Self::edited`] is: a
    /// `Drawing` inline here would be paid for by every surface in the
    /// application, on every frame, in a response returned by value.
    pub commit_edit_gesture: Option<Box<InspectorEdit>>,
    /// Hide or show the selection.
    pub toggle_selected_hidden: bool,
    /// Lock or unlock the selection.
    pub toggle_selected_locked: bool,
    /// Delete the selection, through the confirmation rule the keyboard uses.
    pub request_delete: bool,
    /// Delete the selection even though it is locked — the trader answered
    /// the question.
    pub force_delete: bool,
    /// Copy the selection, offset so the copy is visibly a copy.
    pub duplicate: bool,
    /// Place a text note in the middle of the pane and open its editor — the
    /// `QUANTICK_TEXT_NOTE` hook, which needs the placement rules the host
    /// owns.
    pub place_text_note: bool,
    /// Record this object's pre-edit copy as one undo entry: the inline
    /// editor closed, on the pane the note actually lives on, which is not
    /// necessarily the one in front. Boxed like the two above.
    pub record_inline_edit: Option<Box<InlineTextEdit>>,
    /// The caret moved to a different object, or left the chart. Every pane
    /// has to be told, so exactly one object anywhere stands down; the host
    /// reads the new target back by name.
    pub content_editing_changed: bool,
    /// Select this row, and bring it into view.
    pub manager_select: Option<usize>,
    /// Toggle this row's eye.
    pub manager_toggle_hidden: Option<usize>,
    /// Toggle this row's lock.
    pub manager_toggle_locked: Option<usize>,
    /// Raise this row above the rest.
    pub manager_bring_to_front: Option<usize>,
    /// Delete this row, through the same confirmation the inspector raises.
    pub manager_delete: Option<usize>,
    /// Show every hidden object.
    pub show_all: bool,
    /// Unlock every locked object.
    pub unlock_all: bool,
    /// Delete every object, locked ones included — the count-bearing gate was
    /// answered.
    pub delete_all: bool,
    /// Take back every object an assistant placed on this chart.
    pub sweep_authored: bool,
    /// A named default was remembered, and is owed a line the trader can read.
    pub saved_default: Option<SavedDefault>,
    /// Writes to the preset bank, in the order the frame made them.
    pub presets: Vec<PresetWrite>,
}

impl DrawingChromeAsk {
    /// Fold one piece of chrome's asks into the frame's.
    ///
    /// Flags stay set, so no piece can cancel another's ask by being drawn
    /// after it, and a valued ask keeps the **first** — the same rule
    /// [`super::SurfaceResponse::merge`] follows, for the same reason: two
    /// pieces cannot both be right about one row, and dropping the later ask
    /// deterministically beats letting draw order decide in silence.
    pub(super) fn merge(&mut self, other: Self) {
        self.edited = self.edited.take().or(other.edited);
        self.commit_edit_gesture = self
            .commit_edit_gesture
            .take()
            .or(other.commit_edit_gesture);
        self.toggle_selected_hidden |= other.toggle_selected_hidden;
        self.toggle_selected_locked |= other.toggle_selected_locked;
        self.request_delete |= other.request_delete;
        self.force_delete |= other.force_delete;
        self.duplicate |= other.duplicate;
        self.place_text_note |= other.place_text_note;
        self.record_inline_edit = self.record_inline_edit.take().or(other.record_inline_edit);
        self.content_editing_changed |= other.content_editing_changed;
        self.manager_select = self.manager_select.or(other.manager_select);
        self.manager_toggle_hidden = self.manager_toggle_hidden.or(other.manager_toggle_hidden);
        self.manager_toggle_locked = self.manager_toggle_locked.or(other.manager_toggle_locked);
        self.manager_bring_to_front = self.manager_bring_to_front.or(other.manager_bring_to_front);
        self.manager_delete = self.manager_delete.or(other.manager_delete);
        self.show_all |= other.show_all;
        self.unlock_all |= other.unlock_all;
        self.delete_all |= other.delete_all;
        self.sweep_authored |= other.sweep_authored;
        self.saved_default = self.saved_default.or(other.saved_default);
        self.presets.extend(other.presets);
    }
}

/// The state the four pieces share, and the reason they are one surface.
///
/// Private to this module: the whole point of the member being one rather
/// than four is that nothing outside can reach across the subsystem.
#[derive(Default)]
pub(crate) struct Shared {
    /// Whether the floating inspector is open. Selecting a drawing no longer
    /// opens it: the context bar is what a selection raises, and the gear on
    /// that bar is the one thing that opens the full panel.
    open: bool,
    /// Whether the inspector is docked at the chart's side instead.
    pinned: bool,
    /// The selection the last automatic placement was computed for.
    last_selection: Option<usize>,
    /// A locked object's delete was asked for and is awaiting its answer.
    delete_confirm: bool,
    /// The pre-edit copy held across an in-flight edit gesture.
    edit_baseline: Option<InspectorEdit>,
}

/// The chrome that speaks for the selected drawing: the context bar, the
/// inline text editor, the inspector and the object manager.
#[derive(Default)]
pub(crate) struct DrawingChromeSurface {
    shared: Shared,
    inspector: inspector::Inspector,
    bar: context_bar::ContextBarState,
    manager: manager::Manager,
    inline: inline_editor::InlineEditor,
    /// One-shot: the placement that just happened asked for the caret. The
    /// object it made is the selected one, so the editor opens on the frame
    /// the placement landed.
    pending_text_edit: bool,
    /// One-shot: the `QUANTICK_TEXT_NOTE` hook wants a note placed and typed.
    /// The placement rules belong to the host, so this leaves through
    /// [`DrawingChromeAsk::place_text_note`].
    pending_text_note: bool,
    /// One-shot: the object just placed asked for its settings panel. Applied
    /// *after* the new-selection reset, because the placement that made the
    /// request also made the selection change that clears it.
    pending_open_settings: bool,
}

impl DrawingChromeSurface {
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

    /// Whether the inspector is docked at the chart's side. The host asks
    /// before it lays out the central canvas, because a docked panel is
    /// declared with the chrome and the canvas pays its width.
    pub fn inspector_pinned(&self) -> bool {
        self.shared.pinned
    }

    /// Which note is being typed on the chart right now — what a second
    /// operator reads to know the keyboard belongs to an object.
    pub fn inline_text_editing(&self) -> Option<usize> {
        self.inline.editing_index()
    }

    /// The same answer with the tab and pane that make it unambiguous: an
    /// index alone names a different object on every pane.
    pub fn content_editing_target(&self) -> Option<(u64, PaneSide, usize)> {
        self.inline.target()
    }

    /// The parked inspector position, for the workspace file.
    pub fn remembered_inspector_position(&self) -> Option<[f32; 2]> {
        self.inspector.remembered_position()
    }

    // ---- What the host commands -----------------------------------------

    /// Open the object manager, or shut it.
    pub fn set_manager_open(&mut self, open: bool) {
        self.manager.open = open;
    }

    /// Open the inspector, or shut it.
    #[cfg(test)]
    pub fn set_inspector_open(&mut self, open: bool) {
        self.shared.open = open;
    }

    /// Whether the inspector is open.
    #[cfg(test)]
    pub fn inspector_open(&self) -> bool {
        self.shared.open
    }

    /// Carry an open inspector across a selection the host is about to make.
    ///
    /// One method rather than the line copied into each hook, because the
    /// omission is silent: the demo that forgets it produces a screenshot
    /// that looks merely uninteresting, and that is how three of these hooks
    /// came to disagree about it.
    pub fn carry_across_selection(&mut self) {
        self.pending_open_settings |= self.shared.open;
    }

    /// Ask for the caret on the next frame — the placement that just happened
    /// wants its note typed.
    pub fn request_text_edit(&mut self) {
        self.pending_text_edit = true;
    }

    /// The host placed the note the `QUANTICK_TEXT_NOTE` hook asked for.
    ///
    /// The ask is repeated every frame until this lands, because at launch
    /// there is no chart to place against and no bar to place at: the hook
    /// waits for a laid-out pane rather than firing once into an empty one
    /// and photographing nothing.
    pub fn note_text_note_placed(&mut self) {
        self.pending_text_note = false;
    }

    /// Whether the open inline edit belongs to this pane — asked before its
    /// drawing store is swapped out from under the caret.
    pub fn inline_edit_is_on(&self, tab: u64, side: PaneSide) -> bool {
        self.inline
            .target()
            .is_some_and(|(edit_tab, edit_side, _)| edit_tab == tab && edit_side == side)
    }

    /// Drop an undo baseline that describes this pane, rather than record it.
    ///
    /// Its object is about to be replaced by another set's, and an entry
    /// recorded against that would undo something the trader never did.
    pub fn drop_edit_baseline_on(&mut self, tab: u64, side: PaneSide) {
        if self
            .shared
            .edit_baseline
            .as_ref()
            .is_some_and(|edit| edit.tab == tab && edit.side == side)
        {
            self.shared.edit_baseline = None;
        }
    }

    /// Put the caret in a note now. The host owns the store, so it hands the
    /// object over; refused for one that holds no words or is locked, because
    /// an editor that opened and then dropped every keystroke would be worse
    /// than none.
    pub fn begin_inline_text_edit(
        &mut self,
        tab: u64,
        side: PaneSide,
        index: usize,
        drawing: &Drawing,
    ) -> bool {
        self.inline.begin(tab, side, index, drawing)
    }

    /// Close the editor, if one is open, and hand back what it owes the undo
    /// history.
    pub fn end_inline_text_edit(&mut self) -> Option<InlineTextEdit> {
        self.inline.end()
    }

    /// Park the inspector where a hand put it. The hook that stands in for one
    /// reaches [`inspector::Inspector::place_by_hand`] directly, from
    /// [`Surface::apply_env_hook`].
    #[cfg(test)]
    pub fn place_inspector_by_hand(&mut self, position: egui::Pos2) {
        self.inspector.place_by_hand(position);
    }

    /// Restore the position a workspace file remembered.
    pub fn restore_inspector_position(&mut self, remembered: Option<[f32; 2]>) {
        self.inspector.restore_position(remembered);
    }

    /// Whether the baseline in flight describes an object other than this
    /// one, which is a gesture that outlived its selection and must commit.
    pub fn baseline_is_stale(&self, index: Option<usize>) -> bool {
        self.shared
            .edit_baseline
            .as_ref()
            .is_some_and(|edit| Some(edit.index) != index)
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
        inspector::draw_pinned(self, ctx, env)
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
    pub fn draw_floating(&mut self, ctx: &egui::Context, env: &DrawingEnv<'_>) -> DrawingChromeAsk {
        let mut ask = context_bar::draw(self, ctx, env);
        ask.merge(inline_editor::draw(self, ctx, env));
        ask.merge(inspector::draw_floating(self, ctx, env));
        ask.merge(manager::draw(self, ctx, env));
        ask
    }

    /// Whether a locked object's delete is awaiting its answer. The keyboard
    /// owns Escape and the Delete key, so it reads and clears this by name
    /// rather than reaching into the surface.
    pub fn delete_confirm(&self) -> bool {
        self.shared.delete_confirm
    }

    /// Raise or drop that question.
    pub fn set_delete_confirm(&mut self, confirming: bool) {
        self.shared.delete_confirm = confirming;
    }

    /// Whether the parked inspector position owes the workspace file a write,
    /// taken so it is owed once.
    pub fn take_inspector_position_dirty(&mut self) -> bool {
        self.inspector.take_position_dirty()
    }

    /// The rest is what a test needs to say about state that has no other
    /// door: the pin it is about to click, the position it is about to check,
    /// the flag a capture hook would otherwise have to be launched to set.
    #[cfg(test)]
    pub fn set_inspector_pinned(&mut self, pinned: bool) {
        self.shared.pinned = pinned;
    }

    #[cfg(test)]
    pub fn set_inspector_tab(&mut self, tab: InspectorTab) {
        self.inspector.tab = tab;
    }

    #[cfg(test)]
    pub fn inspector_pos(&self) -> Option<egui::Pos2> {
        self.inspector.pos
    }

    #[cfg(test)]
    pub fn set_inspector_pos(&mut self, position: Option<egui::Pos2>) {
        self.inspector.pos = position;
    }

    #[cfg(test)]
    pub fn inspector_moved(&self) -> bool {
        self.inspector.moved
    }

    #[cfg(test)]
    pub fn set_inspector_moved(&mut self, moved: bool) {
        self.inspector.moved = moved;
    }

    #[cfg(test)]
    pub fn inspector_pin_touched(&self) -> bool {
        self.inspector.pin_touched
    }

    #[cfg(test)]
    pub fn set_inspector_pin_touched(&mut self, touched: bool) {
        self.inspector.pin_touched = touched;
    }

    #[cfg(test)]
    pub fn context_bar(&self) -> &crate::drawings::context_bar::ContextBar {
        &self.bar.bar
    }

    #[cfg(test)]
    pub fn context_bar_mut(&mut self) -> &mut crate::drawings::context_bar::ContextBar {
        &mut self.bar.bar
    }

    #[cfg(test)]
    pub fn set_pending_text_note(&mut self, pending: bool) {
        self.pending_text_note = pending;
    }

    /// Forget the rectangle the bar was last drawn at, so a test can prove a
    /// frame drew it again rather than read a stale one.
    #[cfg(test)]
    pub fn forget_context_bar_rect(&mut self) {
        self.bar.last_drawn_rect = None;
    }

    /// Open an edit gesture by hand, so a test can prove that one which
    /// outlives its selection is committed rather than dropped.
    #[cfg(test)]
    pub fn open_edit_gesture(&mut self, tab: u64, side: PaneSide, index: usize, before: Drawing) {
        self.shared.edit_baseline = Some(InspectorEdit {
            tab,
            side,
            index,
            before,
        });
    }

    #[cfg(test)]
    pub fn context_bar_rect(&self) -> Option<egui::Rect> {
        self.bar.last_drawn_rect
    }

    #[cfg(test)]
    pub fn inspector_pin_rect(&self) -> Option<egui::Rect> {
        self.inspector.pin_rect
    }

    #[cfg(test)]
    pub fn manager_action_rects(&self) -> &[(usize, &'static str, egui::Rect)] {
        &self.manager.action_rects
    }
}

impl Surface for DrawingChromeSurface {
    fn id(&self) -> &'static str {
        "drawing-chrome"
    }

    /// The port's uniform entry, which forwards to [`Self::draw_floating`].
    ///
    /// [`Surfaces::draw_all`] deliberately does not call it: this surface is
    /// anchored to the *chart* rather than floating over the window, so the
    /// host registers it after the central canvas — both because a Background
    /// panel drawn afterwards would sit over an `Area` created before it, and
    /// because the pane geometry every piece here places against
    /// (`last_chart_area`, `last_auto_range`, the lane divider) is written by
    /// the canvas as it draws. Reading it a phase early would place the bar
    /// and the panel against the previous frame's chart.
    ///
    /// Kept rather than dropped, and it is a forward rather than a second
    /// implementation: this surface satisfies exactly the contract its eight
    /// neighbours do, `apply_env_hook` hangs off the same trait, and if the
    /// frame is ever reordered so the canvas comes first, adding this member
    /// to `draw_all` is one line.
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        SurfaceResponse {
            drawing: self.draw_floating(ctx, &env.drawing),
            ..SurfaceResponse::default()
        }
    }

    /// Five `QUANTICK_*` hooks, all of them state this surface owns.
    ///
    /// They ran in the constructor before this module existed, which put the
    /// hook for a surface in a different file from the surface. Here they are
    /// beside the fields they set, and a hook that stops matching its field
    /// stops compiling.
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        // The on-chart note editor exists only between a placement and the
        // first click elsewhere, so no click-free launch could photograph it
        // without a hook — the same gap `QUANTICK_DRAWING_DRAFT` fills for a
        // half-placed object. The placement itself is the host's, so this
        // raises the ask rather than making the object.
        if std::env::var("QUANTICK_TEXT_NOTE").is_ok_and(|value| value == "1") {
            self.pending_text_note = true;
        }
        // The object manager: where the "off series", "other market" and band
        // badges live, and the only place a mark clamped to an edge can be
        // found when it is nowhere near the visible window.
        if std::env::var("QUANTICK_DRAWINGS_MANAGER").is_ok_and(|value| value == "1") {
            self.manager.open = true;
        }
        // The context bar only exists while something is selected, so the
        // hook that reaches it is a hook that *selects*: pair this with
        // QUANTICK_DRAWINGS_DEMO_SELECT. This one opens the panel behind the
        // gear on top, which is the state a screenshot cannot otherwise reach
        // without a click.
        if std::env::var("QUANTICK_DRAWING_INSPECTOR").is_ok_and(|value| value == "1") {
            self.shared.open = true;
        }
        // Which tab the panel opens on. The panel is one hook away, but its
        // tool-owned tab — where a Fib's levels and colours are built, and
        // where the two default controls sit — is a click deeper, and a
        // capture has no hand for it.
        if let Ok(tab) = std::env::var("QUANTICK_DRAWING_INSPECTOR_TAB") {
            match tab.trim() {
                "style" => self.inspector.tab = InspectorTab::Style,
                "extra" => self.inspector.tab = InspectorTab::Extra,
                "coordinates" => self.inspector.tab = InspectorTab::Coordinates,
                // Refused rather than guessed: a typo shows the default tab,
                // never a confident capture of the wrong one.
                other => tracing::warn!(tab = other, "unknown drawing inspector tab"),
            }
        }
        // The trader's own drag, scripted: `x,y` in screen points parks the
        // properties popup exactly as a hand on the title bar would, through
        // that gesture's own function. Without it the remembered position is
        // unreachable from a launch — a drag is the only way to set one, and
        // a capture run has no hand. Nonsense is refused rather than guessed,
        // so a typo photographs automatic placement instead of an invented
        // pixel.
        if let Some(position) = std::env::var("QUANTICK_DRAWING_INSPECTOR_POS")
            .ok()
            .and_then(|value| parse_point(&value))
        {
            self.inspector.place_by_hand(position);
        }
        // And the same drag on the context bar, which keeps its position
        // across selections too. Its own gesture is the grip, and a capture
        // run has no more hand for that one than for the title bar.
        if let Some(position) = std::env::var("QUANTICK_CONTEXT_BAR_POS")
            .ok()
            .and_then(|value| parse_point(&value))
        {
            self.bar.bar.set_manual(position);
        }
    }
}

/// Apply what a frame of chrome asked for.
///
/// One implementation for three hosts — the context bar, the floating
/// inspector and the docked panel — so lock, delete, hide, the pin and the
/// undo coalescing cannot drift between them. What this surface owns it
/// changes here; what the host owns leaves as an ask.
fn apply_actions(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    actions: InspectorActions,
    index: usize,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let mut ask = DrawingChromeAsk::default();
    // The undo baseline, taken *before* the edit this frame asked for lands.
    // A copy made afterwards would equal the new state and the change would
    // never reach the history. `env` still describes the object as it was at
    // the top of the frame, which is exactly the copy that is wanted — and it
    // costs its allocation only on the frame a gesture opens.
    if actions.edited
        && chrome.shared.edit_baseline.is_none()
        && let Some(selected) = env.selected.as_ref()
    {
        chrome.shared.edit_baseline = Some(InspectorEdit {
            tab: env.tab,
            side: env.side,
            index,
            before: selected.drawing.clone(),
        });
    }
    let gesture_settled = ctx.input(|input| !input.pointer.any_down())
        && ctx.memory(|memory| memory.focused().is_none());
    if gesture_settled {
        ask.commit_edit_gesture = chrome.shared.edit_baseline.take().map(Box::new);
    }
    ask.toggle_selected_hidden |= actions.toggle_hidden;
    if actions.toggle_lock {
        ask.toggle_selected_locked = true;
        chrome.shared.delete_confirm = false;
    }
    if actions.toggle_pin {
        chrome.shared.pinned = !chrome.shared.pinned;
        // The user has expressed a preference: the auto-pin width rule stops
        // firing for the rest of the session.
        chrome.inspector.pin_touched = true;
        if !chrome.shared.pinned {
            // Unpinning re-opens the floating window. The pinned host has been
            // claiming the selection each frame, so treat it as fresh again —
            // otherwise automatic placement never runs and the window falls
            // back to the fixed default corner.
            chrome.shared.last_selection = None;
            chrome.inspector.settle_frame = true;
        }
    }
    ask.request_delete |= actions.delete;
    if actions.cancel_delete {
        chrome.shared.delete_confirm = false;
    }
    if actions.force_delete {
        chrome.shared.delete_confirm = false;
        ask.force_delete = true;
    }
    if actions.close {
        // Closing the panel is a smaller act than it used to be: it puts the
        // trader back on the context bar, with the object still selected.
        // Clearing the selection here would make the X the only control that
        // answers a bigger question than it asks.
        chrome.shared.open = false;
        if chrome.shared.pinned {
            chrome.shared.pinned = false;
            chrome.inspector.pin_touched = true;
        }
        chrome.shared.delete_confirm = false;
    }
    // Nothing to undo: this changed a preference, not the chart.
    ask.saved_default = actions.saved_default;
    ask
}

/// `x,y` in screen points, or nothing. Refused rather than guessed: a typo
/// photographs the automatic behaviour, never an invented pixel.
fn parse_point(raw: &str) -> Option<egui::Pos2> {
    let (x, y) = raw.split_once(',')?;
    let x: f32 = x.trim().parse().ok()?;
    let y: f32 = y.trim().parse().ok()?;
    (x.is_finite() && y.is_finite()).then_some(egui::pos2(x, y))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A bank with one saved preset, to test the recorder's refusal against
    /// the real store's.
    ///
    /// Its own file every call. The store persists on every write, so two
    /// banks sharing a path would let the first call's save leak into the
    /// second and turn a free name into a taken one.
    fn bank(tag: &str) -> crate::drawings::presets::PresetStore {
        let mut store =
            crate::drawings::presets::PresetStore::load_from(std::env::temp_dir().join(format!(
                "quantick-drawing-chrome-{}-{:?}-{tag}.toml",
                std::process::id(),
                std::thread::current().id()
            )));
        // Start from a clean file even if an earlier run left one behind.
        for name in store.custom_preset_names("fib") {
            store.delete_custom_preset("fib", &name);
        }
        store.save_custom_preset("fib", "taken", toml::Value::Integer(1), false);
        store
    }

    /// The one rule the recorder has to answer for itself, pinned against the
    /// bank that would otherwise answer it. Two surfaces that disagree about
    /// whether a name is free would let the inspector's "name already used?"
    /// prompt appear when the save would have succeeded, or skip it when the
    /// save is about to overwrite.
    #[test]
    fn recorder_refuses_a_taken_name_exactly_as_the_bank_does() {
        for (name, overwrite) in [
            ("taken", false),
            ("taken", true),
            ("free", false),
            ("free", true),
        ] {
            let mut real = bank(&format!("real-{name}-{overwrite}"));
            let real_answer =
                real.save_custom_preset("fib", name, toml::Value::Integer(2), overwrite);
            let read_only = bank(&format!("copy-{name}-{overwrite}"));
            let mut recorder = RecordingPresetHost::new(&read_only);
            let recorded =
                recorder.save_custom_preset("fib", name, toml::Value::Integer(2), overwrite);
            assert_eq!(
                recorded, real_answer,
                "the recorder and the bank disagreed about {name:?} with overwrite={overwrite}"
            );
            assert_eq!(
                recorder.writes.is_empty(),
                !real_answer,
                "a refused save must record nothing, and an accepted one exactly one write"
            );
            let _ = std::fs::remove_file(real.path());
            let _ = std::fs::remove_file(read_only.path());
        }
    }

    /// Every read is the bank's own answer, so a tool tab cannot see a
    /// different world through the recorder than through the store.
    #[test]
    fn recorder_delegates_every_read_to_the_bank() {
        let store = bank("reads");
        let recorder = RecordingPresetHost::new(&store);
        assert_eq!(
            recorder.custom_preset_names("fib"),
            store.custom_preset_names("fib")
        );
        assert_eq!(
            recorder.load_custom_preset("fib", "taken"),
            store.load_custom_preset("fib", "taken")
        );
        assert_eq!(recorder.default_preset("fib"), store.default_preset("fib"));
        assert_eq!(recorder.default_style("fib"), store.default_style("fib"));
        assert_eq!(recorder.default_config("fib"), store.default_config("fib"));
        assert_eq!(
            recorder.has_default_config("fib"),
            store.has_default_config("fib")
        );
        let _ = std::fs::remove_file(store.path());
    }

    /// A harness hook that guessed would photograph an invented pixel and call
    /// it the trader's. Refuse instead, and the capture shows the default.
    #[test]
    fn the_popup_position_hook_refuses_what_is_not_a_point() {
        assert_eq!(parse_point("420,200"), Some(egui::pos2(420.0, 200.0)));
        assert_eq!(parse_point(" 420 , 200.5 "), Some(egui::pos2(420.0, 200.5)));
        for raw in ["", "420", "420,", ",200", "left,top", "420x200"] {
            assert_eq!(parse_point(raw), None, "{raw:?} is not a point");
        }
    }

    /// The fold keeps a flag set and takes the first valued ask, which is the
    /// half of a merge a newly added field is most likely to get wrong.
    #[test]
    fn the_fold_keeps_flags_and_the_first_value() {
        let mut first = DrawingChromeAsk {
            show_all: true,
            manager_delete: Some(3),
            presets: vec![PresetWrite::Delete {
                tool_id: "fib".into(),
                name: "a".into(),
            }],
            ..DrawingChromeAsk::default()
        };
        first.merge(DrawingChromeAsk {
            unlock_all: true,
            manager_delete: Some(7),
            presets: vec![PresetWrite::Delete {
                tool_id: "fib".into(),
                name: "b".into(),
            }],
            ..DrawingChromeAsk::default()
        });
        assert!(first.show_all, "a set flag survives the fold");
        assert!(first.unlock_all, "and the other side's flag arrives");
        assert_eq!(
            first.manager_delete,
            Some(3),
            "the first valued ask wins; the later one is dropped"
        );
        assert_eq!(
            first.presets.len(),
            2,
            "preset writes are a sequence, not an answer to one question: both land, in order"
        );
    }
}
