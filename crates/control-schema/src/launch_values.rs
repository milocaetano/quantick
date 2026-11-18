//! A scripted launch's values read as the typed requests their owners take,
//! for the hooks no registered call stands for yet. Each reader refuses what
//! it cannot read rather than guessing: a typo in a validation script leaves
//! the state alone, and the caller reports it.

/// A `1` switch: exactly `1` is on, anything else, or no value, is off.
#[must_use]
pub fn one(value: Option<&str>) -> bool {
    value == Some("1")
}

/// A trimmed `1` or `0`; anything else is no answer.
#[must_use]
pub fn binary(value: &str) -> Option<bool> {
    match value.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// `1`, `true` or `on`; `0`, `false` or `off`; anything else is no answer.
#[must_use]
pub fn flag(value: &str) -> Option<bool> {
    match value.trim() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

/// A whole number above zero.
#[must_use]
pub fn positive(value: &str) -> Option<usize> {
    value
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|count| *count > 0)
}

/// Milliseconds, zero or more.
#[must_use]
pub fn non_negative_ms(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok().filter(|ms| *ms >= 0)
}

/// A comma-separated list, entries trimmed, blanks dropped.
pub fn names(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// What `QUANTICK_TAPE_LAYERS` asks of the tape's two lane layers: `None`
/// leaves a layer as it was, the last entry naming one wins.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LaneLayers {
    /// The heatmap behind the lane.
    pub depth: Option<bool>,
    /// The bubbles on it.
    pub bubbles: Option<bool>,
}

/// `heatmap`, `bubbles`, `no-heatmap`, `no-bubbles`, or `none` for both off.
#[must_use]
pub fn lane_layers(value: &str) -> LaneLayers {
    let mut layers = LaneLayers::default();
    for entry in names(value) {
        match entry {
            "none" => {
                return LaneLayers {
                    depth: Some(false),
                    bubbles: Some(false),
                };
            }
            "heatmap" => layers.depth = Some(true),
            "bubbles" => layers.bubbles = Some(true),
            "no-heatmap" => layers.depth = Some(false),
            "no-bubbles" => layers.bubbles = Some(false),
            _ => {}
        }
    }
    layers
}

/// `QUANTICK_REPLAY_AUTOSTART`: `1` loads and plays, `paused` loads and
/// waits; `Some(play)` when either.
#[must_use]
pub fn replay_autostart(value: Option<&str>) -> Option<bool> {
    match value.unwrap_or_default().trim() {
        "1" => Some(true),
        "paused" => Some(false),
        _ => None,
    }
}

/// A replay speed above zero, else normal speed.
#[must_use]
pub fn replay_speed(value: Option<&str>) -> f32 {
    value
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|speed| *speed > 0.0)
        .unwrap_or(1.0)
}

/// A trimmed, non-blank value.
#[must_use]
pub fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The one context pane to fold, `Some(upper)`, when exactly one of the two
/// is asked for.
#[must_use]
pub fn context_fold(upper: Option<&str>, lower: Option<&str>) -> Option<bool> {
    let (upper, lower) = (one(upper), one(lower));
    (upper ^ lower).then_some(upper)
}

#[cfg(test)]
#[path = "launch_values_tests.rs"]
mod tests;
