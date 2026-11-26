//! A position's floating bar asking for its per-tool default: the writes
//! go to the preset bank through the frame's ask, and the trader is owed
//! the same line the inspector's buttons give.
use quantick_drawings::position_edit::{PositionDefault, apply_position_default};
use quantick_drawings::{PresetContext, PresetHost};

use super::{Drawing, DrawingChromeAsk, RecordingPresetHost, SavedDefault};

/// Record `action` for `drawing`'s tool into `ask`. A drawing of another
/// tool asks nothing.
pub fn ask_position_default(
    ask: &mut DrawingChromeAsk,
    presets: PresetContext<&dyn PresetHost>,
    drawing: &Drawing,
    action: PositionDefault,
) {
    let ground = presets.ground();
    let mut recorder = RecordingPresetHost::new(presets.host());
    if apply_position_default(&mut presets.on(&mut recorder), drawing, action).is_err() {
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
