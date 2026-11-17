//! The indicator owner's control ports: the bound workspace's indicators,
//! the picker, the script library and its editor, served by the split-borrow
//! adapter the window lends ([`IndicatorHost`]).
//!
//! [`IndicatorHost`]: quantick_control_handlers::indicator_host::IndicatorHost
use super::indicator_manager::IndicatorEdit;
use super::indicator_wiring::IndicatorWiring;
use super::script_editor_wiring;
use crate::indicator_worker::SlotId;
use quantick_control_handlers::indicator_host::{
    AttachedScript, IndicatorGuidePort, IndicatorPickerPort, IndicatorRefusal, NativePort,
    ScriptEditorPort, ScriptsPort,
};
use quantick_control_schema::indicator_picker::{PickerInput, PickerResult};
use quantick_control_schema::native_indicator as wire;
use quantick_control_schema::script_editor::{EditorResult, NumericValue};
use quantick_stores::indicator_state::SavedIndicator;
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction};

impl IndicatorPickerPort for IndicatorWiring<'_> {
    fn indicator_picker(&mut self, input: PickerInput) -> Result<PickerResult, IndicatorRefusal> {
        let target = (
            self.runtime.tabs.active_id(),
            self.runtime.active_tab().focused_pane().id,
        );
        Ok(quantick_ui_model::indicator_picker::Model::control(
            &mut self.surfaces.indicator_picker.model_state,
            target,
            self.runtime.indicators.script_library.entries(),
            input,
        )?)
    }
}

impl ScriptsPort for IndicatorWiring<'_> {
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> AttachedScript {
        let runtime = &mut *self.runtime;
        let active = runtime.tabs.active_index();
        let target = (
            runtime.tabs.active_id(),
            runtime.tabs[active].focused_side(),
        );
        let pane = runtime.tabs.runtime_mut(active).pane_mut(target.1);
        let attached = runtime
            .indicators
            .attach_script(pane, target, name, source, by_operator);
        let owner = attached.target;
        self.apply_indicator_edit(IndicatorEdit::Attached(attached));
        AttachedScript {
            tab_id: owner.tab,
            pane_side: crate::control::pane_side_dto(owner.side),
            slot: owner.slot.0,
        }
    }

    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, IndicatorRefusal> {
        let operator = self.runtime.indicators.operator_target(slot);
        let Some(target) = operator.map_err(|()| IndicatorRefusal::TradersOwn)? else {
            return Ok(false);
        };
        self.runtime
            .validate_target(target)
            .map_err(|_| IndicatorRefusal::TradersOwn)?;
        self.apply_indicator_edit(IndicatorEdit::Remove(target));
        Ok(true)
    }
}

impl ScriptEditorPort for IndicatorWiring<'_> {
    fn set_script_numeric_input(
        &mut self,
        id: &str,
        name: &str,
        value: NumericValue,
    ) -> Result<(), IndicatorRefusal> {
        script_editor_wiring::set_numeric_input(self, id, name, value)
    }
    fn add_library_script(&mut self, id: &str) -> Result<(), IndicatorRefusal> {
        script_editor_wiring::add_to_chart(self, id)
    }
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, IndicatorRefusal> {
        Ok(script_editor_wiring::apply(
            self.surfaces,
            &mut self.runtime.indicators,
            action,
        )?)
    }
    fn script_library_state(&self, offset: usize, limit: usize) -> EditorResult {
        script_editor_wiring::inspect(
            self.surfaces,
            &self.runtime.indicators,
            &self.runtime.tabs,
            self.runtime.workspace.layouts(),
            offset,
            limit,
        )
    }
}

impl NativePort for IndicatorWiring<'_> {
    fn saved(
        &mut self,
        target: &wire::Target,
    ) -> Result<Vec<(u64, SavedIndicator, bool)>, IndicatorRefusal> {
        self.layout().saved(target)
    }
    fn settings_open(&self) -> bool {
        self.surfaces
            .indicator_settings
            .model_state
            .dialog()
            .is_some()
    }
    fn apply(
        &mut self,
        target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, IndicatorRefusal> {
        self.layout().apply(target, slot, edit)
    }
}

impl IndicatorGuidePort for IndicatorWiring<'_> {
    fn set_indicator_guide(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        slot: u64,
        enabled: bool,
    ) -> Result<(), IndicatorRefusal> {
        let mut layout = self.layout();
        super::set_indicator_mouse_vertical_line(
            &mut layout,
            tab_id,
            pane_id,
            SlotId(slot),
            enabled,
        )
        .then_some(())
        .ok_or(IndicatorRefusal::NoLiveIndicator)
    }
}
