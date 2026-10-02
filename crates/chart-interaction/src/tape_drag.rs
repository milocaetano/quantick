//! Classify a Tape drag once from its initial direction; vertical wobble stays live.

/// Travel before the hand's initial direction can be distinguished from a click.
const DRAG_JUDGED_AFTER_PX: f32 = 6.0;

/// Per-canvas gesture state; the caller stores it beside its input surface.
#[derive(Clone, Copy, Debug, Default)]
pub struct TapeDrag {
    judged: Option<(u64, bool)>,
}
impl TapeDrag {
    /// Supplied press identity and displacement from that press, in screen pixels.
    /// Only a drag that sets off sideways may move Tape time; Y remains independent.
    pub fn moves_time(&mut self, pressed_at: Option<f64>, travel: [f32; 2]) -> bool {
        let Some(press) = pressed_at.map(f64::to_bits) else {
            return false;
        };
        if let Some((judged, sideways)) = self.judged
            && judged == press
        {
            return sideways;
        }
        let [x, y] = travel;
        if x.hypot(y) < DRAG_JUDGED_AFTER_PX {
            return false;
        }
        let sideways = x.abs() > y.abs();
        self.judged = Some((press, sideways));
        sideways
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direction_latches_until_a_new_press_and_small_travel_is_not_a_pan() {
        let mut drag = TapeDrag::default();
        assert!(!drag.moves_time(None, [100.0, 0.0]));
        assert!(!drag.moves_time(Some(1.0), [5.0, 0.0]));
        assert!(!drag.moves_time(Some(1.0), [1.0, 8.0]));
        assert!(!drag.moves_time(Some(1.0), [100.0, 9.0]));
        assert!(drag.moves_time(Some(2.0), [8.0, 1.0]));
        assert!(drag.moves_time(Some(2.0), [1.0, 100.0]));
        assert!(!drag.moves_time(Some(3.0), [6.0, 6.0]));
    }
}
