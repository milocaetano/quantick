#![cfg(test)]

use crate::indicator_host::{
    AttachedScript, IndicatorGuidePort, IndicatorPickerPort, IndicatorRefusal, NativePort,
    ScriptEditorPort, ScriptsPort,
};
use crate::wire::indicator_picker::{PickerInput, PickerResult};
use crate::wire::native_indicator as wire;
use crate::wire::script_editor::{EditorResult, NumericValue, ScriptDocument, ScriptEditorAction};
use quantick_control_host::wire::PaneSideDto;
use quantick_stores::indicator_state::SavedIndicator;

/// A fake indicator owner: what each indicator and script call reached it
/// with, and the refusal it answers when a test sets one.
#[derive(Default)]
pub(crate) struct FakeIndicators {
    /// Every script attached: name, source, attached by an operator.
    pub attached: Vec<(String, String, bool)>,
    /// Every guide set: tab, pane, slot, on.
    pub guides: Vec<(u64, u64, u64, bool)>,
    /// Every library script added to the chart.
    pub added: Vec<String>,
    /// Every editor action run.
    pub actions: Vec<ScriptEditorAction>,
    /// Every native edit applied: slot, edit.
    pub applied: Vec<(Option<u64>, wire::Prepared)>,
    /// The saved native indicators every target reads.
    pub saved: Vec<(u64, SavedIndicator, bool)>,
    /// What the picker, guide, editor and `saved` calls answer instead of
    /// succeeding; attach, detach and `apply` keep their own rules.
    pub refusal: Option<IndicatorRefusal>,
}

impl FakeIndicators {
    fn refused(&self) -> Result<(), IndicatorRefusal> {
        self.refusal.clone().map_or(Ok(()), Err)
    }
}

impl IndicatorPickerPort for &mut FakeIndicators {
    fn indicator_picker(&mut self, input: PickerInput) -> Result<PickerResult, IndicatorRefusal> {
        self.refused()?;
        Ok(PickerResult {
            open: true,
            tab_id: None,
            pane_id: None,
            query: String::new(),
            category: String::new(),
            categories: Vec::new(),
            entries: Vec::new(),
            total: 0,
            next_offset: Some(input.offset),
        })
    }
}

impl ScriptsPort for &mut FakeIndicators {
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> AttachedScript {
        self.attached.push((name, source, by_operator));
        AttachedScript {
            tab_id: 1,
            pane_side: PaneSideDto::Flow,
            slot: self.attached.len() as u64 - 1,
        }
    }
    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, IndicatorRefusal> {
        let Some((_, _, by_operator)) = self.attached.get(slot as usize) else {
            return Ok(false);
        };
        if *by_operator {
            Ok(true)
        } else {
            Err(IndicatorRefusal::TradersOwn)
        }
    }
}

impl IndicatorGuidePort for &mut FakeIndicators {
    fn set_indicator_guide(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        slot: u64,
        enabled: bool,
    ) -> Result<(), IndicatorRefusal> {
        self.refused()?;
        self.guides.push((tab_id, pane_id, slot, enabled));
        Ok(())
    }
}

impl ScriptEditorPort for &mut FakeIndicators {
    fn set_script_numeric_input(
        &mut self,
        _id: &str,
        _name: &str,
        _value: NumericValue,
    ) -> Result<(), IndicatorRefusal> {
        self.refused()
    }
    fn add_library_script(&mut self, id: &str) -> Result<(), IndicatorRefusal> {
        self.refused()?;
        self.added.push(id.to_owned());
        Ok(())
    }
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, IndicatorRefusal> {
        self.refused()?;
        self.actions.push(action);
        Ok(None)
    }
    fn script_library_state(&self, _offset: usize, _limit: usize) -> EditorResult {
        EditorResult::default()
    }
}

impl NativePort for &mut FakeIndicators {
    fn saved(
        &mut self,
        _target: &wire::Target,
    ) -> Result<Vec<(u64, SavedIndicator, bool)>, IndicatorRefusal> {
        self.refused()?;
        Ok(self.saved.clone())
    }
    fn settings_open(&self) -> bool {
        false
    }
    fn apply(
        &mut self,
        _target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, IndicatorRefusal> {
        self.applied.push((slot, edit));
        Ok(slot.unwrap_or(7))
    }
}
