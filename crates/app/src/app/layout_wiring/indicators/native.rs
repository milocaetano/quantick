//! Map the headless native port to the existing layout edit owner.
use super::super::super::indicator_manager::IndicatorEdit;
use super::*;
use quantick_control_schema::native_indicator as wire;
impl LayoutAdapter<'_> {
    fn native_side(&self, target: &wire::Target) -> Result<PaneSide, String> {
        self.tabs
            .by_id(target.tab_id.get())
            .and_then(|tab| {
                tab.panes()
                    .find(|(pane, _)| pane.id == target.pane_id.get())
                    .map(|(_, side)| side)
            })
            .ok_or_else(|| "unknown tab or pane".into())
    }
}
impl quantick_control_handlers::native_indicator::NativePort for LayoutAdapter<'_> {
    fn saved(&self, target: &wire::Target) -> Result<Vec<(u64, SavedIndicator, bool)>, String> {
        let side = self.native_side(target)?;
        let tab = target.tab_id.get();
        let pane = self.pane_at(tab, side).ok_or("unknown pane")?;
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
        self.indicators.indicator_settings.is_some()
    }
    fn apply(
        &mut self,
        target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, String> {
        let side = self.native_side(target)?;
        let tab = target.tab_id.get();
        if let wire::Prepared::Add(id) = edit {
            let kind = SavedKind::native(&id);
            let slot = self
                .add_indicator_at(tab, side, &kind)
                .ok_or("native indicator could not be attached")?;
            let owner = self
                .slot_at(tab, side, slot)
                .ok_or("unknown indicator owner")?;
            self.mirror_add(owner, &kind);
            self.note_indicator_edit_at(tab, side);
            return Ok(slot.0);
        }
        let slot = SlotId(slot.ok_or("missing slot")?);
        let owner = self
            .slot_at(tab, side, slot)
            .ok_or("unknown indicator owner")?;
        let edit = match edit {
            wire::Prepared::Inputs(values) => {
                self.pane_at(tab, side)
                    .ok_or("unknown pane")?
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
                    .ok_or("indicator view closed")?;
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
                    .ok_or("indicator view closed")?
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
