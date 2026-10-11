use super::*;
use crate::style::ChartStyle;

fn env(style: &ChartStyle, now: Instant) -> SurfaceEnv<'_> {
    SurfaceEnv {
        style,
        ..SurfaceEnv::quiet(now)
    }
}

fn draw(
    surface: &mut ChartSettingsSurface,
    ctx: &egui::Context,
    env: &SurfaceEnv<'_>,
) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(Default::default(), |ctx| {
        response = surface.draw(ctx, env);
    });
    response
}

fn pending(open: bool, now: Instant) -> ChartSettingsSurface {
    ChartSettingsSurface {
        open,
        log_pending: true,
        last_change: Some(now),
        ..ChartSettingsSurface::default()
    }
}

/// A closed window reads nothing and asks for nothing.
#[test]
fn a_closed_window_asks_for_nothing() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    let mut surface = ChartSettingsSurface::default();
    let response = draw(&mut surface, &ctx, &env(&style, Instant::now()));
    assert_eq!(response, SurfaceResponse::default());
    assert!(!surface.is_open());
}

/// An open window on a frame nobody touched reports no edit on either tab —
/// otherwise every open window would bump the style or rewrite the footprint
/// file at the refresh rate.
#[test]
fn an_untouched_window_reports_no_edit_on_either_tab() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    for tab in ChartSettingsTab::ALL {
        let mut surface = ChartSettingsSurface::default();
        surface.open(tab);
        let response = draw(&mut surface, &ctx, &env(&style, Instant::now()));
        assert_eq!(response, SurfaceResponse::default(), "{tab:?}");
        assert!(surface.is_open_on(tab));
    }
}

/// One window, one flag: the doors switch the tab rather than stacking a
/// second window, and the toolbar's flip closes only the tab it opens.
#[test]
fn every_door_reaches_the_one_window() {
    let mut surface = ChartSettingsSurface::default();
    surface.open(ChartSettingsTab::Footprint);
    assert!(surface.is_open_on(ChartSettingsTab::Footprint));
    surface.toggle(ChartSettingsTab::Candles);
    assert!(
        surface.is_open_on(ChartSettingsTab::Candles),
        "flipped to candles"
    );
    surface.toggle(ChartSettingsTab::Candles);
    assert!(!surface.is_open());
    assert_eq!(surface.tab(), ChartSettingsTab::Candles, "remembered");
}

/// Drawing an open window starts the sitting on the style in force; Reset
/// returns there however far the chart moved; closing forgets it.
#[test]
fn reset_returns_to_the_look_the_window_opened_on() {
    let ctx = egui::Context::default();
    let opened_on = ChartStyle::default();
    let mut surface = ChartSettingsSurface::default();
    surface.open(ChartSettingsTab::Candles);
    draw(&mut surface, &ctx, &env(&opened_on, Instant::now()));
    let mut moved = opened_on;
    moved.candles = crate::style::CandlePreset::Glass.style();
    moved.canvas.grid_enabled = !opened_on.canvas.grid_enabled;
    // A later frame over the moved style does not move the starting point.
    draw(&mut surface, &ctx, &env(&moved, Instant::now()));
    assert_eq!(surface.candles.reset(&moved), Some(opened_on));
    surface.close();
    assert_eq!(
        surface.candles.reset(&moved),
        None,
        "closing ends the sitting"
    );
    surface.open(ChartSettingsTab::Footprint);
    draw(&mut surface, &ctx, &env(&moved, Instant::now()));
    assert_eq!(
        surface.candles.reset(&opened_on),
        Some(moved),
        "a new sitting starts on the look in force, whichever tab opened it"
    );
}

/// The debounce holds a change back while the gesture runs, lets it go once
/// the trader stops, then stays quiet however many frames follow.
#[test]
fn the_log_waits_for_the_gesture_to_settle() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    let start = Instant::now();
    let mut surface = pending(true, start);
    let response = draw(
        &mut surface,
        &ctx,
        &env(&style, start + STYLE_LOG_DEBOUNCE / 2),
    );
    assert!(response.log_style_change.is_none(), "still mid-gesture");
    let response = draw(&mut surface, &ctx, &env(&style, start + STYLE_LOG_DEBOUNCE));
    assert!(response.log_style_change.is_some(), "the gesture settled");
    let response = draw(
        &mut surface,
        &ctx,
        &env(&style, start + STYLE_LOG_DEBOUNCE * 4),
    );
    assert!(response.log_style_change.is_none(), "one change, one line");
}

/// Closing does not make the record wait out a timer; an open window
/// mid-gesture does wait; nothing pending logs nothing.
#[test]
fn closing_the_window_logs_without_waiting() {
    let now = Instant::now();
    assert!(!pending(true, now).should_log(now), "the debounce holds");
    assert!(pending(false, now).should_log(now), "the window left");
    assert!(!ChartSettingsSurface::default().should_log(now));
}

/// Closed from the toolbar between frames, the window still flushes the
/// edit on its next draw, once, naming the preset a click applied.
#[test]
fn closing_from_the_toolbar_still_records_the_edit() {
    let ctx = egui::Context::default();
    let style = ChartStyle::default();
    let now = Instant::now();
    let mut surface = pending(true, now);
    surface.pending_preset = Some(crate::style::CandlePreset::Classic);
    surface.toggle(ChartSettingsTab::Candles);
    assert!(!surface.is_open());
    let response = draw(&mut surface, &ctx, &env(&style, now));
    assert_eq!(
        response.log_style_change,
        Some(StyleLogRequest {
            applied_preset: Some(crate::style::CandlePreset::Classic)
        })
    );
    let response = draw(&mut surface, &ctx, &env(&style, now));
    assert!(response.log_style_change.is_none(), "recorded once");
}
