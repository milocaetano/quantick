use super::*;

#[test]
fn a_budgeted_hook_gives_up_on_the_frame_that_spends_its_last() {
    let mut harness = Harness::default();
    harness.arm_load_older(2, 2);
    assert!(!harness.spend_load_older_frame().gave_up, "one frame left");
    assert_eq!(harness.load_older_remaining(), Some((2, 1)));
    assert!(!harness.spend_load_older_frame().gave_up, "the last frame");
    assert_eq!(harness.load_older_remaining(), Some((2, 0)));
    assert!(
        harness.spend_load_older_frame().gave_up,
        "the budget is finite"
    );
    assert_eq!(harness.load_older_remaining(), None, "and it disarmed");
}

#[test]
fn a_page_that_went_out_costs_a_page_and_no_budget() {
    let mut harness = Harness::default();
    harness.arm_load_older(2, 10);
    harness.load_older_page_sent();
    assert_eq!(
        harness.load_older_remaining(),
        Some((1, 10)),
        "one still owed, on the same budget"
    );
    harness.load_older_page_sent();
    assert_eq!(harness.load_older_remaining(), None, "both pages asked for");
}

#[test]
fn spending_a_hook_that_was_never_armed_reports_nothing() {
    let mut harness = Harness::default();
    assert_eq!(
        harness.spend_load_older_frame(),
        HookFrame::default(),
        "an unset hook never gave up, because it never waited"
    );
}

/// `QUANTICK_POINTER` parks the mouse over the candles, which is the only
/// way a scripted run photographs anything that exists while a pointer is
/// over the chart — the compass, the crosshair, every hover readout.
///
/// Fractions of the *candles'* pane, so `0.5,0.5` frames the same place
/// whatever the window size and whatever share the live lane has taken.
/// Where that lands in window points is the trunk's half of the hook, and
/// `the_pointer_hook_parks_the_mouse_among_the_candles` asserts it.
#[test]
fn a_pointer_fraction_outside_the_canvas_is_refused() {
    assert_eq!(
        parse_pointer_fraction("0.25, 0.75"),
        Some(egui::vec2(0.25, 0.75))
    );
    for refused in ["0.5", "2,0.5", "-0.1,0.5", "half,half", ""] {
        assert_eq!(
            parse_pointer_fraction(refused),
            None,
            "{refused:?} is not a position, and a typo must photograph nothing rather than the wrong place"
        );
    }
}

/// The harness hook is the only way a scripted capture can see the
/// settings dialog, so it has to name both halves — and refuse nonsense
/// rather than guess, which would produce a screenshot of the wrong tab.
#[test]
fn the_settings_hook_names_an_indicator_and_a_tab() {
    assert_eq!(
        parse_settings_hook("0"),
        Some((0, SettingsTab::Inputs)),
        "a bare index opens on Inputs"
    );
    assert_eq!(
        parse_settings_hook("2:style"),
        Some((2, SettingsTab::Style))
    );
    assert_eq!(
        parse_settings_hook("1:1"),
        Some((1, SettingsTab::Style)),
        "the numeric spelling works too"
    );
    assert_eq!(parse_settings_hook("0:colours"), None);
    assert_eq!(parse_settings_hook("first"), None);
    assert_eq!(parse_settings_hook(""), None);
}

#[test]
fn a_context_menu_pane_is_named_by_either_of_its_two_words() {
    assert_eq!(
        ContextMenuPane::from_env_value("candles"),
        Some(ContextMenuPane::Chart)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("chart-layers"),
        Some(ContextMenuPane::Chart)
    );
    assert!(context_menu_expands_chart_layers("CHART-LAYERS"));
    assert!(!context_menu_expands_chart_layers("chart"));
    assert_eq!(
        ContextMenuPane::from_env_value("Lane"),
        Some(ContextMenuPane::Tape)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("legend"),
        None,
        "no such pane"
    );
}
