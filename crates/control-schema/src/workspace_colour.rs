//! A workspace's colour palette as named calls: `workspace.colour.palette`
//! reads what the header swatch's popover shows, and
//! `workspace.colour.popover` drives the popover as a hand does, through
//! the same state, so an agent opens it, picks, types a Custom colour and
//! answers the swap question without a mouse. `workspace.colour.set`, in
//! `workspace_book`, sets a colour outright.

use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control_host::authority::{OBSERVE_PERMISSION_ID, ReadSpec, read_descriptor};
use quantick_stores::workspaces::palette::{ColourWarning, Palette, Swatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::transient_descriptor;
use crate::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};
use crate::workspace::{MODULE_ID, WorkspaceColourDto};
use crate::workspace_book::{WORKSPACE_COLOUR_SET_EVENT_KIND, writes_the_book};

pub const WORKSPACE_COLOUR_PALETTE_CAPABILITY_ID: &str = "workspace.colour.palette";
pub const WORKSPACE_COLOUR_POPOVER_CAPABILITY_ID: &str = "workspace.colour.popover";
/// The `details.code` of a popover act other than `open` while it is closed.
pub const POPOVER_CLOSED_CODE: &str = "workspace.colour_popover_closed";

/// One warning a colour raises; none blocks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColourWarningDto {
    /// Hard to tell from this workspace's colour (OKLab ΔE below 0.03 in
    /// either theme).
    Close { workspace: String },
    /// Within 0.06 OKLab of a theme's buy green.
    NearBuy,
    /// Within 0.06 OKLab of a theme's sell red.
    NearSell,
    /// Below 1.5:1 on the `dark` or `light` tab strip.
    LowContrast { theme: String },
}

impl From<&ColourWarning> for ColourWarningDto {
    fn from(warning: &ColourWarning) -> Self {
        match warning {
            ColourWarning::Close { workspace, .. } => Self::Close {
                workspace: workspace.clone(),
            },
            ColourWarning::NearBuy => Self::NearBuy,
            ColourWarning::NearSell => Self::NearSell,
            ColourWarning::LowContrast { dark } => Self::LowContrast {
                theme: if *dark { "dark" } else { "light" }.to_owned(),
            },
        }
    }
}

/// Read one workspace's palette.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceColourPaletteInput {
    /// The workspace. Left out, the one the tab on screen uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One swatch as the popover shows it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct PaletteSwatchDto {
    /// What `workspace.colour.popover`'s pick names it by: a preset's
    /// lower-case name, or `last_custom`.
    pub key: String,
    /// The words under it: the preset's name or the custom `#RRGGBB`.
    pub name: String,
    pub colour: WorkspaceColourDto,
    /// The workspace's own colour.
    pub current: bool,
    /// The workspace holding it, if another does: a pick asks to swap.
    pub holder: Option<String>,
    /// The holder's initials, as the swatch shows them.
    pub initials: Option<String>,
}

impl From<&Swatch> for PaletteSwatchDto {
    fn from(swatch: &Swatch) -> Self {
        Self {
            key: swatch.key.clone(),
            name: swatch.name.clone(),
            colour: swatch.colour.into(),
            current: swatch.current,
            holder: swatch.holder.clone(),
            initials: swatch.initials.clone(),
        }
    }
}

/// The palette the header swatch opens: the sixteen presets, who holds
/// each, the last custom colour and the colour a new workspace would take.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceColourPaletteResult {
    pub workspace: String,
    pub current: WorkspaceColourDto,
    pub presets: Vec<PaletteSwatchDto>,
    /// The seventeenth swatch: the last custom colour applied, once one was.
    pub last_custom: Option<PaletteSwatchDto>,
    pub next_free: WorkspaceColourDto,
}

impl From<&Palette> for WorkspaceColourPaletteResult {
    fn from(palette: &Palette) -> Self {
        Self {
            workspace: palette.workspace.clone(),
            current: palette.current.into(),
            presets: palette.presets.iter().map(Into::into).collect(),
            last_custom: palette.last_custom.as_ref().map(Into::into),
            next_free: palette.next_free.into(),
        }
    }
}

/// One act on the popover of the tab on screen, as a hand does it.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "act", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceColourPopoverInput {
    /// Open the workspace menu and its popover.
    Open,
    /// Close the popover; the menu stays.
    Close,
    /// Press a swatch: a preset's name or `last_custom`.
    Pick { colour: String },
    /// Open Custom, on `hex` or the current colour's Dark value.
    Custom {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hex: Option<String>,
    },
    /// Custom's Apply.
    Apply,
    /// Answer the swap question: Swap (`true`) or Cancel.
    Answer { swap: bool },
}

/// The popover after the act.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceColourPopoverResult {
    pub open: bool,
    pub workspace: String,
    /// `closed`, `grid`, `custom` (open under the grid) or `question`.
    pub face: String,
    /// The swap question, while it stands in the grid's place.
    pub question: Option<String>,
    /// Custom's field, while it is open.
    pub hex: Option<String>,
    /// Why Apply is disabled: the field is no colour.
    pub hex_hint: Option<String>,
    /// What Custom's colour warns, live; none blocks.
    pub warnings: Vec<ColourWarningDto>,
    /// What the trader was told: after a swap, the workspace that changed.
    pub note: Option<String>,
}

pub fn workspace_colour_palette_descriptor() -> CapabilityDescriptor {
    read_descriptor::<WorkspaceColourPaletteInput, WorkspaceColourPaletteResult>(&ReadSpec {
        id: WORKSPACE_COLOUR_PALETTE_CAPABILITY_ID,
        module: MODULE_ID,
        title: "Read a workspace's colour palette",
        description: "Reads the palette the workspace menu's header swatch opens, for one workspace (left out, the active tab's): the sixteen presets with their index, name and Light and Dark values, which is current, which another workspace holds with its initials (a pick asks to swap), the last custom colour applied (the seventeenth swatch) and the colour a new workspace would take next. Set a colour with workspace.colour.set or drive the popover with workspace.colour.popover.",
        permissions: &[OBSERVE_PERMISSION_ID, "observe.workspace"],
        pagination: None,
    })
}

pub fn workspace_colour_popover_descriptor() -> CapabilityDescriptor {
    let mut descriptor = writes_the_book(transient_descriptor::<
        WorkspaceColourPopoverInput,
        WorkspaceColourPopoverResult,
    >(
        WORKSPACE_COLOUR_POPOVER_CAPABILITY_ID,
        MODULE_ID,
        "Drive the workspace colour popover",
        "Acts on the palette popover under the workspace menu's header swatch, for the tab on screen, as a hand does: open (the menu too), close (the menu stays), pick a swatch (a free colour applies at once and closes the popover; one another workspace holds puts the swap question in the grid's place), custom (opens the Custom picker on a hex, #RRGGBB or RRGGBB, and answers its warnings: close to another workspace's colour, near the buy green or sell red, low contrast on a tab strip; none blocks), apply (Custom's colour, through the same path; a malformed hex answers hex_hint and changes nothing; an identical colour asks to swap) and answer (swap true takes the colour and gives the holder this workspace's old one, false cancels). Each answer is the popover's state. Any act but open while it is closed is refused with workspace.colour_popover_closed.",
        "Opening twice leaves it open; a pick names its colour, so a repeat answers no change; the question is answered once.",
    ));
    descriptor.idempotency = IdempotencyPolicy::Forbidden;
    descriptor
}

/// How a client reconciles an interrupted act.
pub const READBACKS: &[Readback] = &[journal(
    WORKSPACE_COLOUR_POPOVER_CAPABILITY_ID,
    IdempotencyPolicy::Forbidden,
    WORKSPACE_COLOUR_SET_EVENT_KIND,
    "payload.colour",
    "an event after the pre-call cursor gives the workspace the picked colour's word; opening, closing and Custom's field change no file",
    &[EVERY_FORBIDDEN_TEST],
)];

/// Published schemas of the colour calls.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "workspace-colour-palette-input-v1.schema.json",
            schema::<WorkspaceColourPaletteInput>(),
        ),
        (
            "workspace-colour-palette-result-v1.schema.json",
            schema::<WorkspaceColourPaletteResult>(),
        ),
        (
            "workspace-colour-popover-input-v1.schema.json",
            schema::<WorkspaceColourPopoverInput>(),
        ),
        (
            "workspace-colour-popover-result-v1.schema.json",
            schema::<WorkspaceColourPopoverResult>(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Each act reads as the description names it; a stranger is refused.
    #[test]
    fn the_popover_acts_read_their_words() {
        let read = |value| serde_json::from_value::<WorkspaceColourPopoverInput>(value);
        assert!(matches!(read(json!({"act": "open"})), Ok(WorkspaceColourPopoverInput::Open)));
        let pick = read(json!({"act": "pick", "colour": "sky"})).unwrap();
        assert!(matches!(pick, WorkspaceColourPopoverInput::Pick { colour } if colour == "sky"));
        let custom = read(json!({"act": "custom"})).unwrap();
        assert!(matches!(custom, WorkspaceColourPopoverInput::Custom { hex: None }));
        let answer = read(json!({"act": "answer", "swap": true})).unwrap();
        assert!(matches!(answer, WorkspaceColourPopoverInput::Answer { swap: true }));
        assert!(read(json!({"act": "paint"})).is_err());
        assert!(read(json!({"act": "pick"})).is_err());
    }

    /// A warning names its kind, and the workspace or the theme it is about.
    #[test]
    fn a_warning_names_its_kind_on_the_wire() {
        let close = ColourWarning::Close {
            workspace: "Scalp".to_owned(),
            colour: "Sky".to_owned(),
        };
        assert_eq!(
            json!(ColourWarningDto::from(&close)),
            json!({"kind": "close", "workspace": "Scalp"})
        );
        assert_eq!(json!(ColourWarningDto::NearBuy), json!({"kind": "near_buy"}));
        let low = ColourWarningDto::from(&ColourWarning::LowContrast { dark: false });
        assert_eq!(json!(low), json!({"kind": "low_contrast", "theme": "light"}));
    }
}
