//! The window hooks: a validation run's requested size, and a maximised
//! first frame. Compiled only with `scenario-harness` (or under test); the
//! composition root captures them beside the configuration.
#![cfg(any(feature = "scenario-harness", test))]

use eframe::egui;
use std::ffi::OsString;

/// `QUANTICK_WINDOW_SIZE` and `QUANTICK_WINDOW_MAXIMIZED`, captured.
#[derive(Debug, Default, Clone)]
pub(crate) struct WindowHooks {
    size: Option<String>,
    maximize: bool,
}

impl WindowHooks {
    pub fn capture(lookup: &mut impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut text = |name: &str| lookup(name).and_then(|value| value.into_string().ok());
        Self {
            size: text("QUANTICK_WINDOW_SIZE"),
            maximize: text("QUANTICK_WINDOW_MAXIMIZED").is_some_and(|value| value == "1"),
        }
    }

    /// The size a validation run asked for, when it asked for one that parses.
    ///
    /// The hook exists because window size is not decoration here — it is what
    /// decides whether the indicator band has room for its panes and whether
    /// the time axis has room for its labels. Not clamped and not floored: a
    /// run proving the app survives a degenerate window has to be able to ask
    /// for one. A value that does not parse is ignored with a warning rather
    /// than failing the launch.
    pub fn requested_size(&self) -> Option<[f32; 2]> {
        let raw = self.size.as_deref()?;
        match parse_window_size(raw) {
            Some(size) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "APP_WINDOW_SIZE_OVERRIDE",
                    width = size[0],
                    height = size[1],
                    "opening at the requested window size"
                );
                Some(size)
            }
            None => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "APP_WINDOW_SIZE_REJECTED",
                    value = %raw,
                    action = "using_default",
                    "QUANTICK_WINDOW_SIZE is not WIDTHxHEIGHT"
                );
                None
            }
        }
    }

    pub fn into_window_state(self) -> WindowStartupState {
        WindowStartupState {
            maximize: self.maximize,
        }
    }
}

/// A window-manager command waiting for the first drawable frame.
#[derive(Default)]
pub(crate) struct WindowStartupState {
    maximize: bool,
}

impl WindowStartupState {
    /// Keep the first-frame command: the viewport builder's maximized flag is
    /// not honored beside inner_size on the supported eframe path.
    pub fn apply(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.maximize) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "WINDOW_MAXIMIZE_AUTOSTART",
            action = "maximize",
            "QUANTICK_WINDOW_MAXIMIZED asked for the maximised layout"
        );
    }
}

/// `WIDTHxHEIGHT` in pixels, as asked for. `None` when the text is not two
/// positive numbers.
fn parse_window_size(raw: &str) -> Option<[f32; 2]> {
    let (width, height) = raw.split_once(['x', 'X'])?;
    let width: f32 = width.trim().parse().ok()?;
    let height: f32 = height.trim().parse().ok()?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some([width, height])
}

crate::hooks::declare_hooks!["QUANTICK_WINDOW_SIZE", "QUANTICK_WINDOW_MAXIMIZED"];

#[cfg(test)]
mod window_hook_tests {
    use super::*;
    use crate::launch::LaunchConfig;

    #[test]
    fn a_requested_size_wins_over_the_saved_one_and_maximize_waits_for_a_frame() {
        let mut values = std::collections::BTreeMap::from([
            ("QUANTICK_WINDOW_SIZE", OsString::from("1x1")),
            ("QUANTICK_WINDOW_MAXIMIZED", OsString::from("1")),
        ]);
        let launch = LaunchConfig::capture(|name| values.get(name).cloned());
        values.clear();
        assert_eq!(launch.window_size(Some([1200.0, 800.0])), [1.0, 1.0]);
        assert_eq!(launch.window_size(None), [1.0, 1.0]);
        let mut window = launch.window.into_window_state();
        let ctx = egui::Context::default();
        for expected in [1, 0] {
            let output = ctx.run(egui::RawInput::default(), |ctx| window.apply(ctx));
            let count = output
                .viewport_output
                .values()
                .flat_map(|viewport| &viewport.commands)
                .filter(|command| matches!(command, egui::ViewportCommand::Maximized(true)))
                .count();
            assert_eq!(count, expected);
        }
    }

    #[test]
    fn absent_and_invalid_values_keep_the_saved_size_and_no_maximize() {
        for raw in [None, Some(""), Some("true"), Some(" 1 ")] {
            let launch = LaunchConfig::capture(|_| raw.map(OsString::from));
            assert_eq!(launch.window_size(Some([900.0, 700.0])), [900.0, 700.0]);
            assert!(!launch.window.into_window_state().maximize);
        }
    }

    #[test]
    fn a_requested_size_is_honoured_however_small_it_is() {
        assert_eq!(parse_window_size("1280x720"), Some([1280.0, 720.0]));
        assert_eq!(parse_window_size(" 1280 X 720 "), Some([1280.0, 720.0]));
        assert_eq!(
            parse_window_size("200x100"),
            Some([200.0, 100.0]),
            "the hook reaches a degenerate layout on purpose"
        );
        assert_eq!(
            parse_window_size("1x1"),
            Some([1.0, 1.0]),
            "there is no floor left to hit"
        );
    }

    #[test]
    fn a_malformed_size_is_rejected_rather_than_guessed_at() {
        for raw in [
            "",
            "1280",
            "1280x",
            "x720",
            "wide x tall",
            "0x0",
            "-5x10",
            "NaNx100",
            "100xinf",
        ] {
            assert_eq!(parse_window_size(raw), None, "{raw:?}");
        }
    }
}
