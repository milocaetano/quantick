//! Canvas transactions over the tab and workspace owners, independent of painting.
use super::{CanvasAct, CanvasHost, CanvasResult, ControlError, Drawn, LayoutPickerResult};
use quantick_workspace::canvas::{CanvasRect, CanvasState, LayoutPreset};

/// The tab's model and the platform effects a canvas transaction can reach.
pub trait CanvasOwner {
    fn state(&self) -> &CanvasState;
    fn drawn(&self) -> Drawn;
    fn saved(&self) -> Option<CanvasState>;
    fn unsaved(&self) -> bool;
    fn origin(&self) -> (f32, f32);
    fn monitor(&self, rect: CanvasRect) -> Option<String>;
    fn pick_layout(&mut self, preset: &'static LayoutPreset);
    fn show_widget(&mut self, id: &'static str) -> Result<(), String>;
    /// Mirror the column split before and after acting, even when the act fails.
    fn act(
        &mut self,
        saved: Option<&CanvasState>,
        act: CanvasAct,
    ) -> Result<(bool, Option<u32>), ControlError>;
    fn open_window_menu(&mut self) -> bool;
    fn picker_read(&self) -> LayoutPickerResult;
}

pub fn answer(
    host: &mut (impl CanvasOwner + ?Sized),
    act: CanvasAct,
) -> Result<CanvasResult, ControlError> {
    let saved = host.saved();
    let act = super::placed_pop_out(
        act,
        (host.state(), host.drawn().area),
        host.origin(),
        |rect| host.monitor(rect),
    );
    let (changed, slot) = if let CanvasAct::PickLayout { preset_id } = &act {
        let before = host.state().clone();
        host.pick_layout(super::pick_on(&before, preset_id)?);
        (host.state() != &before, None)
    } else if let Some(id) = super::surface_widget(&act) {
        host.show_widget(id)
            .map_err(ControlError::invalid_request)?;
        (true, None)
    } else {
        host.act(saved.as_ref(), act)?
    };
    Ok(super::result(
        host.state(),
        changed,
        slot,
        host.unsaved(),
        host.drawn(),
    ))
}

/// Before the first paint the model uses the same nominal area for every host.
pub fn canvas_area(drawn: CanvasRect) -> CanvasRect {
    if drawn.w > 0.0 && drawn.h > 0.0 {
        drawn
    } else {
        CanvasRect::new(0.0, 0.0, 1280.0, 720.0)
    }
}

/// Consume scripted acts once, only after the canvas has drawn.
pub fn canvas_hooks(
    drawn: bool,
    hooked: &mut bool,
    inputs: impl FnOnce() -> (String, String),
) -> (bool, Vec<CanvasAct>) {
    if !drawn || std::mem::replace(hooked, true) {
        return (false, Vec::new());
    }
    let (add, acts) = inputs();
    (add == "1", super::hook_acts(&acts))
}

/// A pinned gesture survives later hook observations unchanged.
pub fn hold_from_hook(
    state: &CanvasState,
    value: impl FnOnce() -> String,
    area: CanvasRect,
    pinned: bool,
) -> Option<(quantick_workspace::canvas::FloatGesture, (f32, f32))> {
    if pinned {
        return None;
    }
    let (held, (x, y)) = super::held_by_hook(state, &value())?;
    Some((held, (area.x + x * area.w, area.y + y * area.h)))
}

pub(super) fn lend(
    host: &mut (impl CanvasHost + ?Sized),
) -> Result<Box<dyn CanvasOwner + '_>, ControlError> {
    host.canvas_owner().ok_or_else(super::unavailable)
}

#[cfg(test)]
mod tests;
