//! The workspace canvas's capabilities: the window menu door, the layout
//! catalogue, the per-chart menu (minimize, maximize, restore, remove), the
//! add button, and floating, docking, parking, popping out and returning a
//! chart. One named capability per action, each answering the canvas as it
//! stands afterwards, so a caller reads the effect off the result.
//!
//! The model behind them is `quantick_workspace::canvas`; these are its wire
//! words. Slots are the canvas's stable pane ids.

use quantick_control::registry::CapabilityDescriptor;
use quantick_control::schema::generated_schema;
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
];

/// A canvas edge a floating chart docks at.
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DockPaneInput {
    pub slot: u32,
    pub edge: CanvasEdge,
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

/// One pane of the canvas after the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CanvasPaneDto {
    pub slot: u32,
    pub kind: CanvasPaneKind,
    pub mode: CanvasPaneMode,
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
                crate::layout::LAYOUT_MODULE_ID,
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
            "Inserts a floating chart into the layout at a canvas edge, as a drop on that edge does. The result names the preset the layout now matches, if any.",
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
        _ => return None,
    };
    Some(descriptor)
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
    }
}
