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
