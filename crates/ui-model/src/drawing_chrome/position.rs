//! A position's floating bar asking for its per-tool default: the writes
//! go to the preset bank through the frame's ask, and the trader is owed
//! the same line the inspector's buttons give.
use quantick_appearance::PaneCanvas;
use quantick_drawings::PresetHost;
use quantick_drawings::position_edit::{PositionDefault, apply_position_default};

use super::{Drawing, DrawingChromeAsk, RecordingPresetHost, SavedDefault};

/// Record `action` for `drawing`'s tool into `ask`. A drawing of another
/// tool asks nothing.
pub fn ask_position_default(
    ask: &mut DrawingChromeAsk,
    bank: &dyn PresetHost,
    canvas: PaneCanvas,
    drawing: &Drawing,
    action: PositionDefault,
) {
    let ground = quantick_drawings::defaults_ground(bank, canvas);
    let mut recorder = RecordingPresetHost::new(bank);
    if apply_position_default(&mut recorder, canvas, drawing, action).is_err() {
        return;
    }
    ask.presets.extend(recorder.into_writes());
    ask.saved_default = Some(match action {
        PositionDefault::Save => SavedDefault::OneTool(ground),
        PositionDefault::Reset => SavedDefault::Forgotten(ground),
    });
}

#[cfg(test)]
#[path = "position_tests.rs"]
mod tests;
