//! Executable startup inputs, captured before threads or owner construction.
//! Only ordinary window/log configuration lives here; scenario inputs remain
//! with their feature-gated owners and are consumed by the constructor.

use eframe::egui;
use std::ffi::OsString;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogFormat {
    #[default]
    Text,
    Json,
}

#[derive(Default)]
pub(crate) struct StartupConfig {
    pub log_format: LogFormat,
    window_size: Option<String>,
    maximize: bool,
}

impl StartupConfig {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let log = lookup("QUANTICK_LOG_FORMAT").and_then(|value| value.into_string().ok());
        let window_size = lookup("QUANTICK_WINDOW_SIZE").and_then(|value| value.into_string().ok());
        let maximize = lookup("QUANTICK_WINDOW_MAXIMIZED")
            .and_then(|value| value.into_string().ok())
            .is_some_and(|value| value == "1");
        Self {
            log_format: if log.is_some_and(|value| value.eq_ignore_ascii_case("json")) {
                LogFormat::Json
            } else {
                LogFormat::Text
            },
            window_size,
            maximize,
        }
    }

    pub fn window_size(&self, saved: Option<[f32; 2]>) -> [f32; 2] {
        window_size(self.window_size.as_deref(), saved)
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

crate::hooks::declare_hooks![
    "QUANTICK_LOG_FORMAT",
    "QUANTICK_WINDOW_SIZE",
    "QUANTICK_WINDOW_MAXIMIZED"
];

/// Size the window opens at when nothing asks for another.
const DEFAULT_WINDOW_PX: [f32; 2] = [1100.0, 650.0];
/// Smallest window the app will *reopen* at, whatever the last session left
/// behind.
///
/// The window itself has no minimum — it drags down to nothing, which is the
/// point. But a size is remembered across launches, and a chart squeezed to a
/// sliver and then closed would come back as a sliver: a window with no title
/// bar to grab and no edge to find. That is a trap the trader cannot get out
/// of from inside the app, so the *restore* path floors what it reads.
///
/// Small enough to be a deliberately tiny window, large enough to have an edge
/// and a title bar to drag. It is a recovery floor, not a layout one: nothing
/// about the chrome is promised at this size.
///
/// `QUANTICK_WINDOW_SIZE` is not floored — it is an explicit request for this
/// one run, made by someone who can unset it, and reaching a degenerate layout
/// on purpose is exactly what that hook is for.
const REOPEN_FLOOR_PX: [f32; 2] = [320.0, 240.0];

/// Size the window opens at: `QUANTICK_WINDOW_SIZE=WxH` when it is set, else
/// the `saved` size from the workspace, else [`DEFAULT_WINDOW_PX`].
///
/// The env var wins over the saved size for the same reason it wins over the
/// saved market: it is an explicit request for this one run, and a validation
/// run asking for a small window must get one whatever the last session left
/// behind.
///
/// The hook exists because window size is not decoration here — it is what
/// decides whether the indicator band has room for its panes and whether the
/// time axis has room for its labels. Without a way to ask for a small window,
/// that entire class of defect is invisible to any validation that is not a
/// human dragging a corner.
///
/// Not clamped: the hook can ask for a window of any positive size, including
/// one far too small to lay anything out in, because a validation run proving
/// the app survives a degenerate window has to be able to *ask* for one. A
/// value that does not parse is ignored with a warning rather than failing
/// the launch: a malformed env var must not stand between the user and their
/// chart.
fn window_size(raw: Option<&str>, saved: Option<[f32; 2]>) -> [f32; 2] {
    let fallback = restore_size(saved);
    let Some(raw) = raw else {
        return fallback;
    };
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
            size
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
            fallback
        }
    }
}

/// The size a saved workspace reopens at, floored at [`REOPEN_FLOOR_PX`].
///
/// Not to protect the layout, which is free to be cramped, but so a session
/// closed on a sliver of a window reopens on something the trader can grab.
/// Split out from [`window_size`] because it is the whole of the restore
/// policy and reads no environment, so a test can state it without touching a
/// process-wide variable other tests are reading at the same time.
fn restore_size(saved: Option<[f32; 2]>) -> [f32; 2] {
    saved.map_or(DEFAULT_WINDOW_PX, |[width, height]| {
        [
            width.max(REOPEN_FLOOR_PX[0]),
            height.max(REOPEN_FLOOR_PX[1]),
        ]
    })
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

#[cfg(test)]
mod window_size_tests {
    use super::*;

    #[test]
    fn captured_inputs_survive_changes_to_the_lookup_source() {
        let mut values = std::collections::BTreeMap::from([
            ("QUANTICK_LOG_FORMAT", OsString::from("JSON")),
            ("QUANTICK_WINDOW_SIZE", OsString::from("1x1")),
            ("QUANTICK_WINDOW_MAXIMIZED", OsString::from("1")),
        ]);
        let mut reads = Vec::new();
        let startup = StartupConfig::capture(|name| {
            reads.push(name.to_owned());
            values.get(name).cloned()
        });
        values.clear();
        assert_eq!(
            reads,
            [
                "QUANTICK_LOG_FORMAT",
                "QUANTICK_WINDOW_SIZE",
                "QUANTICK_WINDOW_MAXIMIZED"
            ]
        );
        assert_eq!(startup.log_format, LogFormat::Json);
        assert_eq!(startup.window_size(Some([1200.0, 800.0])), [1.0, 1.0]);
        assert_eq!(startup.window_size(None), [1.0, 1.0]);
        let mut window = startup.into_window_state();
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
    fn absence_invalid_values_and_existing_flag_grammar_preserve_defaults() {
        for raw in [None, Some(""), Some("true"), Some(" 1 "), Some("JSON ")] {
            let startup = StartupConfig::capture(|_| raw.map(OsString::from));
            assert_eq!(startup.log_format, LogFormat::Text);
            assert_eq!(startup.window_size(Some([900.0, 700.0])), [900.0, 700.0]);
            assert!(!startup.into_window_state().maximize);
        }
        for raw in ["JSON", "json", "JsOn"] {
            let startup =
                StartupConfig::capture(|name| (name == "QUANTICK_LOG_FORMAT").then(|| raw.into()));
            assert_eq!(startup.log_format, LogFormat::Json);
        }
        assert_eq!(window_size(None, None), DEFAULT_WINDOW_PX);
    }

    #[cfg(windows)]
    #[test]
    fn invalid_unicode_is_ignored_like_the_original_string_environment_read() {
        use std::os::windows::ffi::OsStringExt;
        let invalid = OsString::from_wide(&[0xD800]);
        let startup = StartupConfig::capture(|_| Some(invalid.clone()));
        assert_eq!(startup.log_format, LogFormat::Text);
        assert_eq!(startup.window_size(None), DEFAULT_WINDOW_PX);
        assert!(!startup.into_window_state().maximize);
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

    /// The window drags to nothing, but a session closed on nothing must not
    /// reopen on nothing: the restore path is the one place a floor survives,
    /// and it is a recovery floor, not a layout one.
    #[test]
    fn a_saved_sliver_reopens_on_something_the_trader_can_grab() {
        assert_eq!(restore_size(Some([0.0, 0.0])), REOPEN_FLOOR_PX);
        assert_eq!(
            restore_size(Some([4.0, 900.0])),
            [REOPEN_FLOOR_PX[0], 900.0]
        );
        assert_eq!(restore_size(Some([1280.0, 720.0])), [1280.0, 720.0]);
        assert_eq!(restore_size(None), DEFAULT_WINDOW_PX);
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
