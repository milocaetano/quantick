//! The workspace canvas's capabilities: the window menu door, the layout
//! catalogue, the per-chart menu (minimize, maximize, restore, remove), the
//! add button, and floating, docking, parking, popping out and returning a
//! chart. One named capability per action, each answering the canvas as it
//! stands afterwards, so a caller reads the effect off the result.
//!
//! The model behind them is `quantick_workspace::canvas`; these are its wire
//! words. Slots are the canvas's stable pane ids.

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control::schema::generated_schema;
use quantick_control::wire::CanonicalDecimal;
use quantick_control_host::authority::CANVAS_MODULE_ID;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Nine panes with their modes, the preset id and the flags fit well inside.
const RESULT_MAX_BYTES: usize = 2048;

pub const OPEN_WINDOW_MENU_CAPABILITY_ID: &str = "canvas.window_menu.open";
pub const PICK_LAYOUT_CAPABILITY_ID: &str = "canvas.layout.pick";
pub const MINIMIZE_CAPABILITY_ID: &str = "canvas.pane.minimize";
pub const MAXIMIZE_CAPABILITY_ID: &str = "canvas.pane.maximize";
pub const RESTORE_CAPABILITY_ID: &str = "canvas.pane.restore";
pub const REMOVE_CAPABILITY_ID: &str = "canvas.pane.remove";
pub const ADD_WIDGET_CAPABILITY_ID: &str = "canvas.widget.add";
pub const FLOAT_CAPABILITY_ID: &str = "canvas.pane.float";
pub const DOCK_CAPABILITY_ID: &str = "canvas.pane.dock";
pub const PARK_CAPABILITY_ID: &str = "canvas.pane.park";
pub const POP_OUT_CAPABILITY_ID: &str = "canvas.pane.pop_out";
pub const RETURN_CAPABILITY_ID: &str = "canvas.pane.return";
pub const RESIZE_CAPABILITY_ID: &str = "canvas.pane.resize";
pub const SEAM_DRAG_CAPABILITY_ID: &str = "canvas.seam.drag";
/// The longest seam drag a call takes, in points either way.
pub const MAX_SEAM_TRAVEL_PT: u32 = 100_000;

/// Decimal places a pane's share is written and accepted with: the same as
/// `workspace.summary`'s split fraction, so a sent share reads back as sent.
pub const SHARE_DECIMAL_PLACES: u32 = 6;

/// Every canvas capability, in the order the parent feature lists the actions.
pub const CANVAS_CAPABILITY_IDS: &[&str] = &[
    OPEN_WINDOW_MENU_CAPABILITY_ID,
    PICK_LAYOUT_CAPABILITY_ID,
    MINIMIZE_CAPABILITY_ID,
    MAXIMIZE_CAPABILITY_ID,
    RESTORE_CAPABILITY_ID,
    REMOVE_CAPABILITY_ID,
    ADD_WIDGET_CAPABILITY_ID,
    FLOAT_CAPABILITY_ID,
    DOCK_CAPABILITY_ID,
    PARK_CAPABILITY_ID,
    POP_OUT_CAPABILITY_ID,
    RETURN_CAPABILITY_ID,
    RESIZE_CAPABILITY_ID,
    SEAM_DRAG_CAPABILITY_ID,
];

/// A side of the canvas, or of a docked chart, a floating chart docks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// A canvas corner a floating chart parks in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What a pane is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasPaneKind {
    Time,
    Flow,
}

/// How a pane is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasPaneMode {
    Docked,
    Minimized,
    Maximized,
    Floating,
    OsWindow,
}

/// Which way a docked chart is resized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasDimension {
    /// Along a split that lays charts left to right.
    Width,
    /// Along a split that lays charts top to bottom.
    Height,
}

/// A rectangle in canvas points (a monitor's points for an OS window).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanvasRectDto {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowMenuInput {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WindowMenuResult {
    /// The window menu is up, hung from the toolbar's list door.
    pub open: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PickLayoutInput {
    /// A preset id from the layout registry.
    pub preset_id: String,
}

/// One pane, named by its slot.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaneInput {
    pub slot: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePaneInput {
    pub slot: u32,
    /// Required when the saved workspace holds the pane: the trader's dialog
    /// asks first, and so does this call.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddWidgetInput {
    /// A widget id from the add menu's registry.
    pub widget_id: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FloatPaneInput {
    pub slot: u32,
    /// Where it floats; absent centres it over the canvas.
    #[serde(default)]
    pub rect: Option<CanvasRectDto>,
}

/// Where a chart docks: exactly one of `edge`, `beside` with `side`, or
/// `swap_with`. A floating chart docks there; a docked chart moves there.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DockPaneInput {
    pub slot: u32,
    /// A canvas edge: the chart takes a fair share of the canvas there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<CanvasEdge>,
    /// A docked chart's slot: this chart docks on its `side`. When that
    /// chart's split already lays charts that way, this one joins the split
    /// beside it and every chart in it gives a fair share; otherwise that
    /// chart splits in half with this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside: Option<u32>,
    /// The side of `beside` the chart docks on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<CanvasEdge>,
    /// A docked chart's slot: this chart takes its place. A floating chart
    /// leaves it floating where this one stood; two docked charts trade
    /// places, both staying docked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap_with: Option<u32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParkPaneInput {
    pub slot: u32,
    pub corner: CanvasCorner,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PopOutInput {
    pub slot: u32,
    /// The monitor the window opens on; absent keeps Quantick's.
    #[serde(default)]
    pub monitor: Option<String>,
    /// The window's place on that monitor; absent keeps the pane's size.
    #[serde(default)]
    pub rect: Option<CanvasRectDto>,
}

/// Resize a docked chart by giving it a share of the layout around it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResizePaneInput {
    pub slot: u32,
    /// `width` resizes it within the nearest split laying charts left to
    /// right, `height` within the nearest laying them top to bottom.
    pub dimension: CanvasDimension,
    /// The share of that split the chart's branch takes, strictly between
    /// 0 and 1, with at most six decimal places. Its neighbour gives or
    /// takes the difference; both keep the minimum a drag keeps.
    pub share: CanonicalDecimal,
}

/// Drag one seam of the docked tree, as the hand drags it: pressed, moved
/// `travel` points along its split, released.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SeamDragInput {
    /// The child indices from the root down to the split the seam divides,
    /// as the scene's `pane_menu.<slot>.rail` neighbours and `tree_seams` list
    /// them; empty for the root split.
    #[serde(default)]
    #[schemars(length(max = 8))]
    pub path: Vec<u32>,
    /// The seam after child `index` of that split.
    pub index: u32,
    /// Points along the split, a decimal string of at most six places and
    /// at most [`MAX_SEAM_TRAVEL_PT`] either way: positive moves the seam
    /// right or down. Within the collapse threshold of the pair's edge that
    /// side folds to its rail; dragged from a rail back past it, it reopens.
    pub travel: CanonicalDecimal,
}

/// One pane of the canvas after the call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CanvasPaneDto {
    pub slot: u32,
    pub kind: CanvasPaneKind,
    pub mode: CanvasPaneMode,
    /// A docked chart's share of the nearest split laying charts left to
    /// right, to six places; absent when none holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_share: Option<CanonicalDecimal>,
    /// The same along the nearest split laying them top to bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_share: Option<CanonicalDecimal>,
    /// A `minimized` chart a seam drag or a collapse call folded to a rail,
    /// not to its menu strip; absent otherwise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub railed: bool,
}

/// One tab's canvas as the workspace summary reads it: the readback every
/// canvas edit reconciles against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CanvasView {
    /// The preset the docked panes match; absent for a custom arrangement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    /// Every pane, in slot order.
    pub panes: Vec<CanvasPaneDto>,
    /// Where each floating pane stands, in whole points: over the canvas, or
    /// on `monitor` for one in its own window.
    #[serde(default)]
    pub floating: Vec<CanvasPlaceDto>,
}

/// A floating pane's place, rounded to whole points.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CanvasPlaceDto {
    pub slot: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The canvas as it stands after a canvas call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CanvasResult {
    /// Whether the call changed the canvas; a repeat answers false.
    pub changed: bool,
    /// The pane the call acted on, or the one it added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
    /// The preset the docked panes match; absent for a custom arrangement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    /// Whether the workspace now differs from its save.
    pub unsaved: bool,
    /// Every pane, in slot order.
    pub panes: Vec<CanvasPaneDto>,
}

fn canvas_descriptor<Input: JsonSchema, Output: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
) -> CapabilityDescriptor {
    let mut descriptor =
        crate::layout::descriptor(id, title, description, generated_schema::<Input>());
    descriptor.module =
        quantick_control::id::ModuleId::new(CANVAS_MODULE_ID).expect("static module ID is valid");
    descriptor.output_schema = generated_schema::<Output>();
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

/// The descriptor of canvas capability `id`, or `None` for another name.
#[must_use]
pub fn descriptor(id: &str) -> Option<CapabilityDescriptor> {
    let descriptor = match id {
        OPEN_WINDOW_MENU_CAPABILITY_ID => {
            let mut descriptor = crate::layout::transient_descriptor::<
                WindowMenuInput,
                WindowMenuResult,
            >(
                id,
                CANVAS_MODULE_ID,
                "Open the window menu",
                "Opens the window menu below the toolbar's list door, as a click on it does: the same rows as the title bar's menu, its submenus opening rightwards.",
                "Opening an open menu leaves it open.",
            );
            descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
            descriptor
        }
        PICK_LAYOUT_CAPABILITY_ID => canvas_descriptor::<PickLayoutInput, CanvasResult>(
            id,
            "Pick a canvas layout",
            "Lays the docked charts out as a preset from the layout catalogue, as a cell of the layout popover does. Charts already docked keep their place where the preset has one of their kind; folds and maximize are cleared; floating charts stay.",
        ),
        MINIMIZE_CAPABILITY_ID => canvas_descriptor::<PaneInput, CanvasResult>(
            id,
            "Minimize a chart",
            "Folds a docked chart to a strip at its edge; the other charts take the space. Never asks. The last unfolded chart is refused.",
        ),
        MAXIMIZE_CAPABILITY_ID => canvas_descriptor::<PaneInput, CanvasResult>(
            id,
            "Maximize a chart",
            "Fills the canvas with one docked chart until canvas.pane.restore, Esc or another maximize.",
        ),
        RESTORE_CAPABILITY_ID => canvas_descriptor::<PaneInput, CanvasResult>(
            id,
            "Restore a chart",
            "Unfolds a minimized chart, or brings a maximized one back to its place.",
        ),
        REMOVE_CAPABILITY_ID => canvas_descriptor::<RemovePaneInput, CanvasResult>(
            id,
            "Remove a chart",
            "Takes a chart off the canvas. When the saved workspace holds it the call is refused unless confirm is true, as the trader's dialog asks first. Removing the last flow chart lays the rest out as a TIMEFRAMES layout; the last docked chart stays.",
        ),
        ADD_WIDGET_CAPABILITY_ID => canvas_descriptor::<AddWidgetInput, CanvasResult>(
            id,
            "Add a widget",
            "Opens a new chart from the add menu's registry, floating and centred over the canvas. The result's slot names it.",
        ),
        FLOAT_CAPABILITY_ID => canvas_descriptor::<FloatPaneInput, CanvasResult>(
            id,
            "Float a chart",
            "Floats a chart over the canvas at rect: a docked chart leaves the layout, a floating one moves.",
        ),
        DOCK_CAPABILITY_ID => canvas_descriptor::<DockPaneInput, CanvasResult>(
            id,
            "Dock a chart",
            "Inserts a floating chart into the layout, as a drop does: at a canvas edge (edge), beside a docked chart (beside with side: it joins that chart's split along that side for a fair share, or that chart splits in half), or in a docked chart's place (swap_with: that chart floats where this one stood over the canvas; a chart in its own window cannot swap). A docked chart named in slot moves there instead, and swapping two docked charts trades their places. Name exactly one. The result names the preset the layout now matches, if any.",
        ),
        PARK_CAPABILITY_ID => canvas_descriptor::<ParkPaneInput, CanvasResult>(
            id,
            "Park a chart in a corner",
            "Snaps a chart floating over the canvas into a corner with a 12 px margin, keeping its size.",
        ),
        POP_OUT_CAPABILITY_ID => canvas_descriptor::<PopOutInput, CanvasResult>(
            id,
            "Open a chart in its own window",
            "Moves a chart into its own OS window, as Open in new window does. It keeps its feed and its bars.",
        ),
        RETURN_CAPABILITY_ID => canvas_descriptor::<PaneInput, CanvasResult>(
            id,
            "Return a chart to the canvas",
            "Brings a chart from its own window back over the canvas, floating and centred.",
        ),
        SEAM_DRAG_CAPABILITY_ID => canvas_descriptor::<SeamDragInput, CanvasResult>(
            id,
            "Drag a seam between docked charts",
            "Runs the hand's seam drag: the seam after child index of the split at path moves travel points; within the collapse threshold of the pair's edge that side folds to its rail (the result's panes read railed), and travel back past it from a rail reopens it. slot names the first chart that folded or reopened. Refused with canvas.no_seam while a chart is maximized or the seam does not exist.",
        ),
        RESIZE_CAPABILITY_ID => canvas_descriptor::<ResizePaneInput, CanvasResult>(
            id,
            "Resize a docked chart",
            "Moves the seam between a docked chart and its neighbour, as dragging the divider between them does: the chart's branch takes share (0..1, six places) of the nearest split along dimension, held to the minimum a drag keeps. The result's panes carry each chart's width_share and height_share.",
        ),
        _ => return None,
    };
    Some(descriptor)
}

/// How a client reconciles an interrupted canvas call: the window menu reads
/// back as the list door's scene control, every canvas edit as the tab's
/// `canvas` in the workspace summary.
pub const READBACKS: &[Readback] = &[
    snapshot(
        OPEN_WINDOW_MENU_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::scene::CONTROLS_SCOPE_ID,
        "controls[].selected",
        "the toolbar.window_menu control is listed and selected",
        &[EVERY_OPTIONAL_TEST],
    ),
    edit(
        PICK_LAYOUT_CAPABILITY_ID,
        "the tab's canvas names the picked preset",
    ),
    edit(MINIMIZE_CAPABILITY_ID, "the pane reads minimized"),
    edit(MAXIMIZE_CAPABILITY_ID, "the pane reads maximized"),
    edit(RESTORE_CAPABILITY_ID, "the pane reads docked"),
    edit(REMOVE_CAPABILITY_ID, "the pane is no longer listed"),
    edit(ADD_WIDGET_CAPABILITY_ID, "a new floating pane is listed"),
    edit(FLOAT_CAPABILITY_ID, "the pane reads floating"),
    edit(DOCK_CAPABILITY_ID, "the pane reads docked"),
    edit(
        PARK_CAPABILITY_ID,
        "the pane still reads floating; its place moved",
    ),
    edit(POP_OUT_CAPABILITY_ID, "the pane reads os_window"),
    edit(RETURN_CAPABILITY_ID, "the pane reads floating"),
    edit(
        RESIZE_CAPABILITY_ID,
        "the pane's width_share or height_share reads the applied share",
    ),
    edit(
        SEAM_DRAG_CAPABILITY_ID,
        "the folded side reads railed, or the reopened side no longer does",
    ),
];

const fn edit(capability: &'static str, applied: &'static str) -> Readback {
    snapshot(
        capability,
        IdempotencyPolicy::Optional,
        crate::workspace::SCOPE_ID,
        "tabs[].canvas",
        applied,
        &[EVERY_OPTIONAL_TEST],
    )
}

/// Published schemas of the canvas capabilities.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "canvas-window-menu-input-v1.schema.json",
            schema::<WindowMenuInput>(),
        ),
        (
            "canvas-window-menu-result-v1.schema.json",
            schema::<WindowMenuResult>(),
        ),
        (
            "canvas-pick-layout-input-v1.schema.json",
            schema::<PickLayoutInput>(),
        ),
        ("canvas-pane-input-v1.schema.json", schema::<PaneInput>()),
        (
            "canvas-remove-input-v1.schema.json",
            schema::<RemovePaneInput>(),
        ),
        (
            "canvas-add-widget-input-v1.schema.json",
            schema::<AddWidgetInput>(),
        ),
        (
            "canvas-float-input-v1.schema.json",
            schema::<FloatPaneInput>(),
        ),
        (
            "canvas-dock-input-v1.schema.json",
            schema::<DockPaneInput>(),
        ),
        (
            "canvas-park-input-v1.schema.json",
            schema::<ParkPaneInput>(),
        ),
        (
            "canvas-pop-out-input-v1.schema.json",
            schema::<PopOutInput>(),
        ),
        (
            "canvas-resize-input-v1.schema.json",
            schema::<ResizePaneInput>(),
        ),
        (
            "canvas-seam-drag-input-v1.schema.json",
            schema::<SeamDragInput>(),
        ),
        ("canvas-result-v1.schema.json", schema::<CanvasResult>()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_canvas_id_has_one_descriptor_and_extra_fields_are_rejected() {
        for id in CANVAS_CAPABILITY_IDS {
            let descriptor = descriptor(id).unwrap_or_else(|| panic!("{id} has no descriptor"));
            assert_eq!(descriptor.id.as_str(), *id);
            assert!(!descriptor.description.is_empty());
        }
        assert!(descriptor("canvas.unknown").is_none());
        let extra = serde_json::json!({"slot": 1, "tab_id": "1"});
        assert!(serde_json::from_value::<PaneInput>(extra).is_err());
        assert!(
            serde_json::from_value::<DockPaneInput>(
                serde_json::json!({"slot": 1, "edge": "middle"})
            )
            .is_err()
        );
        let beside = serde_json::json!({"slot": 3, "beside": 1, "side": "bottom"});
        let beside = serde_json::from_value::<DockPaneInput>(beside).unwrap();
        assert_eq!(
            (beside.beside, beside.side),
            (Some(1), Some(CanvasEdge::Bottom))
        );
        let edge =
            serde_json::from_value::<DockPaneInput>(serde_json::json!({"slot": 3, "edge": "left"}));
        assert_eq!(
            edge.unwrap().edge,
            Some(CanvasEdge::Left),
            "edge alone still docks"
        );
        let resize = |share: serde_json::Value| {
            serde_json::from_value::<ResizePaneInput>(
                serde_json::json!({"slot": 0, "dimension": "width", "share": share}),
            )
        };
        assert!(resize("0.6".into()).is_ok());
        assert!(resize(serde_json::json!(0.6)).is_err(), "no JSON floats");
        assert!(resize("0.60".into()).is_err(), "canonical decimals only");
    }
}
