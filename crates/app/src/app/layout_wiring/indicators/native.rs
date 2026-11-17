//! Map the headless native port to the existing layout edit owner.
use super::super::super::indicator_manager::IndicatorEdit;
use super::*;
use quantick_control_handlers::indicator_host::IndicatorRefusal;
use quantick_control_schema::native_indicator as wire;
impl LayoutAdapter<'_> {
    fn native_side(&self, target: &wire::Target) -> Result<PaneSide, IndicatorRefusal> {
        self.tabs
            .by_id(target.tab_id.get())
            .and_then(|tab| {
                tab.panes()
                    .find(|(pane, _)| pane.id == target.pane_id.get())
                    .map(|(_, side)| side)
            })
            .ok_or(IndicatorRefusal::UnknownTabOrPane)
    }
}
impl quantick_control_handlers::indicator_host::NativePort for LayoutAdapter<'_> {
    fn saved(
        &mut self,
        target: &wire::Target,
    ) -> Result<Vec<(u64, SavedIndicator, bool)>, IndicatorRefusal> {
        let side = self.native_side(target)?;
        let tab = target.tab_id.get();
        let pane = self
            .pane_at(tab, side)
            .ok_or(IndicatorRefusal::UnknownPane)?;
        Ok(self
            .layout_slots_at(tab, side)
            .iter()
            .filter_map(|slot| {
                let owner = self.slot_at(tab, side, *slot)?;
                let (layout, index) = self.edit_coordinates(owner)?;
                let entry = self
                    .store
                    .book()
                    .get(layout)?
                    .indicators
                    .get(index)?
                    .clone();
                Some((
                    slot.0,
                    entry,
                    pane.indicators
                        .all()
                        .iter()
                        .any(|v| v.slot == *slot && v.error.is_none()),
                ))
            })
            .collect())
    }
    fn settings_open(&self) -> bool {
        self.indicator_settings.model_state.dialog().is_some()
    }
    fn apply(
        &mut self,
        target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, IndicatorRefusal> {
        let side = self.native_side(target)?;
        let tab = target.tab_id.get();
        if let wire::Prepared::Add(id) = edit {
            let kind = SavedKind::native(&id);
            let slot = self
                .add_indicator_at(tab, side, &kind)
                .ok_or(IndicatorRefusal::NotAttachable)?;
            let owner = self
                .slot_at(tab, side, slot)
                .ok_or(IndicatorRefusal::UnknownOwner)?;
            self.mirror_add(owner, &kind);
            self.note_indicator_edit_at(tab, side);
            return Ok(slot.0);
        }
        let slot = SlotId(slot.ok_or(IndicatorRefusal::MissingSlot)?);
        let owner = self
            .slot_at(tab, side, slot)
            .ok_or(IndicatorRefusal::UnknownOwner)?;
        let edit = match edit {
            wire::Prepared::Inputs(values) => {
                self.pane_at(tab, side)
                    .ok_or(IndicatorRefusal::UnknownPane)?
                    .indicator_worker
                    .send(IndicatorCommand::SetInputs {
                        slot,
                        values: values.clone(),
                    });
                IndicatorEdit::Inputs(owner, values)
            }
            wire::Prepared::Style(plot, style) => {
                let view = self
                    .pane_mut_at(tab, side)
                    .and_then(|p| p.indicators.view_mut(slot))
                    .ok_or(IndicatorRefusal::ViewClosed)?;
                view.style.set(
                    plot,
                    crate::indicator_style::PlotOverride {
                        visible: style.visible,
                        color: style
                            .color
                            .map(|c| quantick_indicators::Rgba8::new(c[0], c[1], c[2], c[3])),
                        width: style.width,
                    },
                );
                IndicatorEdit::Style(owner)
            }
            wire::Prepared::Hidden(hidden) => {
                self.pane_mut_at(tab, side)
                    .and_then(|p| p.indicators.view_mut(slot))
                    .ok_or(IndicatorRefusal::ViewClosed)?
                    .hidden = hidden;
                IndicatorEdit::Hidden(owner)
            }
            wire::Prepared::Remove => IndicatorEdit::Remove(owner),
            wire::Prepared::Add(_) => unreachable!("add dispatched above"),
        };
        self.apply_indicator_edit(edit);
        Ok(slot.0)
    }
}
