//! Beside the candles, the native tape's settings keep the divider's width
//! and offer the native switch next to tape only.

use super::*;

#[test]
fn native_split_settings_keep_the_width_and_the_native_controls() {
    let mut view = view(true);
    view.config.live_lane.tape_only = false;
    view.config.live_lane.native_tape = true;
    let before = view.config.clone();
    let (_, text) = open_controls(&mut view);
    for inactive in ["cluster", "summarize closed bars", "bubble size"] {
        assert!(!has(&text, inactive), "inactive tape control: {inactive}");
    }
    for meaningful in [
        "Native tape",
        "Tape only (hide candles)",
        "Ignore opening burst in scale",
        "width",
        "window",
    ] {
        assert!(has(&text, meaningful), "missing control: {meaningful}");
    }
    assert_eq!(
        view.config, before,
        "opening settings must not rewrite a preset"
    );
}
