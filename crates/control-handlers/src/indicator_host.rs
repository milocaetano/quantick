//! The indicator and script families' one port: the window lends the owner
//! of the bound workspace's indicators, and that owner serves every call —
//! the picker, the scripts, the source editor, native indicators and the
//! per-indicator guide.
use quantick_control::error::{ControlError, codes};
use quantick_control_host::{admission::known_error, wire::PaneSideDto};
use quantick_control_schema::indicator_picker::{PickerInput, PickerResult};

use quantick_control_schema::native_indicator as wire;
use quantick_control_schema::script_editor::{
    EditorResult, NumericValue, ScriptDocument, ScriptEditorAction,
};
use quantick_stores::indicator_state::SavedIndicator;

/// The window's indicator job: lend the owner of the bound workspace's
/// indicators. Everything else is the owner's.
pub trait IndicatorHost {
    /// The owner every indicator and script call lands on.
    fn indicators(&mut self) -> Box<dyn IndicatorOwner + '_>;
}

/// Every port the indicator owner serves, as one object the window lends.
pub trait IndicatorOwner:
    IndicatorPickerPort + ScriptsPort + ScriptEditorPort + NativePort + IndicatorGuidePort
{
}

impl<T> IndicatorOwner for T where
    T: IndicatorPickerPort
        + ScriptsPort
        + ScriptEditorPort
        + NativePort
        + IndicatorGuidePort
        + ?Sized
{
}

/// The searchable indicator dialog's shared state.
pub trait IndicatorPickerPort {
    /// Drive the picker on the focused pane of the tab on screen.
    ///
    /// # Errors
    ///
    /// The picker's refusal of the input.
    fn indicator_picker(&mut self, input: PickerInput) -> Result<PickerResult, IndicatorRefusal>;
}

/// Indicator scripts an operator attaches and detaches.
pub trait ScriptsPort {
    /// Attach compiled `source` to the focused pane of the tab on screen, as
    /// an operator's or the trader's own; answers where it landed.
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> AttachedScript;
    /// Detach an operator-attached slot; `Ok(false)` when there was nothing
    /// to detach.
    ///
    /// # Errors
    ///
    /// [`IndicatorRefusal::TradersOwn`] when the slot is the trader's own.
    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, IndicatorRefusal>;
}

/// The per-indicator price-hover guide, shared by the pane menu and control.
pub trait IndicatorGuidePort {
    /// Switch the guide of `slot` on the pane `tab_id` and `pane_id` name.
    ///
    /// # Errors
    ///
    /// [`IndicatorRefusal::NoLiveIndicator`] when the target names no live
    /// non-price indicator.
    fn set_indicator_guide(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        slot: u64,
        enabled: bool,
    ) -> Result<(), IndicatorRefusal>;
}

/// Native indicators on a pane, as their edit and list read them.
pub trait NativePort {
    /// The saved indicators on `target`'s pane: slot, entry, and whether
    /// its view is live.
    ///
    /// # Errors
    ///
    /// The target names no open pane.
    fn saved(
        &mut self,
        target: &wire::Target,
    ) -> Result<Vec<(u64, SavedIndicator, bool)>, IndicatorRefusal>;
    fn settings_open(&self) -> bool;
    /// Apply a prepared edit; answers the slot it landed on.
    ///
    /// # Errors
    ///
    /// The target, its slot or its view is gone.
    fn apply(
        &mut self,
        target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, IndicatorRefusal>;
}

/// The script library and its editor, as the editor call drives them.
pub trait ScriptEditorPort {
    /// Set one numeric input of the open, attached script.
    ///
    /// # Errors
    ///
    /// Why the input cannot be edited now.
    fn set_script_numeric_input(
        &mut self,
        id: &str,
        name: &str,
        value: NumericValue,
    ) -> Result<(), IndicatorRefusal>;
    /// Attach library script `id` to the focused pane, once.
    ///
    /// # Errors
    ///
    /// The library holds no such script.
    fn add_library_script(&mut self, id: &str) -> Result<(), IndicatorRefusal>;
    /// Run one editor action; answers the document it leaves open.
    ///
    /// # Errors
    ///
    /// The editor refuses the action.
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, IndicatorRefusal>;
    /// The editor and one page of the library.
    fn script_library_state(&self, offset: usize, limit: usize) -> EditorResult;
}

/// Where an attached script landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachedScript {
    pub tab_id: u64,
    pub pane_side: PaneSideDto,
    pub slot: u64,
}

/// Why an indicator or script call was refused, so a caller branches on the
/// reason instead of parsing a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndicatorRefusal {
    /// No open tab has the id, or the tab has no pane with it.
    UnknownTabOrPane,
    /// The addressed pane is gone.
    UnknownPane,
    /// The slot belongs to no indicator owner.
    UnknownOwner,
    /// The edit names no slot.
    MissingSlot,
    /// The indicator's view closed under the edit.
    ViewClosed,
    /// The native indicator found no slot to attach to.
    NotAttachable,
    /// The guide's target is no live non-price indicator.
    NoLiveIndicator,
    /// The slot is the trader's own; operators detach only their own.
    TradersOwn,
    /// The editor holds a different script, or none.
    ScriptNotOpen,
    /// The indicator settings dialog is open over the inputs.
    SettingsOpen,
    /// The script is not attached to the focused pane.
    NotAttached,
    /// The attached script's view has not loaded yet.
    NotLoaded,
    /// The source or the attached view carries errors.
    SourceErrors,
    /// Validation or the chart reload is still running.
    NotValidated,
    /// The layout keeps no inputs for the script.
    InputsUnavailable,
    /// The library holds no script by that id.
    UnknownLibraryScript,
    /// A lower layer refused in its own words: the request schema, the
    /// picker model or the script library.
    Rejected(String),
}

impl IndicatorRefusal {
    /// Why, in the words the calls refuse with.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::UnknownTabOrPane => "unknown tab or pane",
            Self::UnknownPane => "unknown pane",
            Self::UnknownOwner => "unknown indicator owner",
            Self::MissingSlot => "missing slot",
            Self::ViewClosed => "indicator view closed",
            Self::NotAttachable => "native indicator could not be attached",
            Self::NoLiveIndicator => {
                "the target must name a live non-price indicator reported by analysis.indicators"
            }
            Self::TradersOwn => {
                "that indicator is the trader's own; this tier detaches only what an operator attached"
            }
            Self::ScriptNotOpen => "open the attached script before editing its inputs",
            Self::SettingsOpen => "close indicator settings before editing inputs through control",
            Self::NotAttached => "script is not attached to the focused pane",
            Self::NotLoaded => "wait for the attached script to load",
            Self::SourceErrors => "repair source errors before editing inputs",
            Self::NotValidated => {
                "wait for current source validation and chart reload before editing inputs"
            }
            Self::InputsUnavailable => "script input persistence is unavailable",
            Self::UnknownLibraryScript => "unknown library script",
            Self::Rejected(reason) => reason,
        }
    }
}

impl std::fmt::Display for IndicatorRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for IndicatorRefusal {}

impl From<String> for IndicatorRefusal {
    fn from(reason: String) -> Self {
        Self::Rejected(reason)
    }
}

impl From<IndicatorRefusal> for ControlError {
    fn from(refusal: IndicatorRefusal) -> Self {
        match refusal {
            IndicatorRefusal::TradersOwn => {
                known_error(codes::PERMISSION_DENIED, refusal.reason(), false)
            }
            refusal => ControlError::invalid_request(refusal.reason()),
        }
    }
}

#[cfg(test)]
#[path = "indicator_host_tests.rs"]
mod tests;
