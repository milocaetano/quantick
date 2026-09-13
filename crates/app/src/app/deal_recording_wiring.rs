//! Window-level wiring for deal recorders.

use crate::deal_recording::{self, DealRecorder};

use super::QuantickApp;

fn recorder_for(
    app: &mut QuantickApp,
    feed_id: &str,
    symbol: &str,
    day_cache: deal_recording::DayCache,
) -> DealRecorder {
    let default_on = app
        .harness
        .deal_recording_default()
        .or(app.chrome.record_deals)
        .unwrap_or_else(|| app.config.records_deals(feed_id));
    let mut recorder = DealRecorder::with_cache(
        symbol,
        deal_recording::resolve_dir(app.config.deals_dir()),
        default_on,
        day_cache,
    )
    .for_feed(feed_id);
    recorder.set_timezone(app.tz.minutes());
    recorder
}

/// Ensure every tab owns a recorder for its current feed and symbol.
pub(super) fn ensure(app: &mut QuantickApp) {
    let tz_minutes = app.tz.minutes();
    for index in 0..app.tabs.len() {
        let (feed_id, symbol) = {
            let tab = &mut app.tabs[index];
            tab.deal_recorder.set_timezone(tz_minutes);
            if tab.deal_recorder.is_for(&tab.active.0, &tab.active.1) {
                continue;
            }
            tab.active.clone()
        };
        app.tabs[index].deal_recorder.stop();
        let day_cache = app.tabs[index].deal_recorder.take_day_cache();
        let mut recorder = recorder_for(app, &feed_id, &symbol, day_cache);
        let tab = &mut app.tabs[index];
        recorder.set_available(tab.feed_capabilities.borrow().deal_counter);
        tab.deal_recorder = recorder;
    }
}

fn default_for(app: &QuantickApp) -> bool {
    app.chrome
        .record_deals
        .unwrap_or_else(|| app.config.records_deals(&app.active_tab().feed_id))
}

/// Draw the saved recording default through the Tools menu.
pub(super) fn draw_toggle(app: &mut QuantickApp, ui: &mut eframe::egui::Ui) {
    let mut enabled = default_for(app);
    if ui
        .checkbox(&mut enabled, "Record deals by default")
        .on_hover_text(
            "start writing the venue's deal counter when a MetaTrader B3 symbol connects",
        )
        .changed()
    {
        set_default(app, enabled);
    }
}

/// Save the default and apply it to undecided recorders.
pub(crate) fn set_default(app: &mut QuantickApp, enabled: bool) {
    app.chrome.record_deals = Some(enabled);
    for tab in &mut app.tabs {
        tab.deal_recorder.set_default(enabled);
    }
}
